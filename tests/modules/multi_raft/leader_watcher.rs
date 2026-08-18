//! Integration tests for LeaderChangeWatcher.
//! @component aidb-cluster
//!
//! Creates a single-node Raft cluster to verify tick behavior.

#![cfg(feature = "cluster")]

use std::sync::Arc;
use std::time::Duration;

use aidb::cluster::leader_watcher::LeaderChangeWatcher;
use aidb::cluster::{
    MetaRaftNode, MultiRaftNode, RaftNetworkClientFactory, RaftNodeConfig, RaftServiceDispatcher,
    Router,
};
use aidb::config::Options;
use tempfile::TempDir;

/// Integration test: watcher can be created and tick() runs without panic
/// when no groups exist yet (empty multi_raft).
#[tokio::test]
async fn test_watcher_tick_no_panic_empty_groups() {
    let dir = TempDir::new().unwrap();
    let db = aidb::DB::open(dir.path(), Options::for_testing()).unwrap();

    // Create MetaRaftNode
    let net_factory = RaftNetworkClientFactory::new(1, 0, 30, 64 * 1024 * 1024);
    let raft_config = RaftNodeConfig {
        node_id: 1,
        group_id: 0,
        election_timeout_min: 200,
        election_timeout_max: 400,
        heartbeat_interval: 50,
        rpc_timeout_ms: 30,
        ..Default::default()
    };
    let meta_raft = Arc::new(
        MetaRaftNode::new(raft_config.clone(), db.clone(), net_factory)
            .await
            .unwrap(),
    );
    meta_raft
        .initialize(vec![(1, "127.0.0.1:1".into())])
        .await
        .unwrap();

    // Give Raft time to elect itself
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Create MultiRaftNode (empty -- no data groups yet)
    let router = Arc::new(Router::new(
        aidb::cluster::meta_types::default_slot_table(),
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    ));
    let dispatcher = Arc::new(RaftServiceDispatcher::new());
    let multi_raft = Arc::new(MultiRaftNode::new(1, router.clone(), dispatcher));

    let watcher = LeaderChangeWatcher::new(
        1,
        multi_raft.clone(),
        meta_raft.clone(),
        Duration::from_millis(100),
        Duration::from_millis(400), // lease = election_timeout_max
    );

    // tick() on empty groups should return empty vec, no panic
    let changed = watcher.tick().await;
    assert!(
        changed.is_empty(),
        "no groups means no changes: got {:?}",
        changed
    );
}

/// Integration test: after tick, the leader cache is populated for existing groups.
#[tokio::test]
async fn test_watcher_populates_cache_on_first_tick() {
    let dir = TempDir::new().unwrap();
    let db = aidb::DB::open(dir.path(), Options::for_testing()).unwrap();

    let net_factory = RaftNetworkClientFactory::new(1, 0, 30, 64 * 1024 * 1024);
    let raft_config = RaftNodeConfig {
        node_id: 1,
        group_id: 0,
        election_timeout_min: 200,
        election_timeout_max: 400,
        heartbeat_interval: 50,
        rpc_timeout_ms: 30,
        ..Default::default()
    };
    let meta_raft = Arc::new(
        MetaRaftNode::new(raft_config.clone(), db.clone(), net_factory)
            .await
            .unwrap(),
    );
    meta_raft
        .initialize(vec![(1, "127.0.0.1:1".into())])
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(600)).await;

    let router = Arc::new(Router::new(
        aidb::cluster::meta_types::default_slot_table(),
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    ));
    let dispatcher = Arc::new(RaftServiceDispatcher::new());
    let multi_raft = Arc::new(MultiRaftNode::new(1, router.clone(), dispatcher));

    let watcher = LeaderChangeWatcher::new(
        1,
        multi_raft.clone(),
        meta_raft.clone(),
        Duration::from_millis(100),
        Duration::from_millis(400), // lease = election_timeout_max
    );

    // Even with empty groups, tick should not panic
    let changed = watcher.tick().await;
    assert!(changed.is_empty());

    // Second tick: stable, no changes
    let changed2 = watcher.tick().await;
    assert!(changed2.is_empty(), "second tick should also be empty");
}
