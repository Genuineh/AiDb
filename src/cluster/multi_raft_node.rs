//! Multi-Raft node manager — combines Router, LifecycleManager,
//! gRPC dispatcher, and per-group storage.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::cluster::lifecycle_manager::{LifecycleManager, MetaRaftProvider};
use crate::cluster::meta_types::{default_slot_table, ClusterMeta, SlotMigrationState, SlotTable};
use crate::cluster::network::{
    raft_rpc, RaftNetworkClientFactory, RaftServiceDispatcher, RaftServiceImpl,
};
use crate::cluster::node::OpenRaftNode;
use crate::cluster::router::Router;
use crate::cluster::sharded_storage::ShardedStorage;
use crate::cluster::types::{ClusterError, NodeId, RaftNodeConfig, Request, Response};
use crate::config::Options;
use crate::error::Result;

/// 多 Group Raft 节点管理器
///
/// 组合以下模块:
/// - `Router` — key/slot 路由
/// - `RaftServiceDispatcher` — 统一 gRPC 分发
/// - `ShardedStorage` — 每 Group 独立 DB
/// - `LifecycleManager` — Group 拓扑变化检测
pub struct MultiRaftNode {
    node_id: NodeId,
    groups: Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>>,
    router: Arc<Router>,
    storages: Arc<RwLock<HashMap<u64, ShardedStorage>>>,
    grpc_dispatcher: Arc<RaftServiceDispatcher>,
    lifecycle: Arc<LifecycleManager>,
    shutdown_tx: parking_lot::Mutex<Option<watch::Sender<bool>>>,
    server_handle: parking_lot::Mutex<Option<JoinHandle<()>>>,
    /// Override set for group locality checks (used in testing).
    local_group_overrides: Arc<RwLock<HashSet<u64>>>,
    /// Override set for elected-leader checks (used in testing).
    elected_leader_overrides: Arc<RwLock<HashSet<u64>>>,
}

/// Group 生命周期配置 (用于 start_lifecycle_with_data).
#[derive(Clone)]
pub struct LifecycleConfig {
    pub data_dir: std::path::PathBuf,
    pub raft_node_config: RaftNodeConfig,
    pub options: Options,
}

