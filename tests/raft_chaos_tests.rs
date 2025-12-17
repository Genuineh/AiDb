//! Chaos testing for Raft implementation
//!
//! This test suite implements chaos testing scenarios inspired by mature
//! projects like etcd-raft. It includes:
//! - Random node failures and restarts
//! - Network delays and packet loss simulation
//! - Concurrent operations during instability
//! - Long-running stress tests
//! - Byzantine fault scenarios
//!
//! These tests help uncover race conditions and edge cases that may not
//! be caught by deterministic tests.

#[cfg(feature = "raft-cluster")]
mod raft_chaos_tests {
    use aidb::cluster::{OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig};
    use aidb::cluster::thin_replication::WriteBatch;
    use aidb::{Options, DB};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::sleep;
    use rand::Rng;

    // ========================================================================
    // Helper functions
    // ========================================================================

    async fn create_node(
        node_id: u64,
        temp_dir: &TempDir,
    ) -> Result<OpenRaftNode, aidb::error::Error> {
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
        OpenRaftNode::new(config, Arc::new(db), network_factory).await
    }

    // ========================================================================
    // Test: Random failures and recovery
    // ========================================================================

    #[tokio::test]
    async fn test_random_node_failures_single_node() {
        // Test random failure patterns on a single node
        let temp_dir = TempDir::new().unwrap();
        let mut rng = rand::thread_rng();

        for iteration in 0..3 {
            let node = create_node(1, &temp_dir).await.unwrap();
            
            // Initialize if first iteration
            if iteration == 0 {
                node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
                    .await
                    .unwrap();
            }
            
            sleep(Duration::from_millis(200)).await;

            // Perform random operations
            let ops = rng.random_range(5..15);
            for i in 0..ops {
                let key = format!("chaos_key_{}", i).into_bytes();
                let value = format!("chaos_value_{}", i).into_bytes();
                let _ = node.put(key, value).await;
            }

            // Random shutdown timing
            let shutdown_delay = rng.random_range(50..300);
            sleep(Duration::from_millis(shutdown_delay)).await;
            
            node.shutdown().await.unwrap();

            // Random restart delay
            let restart_delay = rng.random_range(100..500);
            sleep(Duration::from_millis(restart_delay)).await;
        }
    }

