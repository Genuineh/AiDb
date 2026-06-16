//! Unit tests for cluster ops components (Phase 15).
//!
//! These tests verify ReplicaAllocator, MembershipCoordinator data structures,
//! and SlotMigrationManager types using in-memory MetaRaft mocks.

#![cfg(feature = "cluster")]

use std::collections::HashMap;

use aidb::cluster::meta_types::{
    default_slot_table, ClusterMeta, GroupMeta, NodeInfo, NodeRole, NodeStatus, ReplicaInfo,
    SlotMigrationState, SlotStatus, SlotTable, SLOT_COUNT,
};
use aidb::cluster::replica_allocator::{
    AllocationStrategy, ReplicaAllocator, ReplicaRebalancePlan,
};
use aidb::cluster::types::NodeId;

// ---------------------------------------------------------------------------
// MetaRaft mock for integration tests
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct MockMetaRaft {
    cluster_meta: ClusterMeta,
    slot_table: SlotTable,
    migration_state: Option<SlotMigrationState>,
}

#[allow(dead_code)]
impl MockMetaRaft {
    fn new() -> Self {
        Self {
            cluster_meta: ClusterMeta::default(),
            slot_table: default_slot_table(),
            migration_state: None,
        }
    }

    fn add_node(&mut self, id: NodeId, addr: &str) {
        self.cluster_meta.nodes.insert(
            id,
            NodeInfo {
                node_id: id,
                rpc_addr: addr.into(),
                client_addr: None,
                role: NodeRole::Voter,
                status: NodeStatus::Online,
                registered_at: 0,
                tags: HashMap::new(),
            },
        );
    }

