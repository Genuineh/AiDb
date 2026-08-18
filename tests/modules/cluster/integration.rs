//! @component aidb-cluster

use std::time::Duration;

use tokio::time::sleep;

use super::harness::ClusterTestHarness;

#[tokio::test]
async fn test_three_node_cluster_formation() {
    let harness = ClusterTestHarness::new_3node().await;
    harness.bootstrap().await;
    assert!(harness.wait_leader().await.is_some());
    harness.shutdown_all().await;
}

#[tokio::test]
async fn test_log_replication() {
    let harness = ClusterTestHarness::new_3node().await;
    harness.bootstrap().await;
    let leader = harness.leader().await;
    leader
        .put(b"key1".to_vec(), b"value1".to_vec())
        .await
        .unwrap();
    sleep(Duration::from_millis(500)).await;

    for node in &harness.nodes {
        assert_eq!(
            node.storage().get_state_machine_value(b"key1").unwrap(),
            Some(b"value1".to_vec()),
            "node {} sm must match leader write",
            node.node_id()
        );
    }
    harness.shutdown_all().await;
}

#[tokio::test]
async fn test_leader_failover() {
    let harness = ClusterTestHarness::new_3node().await;
    harness.bootstrap().await;
    let leader = harness.leader().await;
    leader.put(b"k".to_vec(), b"v".to_vec()).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    leader.shutdown().await.unwrap();

    sleep(Duration::from_secs(2)).await;
    let new_leader = harness.leader().await;
    assert_eq!(
        new_leader.get(b"k".to_vec()).await.unwrap(),
        Some(b"v".to_vec())
    );
    harness.shutdown_all().await;
}

#[tokio::test]
async fn test_raft_node_start_stop() {
    let harness = ClusterTestHarness::new_3node().await;
    harness.bootstrap().await;
    harness.shutdown_all().await;
}
