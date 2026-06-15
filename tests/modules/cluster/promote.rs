#![cfg(feature = "cluster")]

//! MetaRaft promote 集成测试.
//!
//! 验证 `promote_learner_to_voter` 的完整链路:
//! add_learner → register → promote → NodeRole::Voter.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use aidb::cluster::meta_raft_node::MetaRaftNode;
use aidb::cluster::meta_types::{MetaRequest, NodeRole, METARAFT_GROUP_ID};
use aidb::cluster::network::RaftNetworkClientFactory;
use aidb::cluster::types::RaftNodeConfig;
use aidb::config::Options;
use aidb::DB;

/// 分配可用端口.
fn pick_addr() -> SocketAddr {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  listener.local_addr().unwrap()
}

/// 后台启动 gRPC server.
fn spawn_server(node: Arc<MetaRaftNode>, addr: SocketAddr, max_msg_size: u64) -> JoinHandle<()> {
  tokio::spawn(async move {
    let _ = node.start_server(addr, max_msg_size).await;
  })
}

/// 双节点: N1 leader + N2 learner → promote → N2 becomes Voter.
#[tokio::test]
async fn test_promote_single_learner_success() {
  let mut _server_handles: Vec<JoinHandle<()>> = Vec::new();

  // ---- Arrange: 启动 N1 (leader) ----
  let dir1 = TempDir::new().unwrap();
  let db1 = DB::open(dir1.path(), Options::for_testing()).unwrap();

  let factory1 = RaftNetworkClientFactory::new(
    1,
    METARAFT_GROUP_ID,
    RaftNodeConfig::default().rpc_timeout_ms,
    RaftNodeConfig::default().grpc_max_message_size,
  );
  let cfg1 = RaftNodeConfig {
    node_id: 1,
    group_id: METARAFT_GROUP_ID,
    election_timeout_min: 500,
    election_timeout_max: 1000,
    heartbeat_interval: 50,
    ..Default::default()
  };
  let n1 = Arc::new(MetaRaftNode::new(cfg1, db1, factory1).await.unwrap());

  let n1_addr = pick_addr();
  n1.initialize(vec![(1, format!("http://{n1_addr}"))])
    .await
    .unwrap();

  _server_handles.push(spawn_server(Arc::clone(&n1), n1_addr, 64 * 1024 * 1024));

  // 等待 leader 就绪
  for _ in 0..30 {
    if n1.is_leader().await {
      break;
    }
    sleep(Duration::from_millis(100)).await;
  }
  assert!(n1.is_leader().await, "N1 should be leader after initialize");

  // ---- Arrange: 启动 N2 (learner) ----
  let dir2 = TempDir::new().unwrap();
  let db2 = DB::open(dir2.path(), Options::for_testing()).unwrap();

  let factory2 = RaftNetworkClientFactory::new(
    2,
    METARAFT_GROUP_ID,
    RaftNodeConfig::default().rpc_timeout_ms,
    RaftNodeConfig::default().grpc_max_message_size,
  );
  let cfg2 = RaftNodeConfig {
    node_id: 2,
    group_id: METARAFT_GROUP_ID,
    election_timeout_min: 500,
    election_timeout_max: 1000,
    heartbeat_interval: 50,
    ..Default::default()
  };
  let n2 = Arc::new(MetaRaftNode::new(cfg2, db2, factory2).await.unwrap());

  // 注册 leader 地址并启动 N2 server
  n2.add_node_address(1, format!("http://{n1_addr}"));
  let n2_addr = pick_addr();
  _server_handles.push(spawn_server(Arc::clone(&n2), n2_addr, 64 * 1024 * 1024));
  sleep(Duration::from_millis(300)).await;

  // ---- Act: N1 adds N2 as learner + promote to voter ----
  let n2_addr_str = format!("http://{n2_addr}");

  // Step 1: 在 Raft 层添加 N2 为 learner (non-voter)
  n1.add_learner_nonblocking(2, n2_addr_str.clone())
    .await
    .unwrap();

  // Step 2: 在 ClusterMeta 中注册 N2 (promote 内部的 ChangeNodeRole 要求节点已存在)
  n1.propose(MetaRequest::RegisterNode {
    node_id: 2,
    rpc_addr: n2_addr_str,
    client_addr: None,
    tags: HashMap::new(),
  })
  .await
  .unwrap();

  // Step 3: promote_learner_to_voter 内含两道复制屏障
  tokio::time::timeout(Duration::from_secs(40), n1.promote_learner_to_voter(2))
    .await
    .expect("promote timed out")
    .expect("promote failed");

  // ---- Assert: N2 is now Voter in ClusterMeta ----
  let meta = n1.get_cluster_meta();
  let n2_info = meta.nodes.get(&2).expect("N2 not in cluster meta");
  assert_eq!(n2_info.role, NodeRole::Voter);

  // ---- Cleanup ----
  let _ = n2.shutdown().await;
  let _ = n1.shutdown().await;
  for handle in _server_handles {
    handle.abort();
  }
}
