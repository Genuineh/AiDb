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

/// 单个 group 的自愈重启退避状态.
struct GroupRestartState {
    last_attempt: Option<std::time::Instant>,
    consecutive_failures: u32,
}

/// 计算第 N 次连续失败后, 下一次重启尝试前应等待的退避时长.
///
/// 指数退避 (2s * 2^n), 上限 60s — 既能在偶发单次故障后快速自愈,
/// 也能避免持续性故障 (例如磁盘真的坏了) 时反复重启拖垮节点.
fn group_restart_backoff(consecutive_failures: u32) -> std::time::Duration {
    const BASE: std::time::Duration = std::time::Duration::from_secs(2);
    const MAX: std::time::Duration = std::time::Duration::from_secs(60);
    let exp = consecutive_failures.min(6);
    let millis = BASE.as_millis().saturating_mul(1u128 << exp);
    std::time::Duration::from_millis(millis.min(MAX.as_millis()) as u64)
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
        let restart_state: Arc<RwLock<HashMap<u64, GroupRestartState>>> =
            Arc::new(RwLock::new(HashMap::new()));

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

                      // Fail-fast 自愈: apply.rs 遇到真实存储故障时会让 openraft
                      // 把该 group 标记为 Fatal 并停止服务 (见 stage2-apply)。
                      // 这里检测 Fatal 状态并尝试就地重开 group, 避免每次真实
                      // 磁盘故障之外的偶发错误都需要人工介入重启整个进程。
                      Self::supervise_groups(
                        &groups,
                        &storages,
                        &dispatcher,
                        cfg,
                        &net_factory,
                        &restart_state,
                      )
                      .await;
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

    /// 扫描本地 group, 检测是否有实例进入 openraft 的 `Fatal` 状态 (由
    /// `apply_to_state_machine` 遇到真实存储错误后 fail-fast 触发), 并对
    /// 命中的 group 尝试就地重开 (等价于"重启这一个 group 的进程", 但不影响
    /// 同一节点上其它 group 或 gRPC server)。
    ///
    /// 重开时**不**传 `init_as_voter=true` —— 该 group 之前已经是集群里正常
    /// 运行的成员 (leader 或 follower), 磁盘上已经有它自己的 log/vote/
    /// membership 状态; 这里只是重新加载现有状态, 不是重新做单节点 bootstrap。
    async fn supervise_groups(
        groups: &Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>>,
        storages: &Arc<RwLock<HashMap<u64, ShardedStorage>>>,
        dispatcher: &Arc<RaftServiceDispatcher>,
        cfg: &LifecycleConfig,
        net_factory: &Arc<RwLock<RaftNetworkClientFactory>>,
        restart_state: &Arc<RwLock<HashMap<u64, GroupRestartState>>>,
    ) {
        let fatal_groups: Vec<(u64, String)> = {
            groups
                .read()
                .iter()
                .filter_map(|(gid, node)| match node.raft().metrics().borrow().running_state {
                    Ok(()) => None,
                    Err(ref fatal) => Some((*gid, fatal.to_string())),
                })
                .collect()
        };

        // 已恢复健康的 group 清除退避计数, 下次真正故障时重新从最短退避开始.
        if !fatal_groups.is_empty() || !restart_state.read().is_empty() {
            let fatal_ids: HashSet<u64> = fatal_groups.iter().map(|(gid, _)| *gid).collect();
            restart_state.write().retain(|gid, _| fatal_ids.contains(gid));
        }

        for (group_id, reason) in fatal_groups {
            #[cfg(feature = "monitoring")]
            crate::cluster::metrics::record_raft_group_fatal(group_id);

            let should_attempt = {
                let mut state = restart_state.write();
                let entry = state.entry(group_id).or_insert(GroupRestartState {
                    last_attempt: None,
                    consecutive_failures: 0,
                });
                let backoff = group_restart_backoff(entry.consecutive_failures);
                let ready = entry.last_attempt.is_none_or(|t| t.elapsed() >= backoff);
                if ready {
                    entry.last_attempt = Some(std::time::Instant::now());
                    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
                }
                ready
            };

            if !should_attempt {
                tracing::debug!(group_id, "raft group still in self-heal backoff window, skip this tick");
                continue;
            }

            tracing::error!(
                group_id,
                error = %reason,
                "raft group entered fatal state, attempting in-process self-heal restart"
            );

            Self::remove_group_inner(group_id, groups, storages, dispatcher).await;
            Self::create_group_inner(
                group_id, false, None, groups, storages, dispatcher, cfg, net_factory,
            )
            .await;

            if groups.read().contains_key(&group_id) {
                tracing::warn!(group_id, "raft group self-heal restart succeeded");
                #[cfg(feature = "monitoring")]
                crate::cluster::metrics::record_raft_group_restart(group_id, "success");
            } else {
                tracing::error!(
                    group_id,
                    "raft group self-heal restart failed to reopen, will retry with backoff"
                );
                #[cfg(feature = "monitoring")]
                crate::cluster::metrics::record_raft_group_restart(group_id, "failure");
            }
        }
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

    /// 扫描 Group 状态机中的全部 key (可选按*用户 key* 范围过滤).
    ///
    /// 返回的 key 已剥离内部 `sm/{gid}/` 前缀, 与调用方写入时传入的 user
    /// key 完全一致 (与 `scan_local_pairs` 语义对齐) —— 之前这里直接返回
    /// DB 原始编码 key (带 `\x01sm/{gid}/` 前缀), 导致所有基于返回值算
    /// `key_to_slot()` 的调用方 (slot 迁移 executor 的 slot 过滤、
    /// `CLUSTER COUNTKEYSINSLOT`/`GETKEYSINSLOT`) 全部算出错误的 slot,
    /// 实质上从未按预期工作过.
    pub async fn scan_keys(
        &self,
        group_id: u64,
        key_range: Option<(Vec<u8>, Vec<u8>)>,
    ) -> Result<Vec<Vec<u8>>> {
        use crate::cluster::storage::keys::{sm_key, sm_range_end, sm_range_start, user_key_from_sm_key};

        let db = {
            let storages = self.storages.read();
            storages.get(&group_id).map(|s| s.db().clone())
        };
        match db {
            Some(db) => {
                tokio::task::spawn_blocking(move || {
                    let (start, end) = match &key_range {
                        Some((s, e)) => (sm_key(group_id, s), sm_key(group_id, e)),
                        None => (sm_range_start(group_id), sm_range_end(group_id)),
                    };
                    let iter = db.scan(Some(start.as_slice()), Some(end.as_slice()))?;
                    let mut keys = Vec::new();
                    for item in iter {
                        let (k, _) = item?;
                        let Some(user_key) = user_key_from_sm_key(group_id, &k) else {
                            continue; // 越界/非本 group 的 sm key, 理论上不会出现
                        };
                        keys.push(user_key);
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

    use super::*;

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
    fn restart_backoff_grows_and_caps() {
        let b0 = super::group_restart_backoff(0);
        let b1 = super::group_restart_backoff(1);
        let b2 = super::group_restart_backoff(2);
        let b_far = super::group_restart_backoff(100);
        assert_eq!(b0, std::time::Duration::from_secs(2));
        assert_eq!(b1, std::time::Duration::from_secs(4));
        assert_eq!(b2, std::time::Duration::from_secs(8));
        assert!(b1 > b0 && b2 > b1, "backoff must strictly increase at first");
        assert_eq!(
            b_far,
            std::time::Duration::from_secs(60),
            "backoff must be capped so a persistently faulty group doesn't restart-storm forever"
        );
    }

    /// 端到端验证 fail-fast + 自愈闭环: 真实存储故障 -> openraft Fatal ->
    /// `supervise_groups` 就地重开 group -> 服务恢复且故障前数据无损.
    #[tokio::test]
    async fn supervise_groups_restarts_fatal_group_and_preserves_data() {
        use std::collections::HashMap;

        use tempfile::TempDir;

        use crate::cluster::network::{RaftNetworkClientFactory, RaftServiceDispatcher};
        use crate::cluster::sharded_storage::ShardedStorage;
        use crate::cluster::types::{RaftNodeConfig, Request};
        use crate::config::Options;

        const GROUP_ID: u64 = 1;

        let dir = TempDir::new().unwrap();
        let cfg = LifecycleConfig {
            data_dir: dir.path().to_path_buf(),
            raft_node_config: RaftNodeConfig {
                node_id: 1,
                group_id: GROUP_ID,
                ..RaftNodeConfig::default()
            },
            options: Options::for_testing(),
        };
        let net_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(
            1,
            0,
            30,
            65 * 1024 * 1024,
        )));
        let groups: Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let storages: Arc<RwLock<HashMap<u64, ShardedStorage>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let dispatcher = Arc::new(RaftServiceDispatcher::new());
        let restart_state: Arc<RwLock<HashMap<u64, GroupRestartState>>> =
            Arc::new(RwLock::new(HashMap::new()));

        MultiRaftNode::create_group_inner(
            GROUP_ID,
            true,
            Some("127.0.0.1:19001"),
            &groups,
            &storages,
            &dispatcher,
            &cfg,
            &net_factory,
        )
        .await;
        assert!(groups.read().contains_key(&GROUP_ID), "group should be created");

        let node = groups.read().get(&GROUP_ID).cloned().unwrap();
        wait_for(std::time::Duration::from_secs(5), || {
            node.raft().metrics().borrow().current_leader == Some(1)
        })
        .await;

        node.propose(Request::Put {
            key: b"k1".to_vec(),
            value: b"v1".to_vec(),
        })
        .await
        .expect("initial write before fault injection must succeed");

        // 制造一次真实的存储故障: 直接关闭底层 DB, 让接下来 PutConditional 的
        // dedup 读 (db.get) 失败, 从而在 apply 路径上产生一次 fail-fast 错误.
        {
            let storages_read = storages.read();
            storages_read.get(&GROUP_ID).unwrap().db().close().unwrap();
        }
        let propose_err = node
            .propose(Request::PutConditional {
                key: b"k2".to_vec(),
                value: b"v2".to_vec(),
            })
            .await;
        assert!(
            propose_err.is_err(),
            "propose against a closed underlying db must surface as an error"
        );

        wait_for(std::time::Duration::from_secs(5), || {
            node.raft().metrics().borrow().running_state.is_err()
        })
        .await;

        // `node` is the only extra strong ref outside of `groups`/`storages`;
        // the underlying DB's file lock won't release for a real reopen until
        // it (and its embedded `Arc<DB>`) is dropped, exactly like production
        // where nothing but the group maps hold onto a fatal `OpenRaftNode`.
        let old_ptr = Arc::as_ptr(&node) as usize;
        drop(node);

        MultiRaftNode::supervise_groups(
            &groups,
            &storages,
            &dispatcher,
            &cfg,
            &net_factory,
            &restart_state,
        )
        .await;

        assert!(
            groups.read().contains_key(&GROUP_ID),
            "group should be reopened in-place after self-heal"
        );
        let node2 = groups.read().get(&GROUP_ID).cloned().unwrap();
        assert_ne!(
            Arc::as_ptr(&node2) as usize,
            old_ptr,
            "self-heal must produce a fresh OpenRaftNode, not reuse the fatal one"
        );
        assert!(
            node2.raft().metrics().borrow().running_state.is_ok(),
            "reopened group must not still be in a Fatal state"
        );

        wait_for(std::time::Duration::from_secs(5), || {
            node2.raft().metrics().borrow().current_leader == Some(1)
        })
        .await;
        assert_eq!(
            node2.get(b"k1".to_vec()).await.unwrap(),
            Some(b"v1".to_vec()),
            "data committed before the fault must survive the in-process restart"
        );
    }

    async fn wait_for<F: Fn() -> bool>(timeout: std::time::Duration, cond: F) {
        let deadline = tokio::time::Instant::now() + timeout;
        while !cond() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "condition did not become true within {timeout:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
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
