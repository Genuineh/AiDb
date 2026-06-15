//! L2 Integration Tests for Multi-Raft
//!
//! These tests combine multiple modules (Router, LifecycleManager, ShardedStorage,
//! MultiRaftNode, RaftServiceDispatcher, MockMetaRaft) to verify end-to-end
//! interactions in a simulated cluster environment.

#![cfg(feature = "cluster")]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use tempfile::TempDir;
use aidb::cluster::lifecycle_manager::MetaRaftProvider;
use aidb::cluster::meta_types::{
  default_slot_table, ClusterMeta, GroupMeta, ReplicaInfo, SlotMigrationState, SlotStatus,
  SlotTable,
};
use aidb::cluster::{
  LifecycleManager, MultiRaftNode, RaftServiceDispatcher, Router, ShardedStorage,
};
use aidb::config::Options;

// ===== Mock MetaRaft for integration tests =====

struct MockMetaRaft {
  inner: Mutex<MockInner>,
}

struct MockInner {
  cluster_meta: ClusterMeta,
  slot_table: SlotTable,
}

impl MockMetaRaft {
  fn new() -> Self {
    Self {
      inner: Mutex::new(MockInner {
        cluster_meta: ClusterMeta::default(),
        slot_table: default_slot_table(),
      }),
    }
  }

  fn add_group(&self, group_id: u64, replicas: Vec<(u64, bool)>) {
    let mut inner = self.inner.lock().unwrap();
    inner.cluster_meta.groups.insert(
      group_id,
      GroupMeta {
        group_id,
        replicas: replicas
          .into_iter()
          .map(|(n, l)| ReplicaInfo {
            node_id: n,
            is_leader: l,
          })
          .collect(),
        slot_ranges: vec![],
        config_version: 0,
      },
    );
  }

  fn remove_group(&self, group_id: u64) {
    self
      .inner
      .lock()
      .unwrap()
      .cluster_meta
      .groups
      .remove(&group_id);
  }

  fn assign_slot(&self, slot: u16, group_id: u64) {
    self.inner.lock().unwrap().slot_table[slot as usize] = SlotStatus::Assigned(group_id);
  }
}

impl MetaRaftProvider for MockMetaRaft {
  fn get_cluster_meta(&self) -> ClusterMeta {
    self.inner.lock().unwrap().cluster_meta.clone()
  }

  fn get_slot_table(&self) -> SlotTable {
    self.inner.lock().unwrap().slot_table.clone()
  }

  fn get_migration_state(&self) -> Option<SlotMigrationState> {
    None
  }
}

// ===== Tests =====

/// Verify 2 ShardedStorage instances with different group_ids are fully independent.
#[tokio::test]
async fn test_sharded_storage_independent_groups() {
  let dir = TempDir::new().unwrap();
  let s1 = ShardedStorage::open(dir.path(), 1, Options::for_testing()).unwrap();
  let s2 = ShardedStorage::open(dir.path(), 2, Options::for_testing()).unwrap();

  s1.db().put(b"k1", b"v1").unwrap();
  s2.db().put(b"k1", b"v2").unwrap();

  assert_eq!(s1.db().get(b"k1").unwrap(), Some(b"v1".to_vec()));
  assert_eq!(s2.db().get(b"k1").unwrap(), Some(b"v2".to_vec()));

  s1.db().delete(b"k1").unwrap();
  assert!(s1.db().get(b"k1").unwrap().is_none());
  assert_eq!(s2.db().get(b"k1").unwrap(), Some(b"v2".to_vec()));

  s1.close().unwrap();
  s2.close().unwrap();
}

