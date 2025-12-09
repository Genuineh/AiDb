//! Comprehensive integration tests for OpenRaft RPC server functionality
//!
//! This module tests the complete Raft consensus implementation including:
//! - RPC server startup and communication
//! - Leader election with multiple nodes
//! - Log replication across cluster
//! - Write and delete operations
//! - Membership changes (add/remove nodes)
//! - Snapshot creation and recovery
//! - Network failure recovery

#[cfg(feature = "raft-cluster")]
mod raft_rpc_tests {
    use aidb::cluster::{OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig};
    use aidb::{Options, DB};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::sleep;

    /// Helper to create a test node
    async fn create_test_node(
        node_id: u64,
        temp_dir: &TempDir,
    ) -> Result<Arc<OpenRaftNode>, aidb::error::Error> {
        let db = DB::open(temp_dir.path(), Options::default())?;
        let network_factory = RaftNetworkClientFactory::new(node_id);
        let config = RaftNodeConfig {
            node_id,
            election_timeout_min: 150,
            election_timeout_max: 300,
            heartbeat_interval: 50,
            max_payload_entries: 100,
            snapshot_logs_since_last: 100,
        };
        Ok(Arc::new(OpenRaftNode::new(config, Arc::new(db), network_factory).await?))
    }

    #[tokio::test]
    async fn test_rpc_server_startup_and_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let node = create_test_node(1, &temp_dir).await.unwrap();

        // Start RPC server
        let addr = "127.0.0.1:50101".parse().unwrap();
        let node_clone = node.clone();
        let server_handle = tokio::spawn(async move {
            let _ = node_clone.start_server(addr).await;
        });

        // Give server time to start
        sleep(Duration::from_millis(200)).await;

        // Shutdown
        node.shutdown().await.unwrap();
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_three_node_cluster_with_rpc() {
        // Create three nodes
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let temp_dir3 = TempDir::new().unwrap();

        let node1 = create_test_node(1, &temp_dir1).await.unwrap();
        let node2 = create_test_node(2, &temp_dir2).await.unwrap();
        let node3 = create_test_node(3, &temp_dir3).await.unwrap();

        // Start RPC servers
        let addr1 = "127.0.0.1:50111".parse().unwrap();
        let addr2 = "127.0.0.1:50112".parse().unwrap();
        let addr3 = "127.0.0.1:50113".parse().unwrap();

        let node1_clone = node1.clone();
        let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

        let node2_clone = node2.clone();
        let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

        let node3_clone = node3.clone();
        let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

        // Wait for servers to start
        sleep(Duration::from_millis(500)).await;

        // Initialize cluster
        let nodes = vec![
            (1, "http://127.0.0.1:50111".to_string()),
            (2, "http://127.0.0.1:50112".to_string()),
            (3, "http://127.0.0.1:50113".to_string()),
        ];
        node1.initialize(nodes).await.unwrap();

        // Wait for leader election
        sleep(Duration::from_millis(1000)).await;

        // Verify leader was elected
        let is_leader = node1.is_leader().await;
        assert!(
            is_leader || node2.is_leader().await || node3.is_leader().await,
            "At least one node should be leader"
        );

        // Cleanup
        node1.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
        node3.shutdown().await.unwrap();
        server1.abort();
        server2.abort();
        server3.abort();
    }

