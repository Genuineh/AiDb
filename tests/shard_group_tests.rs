//! Integration tests for Phase 3: Shard Group functionality
//!
//! Tests for Week 29-30:
//! - Shard group lifecycle management
//! - Node management (Primary and Replicas)
//! - State management and transitions
//! - Multi-shard coordination

#[cfg(feature = "cluster")]
use aidb::cluster::{Coordinator, NodeState, ShardGroupManager, ShardGroupState};

// ============================================================================
// Week 29-30: ShardGroup Basic Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_manager_basic() {
    let manager = ShardGroupManager::new();
    assert_eq!(manager.group_count(), 0);

    // Create a shard group
    manager.create_group("shard1".to_string()).unwrap();
    assert_eq!(manager.group_count(), 1);

    // List groups
    let groups = manager.list_groups();
    assert_eq!(groups.len(), 1);
    assert!(groups.contains(&"shard1".to_string()));
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_lifecycle() {
    let manager = ShardGroupManager::new();

    // Create and configure a shard group
    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();

    // Initially in Initializing state
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Initializing));

    // Start the group
    manager.start_group("shard1").unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Stop the group
    manager.stop_group("shard1").unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Stopped));

    // Remove the group
    manager.remove_group("shard1").unwrap();
    assert_eq!(manager.group_count(), 0);
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_primary_management() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();

    // Set primary
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();

    // Get primary
    let primary = manager.get_primary("shard1").unwrap();
    assert!(primary.is_some());
    let primary = primary.unwrap();
    assert_eq!(primary.id, "primary1");
    assert_eq!(primary.address, "127.0.0.1:50051");
    assert!(primary.is_primary);

    // Cannot set primary twice
    let result =
        manager.set_primary("shard1", "primary2".to_string(), "127.0.0.1:50052".to_string());
    assert!(result.is_err());
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_replica_management() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();

    // Add replicas
    manager
        .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica2".to_string(), "127.0.0.1:50053".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica3".to_string(), "127.0.0.1:50054".to_string())
        .unwrap();

    // Get replicas
    let replicas = manager.get_replicas("shard1").unwrap();
    assert_eq!(replicas.len(), 3);

    // Verify replica properties
    for replica in &replicas {
        assert!(!replica.is_primary);
        assert_eq!(replica.state, NodeState::Starting);
    }

    // Remove a replica
    manager.remove_replica("shard1", "replica2").unwrap();
    let replicas = manager.get_replicas("shard1").unwrap();
    assert_eq!(replicas.len(), 2);

    // Verify removed replica is gone
    let remaining_ids: Vec<String> = replicas.iter().map(|r| r.id.clone()).collect();
    assert!(remaining_ids.contains(&"replica1".to_string()));
    assert!(remaining_ids.contains(&"replica3".to_string()));
    assert!(!remaining_ids.contains(&"replica2".to_string()));
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_state_transitions() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
        .unwrap();

    // Start group
    manager.start_group("shard1").unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Mark primary as unhealthy - should become degraded
    manager.update_node_state("shard1", "primary1", NodeState::Unhealthy).unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Degraded));

    // Mark primary as healthy - should become running again
    manager.update_node_state("shard1", "primary1", NodeState::Healthy).unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Mark replica as unhealthy - should stay running (replicas don't affect state)
    manager.update_node_state("shard1", "replica1", NodeState::Unhealthy).unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_get_all_nodes() {
    let manager = ShardGroupManager::new();

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

    let nodes = manager.get_group_nodes("shard1").unwrap();
    assert_eq!(nodes.len(), 3);

    // Verify we have one primary and two replicas
    let primaries: Vec<_> = nodes.iter().filter(|n| n.is_primary).collect();
    let replicas: Vec<_> = nodes.iter().filter(|n| !n.is_primary).collect();

    assert_eq!(primaries.len(), 1);
    assert_eq!(replicas.len(), 2);
}

#[cfg(feature = "cluster")]
#[test]
fn test_multiple_shard_groups() {
    let manager = ShardGroupManager::new();

    // Create multiple shard groups
    for i in 1..=5 {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();
        manager
            .set_primary(&shard_id, format!("primary{}", i), format!("127.0.0.1:5005{}", i))
            .unwrap();

        // Add 2 replicas per shard
        for j in 1..=2 {
            manager
                .add_replica(
                    &shard_id,
                    format!("replica{}-{}", i, j),
                    format!("127.0.0.1:5006{}{}", i, j),
                )
                .unwrap();
        }
    }

    assert_eq!(manager.group_count(), 5);

    // Verify each group has correct configuration
    for i in 1..=5 {
        let shard_id = format!("shard{}", i);
        let nodes = manager.get_group_nodes(&shard_id).unwrap();
        assert_eq!(nodes.len(), 3); // 1 primary + 2 replicas
    }

    // Start all groups
    for i in 1..=5 {
        let shard_id = format!("shard{}", i);
        manager.start_group(&shard_id).unwrap();
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Running));
    }

    // List all groups
    let groups = manager.list_groups();
    assert_eq!(groups.len(), 5);
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_error_cases() {
    let manager = ShardGroupManager::new();

    // Cannot operate on non-existent group
    let result = manager.start_group("non_existent");
    assert!(result.is_err());

    let result = manager.stop_group("non_existent");
    assert!(result.is_err());

    let result = manager.get_group_nodes("non_existent");
    assert!(result.is_err());

    // Create a group
    manager.create_group("shard1".to_string()).unwrap();

    // Cannot create duplicate group
    let result = manager.create_group("shard1".to_string());
    assert!(result.is_err());

    // Cannot add replica before setting primary (allowed in current design, but let's test it works)
    let result =
        manager.add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string());
    assert!(result.is_ok());

    // Cannot remove non-existent replica
    let result = manager.remove_replica("shard1", "non_existent");
    assert!(result.is_err());

    // Cannot update state of non-existent node
    let result = manager.update_node_state("shard1", "non_existent", NodeState::Healthy);
    assert!(result.is_err());
}