impl MultiRaftNode {
    /// 创建 MultiRaftNode (不启动 gRPC, 不启动 lifecycle task).
    /// 调用 `start()` 启动 gRPC, `start_lifecycle()` 启动后台 task.
    pub fn new(
        node_id: NodeId,
        router: Arc<Router>,
        grpc_dispatcher: Arc<RaftServiceDispatcher>,
    ) -> Self {
        let router_for_lm = Arc::clone(&router);
        Self {
            node_id,
            groups: Arc::new(RwLock::new(HashMap::new())),
            router,
            storages: Arc::new(RwLock::new(HashMap::new())),
            grpc_dispatcher,
            lifecycle: Arc::new(LifecycleManager::new(
                node_id,
                router_for_lm,
                Arc::new(NoopMetaRaftProvider),
            )),
            shutdown_tx: parking_lot::Mutex::new(None),
            server_handle: parking_lot::Mutex::new(None),
            local_group_overrides: Arc::new(RwLock::new(HashSet::new())),
            elected_leader_overrides: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// 创建 MultiRaftNode 并注入 LifecycleManager (含 MetaRaftProvider).
    pub fn new_with_lifecycle(
        node_id: NodeId,
        router: Arc<Router>,
        grpc_dispatcher: Arc<RaftServiceDispatcher>,
        lifecycle: LifecycleManager,
    ) -> Self {
        Self {
            node_id,
            groups: Arc::new(RwLock::new(HashMap::new())),
            router,
            storages: Arc::new(RwLock::new(HashMap::new())),
            grpc_dispatcher,
            lifecycle: Arc::new(lifecycle),
            shutdown_tx: parking_lot::Mutex::new(None),
            server_handle: parking_lot::Mutex::new(None),
            local_group_overrides: Arc::new(RwLock::new(HashSet::new())),
            elected_leader_overrides: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// 节点 ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// 启动统一 gRPC 服务 (绑定单一端口, 所有数据 Group 共享).
    ///
    /// 数据 Group 不启动自己的 gRPC server, 而是通过此统一端口接收 RPC.
    pub async fn start(&self, addr: SocketAddr, max_message_size: u64) -> Result<()> {
        use raft_rpc::raft_service_server::RaftServiceServer;
        use tokio::net::TcpListener;
        use tokio_stream::wrappers::TcpListenerStream;

        let listener = TcpListener::bind(addr).await.map_err(ClusterError::Io)?;
        let service = RaftServiceImpl::new(self.grpc_dispatcher.clone());
        let server = RaftServiceServer::new(service)
            .max_decoding_message_size(max_message_size as usize)
            .max_encoding_message_size(max_message_size as usize);

        let handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(server)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        *self.server_handle.lock() = Some(handle);
        Ok(())
    }

    /// 启动 LifecycleManager 后台 task (仅日志, 无实际 Group 创建/销毁).
    ///
    /// 返回 `watch::Receiver<bool>`, 调用方可通过 `changed()` 监听关闭信号.
    pub fn start_lifecycle(&self) -> watch::Receiver<bool> {
        self.start_lifecycle_impl(None)
    }

    /// 启动 LifecycleManager 后台 task, 并自动创建/销毁数据 Group.
    pub fn start_lifecycle_with_data(&self, config: LifecycleConfig) -> watch::Receiver<bool> {
        self.start_lifecycle_impl(Some(config))
    }

    fn start_lifecycle_impl(
        &self,
        lifecycle_config: Option<LifecycleConfig>,
    ) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        *self.shutdown_tx.lock() = Some(tx);
        let lifecycle = self.lifecycle.clone();
        let groups = self.groups.clone();
        let storages = self.storages.clone();
        let dispatcher = self.grpc_dispatcher.clone();
        let node_id = self.node_id;

        tokio::spawn(async move {
            let mut shutdown_rx = rx;
            let cfg = lifecycle_config;
            let rpc_timeout = cfg.as_ref().map_or(
                RaftNodeConfig::default().rpc_timeout_ms,
                |c: &LifecycleConfig| c.raft_node_config.rpc_timeout_ms,
            );
            let msg_size = cfg
                .as_ref()
                .map_or(RaftNodeConfig::default().grpc_max_message_size, |c| {
                    c.raft_node_config.grpc_max_message_size
                });
            let net_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(
                node_id,
                0,
                rpc_timeout,
                msg_size,
            )));

            loop {
                tokio::select! {
                  _ = tokio::time::sleep(lifecycle.tick_interval()) => {
                    let tick_result = lifecycle.tick();
                    if let Some(ref cfg) = cfg {
                      // 从 MetaRaft 元数据判断本节点是否为 group 的 leader (is_leader=true).
                      // 只有 leader 初始化为 Voter; 其他成员等待 drift 对账被加入.
                      let meta = lifecycle.meta_raft().get_cluster_meta();
                      for gid in tick_result.groups_to_create {
                        let is_leader = meta
                          .groups
                          .get(&gid)
                          .and_then(|g| g.replicas.iter().find(|r| r.node_id == node_id))
                          .map(|r| r.is_leader)
                          .unwrap_or(false);
                        Self::create_group_inner(
                          gid,
                          is_leader,
                          meta.nodes.get(&node_id).map(|n| n.rpc_addr.as_str()),
                          &groups,
                          &storages,
                          &dispatcher,
                          cfg,
                          &net_factory,
                        )
                        .await;
                      }
                      for gid in tick_result.groups_to_remove {
                        Self::remove_group_inner(gid, &groups, &storages, &dispatcher).await;
                      }
                    } else {
                      if !tick_result.groups_to_create.is_empty() {
                        tracing::info!(groups_to_create = ?tick_result.groups_to_create, "group lifecycle tick (no-op)");
                      }
                      if !tick_result.groups_to_remove.is_empty() {
                        tracing::info!(groups_to_remove = ?tick_result.groups_to_remove, "group lifecycle tick (no-op)");
                      }
                    }

                    // Drift detection and reconciliation (only when cfg is Some).
                    if cfg.is_some() {
                      let mut applied = 0;
                      for (group_id, expected_members) in &tick_result.expected_memberships {
                        // 每次 tick 最多处理 1 个 drift, 避免批量 joint-consensus 操作引发集群不稳定.
                        if applied >= 1 {
                          break;
                        }

                        // 1. 获取实际 Raft 成员集合
                        let maybe_node = {
                          let groups_read = groups.read();
                          groups_read.get(group_id).cloned()
                        };
                        let actual = match maybe_node {
                          Some(node) => node.get_members().await,
                          None => continue,
                        };

                        // 2. 计算 drift
                        let to_add: Vec<NodeId> =
                          expected_members.difference(&actual).copied().collect();
                        let to_remove: Vec<NodeId> =
                          actual.difference(expected_members).copied().collect();
                        if to_add.is_empty() && to_remove.is_empty() {
                          continue;
                        }

                        // 3. Leader 检查 — 确保 drop lock 后再 .await
                        let maybe_node = {
                          let groups_read = groups.read();
                          groups_read.get(group_id).cloned()
                        };
                        let is_leader = match maybe_node {
                          Some(node) => node.is_leader().await,
                          None => false,
                        };
                        if !is_leader {
                          continue;
                        }

                        // 4. 执行对账
                        tracing::info!(
                          group_id = *group_id,
                          ?to_add,
                          ?to_remove,
                          "reconciling group membership drift"
                        );

                        let cluster_meta = lifecycle.meta_raft().get_cluster_meta();
                        if let Err(e) = Self::apply_membership_change(
                          *group_id,
                          expected_members,
                          &to_add,
                          &groups,
                          &cluster_meta,
                        )
                        .await
                        {
                          tracing::warn!(
                            group_id = *group_id,
                            error = %e,
                            "membership reconcile failed, will retry next tick"
                          );
                        }
                        applied += 1;
                      }
                    }
                  }
                  _ = shutdown_rx.changed() => {
                    tracing::info!("lifecycle manager shutting down");
                    break;
                  }
                }
            }
        });

        // 返回一个新的 subscriber, 调用方可接收关闭通知
        self.shutdown_tx.lock().as_ref().unwrap().subscribe()
    }

    async fn create_group_inner(
        group_id: u64,
        init_as_voter: bool,
        init_rpc_addr: Option<&str>,
        groups: &Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>>,
        storages: &Arc<RwLock<HashMap<u64, ShardedStorage>>>,
        dispatcher: &Arc<RaftServiceDispatcher>,
        cfg: &LifecycleConfig,
        net_factory: &Arc<RwLock<RaftNetworkClientFactory>>,
    ) {
        if groups.read().contains_key(&group_id) {
            return;
        }
        match Self::open_group(group_id, cfg, net_factory).await {
            Ok((node, storage)) => {
                let node = Arc::new(node);
                let raft = node.raft().clone();
                let node_id = cfg.raft_node_config.node_id;

                // 单节点 Voter 初始化: 优先 MetaRaft RPC 地址 (Docker 网络可达).
                // 若磁盘上已有 Raft 状态, initialize 会拒绝 — 此时仍注册 group 供读写.
                if init_as_voter {
                    let addr = init_rpc_addr
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("127.0.0.1:{}", 10000 + node_id));
                    match node.initialize(vec![(node_id, addr)]).await {
                        Ok(()) => {
                            tracing::info!(
                                group_id,
                                node_id,
                                "group initialized with single voter"
                            );
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("not allowed to initialize") {
                                tracing::info!(
                                    group_id,
                                    node_id,
                                    "group reuses existing raft state"
                                );
                            } else {
                                tracing::warn!(
                                  group_id,
                                  node_id,
                                  error = %e,
                                  "initialize failed, registering opened group"
                                );
                            }
                        }
                    }
                }

                groups.write().insert(group_id, node);
                storages.write().insert(group_id, storage);
                dispatcher.register_group(group_id, raft);
                tracing::info!(group_id, init_as_voter, "group created and registered");
            }
            Err(e) => {
                tracing::error!(group_id, error = %e, "failed to create group");
            }
        }
    }

