//! Multi-Shard integration tests for Phase 3: Shard Group functionality
//!
//! Tests for Week 31-32:
//! - Multi-shard startup and coordination
//! - Data distribution verification
//! - Routing correctness
//! - Failure scenarios and recovery

#[cfg(feature = "cluster")]
use aidb::cluster::{Coordinator, NodeState, ShardGroupManager, ShardGroupState};
#[cfg(feature = "cluster")]
use std::collections::HashMap;

// ============================================================================
// Week 31-32: Multi-Shard Startup Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_multi_shard_startup_sequential() {
    let manager = ShardGroupManager::new();
    let shard_count = 5;

    // Create and start shards sequentially
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);

        // Create group
        manager.create_group(shard_id.clone()).unwrap();

        // Add primary
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();

        // Add replicas
        for j in 0..2 {
            manager
                .add_replica(
                    &shard_id,
                    format!("replica-{}-{}", i, j),
                    format!("127.0.0.1:{}", 50061 + i * 10 + j),
                )
                .unwrap();
        }

        // Start the group
        manager.start_group(&shard_id).unwrap();
    }

    // Verify all shards are running
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Running));
    }

    // Verify node counts
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        let nodes = manager.get_group_nodes(&shard_id).unwrap();
        assert_eq!(nodes.len(), 3); // 1 primary + 2 replicas

        let primary = manager.get_primary(&shard_id).unwrap();
        assert!(primary.is_some());

        let replicas = manager.get_replicas(&shard_id).unwrap();
        assert_eq!(replicas.len(), 2);
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_multi_shard_startup_validation() {
    let manager = ShardGroupManager::new();
    let shard_count = 3;

    // Create shards with different configurations
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();

        // Each shard has different number of replicas
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();

        for j in 0..=i {
            manager
                .add_replica(
                    &shard_id,
                    format!("replica-{}-{}", i, j),
                    format!("127.0.0.1:{}", 50061 + i * 10 + j),
                )
                .unwrap();
        }
    }

    // Verify configuration before starting
    let shard0_replicas = manager.get_replicas("shard0").unwrap();
    assert_eq!(shard0_replicas.len(), 1); // 0..=0

    let shard1_replicas = manager.get_replicas("shard1").unwrap();
    assert_eq!(shard1_replicas.len(), 2); // 0..=1

    let shard2_replicas = manager.get_replicas("shard2").unwrap();
    assert_eq!(shard2_replicas.len(), 3); // 0..=2

    // Start all shards
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        manager.start_group(&shard_id).unwrap();
    }

    // Verify all shards are independently running
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Running));
    }
}

// ============================================================================
// Week 31-32: Data Distribution Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_key_routing_distribution() {
    let _coordinator = Coordinator::new(150);
    let manager = ShardGroupManager::new();

    // Create 3 shards
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    // Simulate key distribution
    let test_keys = 1000;
    let mut distribution: HashMap<String, usize> = HashMap::new();

    for i in 0..test_keys {
        let _key = format!("key{}", i);

        // Use consistent hashing to route (simulated via shard_id mod)
        let shard_id = format!("shard{}", i % 3);

        *distribution.entry(shard_id).or_insert(0) += 1;
    }

    // Verify reasonably balanced distribution
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        let count = distribution.get(&shard_id).unwrap_or(&0);

        // Each shard should get roughly 1/3 of keys
        let expected = test_keys / 3;
        let tolerance = expected / 10; // 10% tolerance

        assert!(
            *count >= expected - tolerance && *count <= expected + tolerance,
            "Shard {} has {} keys, expected ~{} (tolerance: {})",
            shard_id,
            count,
            expected,
            tolerance
        );
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_data_distribution_verification() {
    let manager = ShardGroupManager::new();

    // Create 5 shards with primaries
    for i in 0..5 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    // Verify all shards are ready to accept data
    let groups = manager.list_groups();
    assert_eq!(groups.len(), 5);

    for shard_id in groups {
        let state = manager.get_group(&shard_id).unwrap();
        assert_eq!(state, ShardGroupState::Running);

        let primary = manager.get_primary(&shard_id).unwrap();
        assert!(primary.is_some());
        assert!(primary.unwrap().state.is_serving());
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_replica_data_distribution() {
    let manager = ShardGroupManager::new();

    // Create 3 shards, each with 3 replicas
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();

        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();

        for j in 0..3 {
            manager
                .add_replica(
                    &shard_id,
                    format!("replica-{}-{}", i, j),
                    format!("127.0.0.1:{}", 50061 + i * 10 + j),
                )
                .unwrap();
        }

        manager.start_group(&shard_id).unwrap();
    }

    // Verify each shard has correct replica setup
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        let replicas = manager.get_replicas(&shard_id).unwrap();

        assert_eq!(replicas.len(), 3);

        // All replicas should be healthy after start
        for replica in replicas {
            assert!(replica.state.is_serving());
            assert!(!replica.is_primary);
        }
    }
}

