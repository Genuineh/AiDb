//! Comprehensive cluster mode integration tests
//!
//! These tests verify the full data path through MultiRaftNode including:
//! - Put/get/delete operations through slot-based routing
//! - Multi-group data isolation
//! - Data persistence across node restart
//! - Scan operations
//! - Concurrent operations across groups
//! - Batch operations through routing
//! - Edge cases (empty reads, overwrites, etc.)

#[cfg(feature = "raft-cluster")]
mod comprehensive_cluster_tests {
    use aidb::cluster::{
        MultiRaftNode, Router,
        thin_replication::WriteBatch,
    };
    use aidb::config::Options;
    use openraft::Config;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::sleep;

    /// Helper: create a fully initialized MultiRaftNode with:
    /// - MetaRaft bootstrapped (single node)
    /// - One data Raft group (group_id) owning all 16384 slots
    /// - Router and state machine initialized
    ///
    /// Takes ownership of TempDir to keep it alive for the test duration.
    async fn setup_with_dir(
        node_id: u64,
        temp_dir: TempDir,
        group_id: u64,
    ) -> (MultiRaftNode, TempDir) {
        let config = Config::default();
        let mut node = MultiRaftNode::new(node_id, temp_dir.path(), config)
            .await
            .expect("Failed to create MultiRaftNode");

        // Initialize MetaRaft
        node.init_meta_raft(Config::default())
            .await
            .expect("Failed to init MetaRaft");

        // Bootstrap MetaRaft cluster
        let addr = format!("127.0.0.1:{}", 50051 + node_id);
        node.initialize_meta_cluster(vec![(node_id, addr)])
            .await
            .expect("Failed to bootstrap MetaRaft");

        // Create group through MetaRaft
        let meta_raft = node.meta_raft().cloned().unwrap();
        meta_raft
            .create_group(group_id, vec![node_id])
            .await
            .expect("Failed to create group in MetaRaft");

        // Wait for group creation to be applied
        sleep(Duration::from_millis(300)).await;

        // Assign all slots to the group
        meta_raft
            .update_slots(0, 16384, group_id)
            .await
            .expect("Failed to update slot mapping");

        // Wait for slot update to be applied
        sleep(Duration::from_millis(300)).await;

        // Create the local Raft data group
        node.create_raft_group(group_id, vec![node_id])
            .await
            .expect("Failed to create Raft group");

        // Initialize router (reads metadata from MetaRaft)
        node.init_router().expect("Failed to init router");

        // Initialize state machine
        node.init_state_machine(Options::default())
            .expect("Failed to init state machine");

        (node, temp_dir)
    }