    async fn open_group(
        group_id: u64,
        cfg: &LifecycleConfig,
        net_factory: &Arc<RwLock<RaftNetworkClientFactory>>,
    ) -> Result<(OpenRaftNode, ShardedStorage)> {
        let storage = ShardedStorage::open(&cfg.data_dir, group_id, cfg.options.clone())?;
        let db = storage.db().clone();
        let mut config = cfg.raft_node_config.clone();
        config.group_id = group_id;
        let factory = net_factory.read().clone().with_group_id(group_id);
        let node = OpenRaftNode::new(config, db, factory).await?;
        Ok((node, storage))
    }

    async fn remove_group_inner(
        group_id: u64,
        groups: &Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>>,
        storages: &Arc<RwLock<HashMap<u64, ShardedStorage>>>,
        dispatcher: &Arc<RaftServiceDispatcher>,
    ) {
        let node = groups.write().remove(&group_id);
        let storage = storages.write().remove(&group_id);
        if let Some(n) = node {
            let _ = n.shutdown().await;
        }
        if let Some(s) = storage {
            let _ = s.close();
        }
        dispatcher.unregister_group(group_id);
        tracing::info!(group_id, "group removed and unregistered");
    }

    /// 对账成员变更 — 关联函数, 与 `create_group_inner` / `remove_group_inner` 模式一致.
    ///
    /// 注意: 此方法**不**调用 `meta_raft.propose(ChangeGroupMembership)` —
    /// MetaRaft 元数据已经是期望状态, 此方法只执行 Raft 层面的操作.
    #[instrument(skip(groups, cluster_meta))]
    async fn apply_membership_change(
        group_id: u64,
        expected: &BTreeSet<NodeId>,
        to_add: &[NodeId],
        groups: &Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>>,
        cluster_meta: &crate::cluster::meta_types::ClusterMeta,
    ) -> Result<()> {
        // Step 1: 添加缺失的副本为 Learner.
        let node = {
            let groups_read = groups.read();
            groups_read
                .get(&group_id)
                .cloned()
                .ok_or_else(|| ClusterError::Raft(format!("group {group_id} not found locally")))?
        };

        for node_id in to_add {
            // Raft gRPC 走 MetaRaft RPC 端口 (共享 dispatcher), 不能用 client_addr.
            let addr = cluster_meta
                .nodes
                .get(node_id)
                .map(|n| n.rpc_addr.clone())
                .ok_or_else(|| {
                    ClusterError::InvalidConfig(format!("node {node_id} rpc addr not found"))
                })?;
            node.add_learner_nonblocking(*node_id, addr).await?;
        }

        // Step 2: Joint Consensus 成员变更 (全量替换为期望成员集合).
        // Openraft 的 joint consensus 保证不会出现无 Voter 的中间状态.
        let new_members: Vec<NodeId> = expected.iter().copied().collect();
        node.change_membership(new_members).await?;

        Ok(())
    }

