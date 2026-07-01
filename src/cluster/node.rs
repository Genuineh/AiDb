//! OpenRaft node wrapper.

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

use openraft::{storage::Adaptor, Config, Raft, RaftMetrics};
use tracing::instrument;

use crate::cluster::network::{RaftNetworkClientFactory, RaftServiceDispatcher, RaftServiceImpl};
use crate::cluster::storage::OpenRaftStorage;
use crate::cluster::types::{
    ClusterError, NodeId, RaftNodeConfig, Request, Response, ThinWriteBatch, TypeConfig,
};
use crate::error::{Error, Result};
use crate::DB;

/// 复制屏障超时常量.
const CATCH_UP_TIMEOUT: Duration = Duration::from_secs(30);
const CATCH_UP_POLL: Duration = Duration::from_millis(50);
const CATCH_UP_THRESHOLD: u64 = 5;
const REPLICATION_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
const REPLICATION_POLL: Duration = Duration::from_millis(50);
const REPLICATION_HEARTBEAT_MULTIPLIER: u32 = 3;

pub struct OpenRaftNode {
    node_id: NodeId,
    group_id: u64,
    raft: Arc<Raft<TypeConfig>>,
    storage: OpenRaftStorage,
    network_factory: Arc<RwLock<RaftNetworkClientFactory>>,
    max_entry_size: u64,
    heartbeat_interval_ms: u64,
}

impl OpenRaftNode {
    pub async fn new(
        config: RaftNodeConfig,
        db: Arc<DB>,
        network_factory: RaftNetworkClientFactory,
    ) -> Result<Self> {
        config.validate()?;

        if !db.use_wal() {
            return Err(Error::Cluster(ClusterError::InvalidConfig(
                "raft mode requires use_wal=true".into(),
            )));
        }

        let storage = OpenRaftStorage::new(db.clone(), config.group_id, None)?;
        Self::new_with_storage(config, db, storage, network_factory).await
    }