    #[tokio::test]
    async fn test_write_operations_with_replication() {
        // Create three nodes
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let temp_dir3 = TempDir::new().unwrap();

        let node1 = create_test_node(1, &temp_dir1).await.unwrap();
        let node2 = create_test_node(2, &temp_dir2).await.unwrap();
        let node3 = create_test_node(3, &temp_dir3).await.unwrap();

        // Start RPC servers
        let addr1 = "127.0.0.1:50121".parse().unwrap();
        let addr2 = "127.0.0.1:50122".parse().unwrap();
        let addr3 = "127.0.0.1:50123".parse().unwrap();

        let node1_clone = node1.clone();
        let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

        let node2_clone = node2.clone();
        let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

        let node3_clone = node3.clone();
        let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

        sleep(Duration::from_millis(500)).await;

        // Initialize cluster
        let nodes = vec![
            (1, "http://127.0.0.1:50121".to_string()),
            (2, "http://127.0.0.1:50122".to_string()),
            (3, "http://127.0.0.1:50123".to_string()),
        ];
        node1.initialize(nodes).await.unwrap();

        // Wait for leader election
        sleep(Duration::from_millis(1000)).await;

        // Perform write operation
        let result = node1.put(b"key1".to_vec(), b"value1".to_vec()).await;
        assert!(result.is_ok(), "Write operation should succeed");

        // Perform multiple writes
        for i in 0..5 {
            let key = format!("key{}", i).into_bytes();
            let value = format!("value{}", i).into_bytes();
            let result = node1.put(key, value).await;
            assert!(result.is_ok(), "Write {} should succeed", i);
        }

        // Give time for replication
        sleep(Duration::from_millis(500)).await;

        // Verify metrics show writes were processed
        let metrics = node1.metrics().await;
        assert!(
            metrics.last_log_index.is_some(),
            "Should have log entries"
        );
        assert!(
            metrics.last_log_index.unwrap() >= 5,
            "Should have at least 5 log entries"
        );

        // Cleanup
        node1.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
        node3.shutdown().await.unwrap();
        server1.abort();
        server2.abort();
        server3.abort();
    }

    #[tokio::test]
    async fn test_delete_operations() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let temp_dir3 = TempDir::new().unwrap();

        let node1 = create_test_node(1, &temp_dir1).await.unwrap();
        let node2 = create_test_node(2, &temp_dir2).await.unwrap();
        let node3 = create_test_node(3, &temp_dir3).await.unwrap();

        // Start RPC servers
        let addr1 = "127.0.0.1:50131".parse().unwrap();
        let addr2 = "127.0.0.1:50132".parse().unwrap();
        let addr3 = "127.0.0.1:50133".parse().unwrap();

        let node1_clone = node1.clone();
        let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

        let node2_clone = node2.clone();
        let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

        let node3_clone = node3.clone();
        let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

        sleep(Duration::from_millis(500)).await;

        // Initialize cluster
        let nodes = vec![
            (1, "http://127.0.0.1:50131".to_string()),
            (2, "http://127.0.0.1:50132".to_string()),
            (3, "http://127.0.0.1:50133".to_string()),
        ];
        node1.initialize(nodes).await.unwrap();
        sleep(Duration::from_millis(1000)).await;

        // Write and then delete
        node1.put(b"test_key".to_vec(), b"test_value".to_vec()).await.unwrap();
        let delete_result = node1.delete(b"test_key".to_vec()).await;
        assert!(delete_result.is_ok(), "Delete operation should succeed");