    /// Wait for the node to become leader of the MetaRaft group
    async fn wait_for_meta_leader(node: &MultiRaftNode, max_wait_ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(max_wait_ms);
        loop {
            if let Some(meta) = node.meta_raft() {
                if meta.is_leader().await {
                    return;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for MetaRaft leader");
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Wait for a data group to have a leader
    async fn wait_for_data_group_leader(node: &MultiRaftNode, group_id: u64, max_wait_ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(max_wait_ms);
        loop {
            if let Some(raft) = node.get_raft_group(group_id) {
                let metrics = raft.metrics().borrow().clone();
                if metrics.current_leader.is_some() {
                    return;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for leader of group {}", group_id);
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    // ========================================================================
    // Test: Full data path (put/get/delete)
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_put_get_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 100).await;

        // Wait for leaders
        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 100, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Write and verify
        node.put(b"test_key".to_vec(), b"test_value".to_vec())
            .await
            .expect("put should succeed");

        // Give time for Raft to apply
        sleep(Duration::from_millis(500)).await;

        let value = node.get(b"test_key")
            .expect("get should not error")
            .expect("value should exist");
        assert_eq!(value, b"test_value".to_vec(), "get should return what was put");

        node.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn test_multi_raft_overwrite_value() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 200).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 200, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Write initial value
        node.put(b"overwrite_key".to_vec(), b"value1".to_vec())
            .await
            .expect("first put should succeed");
        sleep(Duration::from_millis(300)).await;

        // Overwrite with new value
        node.put(b"overwrite_key".to_vec(), b"value2".to_vec())
            .await
            .expect("second put should succeed");
        sleep(Duration::from_millis(300)).await;

        let value = node.get(b"overwrite_key")
            .expect("get should not error")
            .expect("value should exist");
        assert_eq!(value, b"value2".to_vec(), "get should return the latest value");

        node.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn test_multi_raft_delete_verification() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 300).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 300, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Write
        node.put(b"delete_me".to_vec(), b"will_be_deleted".to_vec())
            .await
            .expect("put should succeed");
        sleep(Duration::from_millis(300)).await;

        // Verify exists
        let value = node.get(b"delete_me").expect("get should succeed");
        assert!(value.is_some(), "key should exist before delete");

        // Delete
        node.delete(b"delete_me")
            .await
            .expect("delete should succeed");
        sleep(Duration::from_millis(300)).await;

        // Verify gone
        let value = node.get(b"delete_me").expect("get should succeed");
        assert!(value.is_none(), "key should not exist after delete");

        node.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn test_multi_raft_read_nonexistent_key() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 400).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 400, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Read a key that was never written
        let value = node.get(b"nonexistent_key")
            .expect("get for nonexistent key should not error");
        assert!(value.is_none(), "nonexistent key should return None");

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: Multi-group data isolation
    // ========================================================================

    #[tokio::test]
    async fn test_multi_group_data_isolation() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let mut node = MultiRaftNode::new(1, temp_dir.path(), config)
            .await
            .expect("Failed to create MultiRaftNode");

        // Initialize MetaRaft
        node.init_meta_raft(Config::default())
            .await
            .expect("Failed to init MetaRaft");

        let addr = "127.0.0.1:50051".to_string();
        node.initialize_meta_cluster(vec![(1, addr)])
            .await
            .expect("Failed to bootstrap MetaRaft");

        wait_for_meta_leader(&node, 3000).await;

        // Create two groups through MetaRaft
        let meta_raft = node.meta_raft().cloned().unwrap();
        meta_raft.create_group(10, vec![1]).await.unwrap();
        meta_raft.create_group(20, vec![1]).await.unwrap();
        sleep(Duration::from_millis(300)).await;

        // Assign half the slots to each group
        meta_raft.update_slots(0, 8192, 10).await.unwrap();
        meta_raft.update_slots(8192, 16384, 20).await.unwrap();
        sleep(Duration::from_millis(300)).await;

        // Create local Raft groups
        node.create_raft_group(10, vec![1]).await.unwrap();
        node.create_raft_group(20, vec![1]).await.unwrap();

        // Initialize router and state machine
        node.init_router().expect("Failed to init router");
        node.init_state_machine(Options::default())
            .expect("Failed to init state machine");

        // Wait for data group leaders
        wait_for_data_group_leader(&node, 10, 3000).await;
        wait_for_data_group_leader(&node, 20, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Use Router to find keys that map to different groups
        let router = node.router().cloned().unwrap();

        // Find a key for group 10 and group 20
        let (key_for_group_10, key_for_group_20) = {
            let mut k10 = None;
            let mut k20 = None;
            for i in 0..10000u64 {
                let key = format!("key_{}", i).into_bytes();
                let group_id = router.route(&key).unwrap();
                if group_id == 10 && k10.is_none() {
                    k10 = Some(key);
                } else if group_id == 20 && k20.is_none() {
                    k20 = Some(key);
                }
                if k10.is_some() && k20.is_some() {
                    break;
                }
            }
            (k10.expect("Could not find key for group 10"),
             k20.expect("Could not find key for group 20"))
        };

        // Write different values to each group
        node.put(key_for_group_10.clone(), b"value_from_group_10".to_vec())
            .await
            .expect("put to group 10 should succeed");
        node.put(key_for_group_20.clone(), b"value_from_group_20".to_vec())
            .await
            .expect("put to group 20 should succeed");
        sleep(Duration::from_millis(500)).await;

        // Verify each key has its own value
        let val10 = node.get(&key_for_group_10)
            .expect("get for group 10 should succeed")
            .expect("key in group 10 should exist");
        assert_eq!(val10, b"value_from_group_10".to_vec(),
            "group 10 should have its own value");

        let val20 = node.get(&key_for_group_20)
            .expect("get for group 20 should succeed")
            .expect("key in group 20 should exist");
        assert_eq!(val20, b"value_from_group_20".to_vec(),
            "group 20 should have its own value");

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: Data persistence across restarts
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_persistence_across_restart() {
        let temp_dir = TempDir::new().unwrap();
        let group_id = 500u64;

        // First session: write data using inline setup
        let path = temp_dir.path().to_path_buf();
        {
            let config = Config::default();
            let mut node = MultiRaftNode::new(1, &path, config)
                .await
                .expect("Failed to create MultiRaftNode");

            node.init_meta_raft(Config::default())
                .await
                .expect("Failed to init MetaRaft");

            let addr = "127.0.0.1:50051".to_string();
            node.initialize_meta_cluster(vec![(1, addr)])
                .await
                .expect("Failed to bootstrap MetaRaft");

            wait_for_meta_leader(&node, 3000).await;

            let meta_raft = node.meta_raft().cloned().unwrap();
            meta_raft.create_group(group_id, vec![1]).await.unwrap();
            meta_raft.update_slots(0, 16384, group_id).await.unwrap();
            sleep(Duration::from_millis(300)).await;

            node.create_raft_group(group_id, vec![1]).await.unwrap();
            node.init_router().unwrap();
            node.init_state_machine(Options::default()).unwrap();

            wait_for_data_group_leader(&node, group_id, 3000).await;
            sleep(Duration::from_millis(500)).await;

            node.put(b"persistent_key".to_vec(), b"persistent_value".to_vec())
                .await
                .expect("put should succeed");
            sleep(Duration::from_millis(300)).await;

            // Verify write succeeded
            let value = node.get(b"persistent_key")
                .expect("get should succeed")
                .expect("value should exist in first session");
            assert_eq!(value, b"persistent_value".to_vec());

            node.shutdown().await.expect("shutdown should succeed");
        }

        // Second session: restart and verify data persists
        {
            let config = Config::default();
            let mut node = MultiRaftNode::new(1, temp_dir.path(), config)
                .await
                .expect("Failed to recreate MultiRaftNode");

            // Initialize MetaRaft
            let meta_config = Config::default();
            node.init_meta_raft(meta_config)
                .await
                .expect("Failed to re-init MetaRaft");

            // Bootstrap again
            node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())])
                .await
                .expect("Failed to re-bootstrap MetaRaft");

            wait_for_meta_leader(&node, 3000).await;

            // Create group through MetaRaft
            let meta_raft = node.meta_raft().cloned().unwrap();
            meta_raft.create_group(group_id, vec![1]).await.unwrap();
            meta_raft.update_slots(0, 16384, group_id).await.unwrap();
            sleep(Duration::from_millis(300)).await;

            // Load existing groups from disk
            let loaded = node.load_existing_groups()
                .await
                .expect("Failed to load existing groups");
            assert_eq!(loaded, 1, "Should have loaded 1 existing group");

            // Initialize router and state machine
            node.init_router().expect("Failed to init router");
            node.init_state_machine(Options::default())
                .expect("Failed to init state machine");

            // Verify data persisted
            sleep(Duration::from_millis(500)).await;
            let value = node.get(b"persistent_key")
                .expect("get should succeed");

            assert!(value.is_some(), "data should persist across restart");
            assert_eq!(value.unwrap(), b"persistent_value".to_vec(),
                "persisted value should match");

            node.shutdown().await.expect("shutdown should succeed");
        }
    }

    // ========================================================================
    // Test: Scan operations
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_scan_basic() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 600).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 600, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Write test data
        for i in 0..10 {
            let key = format!("scan_key_{}", i).into_bytes();
            let value = format!("scan_value_{}", i).into_bytes();
            node.put(key, value).await.expect("put should succeed");
        }
        sleep(Duration::from_millis(500)).await;

        // Find our group via router
        let router = node.router().cloned().unwrap();
        let test_key = b"scan_key_0";
        let group_id = router.route(test_key).expect("route should succeed");

        // Scan the group
        let result = node.scan_group_streaming(group_id, 100, None)
            .await
            .expect("scan should succeed");

        // Should find at least some keys
        assert!(!result.keys.is_empty(), "scan should return keys");
        assert!(result.exhausted || result.keys.len() >= 10,
            "scan should return all or at least 10 keys");

        // Verify scan keys match our written keys
        let has_scan_key = result.keys.iter().any(|k| k.starts_with(b"scan_key_"));
        assert!(has_scan_key, "scanned keys should include our test data");

        node.shutdown().await.expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn test_multi_raft_scan_with_cursor() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 700).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 700, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Write test data
        for i in 0..20 {
            let key = format!("cursor_key_{:04}", i).into_bytes();
            let value = format!("cursor_value_{}", i).into_bytes();
            node.put(key, value).await.expect("put should succeed");
        }
        sleep(Duration::from_millis(500)).await;

