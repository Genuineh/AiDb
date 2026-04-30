//! Integration tests for sharded routing with Multi-Raft
//!
//! These tests verify that keys are correctly routed to their corresponding
//! Raft groups and that data is properly sharded across multiple groups.

#[cfg(feature = "raft-cluster")]
mod sharded_routing_tests {
    use aidb::cluster::{
        ClusterMeta, GroupMeta, MetaNodeInfo, MultiRaftNode, Router, ShardedStateMachine,
        SLOT_COUNT,
    };
    use aidb::config::Options;
    use openraft::Config;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_slot_calculation_distribution() {
        // Test that CRC16-based slot calculation produces good distribution
        let mut slot_buckets: HashMap<u16, usize> = HashMap::new();

        // Generate 10000 keys and check their distribution
        for i in 0..10000 {
            let key = format!("user:{}", i);
            let slot = Router::key_to_slot(key.as_bytes());

            assert!(slot < SLOT_COUNT as u16, "Slot {} is out of range", slot);

            *slot_buckets.entry(slot / 100).or_insert(0) += 1;
        }

        // We should have keys distributed across many buckets
        // With 10000 keys and 164 buckets (16384/100), we should have at least 100 buckets
        assert!(
            slot_buckets.len() > 100,
            "Poor distribution: only {} buckets used",
            slot_buckets.len()
        );

        println!("Distribution: {} buckets used out of 164", slot_buckets.len());
    }

    #[test]
    fn test_router_with_multiple_groups() {
        // Create metadata with 16 groups
        let mut meta = ClusterMeta::with_uniform_distribution(16);

        // Add group metadata
        for group_id in 0..16 {
            meta.groups.insert(group_id, GroupMeta::new(group_id, vec![1, 2, 3]));
        }

        // Add node information
        meta.nodes.insert(1, MetaNodeInfo::new(1, "127.0.0.1:50051".to_string()));
        meta.nodes.insert(2, MetaNodeInfo::new(2, "127.0.0.1:50052".to_string()));
        meta.nodes.insert(3, MetaNodeInfo::new(3, "127.0.0.1:50053".to_string()));

        let router = Router::new(meta);

        // Test routing multiple keys
        let test_keys: Vec<&[u8]> =
            vec![b"user:1000", b"order:5000", b"product:1234", b"session:abcd"];

        let mut group_distribution = HashMap::new();

        for &key in &test_keys {
            let group_id = router.route(key).unwrap();
            assert!(group_id < 16, "Group ID {} out of range", group_id);

            *group_distribution.entry(group_id).or_insert(0) += 1;

            // Verify we can get nodes for the group
            let nodes = router.route_to_nodes(key).unwrap();
            assert_eq!(nodes, vec![1, 2, 3]);
        }

        println!("Keys distributed across {} groups", group_distribution.len());
    }

    #[test]
    fn test_same_key_same_slot() {
        // Verify that the same key always maps to the same slot
        let key = b"test:key:12345";

        let slot1 = Router::key_to_slot(key);
        let slot2 = Router::key_to_slot(key);
        let slot3 = Router::key_to_slot(key);

        assert_eq!(slot1, slot2);
        assert_eq!(slot2, slot3);
    }