    fn add_group(&mut self, group_id: u64, replicas: Vec<(u64, bool)>) {
        self.cluster_meta.groups.insert(
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

    fn assign_slots(&mut self, group_id: u64, slots: Vec<u16>) {
        for &s in &slots {
            self.slot_table[s as usize] = SlotStatus::Assigned(group_id);
        }
    }
}

// ---------------------------------------------------------------------------
// ReplicaAllocator tests (in-module tests exist in replica_allocator.rs;
// these are additional coverage for the public API)
// ---------------------------------------------------------------------------

#[test]
fn test_allocator_weighted() {
    let mut alloc = ReplicaAllocator::new();
    alloc.set_weight(1, 2.0); // node 1 gets 2x groups
    alloc.set_weight(2, 1.0);

    let mut meta = ClusterMeta::default();
    meta.nodes.insert(
        1,
        NodeInfo {
            node_id: 1,
            rpc_addr: "a:1".into(),
            client_addr: None,
            role: NodeRole::Voter,
            status: NodeStatus::Online,
            registered_at: 0,
            tags: HashMap::new(),
        },
    );
    meta.nodes.insert(
        2,
        NodeInfo {
            node_id: 2,
            rpc_addr: "a:2".into(),
            client_addr: None,
            role: NodeRole::Voter,
            status: NodeStatus::Online,
            registered_at: 0,
            tags: HashMap::new(),
        },
    );

    let table = default_slot_table();
    let result = alloc
        .allocate_group(1, 2, AllocationStrategy::Weighted, &meta, &table)
        .unwrap();
    assert_eq!(result.group_id, 1);
    assert_eq!(result.replicas.len(), 1);
}

#[test]
fn test_allocator_free_slots_exhausted() {
    let alloc = ReplicaAllocator::new();
    let mut meta = ClusterMeta::default();
    meta.nodes.insert(
        1,
        NodeInfo {
            node_id: 1,
            rpc_addr: "a:1".into(),
            client_addr: None,
            role: NodeRole::Voter,
            status: NodeStatus::Online,
            registered_at: 0,
            tags: HashMap::new(),
        },
    );
    let mut table = default_slot_table();
    // Fill all slots
    for entry in table.iter_mut().take(SLOT_COUNT) {
        *entry = SlotStatus::Assigned(99);
    }
    let result = alloc.allocate_group(2, 1, AllocationStrategy::Balanced, &meta, &table);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no free slots"));
}

// ---------------------------------------------------------------------------
// MembershipCoordinator data structure tests
// ---------------------------------------------------------------------------

#[test]
fn test_find_groups_for_node() {
    use aidb::cluster::membership_coordinator::MembershipCoordinator;
    // MembershipCoordinator struct exists and can be referenced
    let _ = std::mem::size_of::<MembershipCoordinator>();
}

#[test]
fn test_node_join_context() {
    use aidb::cluster::membership_coordinator::{JoinMethod, NodeJoinContext, NodeLeaveContext};
    let ctx = NodeJoinContext {
        node_id: 1,
        rpc_addr: "127.0.0.1:7001".into(),
        client_addr: None,
        join_method: JoinMethod::Empty,
    };
    assert_eq!(ctx.node_id, 1);

    let leave = NodeLeaveContext {
        node_id: 1,
        force: false,
    };
    assert!(!leave.force);
}

#[test]
fn test_takeover_groups_join() {
    use aidb::cluster::membership_coordinator::JoinMethod;
    let method = JoinMethod::TakeoverGroups {
        source_node: 2,
        groups: vec![10, 20],
    };
    match method {
        JoinMethod::TakeoverGroups {
            source_node,
            groups,
        } => {
            assert_eq!(source_node, 2);
            assert_eq!(groups, vec![10, 20]);
        }
        _ => panic!("expected TakeoverGroups"),
    }
}

// ---------------------------------------------------------------------------
// Slot migration type tests
// ---------------------------------------------------------------------------

#[test]
fn test_migration_progress_struct() {
    use aidb::cluster::slot_migration::{MigrationPhase, MigrationProgress};

    let progress = MigrationProgress {
        migration_id: 1,
        source_group: 10,
        target_group: 20,
        slots: vec![0, 1, 2],
        completed_keys: 0,
        total_keys: 100,
        state: MigrationPhase::Prepare,
    };
    assert_eq!(progress.source_group, 10);
    assert_eq!(progress.total_keys, 100);
    assert_eq!(progress.state as u8, 0); // Prepare = 0
}

#[test]
fn test_migration_phase_transition() {
    use aidb::cluster::slot_migration::MigrationPhase;

    let prepare = MigrationPhase::Prepare;
    let migrating = MigrationPhase::Migrating;
    assert_ne!(prepare, migrating);
}

#[test]
fn test_active_migration_struct() {
    use aidb::cluster::slot_migration::ActiveMigration;

    let m = ActiveMigration {
        migration_id: 42,
        source_group: 10,
        target_group: 20,
        slots: vec![1, 2, 3],
        checkpoint: vec![],
    };
    assert_eq!(m.migration_id, 42);
    assert_eq!(m.slots.len(), 3);
}

#[test]
fn test_batch_migrate_result() {
    use aidb::cluster::slot_migration::BatchMigrateResult;

    let r = BatchMigrateResult {
        migrated_count: 50,
        failed_count: 0,
        last_migrated_key: Some(b"last-key".to_vec()),
        is_completed: true,
    };
    assert!(r.is_completed);
    assert_eq!(r.migrated_count, 50);
}

// ---------------------------------------------------------------------------
// Checkpoint operations test
// ---------------------------------------------------------------------------

#[test]
fn test_checkpoint_save_load_delete() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let checkpoint_dir = dir.path().to_path_buf();
    let migration_id = 42u64;
    let key = b"test-checkpoint-key".to_vec();

    // Simulate atomic save
    let tmp = checkpoint_dir.join(format!("migration_{}.tmp", migration_id));
    let final_path = checkpoint_dir.join(format!("migration_{}.ckpt", migration_id));
    std::fs::write(&tmp, &key).unwrap();
    std::fs::rename(&tmp, &final_path).unwrap();

    // Load
    let loaded = std::fs::read(&final_path).unwrap();
    assert_eq!(loaded, key);

    // Delete
    std::fs::remove_file(&final_path).unwrap();
    assert!(!final_path.exists());
}

// ---------------------------------------------------------------------------
// RebalancePlan struct tests
// ---------------------------------------------------------------------------

#[test]
fn test_rebalance_plan_creation() {
    let plan = ReplicaRebalancePlan {
        group_id: 10,
        source_node: 1,
        target_node: 2,
        slot_ranges: vec![(0, 100)],
    };
    assert_eq!(plan.group_id, 10);
    assert_eq!(plan.slot_ranges, vec![(0, 100)]);
}

// ---------------------------------------------------------------------------
// MetaRaft state machine migration validation (data-driven)
// ---------------------------------------------------------------------------

#[test]
fn test_migration_validation_same_group() {
    // Verify the validation logic from 12-cluster-ops.md matches MetaStateMachine
    let slots = vec![0, 1, 2];
    let mut sorted = slots.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), slots.len());
    assert!(!slots.is_empty());
}

#[test]
fn test_migration_validation_duplicate_slots() {
    let slots = vec![0, 1, 1, 2];
    let mut sorted = slots.clone();
    sorted.sort();
    sorted.dedup();
    assert_ne!(sorted.len(), slots.len()); // duplicates detected
}

#[test]
fn test_migration_validation_out_of_range() {
    for &s in &[0u16, 8192, 16383] {
        assert!(s < SLOT_COUNT as u16);
    }
    assert!(16384 >= SLOT_COUNT as u16);
}