    // ---- 访问器 ----

    /// LifecycleManager 引用
    pub fn lifecycle(&self) -> &LifecycleManager {
        &self.lifecycle
    }

    /// Router 引用
    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// gRPC 调度器引用
    pub fn grpc_dispatcher(&self) -> &Arc<RaftServiceDispatcher> {
        &self.grpc_dispatcher
    }

    /// 获取指定 group 的当前 Raft 成员集合 (从 OpenRaftNode 获取).
    #[allow(dead_code)]
    pub(crate) async fn get_group_raft_members(&self, group_id: u64) -> Option<BTreeSet<NodeId>> {
        let node = self.groups.read().get(&group_id).cloned();
        match node {
            Some(n) => Some(n.get_members().await),
            None => None,
        }
    }

    /// 检查 Group 是否在本地
    pub fn is_group_local(&self, group_id: u64) -> bool {
        self.groups.read().contains_key(&group_id)
            || self.local_group_overrides.read().contains(&group_id)
    }

    /// Override group locality for testing. Calling this makes `is_group_local`
    /// return `true` for the given group, even if no real Raft group exists.
    pub fn override_group_local(&self, group_id: u64) {
        self.local_group_overrides.write().insert(group_id);
    }

    /// Clear a previous group locality override.
    pub fn clear_group_local_override(&self, group_id: u64) {
        self.local_group_overrides.write().remove(&group_id);
    }