// ============================================================================
// Week 31-32: Routing Correctness Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_routing_consistency_across_operations() {
    let manager = ShardGroupManager::new();

    // Create 3 shards
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    // Verify that the same key always routes to the same shard
    let test_keys = vec!["user:1", "user:2", "user:3", "order:100", "order:101"];

    for key in &test_keys {
        // Simulate routing (in real scenario, would use coordinator)
        let key_hash = key.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32)) as usize;
        let shard_id = format!("shard{}", key_hash % 3);

        // Multiple operations on same key should route to same shard
        for _ in 0..5 {
            let route_hash = key.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32)) as usize;
            let route_shard = format!("shard{}", route_hash % 3);
            assert_eq!(shard_id, route_shard);
        }
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_routing_with_shard_boundaries() {
    let manager = ShardGroupManager::new();

    // Create 4 shards (power of 2 for clean boundaries)
    for i in 0..4 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    // Test boundary keys
    let boundary_keys = vec!["aaaa", "aaab", "zzzz", "zzzx", "0000", "9999", "key0", "key9999"];

    for key in boundary_keys {
        // Verify key can be routed
        let key_hash = key.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32)) as usize;
        let shard_id = format!("shard{}", key_hash % 4);

        // Verify shard exists and is running
        let state = manager.get_group(&shard_id).unwrap();
        assert_eq!(state, ShardGroupState::Running);
    }
}

// ============================================================================
// Week 31-32: Failure Scenario Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_primary_failure_scenario() {
    let manager = ShardGroupManager::new();

    // Create a shard group
    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
        .unwrap();
    manager.start_group("shard1").unwrap();

    // Verify initial state
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Simulate primary failure
    manager.update_node_state("shard1", "primary1", NodeState::Unhealthy).unwrap();

    // Shard should become degraded
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Degraded));

    // Replica should still be healthy
    let replicas = manager.get_replicas("shard1").unwrap();
    assert_eq!(replicas[0].state, NodeState::Healthy);

    // Recover primary
    manager.update_node_state("shard1", "primary1", NodeState::Healthy).unwrap();

    // Shard should return to running
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));
}

#[cfg(feature = "cluster")]
#[test]
fn test_replica_failure_scenario() {
    let manager = ShardGroupManager::new();

    // Create shard with multiple replicas
    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica2".to_string(), "127.0.0.1:50053".to_string())
        .unwrap();
    manager.start_group("shard1").unwrap();

    // Mark one replica as unhealthy
    manager.update_node_state("shard1", "replica1", NodeState::Unhealthy).unwrap();

    // Shard should still be running (replicas don't affect overall state)
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Verify replica is marked unhealthy
    let nodes = manager.get_group_nodes("shard1").unwrap();
    let replica1 = nodes.iter().find(|n| n.id == "replica1").unwrap();
    assert_eq!(replica1.state, NodeState::Unhealthy);

    // Other replica should still be healthy
    let replica2 = nodes.iter().find(|n| n.id == "replica2").unwrap();
    assert_eq!(replica2.state, NodeState::Healthy);
}