    #[test]
    fn test_sharded_state_machine_multiple_groups() {
        let temp_dir = TempDir::new().unwrap();
        let options = Options::default();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), options);

        // Create multiple groups and write data to each
        for group_id in 1..=5 {
            let key = format!("group_{}_key", group_id);
            let value = format!("group_{}_value", group_id);

            state_machine
                .put(group_id, key.as_bytes().to_vec(), value.as_bytes().to_vec())
                .unwrap();
        }

        // Verify each group has its own data
        for group_id in 1..=5 {
            let key = format!("group_{}_key", group_id);
            let expected_value = format!("group_{}_value", group_id);

            let value = state_machine.get(group_id, key.as_bytes()).unwrap();
            assert_eq!(value, Some(expected_value.as_bytes().to_vec()));

            // Verify other groups don't have this key
            for other_group in 1..=5 {
                if other_group != group_id {
                    let value = state_machine.get(other_group, key.as_bytes()).unwrap();
                    assert_eq!(
                        value, None,
                        "Group {} should not have key from group {}",
                        other_group, group_id
                    );
                }
            }
        }

        assert_eq!(state_machine.group_count(), 5);
    }

    #[test]
    fn test_sharded_state_machine_routed_operations() {
        let temp_dir = TempDir::new().unwrap();

        // Create router with metadata
        let mut meta = ClusterMeta::with_uniform_distribution(4);
        for group_id in 0..4 {
            meta.groups.insert(group_id, GroupMeta::new(group_id, vec![1]));
        }

        let router = Router::new(meta);
        let options = Options::default();
        let state_machine =
            ShardedStateMachine::with_router(temp_dir.path(), options, std::sync::Arc::new(router));

        // Write keys and verify they route to the correct groups
        let test_data = vec![
            (b"user:1000".to_vec(), b"alice".to_vec()),
            (b"user:2000".to_vec(), b"bob".to_vec()),
            (b"order:1".to_vec(), b"order_data_1".to_vec()),
            (b"order:2".to_vec(), b"order_data_2".to_vec()),
        ];

        // Write using routed operations
        for (key, value) in &test_data {
            state_machine.put_routed(key.clone(), value.clone()).unwrap();
        }

        // Read using routed operations
        for (key, expected_value) in &test_data {
            let value = state_machine.get_routed(key).unwrap();
            assert_eq!(value.as_ref(), Some(expected_value));
        }

        // Delete using routed operations
        state_machine.delete_routed(&test_data[0].0).unwrap();
        let value = state_machine.get_routed(&test_data[0].0).unwrap();
        assert_eq!(value, None);

        println!("Routed operations test: {} groups created", state_machine.group_count());
    }

    #[tokio::test]
    async fn test_multi_raft_node_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let mut node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

        // Initialize MetaRaft
        let meta_config = Config::default();
        node.init_meta_raft(meta_config).await.unwrap();

        // Initialize router
        node.init_router().unwrap();
        assert!(node.router().is_some());

        // Initialize state machine
        let options = Options::default();
        node.init_state_machine(options).unwrap();
        assert!(node.state_machine().is_some());

        // Verify node is ready
        assert_eq!(node.node_id(), 1);
    }

    #[test]
    fn test_key_slot_consistency_across_restarts() {
        // Verify slot calculation is deterministic across restarts
        let test_keys = [
            b"user:1".to_vec(),
            b"user:2".to_vec(),
            b"order:100".to_vec(),
            b"session:abc".to_vec(),
        ];

        // Calculate slots in "first run"
        let first_run_slots: Vec<u16> = test_keys.iter().map(|k| Router::key_to_slot(k)).collect();

        // Simulate restart by calculating again
        let second_run_slots: Vec<u16> = test_keys.iter().map(|k| Router::key_to_slot(k)).collect();

        // They should be identical
        assert_eq!(first_run_slots, second_run_slots);
    }

    #[test]
    fn test_group_isolation() {
        // Verify that groups are truly isolated
        let temp_dir = TempDir::new().unwrap();
        let options = Options::default();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), options);

        let key = b"shared_key".to_vec();

        // Write different values to different groups with same key
        state_machine.put(1, key.clone(), b"value_in_group_1".to_vec()).unwrap();
        state_machine.put(2, key.clone(), b"value_in_group_2".to_vec()).unwrap();
        state_machine.put(3, key.clone(), b"value_in_group_3".to_vec()).unwrap();

        // Verify each group has its own value
        assert_eq!(state_machine.get(1, &key).unwrap(), Some(b"value_in_group_1".to_vec()));
        assert_eq!(state_machine.get(2, &key).unwrap(), Some(b"value_in_group_2".to_vec()));
        assert_eq!(state_machine.get(3, &key).unwrap(), Some(b"value_in_group_3".to_vec()));
    }

    #[test]
    fn test_router_metadata_versioning() {
        let mut meta1 = ClusterMeta::new();
        meta1.config_version = 1;
        let router = Router::new(meta1);

        assert_eq!(router.get_version(), 1);

        // Update with newer version
        let mut meta2 = ClusterMeta::new();
        meta2.config_version = 2;
        assert!(router.update_metadata(meta2));
        assert_eq!(router.get_version(), 2);

        // Try to update with older version - should be rejected
        let mut meta3 = ClusterMeta::new();
        meta3.config_version = 1;
        assert!(!router.update_metadata(meta3));
        assert_eq!(router.get_version(), 2);

        // Update with same version - should be rejected
        let mut meta4 = ClusterMeta::new();
        meta4.config_version = 2;
        assert!(!router.update_metadata(meta4));
        assert_eq!(router.get_version(), 2);
    }

    #[test]
    fn test_large_scale_key_distribution() {
        // Test with a large number of keys to ensure good distribution
        let mut meta = ClusterMeta::with_uniform_distribution(64);
        for group_id in 0..64 {
            meta.groups.insert(group_id, GroupMeta::new(group_id, vec![1]));
        }

        let router = Router::new(meta);
        let mut group_counts = HashMap::new();

        // Generate 100,000 keys
        for i in 0..100000 {
            let key = format!("key:{}", i);
            let group_id = router.route(key.as_bytes()).unwrap();
            *group_counts.entry(group_id).or_insert(0) += 1;
        }

        // All 64 groups should have keys
        assert_eq!(group_counts.len(), 64, "Not all groups received keys");

        // Calculate standard deviation to ensure relatively even distribution
        let avg = 100000 / 64;
        let variance: f64 = group_counts
            .values()
            .map(|&count| {
                let diff = count as f64 - avg as f64;
                diff * diff
            })
            .sum::<f64>()
            / 64.0;
        let std_dev = variance.sqrt();

        // Standard deviation should be relatively small (< 10% of average)
        let acceptable_std_dev = avg as f64 * 0.1;
        assert!(
            std_dev < acceptable_std_dev,
            "Poor distribution: std_dev={}, acceptable={}",
            std_dev,
            acceptable_std_dev
        );

        println!(
            "Distribution stats: avg={}, std_dev={:.2}, min={}, max={}",
            avg,
            std_dev,
            group_counts.values().min().unwrap(),
            group_counts.values().max().unwrap()
        );
    }
}