    /// Override elected-leader checks for testing.
    pub fn override_elected_leader(&self, group_id: u64) {
        self.elected_leader_overrides.write().insert(group_id);
    }

    /// Clear a previous elected-leader override.
    pub fn clear_elected_leader_override(&self, group_id: u64) {
        self.elected_leader_overrides.write().remove(&group_id);
    }

    /// 同步检查本节点是否已是 OpenRaft 选出的 group leader (非 MetaRaft 元数据).
    pub fn is_elected_leader_sync(&self, group_id: u64) -> bool {
        if self.elected_leader_overrides.read().contains(&group_id) {
            return true;
        }
        let groups = self.groups.read();
        let Some(node) = groups.get(&group_id) else {
            return false;
        };
        node.raft().metrics().borrow().current_leader == Some(self.node_id)
    }

    /// 本地 Group 的 OpenRaftNode 映射
    pub fn get_groups(&self) -> &Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>> {
        &self.groups
    }

    /// 本地 Group 的 ShardedStorage 映射
    pub fn get_storages(&self) -> &Arc<RwLock<HashMap<u64, ShardedStorage>>> {
        &self.storages
    }

    // ---- 读写操作 ----

    /// 向指定 Group 提交提案 (propose).
    #[instrument(skip(self, request))]
    pub async fn propose_group(&self, group_id: u64, request: Request) -> Result<Response> {
        let group = self.groups.read().get(&group_id).cloned();
        match group {
            Some(node) => node.propose(request).await,
            None => Err(ClusterError::Raft(format!("group {} not found locally", group_id)).into()),
        }
    }

    /// 按 key 路由并提交提案 (单 key SET/DEL 入口).
    #[instrument(skip(self))]
    pub async fn propose_key(&self, key: Vec<u8>, value: Option<Vec<u8>>) -> Result<Response> {
        let (gid, _status) = self.router.route_key(&key)?;
        let request = match value {
            Some(v) => Request::Put { key, value: v },
            None => Request::Delete { key },
        };
        self.propose_group(gid, request).await
    }