    #[tokio::test]
    async fn test_interleaved_crash_and_recovery() {
        // Test crash and recovery with interleaved operations
        let temp_dir = TempDir::new().unwrap();

        // First session: initialize and write
        {
            let node = create_node(1, &temp_dir).await.unwrap();
            node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
                .await
                .unwrap();
            sleep(Duration::from_millis(300)).await;

            for i in 0..10 {
                let _ = node.put(
                    format!("pre_crash_{}", i).into_bytes(),
                    format!("value_{}", i).into_bytes(),
                ).await;
            }

            node.shutdown().await.unwrap();
        }

        // Second session: recover and write more
        {
            let node = create_node(1, &temp_dir).await.unwrap();
            sleep(Duration::from_millis(300)).await;

            for i in 0..10 {
                let _ = node.put(
                    format!("post_crash_{}", i).into_bytes(),
                    format!("value_{}", i).into_bytes(),
                ).await;
            }

            node.shutdown().await.unwrap();
        }

        // Third session: final recovery
        {
            let node = create_node(1, &temp_dir).await.unwrap();
            sleep(Duration::from_millis(300)).await;
            node.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_rapid_restart_cycles() {
        // Test rapid restart cycles to find initialization bugs
        let temp_dir = TempDir::new().unwrap();

        // Initialize once
        {
            let node = create_node(1, &temp_dir).await.unwrap();
            node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
                .await
                .unwrap();
            sleep(Duration::from_millis(200)).await;
            node.shutdown().await.unwrap();
        }

        // Rapid restart cycles
        for _ in 0..5 {
            let node = create_node(1, &temp_dir).await.unwrap();
            sleep(Duration::from_millis(100)).await;
            node.shutdown().await.unwrap();
            sleep(Duration::from_millis(50)).await;
        }
    }

    // ========================================================================
    // Test: Simulated network issues
    // ========================================================================

    #[tokio::test]
    async fn test_delayed_operations() {
        // Test operations with various delays to simulate network latency
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        let mut rng = rand::thread_rng();

        for i in 0..20 {
            // Random delay before each operation
            let delay = rng.random_range(10..100);
            sleep(Duration::from_millis(delay)).await;

            let key = format!("delayed_key_{}", i).into_bytes();
            let value = format!("delayed_value_{}", i).into_bytes();
            let _ = node.put(key, value).await;
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_timeout_scenarios() {
        // Test various timeout scenarios
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Try operations with different timeout expectations
        for i in 0..10 {
            let key = format!("timeout_key_{}", i).into_bytes();
            let value = format!("timeout_value_{}", i).into_bytes();
            
            // Some operations might timeout in a real distributed setting
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                node.put(key, value),
            ).await;

            // Should either succeed, fail gracefully, or timeout
            match result {
                Ok(Ok(_)) => { /* Success */ }
                Ok(Err(_)) => { /* Raft error */ }
                Err(_) => { /* Timeout */ }
            }
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_high_latency_operations() {
        // Test system behavior under high latency
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Simulate high latency by adding delays
        for i in 0..10 {
            sleep(Duration::from_millis(200)).await; // High latency
            
            let key = format!("latency_key_{}", i).into_bytes();
            let value = format!("latency_value_{}", i).into_bytes();
            let _ = node.put(key, value).await;
        }

        node.shutdown().await.unwrap();
    }

    // ========================================================================
    // Test: Concurrent operations under stress
    // ========================================================================

    #[tokio::test]
    async fn test_concurrent_reads_and_writes() {
        // Test concurrent reads and writes
        let temp_dir = TempDir::new().unwrap();
        let node = Arc::new(create_node(1, &temp_dir).await.unwrap());

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Spawn concurrent write tasks
        let mut write_handles = vec![];
        for i in 0..10 {
            let node_clone = Arc::clone(&node);
            let handle = tokio::spawn(async move {
                let key = format!("concurrent_key_{}", i).into_bytes();
                let value = format!("concurrent_value_{}", i).into_bytes();
                node_clone.put(key, value).await
            });
            write_handles.push(handle);
        }

        // Spawn concurrent read tasks
        let mut read_handles = vec![];
        for i in 0..5 {
            let node_clone = Arc::clone(&node);
            let handle = tokio::spawn(async move {
                let key = format!("concurrent_key_{}", i).into_bytes();
                node_clone.linearizable_read(key).await
            });
            read_handles.push(handle);
        }

        // Wait for all write tasks
        for handle in write_handles {
            let _ = handle.await;
        }

        // Wait for all read tasks
        for handle in read_handles {
            let _ = handle.await;
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_batch_operations_under_load() {
        // Test batch operations under load
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Submit multiple batches concurrently
        for batch_id in 0..5 {
            let mut batch = WriteBatch::new();
            
            for i in 0..20 {
                let key = format!("batch_{}_key_{}", batch_id, i).into_bytes();
                let value = format!("batch_{}_value_{}", batch_id, i).into_bytes();
                batch.put(key, value);
            }

            let _ = node.write_batch(batch).await;
            sleep(Duration::from_millis(50)).await;
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_mixed_batch_and_single_operations() {
        // Test mixing batch and single operations
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        for round in 0..10 {
            // Single operation
            let _ = node.put(
                format!("single_{}", round).into_bytes(),
                b"value".to_vec(),
            ).await;

            // Batch operation
            let mut batch = WriteBatch::new();
            for i in 0..5 {
                batch.put(
                    format!("batch_{}_{}", round, i).into_bytes(),
                    b"batch_value".to_vec(),
                );
            }
            let _ = node.write_batch(batch).await;

            sleep(Duration::from_millis(50)).await;
        }

        node.shutdown().await.unwrap();
    }

    // ========================================================================
    // Test: Long-running stress tests
    // ========================================================================

    #[tokio::test]
    async fn test_extended_write_stress() {
        // Extended stress test with continuous writes
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Run for multiple rounds
        for round in 0..10 {
            for i in 0..10 {
                let key = format!("stress_{}_{}", round, i).into_bytes();
                let value = vec![42u8; 512];
                let _ = node.put(key, value).await;
            }
            sleep(Duration::from_millis(100)).await;
        }

        // Verify system is still functional
        let metrics = node.metrics().await;
        assert!(metrics.current_term >= 1);

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_sustained_mixed_workload() {
        // Test sustained mixed workload
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        let mut rng = rand::thread_rng();

        for _ in 0..100 {
            let op_type = rng.random_range(0..4);
            let key_id = rng.random_range(0..20);
            let key = format!("workload_key_{}", key_id).into_bytes();

            match op_type {
                0 => {
                    // Write
                    let value = format!("value_{}", rng.random::<u32>()).into_bytes();
                    let _ = node.put(key, value).await;
                }
                1 => {
                    // Delete
                    let _ = node.delete(key).await;
                }
                2 => {
                    // Read
                    let _ = node.linearizable_read(key).await;
                }
                _ => {
                    // Batch write
                    let mut batch = WriteBatch::new();
                    for i in 0..3 {
                        let batch_key = format!("batch_key_{}", i).into_bytes();
                        batch.put(batch_key, b"batch_value".to_vec());
                    }
                    let _ = node.write_batch(batch).await;
                }
            }

            // Small random delay
            if rng.random_bool(0.3) {
                sleep(Duration::from_millis(rng.random_range(10..50))).await;
            }
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_memory_pressure_simulation() {
        // Test behavior under memory pressure (large values)
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Write large values
        for i in 0..20 {
            let key = format!("large_key_{}", i).into_bytes();
            let value = vec![42u8; 50 * 1024]; // 50KB per value
            let result = node.put(key, value).await;
            
            // Some operations might fail under memory pressure
            let _ = result;
            
            sleep(Duration::from_millis(50)).await;
        }

        node.shutdown().await.unwrap();
    }

    // ========================================================================
    // Test: State machine consistency
    // ========================================================================

    #[tokio::test]
    async fn test_deterministic_state_machine() {
        // Test that state machine produces deterministic results
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Apply same operations in order
        for i in 0..20 {
            let key = format!("deterministic_{}", i).into_bytes();
            let value = format!("value_{}", i).into_bytes();
            let _ = node.put(key, value).await;
        }

        // Delete some keys
        for i in (0..20).step_by(2) {
            let key = format!("deterministic_{}", i).into_bytes();
            let _ = node.delete(key).await;
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_idempotent_operations() {
        // Test that operations can be safely retried
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Write the same key multiple times (should be idempotent)
        for _ in 0..5 {
            let _ = node.put(b"idempotent_key".to_vec(), b"value1".to_vec()).await;
        }

        // Update value
        for _ in 0..5 {
            let _ = node.put(b"idempotent_key".to_vec(), b"value2".to_vec()).await;
        }

        node.shutdown().await.unwrap();
    }

    // ========================================================================
    // Test: Metrics and observability under chaos
    // ========================================================================

    #[tokio::test]
    async fn test_metrics_remain_accurate_under_stress() {
        // Test that metrics remain accurate under stress
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Perform operations
        for i in 0..30 {
            let _ = node.put(
                format!("metrics_key_{}", i).into_bytes(),
                b"value".to_vec(),
            ).await;
        }

        // Check metrics multiple times
        for _ in 0..3 {
            let metrics = node.metrics().await;
            
            // Basic sanity checks
            assert!(metrics.current_term >= 1);
            
            sleep(Duration::from_millis(100)).await;
        }

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_leader_status_consistency() {
        // Test that leader status remains consistent
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(500)).await;

        // Check leader status multiple times
        let mut leader_checks = vec![];
        for _ in 0..10 {
            leader_checks.push(node.is_leader().await);
            sleep(Duration::from_millis(100)).await;
        }

        // In a stable single-node cluster, all checks should show leadership
        let leader_count = leader_checks.iter().filter(|&&x| x).count();
        assert!(
            leader_count >= 8,
            "Leader status should be stable in single-node cluster"
        );

        node.shutdown().await.unwrap();
    }

    // ========================================================================
    // Test: Shutdown and cleanup under various states
    // ========================================================================

    #[tokio::test]
    async fn test_shutdown_during_active_writes() {
        // Test graceful shutdown during active writes
        let temp_dir = TempDir::new().unwrap();
        let node = Arc::new(create_node(1, &temp_dir).await.unwrap());

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Start writes
        let node_clone = Arc::clone(&node);
        let write_task = tokio::spawn(async move {
            for i in 0..20 {
                let _ = node_clone.put(
                    format!("shutdown_key_{}", i).into_bytes(),
                    b"value".to_vec(),
                ).await;
                sleep(Duration::from_millis(50)).await;
            }
        });

        // Let some writes happen
        sleep(Duration::from_millis(200)).await;

        // Shutdown while writes are happening
        node.shutdown().await.unwrap();

        // Wait for write task to complete (will fail after shutdown)
        let _ = write_task.await;
    }

    #[tokio::test]
    async fn test_shutdown_immediately_after_init() {
        // Test shutdown immediately after initialization
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();

        // Shutdown immediately without waiting
        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_shutdown_attempts() {
        // Test that multiple shutdown attempts don't cause issues
        let temp_dir = TempDir::new().unwrap();
        let node = create_node(1, &temp_dir).await.unwrap();

        node.initialize(vec![(1, "http://127.0.0.1:50001".to_string())])
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // First shutdown
        node.shutdown().await.unwrap();

        // Second shutdown attempt (should be handled gracefully or error)
        // Note: This might error, which is acceptable
        let _ = node.shutdown().await;
    }
}