    pub async fn new_with_storage(
        config: RaftNodeConfig,
        db: Arc<DB>,
        storage: OpenRaftStorage,
        network_factory: RaftNetworkClientFactory,
    ) -> Result<Self> {
        let heartbeat_interval_ms = config.heartbeat_interval;
        config.validate()?;

        if !db.use_wal() {
            return Err(Error::Cluster(ClusterError::InvalidConfig(
                "raft mode requires use_wal=true".into(),
            )));
        }

        let (log_store, state_machine) = Adaptor::new(storage.clone());

        let raft_config = Config {
            cluster_name: format!("aidb-raft-{}", config.group_id),
            election_timeout_min: config.election_timeout_min,
            election_timeout_max: config.election_timeout_max,
            heartbeat_interval: config.heartbeat_interval,
            max_payload_entries: config.max_payload_entries,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(
                config.snapshot_logs_since_last,
            ),
            ..Default::default()
        }
        .validate()
        .map_err(|e| Error::Cluster(ClusterError::InvalidConfig(e.to_string())))?;

        let network_factory_arc = Arc::new(RwLock::new(network_factory));
        let network_for_raft = network_factory_arc.read().clone();

        let raft = Raft::new(
            config.node_id,
            Arc::new(raft_config),
            network_for_raft,
            log_store,
            state_machine,
        )
        .await
        .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;

        Ok(Self {
            node_id: config.node_id,
            group_id: config.group_id,
            raft: Arc::new(raft),
            storage,
            network_factory: network_factory_arc,
            max_entry_size: config.max_entry_size,
            heartbeat_interval_ms,
        })
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn group_id(&self) -> u64 {
        self.group_id
    }

    pub async fn initialize(&self, nodes: Vec<(NodeId, String)>) -> Result<()> {
        let mut members = std::collections::BTreeMap::new();
        for (node_id, addr) in nodes {
            members.insert(node_id, openraft::BasicNode { addr: addr.clone() });
            self.network_factory.write().add_node(node_id, addr);
        }
        self.raft
            .initialize(members)
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;
        Ok(())
    }

    pub async fn add_learner(&self, node_id: NodeId, address: String) -> Result<()> {
        self.add_learner_inner(node_id, address, true).await
    }

    /// Add a learner without blocking (non-blocking mode).
    ///
    /// Returns immediately after the membership change is committed —
    /// does NOT wait for the learner to catch up on log replication.
    pub async fn add_learner_nonblocking(&self, node_id: NodeId, address: String) -> Result<()> {
        self.add_learner_inner(node_id, address, false).await
    }

    async fn add_learner_inner(
        &self,
        node_id: NodeId,
        address: String,
        blocking: bool,
    ) -> Result<()> {
        tracing::debug!(
          node_id,
          address = %address,
          group_id = self.group_id,
          blocking,
          "add_learner: registering address and calling openraft",
        );
        self.network_factory
            .write()
            .add_node(node_id, address.clone());
        let result = self
            .raft
            .add_learner(node_id, openraft::BasicNode { addr: address }, blocking)
            .await;
        tracing::debug!(
            node_id,
            group_id = self.group_id,
            success = result.is_ok(),
            "add_learner: completed",
        );
        result.map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;
        Ok(())
    }

    /// Change membership with replication barriers.
    ///
    /// Two barriers guard against the leader crashing between `change_membership`
    /// commit and follower `commit_index` propagation:
    /// 1. Wait for all new members to catch up on log replication
    /// 2. Confirm at least one other voter received the entry + heartbeat wait
    #[instrument(skip(self), fields(group_id = self.group_id, member_count = members.len()))]
    pub async fn change_membership(&self, members: Vec<NodeId>) -> Result<()> {
        let set: std::collections::BTreeSet<NodeId> = members.into_iter().collect();
        let total_start = std::time::Instant::now();

        // ── 屏障 1: 等所有成员追平 ──
        let t0 = std::time::Instant::now();
        self.wait_members_catch_up(&set).await?;
        tracing::info!(
            elapsed_ms = t0.elapsed().as_millis(),
            "change_membership: barrier 1 (catch_up) passed"
        );

        self.raft
            .change_membership(set.clone(), false)
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;

        // ── 屏障 2: 确认 entry 已传播 ──
        let t1 = std::time::Instant::now();
        let voter_ids: Vec<NodeId> = set.iter().copied().collect();
        self.confirm_replication(&voter_ids).await?;
        tracing::info!(
            elapsed_ms = t1.elapsed().as_millis(),
            "change_membership: barrier 2 (replication) passed"
        );

        tracing::info!(
            total_elapsed_ms = total_start.elapsed().as_millis(),
            "change_membership: complete"
        );
        Ok(())
    }

    /// 等待所有成员追平 leader 日志 (屏障 1).
    ///
    /// 每个成员必须同时满足:
    /// 1. 已出现在 replication metrics 中 (leader 已与其建立复制连接)
    /// 2. matched_log_index 与 leader last_log_index 差距不超过阈值
    async fn wait_members_catch_up(
        &self,
        members: &std::collections::BTreeSet<NodeId>,
    ) -> Result<()> {
        let start = tokio::time::Instant::now();
        let deadline = start + CATCH_UP_TIMEOUT;
        loop {
            let metrics = self.metrics().await;
            let leader_last = metrics.last_log_index.unwrap_or(0);
            let all_caught_up = members.iter().all(|id| {
                if *id == self.node_id {
                    return true; // 跳过自己
                }
                let matched = Self::matched_log_index(&metrics, *id);
                let connected = Self::is_connected(&metrics, *id);
                // 必须已连接且日志追平 (connected 保证 learner 已加入复制流,
                // 避免 last_log_index=0 时 barrier 立即通过)
                connected && leader_last.saturating_sub(matched) <= CATCH_UP_THRESHOLD
            });

            if all_caught_up {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    ?members,
                    leader_last,
                    elapsed_ms = start.elapsed().as_millis(),
                    "members catch-up timed out"
                );
                return Err(Error::Cluster(ClusterError::Timeout(
                    "members failed to catch up within 30s".into(),
                )));
            }
            tokio::time::sleep(CATCH_UP_POLL).await;
        }
    }

    /// 确认 membership change entry 已传播到至少一个其他 voter (屏障 2).
    async fn confirm_replication(&self, voter_ids: &[NodeId]) -> Result<()> {
        // 快速路径: 只有本节点一个 voter, entry 已本地 committed
        if voter_ids.len() <= 1 {
            tracing::debug!("single voter, replication confirmation skipped");
            return Ok(());
        }

        // Step 1: 确认至少一个其他 voter 收到 entry
        let last_log = self.metrics().await.last_log_index.unwrap_or(0);
        let deadline = tokio::time::Instant::now() + REPLICATION_CONFIRM_TIMEOUT;

        loop {
            let metrics = self.metrics().await;
            let confirmed_voter = voter_ids.iter().find(|id| {
                **id != self.node_id() && Self::matched_log_index(&metrics, **id) >= last_log
            });
            if let Some(voter_id) = confirmed_voter {
                tracing::debug!(
                  confirmed_voter = %voter_id,
                  last_log_index = last_log,
                  "replication confirmed, waiting for commit propagation"
                );
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Cluster(ClusterError::Timeout(
                    "no voter confirmed replication within 5s".into(),
                )));
            }
            tokio::time::sleep(REPLICATION_POLL).await;
        }

