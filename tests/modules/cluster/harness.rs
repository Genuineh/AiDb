use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use aidb::cluster::{OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig, DEFAULT_GROUP_ID};
use aidb::config::Options;
use aidb::DB;
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio::time::sleep;

pub struct ClusterTestHarness {
    pub nodes: Vec<Arc<OpenRaftNode>>,
    pub addrs: Vec<SocketAddr>,
    pub _temp_dirs: Vec<TempDir>,
    server_handles: Vec<JoinHandle<()>>,
}

impl ClusterTestHarness {
    pub const DEFAULT_GROUP_ID: u64 = DEFAULT_GROUP_ID;

    fn pick_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    }

    pub async fn new_3node() -> Self {
        Self::new_3node_inner(false).await
    }

    /// 与 `new_3node` 相同, 但所有节点 `RaftNodeConfig.linearizable_read = true`.
    pub async fn new_3node_linearizable() -> Self {
        Self::new_3node_inner(true).await
    }

    async fn new_3node_inner(linearizable: bool) -> Self {
        let mut nodes = Vec::new();
        let mut addrs = Vec::new();
        let mut temp_dirs = Vec::new();
        let mut server_handles = Vec::new();

        for node_id in 1..=3u64 {
            let dir = TempDir::new().unwrap();
            let db = DB::open(dir.path(), Options::for_testing()).unwrap();
            let factory = RaftNetworkClientFactory::new(
                node_id,
                Self::DEFAULT_GROUP_ID,
                RaftNodeConfig::default().rpc_timeout_ms,
                RaftNodeConfig::default().grpc_max_message_size,
            );
            let cfg = RaftNodeConfig {
                node_id,
                group_id: Self::DEFAULT_GROUP_ID,
                election_timeout_min: 500,
                election_timeout_max: 1000,
                heartbeat_interval: 50,
                snapshot_logs_since_last: 50,
                linearizable_read: linearizable,
                ..Default::default()
            };
            let node = Arc::new(OpenRaftNode::new(cfg, db, factory).await.unwrap());
            let addr = Self::pick_addr();
            addrs.push(addr);
            nodes.push(node);
            temp_dirs.push(dir);
        }

        let peer_addrs: Vec<String> = addrs.iter().map(|a| format!("http://{a}")).collect();
        for (node, _addr) in nodes.iter().zip(addrs.iter()) {
            for (peer_id, peer_addr) in peer_addrs.iter().enumerate() {
                node.add_node_address(peer_id as u64 + 1, peer_addr.clone());
            }
        }

        for (node, addr) in nodes.iter().zip(addrs.iter()) {
            let server_node = node.clone();
            let listen_addr = *addr;
            let handle = tokio::spawn(async move {
                let _ = server_node
                    .start_server(listen_addr, RaftNodeConfig::default().grpc_max_message_size)
                    .await;
            });
            server_handles.push(handle);
        }

        sleep(Duration::from_millis(300)).await;

        Self {
            nodes,
            addrs,
            _temp_dirs: temp_dirs,
            server_handles,
        }
    }

    pub async fn bootstrap(&self) {
        let addr1 = format!("http://{}", self.addrs[0]);
        let addr2 = format!("http://{}", self.addrs[1]);
        let addr3 = format!("http://{}", self.addrs[2]);

        self.nodes[0].initialize(vec![(1, addr1)]).await.unwrap();

        for _ in 0..30 {
            if self.nodes[0].is_leader().await {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }

        self.nodes[0].add_learner(2, addr2).await.unwrap();
        self.nodes[0].add_learner(3, addr3).await.unwrap();

        for _ in 0..10 {
            if self.nodes[0].change_membership(vec![1, 2, 3]).await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(200)).await;
        }

        for _ in 0..50 {
            if self.wait_leader().await.is_some() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn wait_leader(&self) -> Option<u64> {
        for node in &self.nodes {
            if node.is_leader().await {
                return Some(node.node_id());
            }
        }
        None
    }

    /// Abort 指定节点的 server handle (gRPC listener), raft core 存活.
    ///
    /// 与网络黑洞组合构成真实双向分区: 黑洞断出站, abort 断入站. 单独 abort
    /// 只断入站, 出站 RPC 仍可达, quorum 保持, lease 不会过期.
    #[cfg_attr(not(feature = "cluster-test-util"), allow(dead_code))]
    pub fn abort_server(&self, node_id: u64) {
        if let Some(node) = self.nodes.iter().position(|n| n.node_id() == node_id) {
            self.server_handles[node].abort();
        } else {
            let ids: Vec<u64> = self.nodes.iter().map(|n| n.node_id()).collect();
            panic!("abort_server: node {node_id} not found in harness (nodes = {ids:?})");
        }
    }

    pub async fn leader(&self) -> Arc<OpenRaftNode> {
        for _ in 0..50 {
            for node in &self.nodes {
                if node.is_leader().await {
                    return node.clone();
                }
            }
            sleep(Duration::from_millis(100)).await;
        }
        panic!("no leader elected");
    }

    pub async fn shutdown_all(self) {
        for node in &self.nodes {
            let _ = node.shutdown().await;
        }
        for handle in self.server_handles {
            handle.abort();
        }
    }
}