/// Verify LifecycleManager.tick() discovers new groups and detects removals
/// from the topology.
#[tokio::test]
async fn test_lifecycle_tick_discovers_and_removes_groups() {
  let router = Arc::new(Router::new(
    default_slot_table(),
    HashMap::new(),
    HashMap::new(),
  ));
  let mock = Arc::new(MockMetaRaft::new());
  mock.add_group(1, vec![(1, true)]);

  let manager = LifecycleManager::new(1, router.clone(), mock.clone());

  // Tick discovers group 1 (replica with node_id=1)
  let tick_result = manager.tick();
  assert!(
    tick_result.groups_to_create.contains(&1),
    "should discover group 1"
  );
  assert!(
    tick_result.groups_to_remove.is_empty(),
    "should not remove any group"
  );
  assert!(
    manager.local_groups().read().contains(&1),
    "group 1 should be in local groups"
  );

  // Remove group from mock topology
  mock.remove_group(1);

  // Tick discovers removal
  let tick_result = manager.tick();
  assert!(
    tick_result.groups_to_remove.contains(&1),
    "should detect group 1 as removed"
  );
  assert!(
    tick_result.groups_to_create.is_empty(),
    "should not create any group"
  );
  assert!(
    !manager.local_groups().read().contains(&1),
    "group 1 should be removed from local groups"
  );
}

/// Verify LifecycleManager.tick() refreshes the Router's slot table so that
/// routes become available after a tick.
#[tokio::test]
async fn test_router_refresh_on_lifecycle_tick() {
  let router = Arc::new(Router::new(
    default_slot_table(),
    HashMap::new(),
    HashMap::new(),
  ));
  let mock = Arc::new(MockMetaRaft::new());
  mock.add_group(1, vec![(1, true)]);
  mock.assign_slot(0, 1);

  let manager = LifecycleManager::new(1, router.clone(), mock);

  // Before tick: routing fails because router has empty slot table
  assert!(
    router.route_slot(0).is_err(),
    "should fail before tick (empty slot table)"
  );

  // After tick: router is refreshed from MetaRaft
  manager.tick();
  let (gid, _status) = router
    .route_slot(0)
    .expect("should route slot 0 after tick");
  assert_eq!(gid, 1, "slot 0 should route to group 1");
}

/// Verify MultiRaftNode construction with lifecycle, start, and shutdown.
#[tokio::test]
async fn test_multi_raft_node_construction() {
  let router = Arc::new(Router::new(
    default_slot_table(),
    HashMap::new(),
    HashMap::new(),
  ));
  let dispatcher = Arc::new(RaftServiceDispatcher::new());
  let mock = Arc::new(MockMetaRaft::new());
  let manager = LifecycleManager::new(1, router.clone(), mock);

  let node = MultiRaftNode::new_with_lifecycle(1, router, dispatcher, manager);
  let _rx = node.start_lifecycle();
  node.shutdown().await;
}

/// Verify concurrent open/write/read on ShardedStorage with different group_ids.
#[tokio::test]
async fn test_concurrent_sharded_storage_open() {
  let dir = TempDir::new().unwrap();
  let mut handles = Vec::new();

  for i in 0..10u64 {
    let dir_path = dir.path().to_path_buf();
    handles.push(tokio::spawn(async move {
      let storage = ShardedStorage::open(&dir_path, i, Options::for_testing()).unwrap();
      storage
        .db()
        .put(format!("k_{}", i).as_bytes(), b"v")
        .unwrap();
      let val = storage.db().get(format!("k_{}", i).as_bytes()).unwrap();
      assert_eq!(val, Some(b"v".to_vec()));
      storage.close().unwrap();
    }));
  }

  for h in handles {
    h.await.unwrap();
  }
}

/// Verify LifecycleManager::run() async loop terminates on shutdown signal.
#[tokio::test]
async fn test_lifecycle_run_loop_shutdown() {
  let router = Arc::new(Router::new(
    default_slot_table(),
    HashMap::new(),
    HashMap::new(),
  ));
  let mock = Arc::new(MockMetaRaft::new());
  let manager = LifecycleManager::new(1, router, mock);

  let (tx, rx) = tokio::sync::watch::channel(false);
  let handle = tokio::spawn(async move {
    manager.run(rx).await;
  });

  // Send shutdown signal
  let _ = tx.send(true);

  // Verify the run loop exits within timeout
  tokio::time::timeout(std::time::Duration::from_secs(5), handle)
    .await
    .expect("run loop should exit within 5s after shutdown signal")
    .expect("run loop task should not panic");
}