// ============================================================================
// Integration with Coordinator
// ============================================================================

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_shard_group_with_coordinator() {
    let manager = ShardGroupManager::new();
    let coordinator = Coordinator::new(150);

    // Create a shard group
    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "http://127.0.0.1:50051".to_string())
        .unwrap();

    // Start the shard group
    manager.start_group("shard1").unwrap();

    // Verify coordinator doesn't have the shard yet
    assert_eq!(coordinator.shard_count(), 0);

    // In a real scenario, we would register the shard with the coordinator
    // after starting the shard group
    // This test just verifies the manager and coordinator can work together
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_node_state_serving() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager
        .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
        .unwrap();

    // Before starting, nodes are not serving
    let nodes = manager.get_group_nodes("shard1").unwrap();
    for node in &nodes {
        assert!(!node.state.is_serving());
    }

    // After starting, nodes should be serving
    manager.start_group("shard1").unwrap();
    let nodes = manager.get_group_nodes("shard1").unwrap();
    for node in &nodes {
        assert!(node.state.is_serving());
    }

    // After stopping, nodes should not be serving
    manager.stop_group("shard1").unwrap();
    let nodes = manager.get_group_nodes("shard1").unwrap();
    for node in &nodes {
        assert!(!node.state.is_serving());
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_cannot_start_twice() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();

    // Start once - should succeed
    manager.start_group("shard1").unwrap();
    assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

    // Try to start again - should fail
    let result = manager.start_group("shard1");
    assert!(result.is_err());
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_remove_active_group() {
    let manager = ShardGroupManager::new();

    manager.create_group("shard1".to_string()).unwrap();
    manager
        .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
        .unwrap();
    manager.start_group("shard1").unwrap();

    // Remove should stop the group first
    manager.remove_group("shard1").unwrap();
    assert_eq!(manager.group_count(), 0);
}

#[cfg(feature = "cluster")]
#[test]
fn test_shard_group_default_manager() {
    // Test Default trait implementation
    let manager: ShardGroupManager = Default::default();
    assert_eq!(manager.group_count(), 0);
}

// ============================================================================
// Week 31-32: Multi-Shard Coordination Tests (Preview)
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_multi_shard_group_coordination_basic() {
    let manager = ShardGroupManager::new();

    // Simulate a multi-shard setup
    let shard_count = 3;
    let replicas_per_shard = 2;

    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        manager.create_group(shard_id.clone()).unwrap();

        // Add primary
        manager
            .set_primary(&shard_id, format!("primary-{}", i), format!("127.0.0.1:{}", 50051 + i))
            .unwrap();

        // Add replicas
        for j in 0..replicas_per_shard {
            manager
                .add_replica(
                    &shard_id,
                    format!("replica-{}-{}", i, j),
                    format!("127.0.0.1:{}", 50061 + i * 10 + j),
                )
                .unwrap();
        }

        // Start the shard group
        manager.start_group(&shard_id).unwrap();
    }

    // Verify all shards are running
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        assert_eq!(manager.get_group(&shard_id), Some(ShardGroupState::Running));
    }

    // Verify total node count
    let mut total_nodes = 0;
    for i in 0..shard_count {
        let shard_id = format!("shard{}", i);
        let nodes = manager.get_group_nodes(&shard_id).unwrap();
        total_nodes += nodes.len();
    }
    assert_eq!(total_nodes, shard_count * (1 + replicas_per_shard));
}