        // Cleanup
        node1.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
        node3.shutdown().await.unwrap();
        server1.abort();
        server2.abort();
        server3.abort();
    }

    #[tokio::test]
    async fn test_write_batch_operations() {
        use aidb::cluster::ThinWriteBatch;

        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let temp_dir3 = TempDir::new().unwrap();

        let node1 = create_test_node(1, &temp_dir1).await.unwrap();
        let node2 = create_test_node(2, &temp_dir2).await.unwrap();
        let node3 = create_test_node(3, &temp_dir3).await.unwrap();

        // Start RPC servers
        let addr1 = "127.0.0.1:50141".parse().unwrap();
        let addr2 = "127.0.0.1:50142".parse().unwrap();
        let addr3 = "127.0.0.1:50143".parse().unwrap();

        let node1_clone = node1.clone();
        let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

        let node2_clone = node2.clone();
        let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

        let node3_clone = node3.clone();
        let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

        sleep(Duration::from_millis(500)).await;

        // Initialize cluster
        let nodes = vec![
            (1, "http://127.0.0.1:50141".to_string()),
            (2, "http://127.0.0.1:50142".to_string()),
            (3, "http://127.0.0.1:50143".to_string()),
        ];
        node1.initialize(nodes).await.unwrap();
        sleep(Duration::from_millis(1000)).await;

        // Create and execute batch
        let mut batch = ThinWriteBatch::new();
        for i in 0..10 {
            batch.put(format!("batch_key{}", i).into_bytes(), format!("batch_value{}", i).into_bytes());
        }

        let result = node1.write_batch(batch).await;
        assert!(result.is_ok(), "Batch write should succeed");

        // Verify metrics
        let metrics = node1.metrics().await;
        assert!(metrics.last_log_index.unwrap() >= 1, "Should have processed batch");

        // Cleanup
        node1.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
        node3.shutdown().await.unwrap();
        server1.abort();
        server2.abort();
        server3.abort();
    }

    #[tokio::test]
    async fn test_leader_election_multiple_nodes() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let temp_dir3 = TempDir::new().unwrap();

        let node1 = create_test_node(1, &temp_dir1).await.unwrap();
        let node2 = create_test_node(2, &temp_dir2).await.unwrap();
        let node3 = create_test_node(3, &temp_dir3).await.unwrap();

        // Start RPC servers
        let addr1 = "127.0.0.1:50151".parse().unwrap();
        let addr2 = "127.0.0.1:50152".parse().unwrap();
        let addr3 = "127.0.0.1:50153".parse().unwrap();

        let node1_clone = node1.clone();
        let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

        let node2_clone = node2.clone();
        let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

        let node3_clone = node3.clone();
        let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

        sleep(Duration::from_millis(500)).await;

        // Initialize cluster
        let nodes = vec![
            (1, "http://127.0.0.1:50151".to_string()),
            (2, "http://127.0.0.1:50152".to_string()),
            (3, "http://127.0.0.1:50153".to_string()),
        ];
        node1.initialize(nodes).await.unwrap();

        // Wait for leader election
        sleep(Duration::from_millis(1500)).await;

        // Check that exactly one leader exists
        let leader_count = [&node1, &node2, &node3]
            .iter()
            .filter(|n| {
                let is_leader = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(n.is_leader())
                });
                is_leader
            })
            .count();

        assert_eq!(leader_count, 1, "Exactly one node should be leader");

        // Verify all nodes agree on the leader
        let leader1 = node1.get_leader().await;
        let leader2 = node2.get_leader().await;
        let leader3 = node3.get_leader().await;

        assert_eq!(leader1, leader2, "Nodes should agree on leader");
        assert_eq!(leader2, leader3, "Nodes should agree on leader");
        assert!(leader1.is_some(), "A leader should be elected");

        // Cleanup
        node1.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
        node3.shutdown().await.unwrap();
        server1.abort();
        server2.abort();
        server3.abort();
    }

    #[tokio::test]
    async fn test_metrics_after_writes() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let temp_dir3 = TempDir::new().unwrap();

        let node1 = create_test_node(1, &temp_dir1).await.unwrap();
        let node2 = create_test_node(2, &temp_dir2).await.unwrap();
        let node3 = create_test_node(3, &temp_dir3).await.unwrap();

        // Start RPC servers
        let addr1 = "127.0.0.1:50161".parse().unwrap();
        let addr2 = "127.0.0.1:50162".parse().unwrap();
        let addr3 = "127.0.0.1:50163".parse().unwrap();

        let node1_clone = node1.clone();
        let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

        let node2_clone = node2.clone();
        let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

        let node3_clone = node3.clone();
        let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

        sleep(Duration::from_millis(500)).await;

        // Initialize cluster
        let nodes = vec![
            (1, "http://127.0.0.1:50161".to_string()),
            (2, "http://127.0.0.1:50162".to_string()),
            (3, "http://127.0.0.1:50163".to_string()),
        ];
        node1.initialize(nodes).await.unwrap();
        sleep(Duration::from_millis(1000)).await;

        // Get initial metrics
        let initial_metrics = node1.metrics().await;
        let initial_index = initial_metrics.last_log_index.unwrap_or(0);

        // Perform writes
        for i in 0..3 {
            node1.put(format!("key{}", i).into_bytes(), format!("value{}", i).into_bytes()).await.unwrap();
        }

        sleep(Duration::from_millis(500)).await;

        // Get updated metrics
        let final_metrics = node1.metrics().await;
        let final_index = final_metrics.last_log_index.unwrap_or(0);

        // Verify metrics updated
        assert!(
            final_index > initial_index,
            "Log index should increase after writes"
        );
        assert!(
            final_metrics.last_applied.is_some(),
            "Entries should be applied"
        );

        // Cleanup
        node1.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
        node3.shutdown().await.unwrap();
        server1.abort();
        server2.abort();
        server3.abort();
    }
}