    /// 按 key 路由并本地读取 (单 key GET 入口).
    #[instrument(skip(self))]
    pub async fn get_key(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>> {
        let (gid, _status) = self.router.route_key(&key)?;
        let group = self.groups.read().get(&gid).cloned();
        match group {
            Some(node) => node.get(key).await,
            None => Err(ClusterError::Raft(format!("group {} not found locally", gid)).into()),
        }
    }

    // ---- Phase 15 转发方法 ----

    /// 从指定 Group 的状态机直接读取 key (绕过路由).
    pub async fn get_key_from_group(&self, group_id: u64, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let group = self.groups.read().get(&group_id).cloned();
        match group {
            Some(node) => node.get(key.to_vec()).await,
            None => Err(ClusterError::Raft(format!("group {} not found locally", group_id)).into()),
        }
    }

    /// 直接读取本地 group 状态机 (不要求 leader).
    ///
    /// 与 `get_key_from_group` 不同, 本方法不检查 leader 身份, 直接读本地
    /// 状态机, 供数据面读路径 (leader 直读 / 只读副本) 使用.
    pub async fn get_local(&self, group_id: u64, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let node = self.groups.read().get(&group_id).cloned();
        match node {
            Some(node) => {
                let storage = node.storage().clone();
                let key = key.to_vec();
                tokio::task::spawn_blocking(move || storage.get_state_machine_value(&key))
                    .await
                    .map_err(|e| ClusterError::Internal(e.to_string()))?
            }
            None => Err(ClusterError::Raft(format!("group {group_id} not found locally")).into()),
        }
    }

    /// 扫描本地 group 状态机的全部 (user_key, value) 对.
    ///
    /// 返回的 key 已剥离 `sm/{gid}/` 前缀, 即与写入时传入的 user key 一致.
    pub async fn scan_local_pairs(&self, group_id: u64) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        use crate::cluster::storage::keys::{sm_range_end, sm_range_start, user_key_from_sm_key};
        let db = {
            let storages = self.storages.read();
            storages.get(&group_id).map(|s| s.db().clone())
        };
        match db {
            Some(db) => tokio::task::spawn_blocking(move || {
                let start = sm_range_start(group_id);
                let end = sm_range_end(group_id);
                let iter = db.scan(Some(start.as_slice()), Some(end.as_slice()))?;
                let mut out = Vec::new();
                for item in iter {
                    let (k, v) = item?;
                    if let Some(uk) = user_key_from_sm_key(group_id, &k) {
                        out.push((uk, v));
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| ClusterError::Internal(e.to_string()))?,
            None => Err(ClusterError::Raft(format!("group {group_id} not found locally")).into()),
        }
    }

    /// 本地承载的全部 group id 列表.
    pub fn local_group_ids(&self) -> Vec<u64> {
        self.groups.read().keys().copied().collect()
    }

    /// 读取指定 key 在 Group 中的 TTL.
    /// 注意: AiDb 引擎层不支持逐 key TTL; 始终返回 None.
    pub async fn get_ttl_from_group(&self, group_id: u64, key: &[u8]) -> Result<Option<u64>> {
        let _ = (group_id, key);
        Ok(None)
    }

    /// 扫描 Group 状态机中的全部 key (可选范围).
    pub async fn scan_keys(
        &self,
        group_id: u64,
        key_range: Option<(Vec<u8>, Vec<u8>)>,
    ) -> Result<Vec<Vec<u8>>> {
        let db = {
            let storages = self.storages.read();
            storages.get(&group_id).map(|s| s.db().clone())
        };
        match db {
            Some(db) => {
                tokio::task::spawn_blocking(move || {
                    let iter = match &key_range {
                        Some((start, end)) => {
                            db.scan(Some(start.as_slice()), Some(end.as_slice()))?
                        }
                        None => db.scan(None, None)?,
                    };
                    let mut keys = Vec::new();
                    for item in iter {
                        let (k, _) = item?;
                        if k.starts_with(b"\x00") {
                            continue; // skip internal (\x00) keys
                        }
                        keys.push(k);
                    }
                    Ok(keys)
                })
                .await
                .map_err(|e| ClusterError::Internal(e.to_string()))?
            }
            None => Err(ClusterError::Raft(format!("group {} not found locally", group_id)).into()),
        }
    }

    /// 变更数据 Group 的成员 (joint consensus, 全量替换).
    pub async fn change_group_membership(&self, group_id: u64, members: Vec<NodeId>) -> Result<()> {
        let group = self.groups.read().get(&group_id).cloned();
        match group {
            Some(node) => node.change_membership(members).await,
            None => Err(ClusterError::Raft(format!("group {} not found locally", group_id)).into()),
        }
    }

    /// 添加 Learner 到数据 Group (non-blocking, 不等待日志同步).
    pub async fn add_learner_to_group(
        &self,
        group_id: u64,
        node_id: NodeId,
        address: String,
    ) -> Result<()> {
        let group = self.groups.read().get(&group_id).cloned();
        match group {
            Some(node) => node.add_learner_nonblocking(node_id, address).await,
            None => Err(ClusterError::Raft(format!("group {} not found locally", group_id)).into()),
        }
    }

    /// 关闭所有资源.
    pub async fn shutdown(&self) {
        // 发送关闭信号给 lifecycle task
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(true);
        }
        // 关闭所有 OpenRaftNode
        let group_list: Vec<(u64, Arc<OpenRaftNode>)> = self.groups.write().drain().collect();
        for (_, node) in group_list {
            let _ = node.shutdown().await;
        }
        // 清理 storage
        self.storages.write().clear();
        // 中止 gRPC server
        if let Some(handle) = self.server_handle.lock().take() {
            handle.abort();
        }
    }
}

/// Noop MetaRaftProvider for cases where lifecycle is not yet wired.
struct NoopMetaRaftProvider;

impl MetaRaftProvider for NoopMetaRaftProvider {
    fn get_cluster_meta(&self) -> ClusterMeta {
        ClusterMeta::default()
    }
    fn get_slot_table(&self) -> SlotTable {
        default_slot_table()
    }
    fn get_migration_state(&self) -> Option<SlotMigrationState> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    fn compute_drift(expected: &BTreeSet<u64>, actual: &BTreeSet<u64>) -> (Vec<u64>, Vec<u64>) {
        let to_add: Vec<u64> = expected.difference(actual).copied().collect();
        let to_remove: Vec<u64> = actual.difference(expected).copied().collect();
        (to_add, to_remove)
    }

    #[test]
    fn drift_no_drift_when_membership_matches() {
        let expected: BTreeSet<u64> = [1, 2].iter().copied().collect();
        let actual: BTreeSet<u64> = [1, 2].iter().copied().collect();
        let (to_add, to_remove) = compute_drift(&expected, &actual);
        assert!(to_add.is_empty());
        assert!(to_remove.is_empty());
    }

    #[test]
    fn drift_detects_missing_replica() {
        let expected: BTreeSet<u64> = [1, 2].iter().copied().collect();
        let actual: BTreeSet<u64> = [1].iter().copied().collect();
        let (to_add, to_remove) = compute_drift(&expected, &actual);
        assert_eq!(to_add, vec![2]);
        assert!(to_remove.is_empty());
    }

    #[test]
    fn drift_detects_extra_member() {
        let expected: BTreeSet<u64> = [1].iter().copied().collect();
        let actual: BTreeSet<u64> = [1, 2].iter().copied().collect();
        let (to_add, to_remove) = compute_drift(&expected, &actual);
        assert!(to_add.is_empty());
        assert_eq!(to_remove, vec![2]);
    }

    #[test]
    fn drift_detects_both_add_and_remove() {
        let expected: BTreeSet<u64> = [2, 3].iter().copied().collect();
        let actual: BTreeSet<u64> = [1, 2].iter().copied().collect();
        let (to_add, to_remove) = compute_drift(&expected, &actual);
        assert_eq!(to_add, vec![3]);
        assert_eq!(to_remove, vec![1]);
    }

    #[test]
    fn drift_all_missing_empty_actual() {
        let expected: BTreeSet<u64> = [1, 2, 3].iter().copied().collect();
        let actual: BTreeSet<u64> = BTreeSet::new();
        let (to_add, to_remove) = compute_drift(&expected, &actual);
        assert_eq!(to_add.len(), 3);
        assert!(to_remove.is_empty());
    }

    #[test]
    fn learner_addr_uses_rpc_not_client_port() {
        use std::collections::HashMap;

        use crate::cluster::meta_types::{ClusterMeta, NodeInfo, NodeRole, NodeStatus};

        let mut nodes = HashMap::new();
        nodes.insert(
            5u64,
            NodeInfo {
                node_id: 5,
                rpc_addr: "127.0.0.1:17380".into(),
                client_addr: Some("127.0.0.1:7380".into()),
                role: NodeRole::Voter,
                status: NodeStatus::Online,
                registered_at: 0,
                tags: HashMap::new(),
            },
        );
        let meta = ClusterMeta {
            cluster_id: "test".into(),
            nodes,
            groups: HashMap::new(),
            version: 1,
            format_version: 1,
        };

        let addr = meta
            .nodes
            .get(&5)
            .map(|n| n.rpc_addr.clone())
            .expect("rpc addr");
        assert_eq!(addr, "127.0.0.1:17380");
        assert_ne!(addr, "127.0.0.1:7380");
    }
}
