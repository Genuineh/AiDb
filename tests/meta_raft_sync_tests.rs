//! Integration tests for Multi-Raft metadata synchronization
//!
//! This test suite verifies that metadata changes (add_node, create_group, etc.)
//! properly synchronize through the MetaRaft consensus mechanism.

#[cfg(feature = "raft-cluster")]
use aidb::cluster::{MetaRaftNode, MetaResponse};
use openraft::Config;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

/// Test that create_group actually updates metadata
#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_create_group_updates_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let config = Config::default();

    // Create a single-node MetaRaft cluster
    let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize the cluster with this node as the only member
    let members = vec![(1, "127.0.0.1:50051".to_string())];
    node.initialize(members).await.unwrap();

    // Wait for leadership to be established
    sleep(Duration::from_millis(500)).await;

    // Verify initial state
    let meta_before = node.get_cluster_meta();
    assert_eq!(meta_before.groups.len(), 0, "Should start with no groups");

    // Create a new group
    let response = node.create_group(100, vec![1]).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok), "create_group should succeed");

    // Wait for the change to be applied
    sleep(Duration::from_millis(200)).await;

    // Verify the group appears in metadata
    let meta_after = node.get_cluster_meta();
    assert_eq!(meta_after.groups.len(), 1, "Group should appear in metadata after create_group");
    assert!(meta_after.groups.contains_key(&100), "Group 100 should exist");

    let group = meta_after.groups.get(&100).unwrap();
    assert_eq!(group.group_id, 100);
    assert_eq!(group.replicas, vec![1]);

    node.shutdown().await.unwrap();
}

/// Test that add_node actually updates metadata
#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_add_node_updates_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let config = Config::default();

    // Create a single-node MetaRaft cluster
    let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize the cluster
    let members = vec![(1, "127.0.0.1:50051".to_string())];
    node.initialize(members).await.unwrap();

    // Wait for leadership to be established
    sleep(Duration::from_millis(500)).await;

    // Verify initial state
    let meta_before = node.get_cluster_meta();
    assert_eq!(meta_before.nodes.len(), 0, "Should start with no nodes in metadata");

    // Add a node through MetaRaft
    let response = node.add_node(2, "127.0.0.1:50052".to_string()).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok), "add_node should succeed");

    // Wait for the change to be applied
    sleep(Duration::from_millis(200)).await;

    // Verify the node appears in metadata
    let meta_after = node.get_cluster_meta();
    assert_eq!(meta_after.nodes.len(), 1, "Node should appear in metadata after add_node");
    assert!(meta_after.nodes.contains_key(&2), "Node 2 should exist in metadata");

    let node_info = meta_after.nodes.get(&2).unwrap();
    assert_eq!(node_info.node_id, 2);
    assert_eq!(node_info.addr, "127.0.0.1:50052");

    node.shutdown().await.unwrap();
}

/// Test that update_slots actually updates metadata
#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_update_slots_updates_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let config = Config::default();

    // Create a single-node MetaRaft cluster
    let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize the cluster
    let members = vec![(1, "127.0.0.1:50051".to_string())];
    node.initialize(members).await.unwrap();

    // Wait for leadership
    sleep(Duration::from_millis(500)).await;

    // First create a group
    let response = node.create_group(100, vec![1]).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok));
    sleep(Duration::from_millis(200)).await;

    // Verify initial slot mapping (should be 0 by default)
    let meta_before = node.get_cluster_meta();
    assert_eq!(meta_before.slot_to_group(0), 0);
    assert_eq!(meta_before.slot_to_group(99), 0);

    // Update slots 0-100 to group 100
    let response = node.update_slots(0, 100, 100).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok), "update_slots should succeed");

    // Wait for the change to be applied
    sleep(Duration::from_millis(200)).await;

    // Verify the slots are updated
    let meta_after = node.get_cluster_meta();
    for slot in 0..100 {
        assert_eq!(
            meta_after.slot_to_group(slot),
            100,
            "Slot {} should be mapped to group 100",
            slot
        );
    }
    assert_eq!(meta_after.slot_to_group(100), 0, "Slot 100 should still be 0 (exclusive end)");

    node.shutdown().await.unwrap();
}

/// Test multiple metadata operations in sequence
#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_multiple_metadata_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config = Config::default();

    // Create a single-node MetaRaft cluster
    let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize the cluster
    let members = vec![(1, "127.0.0.1:50051".to_string())];
    node.initialize(members).await.unwrap();

    // Wait for leadership
    sleep(Duration::from_millis(500)).await;

    // Add multiple nodes
    for i in 2..=4 {
        let response = node.add_node(i, format!("127.0.0.1:{}", 50050 + i)).await.unwrap();
        assert!(matches!(response, MetaResponse::Ok));
    }
    sleep(Duration::from_millis(200)).await;

    // Create multiple groups
    let response = node.create_group(100, vec![1, 2, 3]).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok));
    let response = node.create_group(101, vec![2, 3, 4]).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok));
    sleep(Duration::from_millis(200)).await;

    // Update different slot ranges
    let response = node.update_slots(0, 8192, 100).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok));
    let response = node.update_slots(8192, 16384, 101).await.unwrap();
    assert!(matches!(response, MetaResponse::Ok));
    sleep(Duration::from_millis(200)).await;

    // Verify all changes
    let meta = node.get_cluster_meta();

    // Check nodes
    assert_eq!(meta.nodes.len(), 3, "Should have 3 nodes (2, 3, 4)");
    for i in 2..=4 {
        assert!(meta.nodes.contains_key(&i), "Node {} should exist", i);
    }

    // Check groups
    assert_eq!(meta.groups.len(), 2, "Should have 2 groups");
    assert!(meta.groups.contains_key(&100));
    assert!(meta.groups.contains_key(&101));

    // Check slot mappings
    assert_eq!(meta.slot_to_group(0), 100);
    assert_eq!(meta.slot_to_group(8191), 100);
    assert_eq!(meta.slot_to_group(8192), 101);
    assert_eq!(meta.slot_to_group(16383), 101);

    node.shutdown().await.unwrap();
}

/// Test config version increments on metadata changes
#[cfg(feature = "raft-cluster")]
#[tokio::test]
async fn test_config_version_increments() {
    let temp_dir = TempDir::new().unwrap();
    let config = Config::default();

    // Create a single-node MetaRaft cluster
    let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

    // Initialize the cluster
    let members = vec![(1, "127.0.0.1:50051".to_string())];
    node.initialize(members).await.unwrap();

    // Wait for leadership
    sleep(Duration::from_millis(500)).await;

    let initial_version = node.get_cluster_meta().config_version;

    // Add a node - should increment version
    node.add_node(2, "127.0.0.1:50052".to_string()).await.unwrap();
    sleep(Duration::from_millis(200)).await;
    let version_after_add = node.get_cluster_meta().config_version;
    assert!(
        version_after_add > initial_version,
        "Config version should increment after add_node"
    );

    // Create a group - should increment version
    node.create_group(100, vec![1]).await.unwrap();
    sleep(Duration::from_millis(200)).await;
    let version_after_create = node.get_cluster_meta().config_version;
    assert!(
        version_after_create > version_after_add,
        "Config version should increment after create_group"
    );

    // Update slots - should increment version
    node.update_slots(0, 100, 100).await.unwrap();
    sleep(Duration::from_millis(200)).await;
    let version_after_update = node.get_cluster_meta().config_version;
    assert!(
        version_after_update > version_after_create,
        "Config version should increment after update_slots"
    );

    node.shutdown().await.unwrap();
}