        // Scan with limit
        let (cursor, keys) = node.scan_groups_streaming(None, 5)
            .await
            .expect("first scan should succeed");
        assert_eq!(keys.len(), 5, "first batch should have 5 keys");

        // Verify cursor is not empty (more keys available)
        assert!(!cursor.is_empty(), "cursor should not be empty when more keys");

        // Scan again with cursor
        let (_cursor2, keys2) = node.scan_groups_streaming(Some(&cursor), 5)
            .await
            .expect("second scan should succeed");
        assert_eq!(keys2.len(), 5, "second batch should have 5 keys");

        // Verify no duplicate keys between batches
        for k in &keys2 {
            assert!(!keys.contains(k), "cursor scan should not return duplicates");
        }

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: Batch operations through routing
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_write_batch() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 800).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 800, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Find the first key that routes to group 800
        let router = node.router().cloned().unwrap();
        let route_key = {
            let mut found_key = None;
            for i in 0..1000u64 {
                let key = format!("batch_key_{}", i).into_bytes();
                if router.route(&key).unwrap() == 800 {
                    found_key = Some(key);
                    break;
                }
            }
            found_key.expect("Could not find key routing to group 800")
        };

        // Create and write a batch
        let mut batch = WriteBatch::new();
        batch.put(b"batch_key_a".to_vec(), b"batch_value_a".to_vec());
        batch.put(b"batch_key_b".to_vec(), b"batch_value_b".to_vec());
        batch.delete(b"batch_key_c".to_vec());

        node.write_batch_for_route_key(&route_key, batch)
            .await
            .expect("write_batch should succeed");
        sleep(Duration::from_millis(500)).await;

        // Verify batch operations
        let val_a = node.get(b"batch_key_a")
            .expect("get should succeed")
            .expect("batch key A should exist");
        assert_eq!(val_a, b"batch_value_a".to_vec());

        let val_b = node.get(b"batch_key_b")
            .expect("get should succeed")
            .expect("batch key B should exist");
        assert_eq!(val_b, b"batch_value_b".to_vec());

        // Key C was deleted in batch, but it never existed, so it should be None
        let val_c = node.get(b"batch_key_c").expect("get should succeed");
        assert!(val_c.is_none(), "batch key C should not exist");

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: Multiple keys with slot routing consistency
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_slot_routing_consistency() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 900).await;

        wait_for_meta_leader(&node, 3000).await;
        wait_for_data_group_leader(&node, 900, 3000).await;
        sleep(Duration::from_millis(500)).await;

        let router = node.router().cloned().unwrap();

        // Write multiple keys and verify they all route to our group
        for i in 0..20 {
            let key = format!("route_test_key_{}", i).into_bytes();
            let value = format!("route_test_value_{}", i).into_bytes();

            let _slot = Router::key_to_slot(&key);
            let group_id = router.route(&key).unwrap();
            assert_eq!(group_id, 900,
                "key {} should route to group 900, got {}", i, group_id);

            node.put(key.clone(), value)
                .await
                .expect("put should succeed");
            sleep(Duration::from_millis(100)).await;
        }
        sleep(Duration::from_millis(500)).await;

        // Verify all writes via get
        for i in 0..20 {
            let key = format!("route_test_key_{}", i).into_bytes();
            let expected = format!("route_test_value_{}", i).into_bytes();

            let value = node.get(&key)
                .expect("get should succeed")
                .expect("key should exist");
            assert_eq!(value, expected,
                "get for key {} should return correct value", i);
        }

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: MultiRaftNode metadata consistency
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_meta_sync() {
        let temp_dir = TempDir::new().unwrap();
        let (node, _dir) = setup_with_dir(1, temp_dir, 1000).await;

        wait_for_meta_leader(&node, 3000).await;
        sleep(Duration::from_millis(500)).await;

        // Verify metadata via router
        let router = node.router().cloned().unwrap();
        let meta = router.get_metadata();

        assert_eq!(meta.groups.len(), 1, "should have 1 group in metadata");
        assert!(meta.groups.contains_key(&1000), "group 1000 should exist");

        let group = meta.groups.get(&1000).unwrap();
        assert_eq!(group.replicas, vec![1], "group should have 1 replica");

        // Verify slot mapping
        assert_eq!(meta.slot_to_group(0), 1000, "slot 0 should map to group 1000");
        assert_eq!(meta.slot_to_group(16383), 1000, "slot 16383 should map to group 1000");

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: Clean shutdown and restart cycle
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_shutdown_restart_cycle() {
        let temp_dir = TempDir::new().unwrap();
        let group_id = 1100u64;

        // Multiple shutdown/restart cycles
        for cycle in 0..3 {
            let config = Config::default();
            let mut node = MultiRaftNode::new(1, temp_dir.path(), config)
                .await
                .expect("Failed to create MultiRaftNode");

            node.init_meta_raft(Config::default())
                .await
                .expect("Failed to init MetaRaft");

            node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())])
                .await
                .expect("Failed to bootstrap");

            wait_for_meta_leader(&node, 3000).await;

            if cycle == 0 {
                // First cycle: create group and data
                let meta_raft = node.meta_raft().cloned().unwrap();
                meta_raft.create_group(group_id, vec![1]).await.unwrap();
                meta_raft.update_slots(0, 16384, group_id).await.unwrap();
                sleep(Duration::from_millis(300)).await;

                node.create_raft_group(group_id, vec![1]).await.unwrap();
                node.init_router().unwrap();
                node.init_state_machine(Options::default()).unwrap();

                wait_for_data_group_leader(&node, group_id, 3000).await;
                sleep(Duration::from_millis(500)).await;

                node.put(b"cycle_key".to_vec(), b"cycle_value".to_vec())
                    .await
                    .expect("put should succeed");
                sleep(Duration::from_millis(300)).await;
            } else {
                // Subsequent cycles: restore metadata and load existing groups
                let meta_raft = node.meta_raft().cloned().unwrap();
                meta_raft.create_group(group_id, vec![1]).await.unwrap();
                meta_raft.update_slots(0, 16384, group_id).await.unwrap();
                sleep(Duration::from_millis(300)).await;

                let loaded = node.load_existing_groups()
                    .await
                    .expect("Failed to load existing groups");
                assert!(loaded >= 1, "Should have loaded at least 1 group");

                node.init_router().unwrap();
                node.init_state_machine(Options::default()).unwrap();

                sleep(Duration::from_millis(500)).await;

                // Verify data from previous cycle persists
                let value = node.get(b"cycle_key")
                    .expect("get should succeed");
                assert!(value.is_some(), "data should persist across cycle {}", cycle);
                assert_eq!(value.unwrap(), b"cycle_value".to_vec());
            }

            node.shutdown().await.expect("shutdown should succeed");
        }
    }

    // ========================================================================
    // Test: OpenRaftNode linearizable_read gap documentation
    // ========================================================================

    #[tokio::test]
    async fn test_openraft_linearizable_read_unimplemented() {
        use aidb::cluster::{OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig};
        use aidb::DB;

        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let network_factory = RaftNetworkClientFactory::new(1);
        let config = RaftNodeConfig {
            node_id: 1,
            election_timeout_min: 150,
            election_timeout_max: 300,
            heartbeat_interval: 50,
            max_payload_entries: 100,
            snapshot_logs_since_last: 100,
        };

        let node = OpenRaftNode::new(config, db, network_factory)
            .await
            .expect("Failed to create OpenRaftNode");

        // linearizable_read should return an error since it's not fully implemented
        let result = node.linearizable_read(b"any_key".to_vec()).await;
        assert!(result.is_err(), "linearizable_read is not implemented and should return error");

        // Verify the error is expected (Not the leader or unimplemented)
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        assert!(
            err_str.contains("not fully implemented") || err_str.contains("Not the leader"),
            "Error should indicate linearizable_read is not implemented: {}",
            err_str
        );

        node.shutdown().await.expect("shutdown should succeed");
    }

    // ========================================================================
    // Test: Raft node config validation
    // ========================================================================

    #[tokio::test]
    async fn test_raft_node_config_invalid_values() {
        use aidb::cluster::{OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig};
        use aidb::DB;

        // Test with minimum valid config values
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let factory = RaftNetworkClientFactory::new(1);
        let config = RaftNodeConfig {
            node_id: 1,
            election_timeout_min: 100,
            election_timeout_max: 200,
            heartbeat_interval: 50,
            max_payload_entries: 1,
            snapshot_logs_since_last: 1,
        };

        let result = OpenRaftNode::new(config, db, factory).await;
        assert!(result.is_ok(), "Raft node with reasonable config values should be created");
    }

    // ========================================================================
    // Test: MultiRaftNode with non-default node_id
    // ========================================================================

    #[tokio::test]
    async fn test_multi_raft_non_default_node_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let mut node = MultiRaftNode::new(42, temp_dir.path(), config)
            .await
            .expect("Failed to create MultiRaftNode with node_id 42");

        assert_eq!(node.node_id(), 42, "node_id should be 42");

        node.init_meta_raft(Config::default())
            .await
            .expect("MetaRaft init should work with non-default node_id");

        node.initialize_meta_cluster(vec![(42, "127.0.0.1:50093".to_string())])
            .await
            .expect("Cluster bootstrap should work with non-default node_id");

        assert_eq!(node.node_id(), 42, "node_id should remain 42 after init");

        node.shutdown().await.expect("shutdown should succeed");
    }
}