        // Step 2: 等 3 个 heartbeat 让 commit_index 传播
        tokio::time::sleep(Duration::from_millis(
            REPLICATION_HEARTBEAT_MULTIPLIER as u64 * self.heartbeat_interval_ms,
        ))
        .await;
        Ok(())
    }

    /// 从 RaftMetrics 提取指定节点的 matched log index.
    fn matched_log_index(
        metrics: &RaftMetrics<NodeId, openraft::BasicNode>,
        node_id: NodeId,
    ) -> u64 {
        metrics
            .replication
            .as_ref()
            .and_then(|r| r.get(&node_id))
            .and_then(|v| v.as_ref())
            .map(|log_id| log_id.index)
            .unwrap_or(0)
    }

    /// 检查节点是否已出现在 replication metrics 中 (leader 已与其建立复制连接).
    fn is_connected(metrics: &RaftMetrics<NodeId, openraft::BasicNode>, node_id: NodeId) -> bool {
        metrics
            .replication
            .as_ref()
            .map(|r| r.contains_key(&node_id))
            .unwrap_or(false)
    }

    fn check_entry_size(&self, request: &Request) -> Result<()> {
        let size = rmp_serde::to_vec(request)
            .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?
            .len() as u64;
        if size > self.max_entry_size {
            return Err(Error::Cluster(ClusterError::InvalidConfig(format!(
                "entry size {size} exceeds max_entry_size {}",
                self.max_entry_size
            ))));
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn propose(&self, request: Request) -> Result<Response> {
        let t0 = std::time::Instant::now();
        self.check_entry_size(&request)?;
        // Retry on ForwardToLeader — during leader re-election, the first
        // forward target may itself be in transition.  Up to 3 retries with
        // 200ms backoff.
        for attempt in 0u32..3 {
            let t1 = std::time::Instant::now();
            match self.raft.client_write(request.clone()).await {
                Ok(response) => {
                    let elapsed = t0.elapsed();
                    tracing::info!(target: "perf", group_id = self.group_id, total_ms = elapsed.as_millis(), client_write_ms = t1.elapsed().as_millis(), attempt, "raft_propose_ok");
                    return Ok(response.data);
                }
                Err(e) => {
                    if let Some(ftl) = e.forward_to_leader() {
                        let leader_addr = ftl.leader_node.as_ref().map(|n| n.addr.clone());
                        // Register the leader address so the network factory can reach it.
                        if let (Some(leader_id), Some(ref addr)) = (ftl.leader_id, &leader_addr) {
                            self.network_factory
                                .write()
                                .add_node(leader_id, addr.clone());
                        }
                        if attempt < 2 {
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            continue;
                        }
                        return Err(Error::Cluster(ClusterError::NotLeader {
                            leader: ftl.leader_id,
                            leader_addr,
                            is_ask: false,
                        }));
                    }
                    return Err(Error::Cluster(ClusterError::Raft(e.to_string())));
                }
            }
        }
        unreachable!()
    }

    pub async fn write_batch(&self, batch: ThinWriteBatch) -> Result<()> {
        match self.propose(Request::WriteBatch(batch)).await? {
            Response::Ok => Ok(()),
            Response::Error(msg) => Err(Error::Cluster(ClusterError::Internal(msg))),
            _ => Err(Error::Cluster(ClusterError::Internal(
                "unexpected write_batch response".into(),
            ))),
        }
    }

    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let mut batch = ThinWriteBatch::new();
        batch.put(key, value);
        self.write_batch(batch).await
    }

    pub async fn delete(&self, key: Vec<u8>) -> Result<()> {
        let mut batch = ThinWriteBatch::new();
        batch.delete(key);
        self.write_batch(batch).await
    }

    pub async fn is_leader(&self) -> bool {
        self.raft.metrics().borrow().current_leader == Some(self.node_id)
    }

    pub async fn get_leader(&self) -> Option<NodeId> {
        self.raft.metrics().borrow().current_leader
    }

    pub async fn metrics(&self) -> RaftMetrics<NodeId, openraft::BasicNode> {
        self.raft.metrics().borrow().clone()
    }

    /// 获取当前 Raft group 的成员节点集合, 用于 LifecycleManager 对账.
    pub async fn get_members(&self) -> std::collections::BTreeSet<NodeId> {
        self.raft
            .metrics()
            .borrow()
            .membership_config
            .nodes()
            .map(|(nid, _)| *nid)
            .collect()
    }

    pub fn raft(&self) -> Arc<Raft<TypeConfig>> {
        self.raft.clone()
    }

    pub async fn get(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>> {
        if !self.is_leader().await {
            let leader = self.get_leader().await;
            return Err(Error::Cluster(ClusterError::NotLeader {
                leader,
                leader_addr: None,
                is_ask: false,
            }));
        }
        let storage = self.storage.clone();
        tokio::task::spawn_blocking(move || storage.get_state_machine_value(&key))
            .await
            .map_err(|e| Error::Cluster(ClusterError::Internal(e.to_string())))?
    }

    pub async fn start_server(
        &self,
        addr: std::net::SocketAddr,
        max_message_size: u64,
    ) -> Result<()> {
        let dispatcher = Arc::new(RaftServiceDispatcher::new());
        dispatcher.register_group(self.group_id, self.raft.clone());
        self.start_server_with_dispatcher(addr, max_message_size, dispatcher)
            .await
    }

    /// 启动 gRPC server, 使用外部共享的 dispatcher.
    ///
    /// 共享 dispatcher 允许同一个端口路由多个 group 的 Raft 消息.
    /// MetaRaft gRPC server 使用此方法共享 MultiRaft 的 dispatcher,
    /// 使得 `add_learner_to_group` 等操作即使发到 MetaRaft 端口也能正确路由.
    pub async fn start_server_with_dispatcher(
        &self,
        addr: std::net::SocketAddr,
        max_message_size: u64,
        dispatcher: Arc<RaftServiceDispatcher>,
    ) -> Result<()> {
        use raft_rpc::raft_service_server::RaftServiceServer;
        use tokio::net::TcpListener;
        use tokio_stream::wrappers::TcpListenerStream;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Cluster(ClusterError::Io(e)))?;
        // 注册本 group 到共享 dispatcher (如果尚未注册).
        dispatcher.register_group(self.group_id, self.raft.clone());
        let service = RaftServiceImpl::new(dispatcher);
        let server = RaftServiceServer::new(service)
            .max_decoding_message_size(max_message_size as usize)
            .max_encoding_message_size(max_message_size as usize);

        tonic::transport::Server::builder()
            .add_service(server)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.raft
            .shutdown()
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;
        Ok(())
    }

    pub fn storage(&self) -> &OpenRaftStorage {
        &self.storage
    }

    pub fn add_node_address(&self, node_id: NodeId, address: String) {
        self.network_factory.write().add_node(node_id, address);
    }
}

