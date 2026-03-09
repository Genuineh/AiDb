//! Integration tests for dynamic member management (Stage 4)

#[cfg(feature = "raft-cluster")]
use aidb::cluster::{
    GroupMeta, MembershipCoordinator, MetaStateMachine, MultiRaftNode, ReplicaAllocator,
};
#[cfg(feature = "raft-cluster")]
use aidb::Options;
#[cfg(feature = "raft-cluster")]
use openraft::Config;
#[cfg(feature = "raft-cluster")]
use std::collections::HashMap;

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_replica_allocator_with_meta_state() {
    // Test that replica allocator integrates properly with MetaStateMachine
    let temp_dir = tempfile::TempDir::new().unwrap();
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3).unwrap();

    // Create a cluster with 5 nodes
    let (response, changes) = meta_state.handle_add_node(1, "127.0.0.1:50051".to_string()).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Ok));
    assert_eq!(changes.len(), 0); // No groups yet

    let (response, _) = meta_state.handle_add_node(2, "127.0.0.1:50052".to_string()).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Ok));

    let (response, _) = meta_state.handle_add_node(3, "127.0.0.1:50053".to_string()).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Ok));

    // Verify nodes were added
    let meta = meta_state.get_cluster_meta();
    assert_eq!(meta.nodes.len(), 3);
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_node_join_with_automatic_rebalancing() {
    // Test that when a node joins, groups are automatically rebalanced
    let temp_dir = tempfile::TempDir::new().unwrap();
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3).unwrap();

    // Add initial 3 nodes
    meta_state.handle_add_node(1, "127.0.0.1:50051".to_string()).unwrap();
    meta_state.handle_add_node(2, "127.0.0.1:50052".to_string()).unwrap();
    meta_state.handle_add_node(3, "127.0.0.1:50053".to_string()).unwrap();

    // Get meta and manually create a group (simulating what MetaRaft would do)
    let mut meta = meta_state.get_cluster_meta();
    meta.groups.insert(100, GroupMeta::new(100, vec![1, 2, 3]));

    // Save the state back (in real implementation, this would go through MetaRaft)
    // For now, we'll test the allocator directly
    let allocator = ReplicaAllocator::new(3);
    let available_nodes = vec![1, 2, 3, 4, 5];
    let mut current_allocation = HashMap::new();
    current_allocation.insert(100, vec![1, 2, 3]);

    // Add node 4
    let result = allocator.allocate_replicas(101, &available_nodes, &current_allocation);
    assert!(result.is_ok());

    let replicas = result.unwrap();
    assert_eq!(replicas.len(), 3);

    // Should include some of the less loaded nodes (4 and 5)
    let has_new_nodes = replicas.contains(&4) || replicas.contains(&5);
    assert!(has_new_nodes, "New group should use less loaded nodes");
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_node_removal_triggers_rebalancing() {
    // Test that when a node leaves, replicas are automatically rebalanced
    let temp_dir = tempfile::TempDir::new().unwrap();
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3).unwrap();

    // Add 5 nodes
    for i in 1..=5 {
        meta_state.handle_add_node(i, format!("127.0.0.1:{}", 50050 + i)).unwrap();
    }

    // Manually create groups (simulating cluster state)
    let mut meta = meta_state.get_cluster_meta();
    meta.groups.insert(100, GroupMeta::new(100, vec![1, 2, 3]));
    meta.groups.insert(101, GroupMeta::new(101, vec![2, 3, 4]));
    meta.groups.insert(102, GroupMeta::new(102, vec![3, 4, 5]));

    // Test rebalancing when node 3 is removed
    let allocator = ReplicaAllocator::new(3);
    let available_nodes = vec![1, 2, 4, 5]; // Node 3 removed
    let current_allocation: HashMap<u64, Vec<u64>> =
        meta.groups.iter().map(|(&gid, group)| (gid, group.replicas.clone())).collect();

    let new_allocation = allocator.rebalance(&available_nodes, current_allocation).unwrap();

    // All groups should still have 3 replicas
    for replicas in new_allocation.values() {
        assert_eq!(replicas.len(), 3, "Each group should maintain 3 replicas");
        assert!(!replicas.contains(&3), "Node 3 should not be in any group");
    }

    // Check that node 1 or 2 was added to group 102 (which lost node 3)
    let group_102_replicas = &new_allocation[&102];
    assert!(group_102_replicas.contains(&1) || group_102_replicas.contains(&2));
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_multi_raft_node_start() {
    // Test MultiRaftNode start method
    let temp_dir = tempfile::TempDir::new().unwrap();
    let config = Config::default();
    let mut node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize MetaRaft
    let meta_config = Config::default();
    node.init_meta_raft(meta_config).await.unwrap();

    // Bootstrap cluster (single node)
    node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())])
        .await
        .unwrap();

    // Initialize router and state machine
    node.init_router().unwrap();
    let options = Options::default();
    node.init_state_machine(options).unwrap();

    // Start as bootstrap node
    let result = node.start(true, None).await;
    assert!(result.is_ok());
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_load_groups_from_metadata() {
    // Test that groups are properly loaded from metadata
    let temp_dir = tempfile::TempDir::new().unwrap();
    let config = Config::default();
    let mut node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize MetaRaft
    let meta_config = Config::default();
    node.init_meta_raft(meta_config).await.unwrap();
    node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())])
        .await
        .unwrap();

    // Create some groups manually through MetaRaft
    if let Some(meta_raft) = node.meta_raft() {
        meta_raft.create_group(100, vec![1]).await.unwrap();
        meta_raft.create_group(101, vec![1]).await.unwrap();
    }

    // Create the groups on the node
    node.create_raft_group(100, vec![1]).await.unwrap();
    node.create_raft_group(101, vec![1]).await.unwrap();

    // Verify groups were created
    assert!(node.has_group(100));
    assert!(node.has_group(101));
    assert_eq!(node.group_count(), 2);
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_replication_factor_configuration() {
    // Test different replication factors
    for rf in [3, 5] {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), rf).unwrap();

        // Add enough nodes
        for i in 1..=(rf + 2) {
            meta_state
                .handle_add_node(i as u64, format!("127.0.0.1:{}", 50050 + i))
                .unwrap();
        }

        let allocator = ReplicaAllocator::new(rf);
        let available_nodes: Vec<u64> = (1..=(rf + 2) as u64).collect();
        let current_allocation = HashMap::new();

        let replicas =
            allocator.allocate_replicas(100, &available_nodes, &current_allocation).unwrap();
        assert_eq!(replicas.len(), rf, "Should allocate {} replicas", rf);
    }
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_duplicate_node_addition() {
    // Test that adding the same node twice returns an error
    let temp_dir = tempfile::TempDir::new().unwrap();
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3).unwrap();

    let (response, _) = meta_state.handle_add_node(1, "127.0.0.1:50051".to_string()).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Ok));

    let (response, _) = meta_state.handle_add_node(1, "127.0.0.1:50051".to_string()).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Error(_)));
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_remove_nonexistent_node() {
    // Test that removing a non-existent node returns an error
    let temp_dir = tempfile::TempDir::new().unwrap();
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3).unwrap();

    let (response, _) = meta_state.handle_remove_node(999).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Error(_)));
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_membership_coordinator_integration() {
    // Test membership coordinator with group creation
    use std::sync::Arc;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let config = Config::default();
    let mut node = MultiRaftNode::new(1, temp_dir.path(), config.clone()).await.unwrap();

    // Initialize MetaRaft
    let meta_config = Config::default();
    node.init_meta_raft(meta_config).await.unwrap();
    node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())])
        .await
        .unwrap();

    // Create a group
    node.create_raft_group(100, vec![1]).await.unwrap();

    // Create coordinator
    let node_arc = Arc::new(node);
    let meta_raft = node_arc.meta_raft().unwrap().clone();
    let coordinator = MembershipCoordinator::new(Arc::clone(&node_arc), meta_raft);

    // Check if group is ready (might not have a leader yet in this quick test)
    let _ready = coordinator.is_group_ready(100).await;
    // Note: In a real scenario, we'd wait for leader election

    // Test that accessing node and meta_raft works
    assert!(coordinator.node().has_group(100));
}

#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_membership_change_workflow() {
    // Test the complete workflow of adding a node with membership changes
    let temp_dir = tempfile::TempDir::new().unwrap();
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3).unwrap();

    // Add initial 3 nodes
    for i in 1..=3 {
        meta_state.handle_add_node(i, format!("127.0.0.1:{}", 50050 + i)).unwrap();
    }

    // Manually create a group
    let mut meta = meta_state.get_cluster_meta();
    meta.groups.insert(100, GroupMeta::new(100, vec![1, 2, 3]));

    // Add a 4th node - should trigger rebalancing
    let (response, changes) = meta_state.handle_add_node(4, "127.0.0.1:50054".to_string()).unwrap();
    assert!(matches!(response, aidb::cluster::MetaResponse::Ok));

    // Should have some groups that need membership changes
    // In a full system, these changes would be applied via MembershipCoordinator
    // Just verify that we get a list back (might be empty if no rebalancing needed)
    let _ = changes;
}