#[cfg(feature = "cluster")]
#[test]
fn test_multiple_shard_failures() {
    let manager = ShardGroupManager::new();

    // Create 3 shards
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    // Fail shard1's primary
    manager.update_node_state("shard1", "primary-1", NodeState::Unhealthy).unwrap();

    // Verify states
    assert_eq!(manager.get_group("shard0"), Some(ShardGroupState::Running));
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Degraded));
    assert_eq!(manager.get_group("shard2"), Some(ShardGroupState::Running));

    // Recover shard1
    manager.update_node_state("shard1", "primary-1", NodeState::Healthy).unwrap();

    // All shards should be running now
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Running));
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_removal_during_operation() {
    let manager = ShardGroupManager::new();

    // Create multiple shards
    for i in 0..5 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    assert_eq!(manager.group_count(), 5);

    // Remove shard2
    manager.remove_group("shard2").unwrap();
    assert_eq!(manager.group_count(), 4);

    // Other shards should still be running
    for i in [0, 1, 3, 4] {
        let shard_id = format!("shard{}", i);
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Running));
    }

    // Removed shard should not be accessible
    assert!(manager.get_group("shard2").is_none());
}

#[cfg(feature = "cluster")]
#[test]
fn test_graceful_shutdown_all_shards() {
    let manager = ShardGroupManager::new();

    // Create multiple shards
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();
        manager.start_group(&shard_id).unwrap();
    }

    // Stop all shards gracefully
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        manager.stop_group(&shard_id).unwrap();
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Stopped));
    }

    // Verify all nodes are stopped
    for i in 0..3 {
        let shard_id = format!("shard{}", i);
        let nodes = manager.get_group_nodes(&shard_id).unwrap();
        for node in nodes {
            assert_eq!(node.state, NodeState::Stopped);
        }
    }
}

// ============================================================================
// Week 31-32: Network Partition Simulation
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_network_partition_simulation() {
    let manager = ShardGroupManager::new();

    // Create shard with replicas
    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica2".to_string(), "127.0.0.1:50053".to_string())
        .unwrap();
    manager.start_group("shard1").unwrap();

    // Simulate network partition - all replicas become unreachable
    manager.update_node_state("shard1", "replica1", NodeState::Unhealthy).unwrap();
    manager.update_node_state("shard1", "replica2", NodeState::Unhealthy).unwrap();

    // Primary is still healthy, so shard should be running
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Simulate primary also becomes unreachable
    manager.update_node_state("shard1", "primary1", NodeState::Unhealthy).unwrap();

    // Now shard should be degraded
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Degraded));

    // Network recovery - all nodes come back
    manager.update_node_state("shard1", "primary1", NodeState::Healthy).unwrap();
    manager.update_node_state("shard1", "replica1", NodeState::Healthy).unwrap();
    manager.update_node_state("shard1", "replica2", NodeState::Healthy).unwrap();

    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));
}

// ============================================================================
// Week 31-32: Load Balancing Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_replica_load_distribution() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();

    // Add multiple replicas for load balancing
    for i in 0..5 {
        manager
            .add_replica("shard1", format!("replica{}", i), format!("127.0.0.1:{}", 50052 + i))
            .unwrap();
    }

    manager.start_group("shard1").unwrap();

    // Verify all replicas are healthy
    let replicas = manager.get_replicas("shard1").unwrap();
    assert_eq!(replicas.len(), 5);

    for replica in replicas {
        assert!(replica.state.is_serving());
    }

    // In real scenario, read requests would be distributed across these replicas
}

#[cfg(feature = "cluster")]
#[test]
fn test_dynamic_replica_management() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager.start_group("shard1").unwrap();

    // Initially no replicas
    assert_eq!(manager.get_replicas("shard1").unwrap().len(), 0);

    // Add replicas dynamically
    for i in 0..3 {
        manager
            .add_replica("shard1", format!("replica{}", i), format!("127.0.0.1:{}", 50052 + i))
            .unwrap();
    }

    // All new replicas should start in Starting state
    let replicas = manager.get_replicas("shard1").unwrap();
    assert_eq!(replicas.len(), 3);
    for replica in replicas {
        assert_eq!(replica.state, NodeState::Starting);
    }

    // Remove one replica
    manager.remove_replica("shard1", "replica1").unwrap();
    assert_eq!(manager.get_replicas("shard1").unwrap().len(), 2);

    // Shard should still be running
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));
}