#[cfg(feature = "cluster")]
use crate::cluster::network::raft_rpc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::storage::DEFAULT_GROUP_ID;
    use crate::config::Options;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_raft_node_creation() {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let factory = RaftNetworkClientFactory::new(1, DEFAULT_GROUP_ID, 30, 65 * 1024 * 1024);
        let config = RaftNodeConfig::default();
        assert!(OpenRaftNode::new(config, db, factory).await.is_ok());
    }

    #[tokio::test]
    async fn test_empty_cluster_proposal() {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let factory = RaftNetworkClientFactory::new(1, DEFAULT_GROUP_ID, 30, 65 * 1024 * 1024);
        let node = OpenRaftNode::new(RaftNodeConfig::default(), db, factory)
            .await
            .unwrap();
        let err = node.put(b"k".to_vec(), b"v".to_vec()).await.unwrap_err();
        // Empty cluster has no leader, so propose returns NotLeader error.
        assert!(matches!(
            err,
            Error::Cluster(ClusterError::NotLeader {
                leader: None,
                leader_addr: None,
                is_ask: false,
            })
        ));
    }

    #[tokio::test]
    async fn test_max_entry_size_rejection() {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let factory = RaftNetworkClientFactory::new(1, DEFAULT_GROUP_ID, 30, 65 * 1024 * 1024);
        let config = RaftNodeConfig {
            max_entry_size: 8,
            ..Default::default()
        };
        let node = OpenRaftNode::new(config, db, factory).await.unwrap();
        let err = node
            .put(b"longkey".to_vec(), b"longvalue".to_vec())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            Error::Cluster(ClusterError::InvalidConfig(_))
        ));
    }

    #[tokio::test]
    async fn test_use_wal_required() {
        let dir = TempDir::new().unwrap();
        let mut opts = Options::for_testing();
        opts.use_wal = false;
        let db = DB::open(dir.path(), opts).unwrap();
        let factory = RaftNetworkClientFactory::new(1, DEFAULT_GROUP_ID, 30, 65 * 1024 * 1024);
        let result = OpenRaftNode::new(RaftNodeConfig::default(), db, factory).await;
        assert!(matches!(
            result,
            Err(Error::Cluster(ClusterError::InvalidConfig(_)))
        ));
    }
}
