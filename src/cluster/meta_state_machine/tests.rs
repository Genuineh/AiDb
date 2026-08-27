use super::*;
use std::collections::HashMap;

use tempfile::TempDir;

use crate::cluster::meta_types::{
    default_slot_table, ClusterMeta, MetaRequest, SlotMigrationState, SlotStatus, SLOT_COUNT,
};
use crate::cluster::types::ClusterError;
use crate::config::Options;
use crate::error::Error;
use crate::DB;

fn sm() -> (MetaStateMachine, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    (MetaStateMachine::new(db).unwrap(), dir)
}

fn register(sm: &MetaStateMachine, id: u64, addr: &str) {
    sm.apply_meta_request(MetaRequest::RegisterNode {
        node_id: id,
        rpc_addr: addr.into(),
        client_addr: None,
        tags: HashMap::new(),
    })
    .unwrap();
}

#[test]
fn test_cluster_meta_default() {
    let (sm, _dir) = sm();
    let meta = sm.get_cluster_meta();
    assert_eq!(meta.cluster_id, "uninitialized");
    assert!(meta.nodes.is_empty());
}

#[test]
fn test_slot_table_default_size() {
    let (sm, _dir) = sm();
    assert_eq!(sm.get_slot_table().len(), SLOT_COUNT);
    assert!(sm
        .get_slot_table()
        .iter()
        .all(|s| *s == SlotStatus::Unallocated));
}

#[test]
fn test_register_node_duplicate() {
    let (sm, _dir) = sm();
    register(&sm, 1, "http://127.0.0.1:1");
    let err = sm
        .validate_meta_request(&MetaRequest::RegisterNode {
            node_id: 1,
            rpc_addr: "http://127.0.0.1:2".into(),
            client_addr: None,
            tags: HashMap::new(),
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));
}

#[test]
fn test_rebuild_slot_ranges() {
    let mut table = default_slot_table();
    table[10] = SlotStatus::Assigned(1);
    table[11] = SlotStatus::Assigned(1);
    table[20] = SlotStatus::Assigned(1);
    assert_eq!(rebuild_slot_ranges(&table, 1), vec![(10, 11), (20, 20)]);
}

#[test]
fn test_assign_and_migration_flow() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    register(&sm, 2, "a:2");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true), (2, false)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 2,
        initial_replicas: vec![(2, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::AssignSlots {
        group_id: 1,
        slots: vec![0, 1, 2],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Migrating(1));
    sm.apply_meta_request(MetaRequest::UpdateMigrationProgress {
        progress: 1,
        total: 10,
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    sm.apply_meta_request(MetaRequest::MarkMigrationReady)
        .unwrap();
    sm.apply_meta_request(MetaRequest::CommitSlotMigration)
        .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Assigned(2));
    assert!(sm.get_migration_state().is_none());
}

// ── L1 error path tests ──

#[test]
fn test_register_node_empty_addr() {
    let (sm, _dir) = sm();
    let err = sm
        .validate_meta_request(&MetaRequest::RegisterNode {
            node_id: 1,
            rpc_addr: String::new(),
            client_addr: None,
            tags: HashMap::new(),
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(_)));
}

#[test]
fn test_remove_nonexistent_node() {
    let (sm, _dir) = sm();
    let err = sm
        .validate_meta_request(&MetaRequest::RemoveNode { node_id: 99 })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(s) if s.contains("not found")));
}

#[test]
fn test_remove_node_with_active_groups() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::RemoveNode { node_id: 1 })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(s) if s.contains("active groups")));
}

#[test]
fn test_create_group_invalid_node() {
    let (sm, _dir) = sm();
    let err = sm
        .validate_meta_request(&MetaRequest::CreateGroup {
            group_id: 1,
            initial_replicas: vec![(99, true)],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(s) if s.contains("not found")));
}

#[test]
fn test_create_group_duplicate_replica() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    let err = sm
        .validate_meta_request(&MetaRequest::CreateGroup {
            group_id: 1,
            initial_replicas: vec![(1, true), (1, false)],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(s) if s.contains("duplicate")));
}

#[test]
fn test_create_group_empty_replicas() {
    let (sm, _dir) = sm();
    let err = sm
        .validate_meta_request(&MetaRequest::CreateGroup {
            group_id: 1,
            initial_replicas: vec![],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(_)));
}

#[test]
fn test_assign_slots_out_of_range() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::AssignSlots {
            group_id: 1,
            slots: vec![16384],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(s) if s.contains("out of range")));
}

#[test]
fn test_assign_slots_empty() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::AssignSlots {
            group_id: 1,
            slots: vec![],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(_)));
}

#[test]
fn test_assign_slots_duplicate() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::AssignSlots {
            group_id: 1,
            slots: vec![0, 0],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(s) if s.contains("duplicate")));
}

#[test]
fn test_begin_migration_same_group() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::BeginSlotMigration {
            source_group: 1,
            target_group: 1,
            slots: vec![0],
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidConfig(_)));
}

#[test]
fn test_commit_without_migration() {
    let (sm, _dir) = sm();
    let err = sm
        .validate_meta_request(&MetaRequest::CommitSlotMigration)
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(s) if s.contains("ReadyToCommit")));
}

#[test]
fn test_cancel_migration_restores_slots() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    register(&sm, 2, "a:2");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 2,
        initial_replicas: vec![(2, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::AssignSlots {
        group_id: 1,
        slots: vec![0, 1],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Migrating(1));
    sm.apply_meta_request(MetaRequest::CancelSlotMigration)
        .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Assigned(1));
    assert!(sm.get_migration_state().is_none());
}

/// FIX-0056-A1: `migration_epoch` 与 `migration_state` 同生命周期 ——
/// Begin 后立即可读, 且等于 apply 后的新 `cluster_meta.version` (与
/// `SlotMigrationManager::start_migration` 事后读到的 `migration_id`
/// 一致); Cancel 后清空.
#[test]
fn test_migration_epoch_lifecycle_on_begin_and_cancel() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    register(&sm, 2, "a:2");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 2,
        initial_replicas: vec![(2, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::AssignSlots {
        group_id: 1,
        slots: vec![0, 1],
    })
    .unwrap();
    assert!(
        sm.get_migration_epoch().is_none(),
        "无活跃迁移时 epoch 必须为 None"
    );

    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    let epoch = sm.get_migration_epoch().expect("Begin 后 epoch 必须可读");
    assert_eq!(
        epoch,
        sm.get_cluster_meta().version,
        "epoch 必须等于 Begin apply 后的新 cluster_meta.version"
    );

    sm.apply_meta_request(MetaRequest::CancelSlotMigration)
        .unwrap();
    assert!(
        sm.get_migration_epoch().is_none(),
        "Cancel 后 epoch 必须随 migration_state 一起清空"
    );
}

#[test]
fn test_version_increment() {
    let (sm, _dir) = sm();
    let v0 = sm.get_cluster_meta().version;
    register(&sm, 1, "a:1");
    let v1 = sm.get_cluster_meta().version;
    assert!(v1 > v0);
    sm.apply_meta_request(MetaRequest::RemoveNode { node_id: 1 })
        .unwrap();
    let v2 = sm.get_cluster_meta().version;
    assert!(v2 > v1);
}

#[test]
fn test_rebuild_slot_ranges_after_commit_and_cancel() {
    let (sm, _dir) = sm();
    register(&sm, 1, "a:1");
    register(&sm, 2, "a:2");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 2,
        initial_replicas: vec![(2, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::AssignSlots {
        group_id: 1,
        slots: vec![0, 1],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::CancelSlotMigration)
        .unwrap();
    let meta = sm.get_cluster_meta();
    let g1 = meta.groups.get(&1).unwrap();
    assert_eq!(g1.slot_ranges, vec![(0, 1)]);
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::UpdateMigrationProgress {
        progress: 1,
        total: 1,
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    sm.apply_meta_request(MetaRequest::MarkMigrationReady)
        .unwrap();
    sm.apply_meta_request(MetaRequest::CommitSlotMigration)
        .unwrap();
    let meta = sm.get_cluster_meta();
    let g1 = meta.groups.get(&1).unwrap();
    let g2 = meta.groups.get(&2).unwrap();
    assert_eq!(g1.slot_ranges, vec![(0, 0)]);
    assert_eq!(g2.slot_ranges, vec![(1, 1)]);
}

#[test]
fn test_format_version_corruption() {
    use crate::cluster::storage::keys::meta_cluster_meta_key;
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    let bad_meta = ClusterMeta {
        format_version: 42,
        ..Default::default()
    };
    db.put(
        &meta_cluster_meta_key(),
        &rmp_serde::to_vec(&bad_meta).unwrap(),
    )
    .unwrap();
    let result = MetaStateMachine::new(db);
    assert!(result.is_err());
    assert!(matches!(result.err().unwrap(), Error::Corruption(_)));
}

// ── F-056: Frozen / ReadyToCommit 相位 ──

fn setup_two_groups_with_slots(sm: &MetaStateMachine) {
    register(sm, 1, "a:1");
    register(sm, 2, "a:2");
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 1,
        initial_replicas: vec![(1, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::CreateGroup {
        group_id: 2,
        initial_replicas: vec![(2, true)],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::AssignSlots {
        group_id: 1,
        slots: vec![0, 1],
    })
    .unwrap();
}

fn begin_and_progress_to_migrating(sm: &MetaStateMachine) {
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    sm.apply_meta_request(MetaRequest::UpdateMigrationProgress {
        progress: 1,
        total: 10,
    })
    .unwrap();
}

#[test]
fn test_freeze_mark_ready_commit_succeeds() {
    let (sm, _dir) = sm();
    setup_two_groups_with_slots(&sm);
    begin_and_progress_to_migrating(&sm);
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Migrating(1));

    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    assert!(matches!(
        sm.get_migration_state(),
        Some(SlotMigrationState::Frozen {
            source_group: 1,
            target_group: 2,
            ..
        })
    ));
    // SlotStatus 保持 Migrating(source) 直到 Commit
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Migrating(1));

    sm.apply_meta_request(MetaRequest::MarkMigrationReady)
        .unwrap();
    assert!(matches!(
        sm.get_migration_state(),
        Some(SlotMigrationState::ReadyToCommit {
            source_group: 1,
            target_group: 2,
            ..
        })
    ));
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Migrating(1));

    sm.apply_meta_request(MetaRequest::CommitSlotMigration)
        .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Assigned(2));
    assert!(sm.get_migration_state().is_none());
}

#[test]
fn test_commit_from_migrating_prepare_frozen_fails() {
    let (sm, _dir) = sm();
    setup_two_groups_with_slots(&sm);

    // Prepare
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::CommitSlotMigration)
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));

    // Migrating
    sm.apply_meta_request(MetaRequest::UpdateMigrationProgress {
        progress: 1,
        total: 10,
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::CommitSlotMigration)
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));

    // Frozen
    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::CommitSlotMigration)
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));
}

#[test]
fn test_mark_ready_from_migrating_fails() {
    let (sm, _dir) = sm();
    setup_two_groups_with_slots(&sm);
    begin_and_progress_to_migrating(&sm);
    let err = sm
        .validate_meta_request(&MetaRequest::MarkMigrationReady)
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));
}

#[test]
fn test_freeze_from_prepare_fails() {
    let (sm, _dir) = sm();
    setup_two_groups_with_slots(&sm);
    sm.apply_meta_request(MetaRequest::BeginSlotMigration {
        source_group: 1,
        target_group: 2,
        slots: vec![1],
    })
    .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::FreezeSlotMigration)
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));
}

#[test]
fn test_cancel_from_frozen_and_ready_restores_slots() {
    let (sm, _dir) = sm();
    setup_two_groups_with_slots(&sm);
    begin_and_progress_to_migrating(&sm);

    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    sm.apply_meta_request(MetaRequest::CancelSlotMigration)
        .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Assigned(1));
    assert!(sm.get_migration_state().is_none());

    begin_and_progress_to_migrating(&sm);
    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    sm.apply_meta_request(MetaRequest::MarkMigrationReady)
        .unwrap();
    sm.apply_meta_request(MetaRequest::CancelSlotMigration)
        .unwrap();
    assert_eq!(sm.get_slot_table()[1], SlotStatus::Assigned(1));
    assert!(sm.get_migration_state().is_none());
}

#[test]
fn test_update_progress_in_frozen_fails() {
    let (sm, _dir) = sm();
    setup_two_groups_with_slots(&sm);
    begin_and_progress_to_migrating(&sm);
    sm.apply_meta_request(MetaRequest::FreezeSlotMigration)
        .unwrap();
    let err = sm
        .validate_meta_request(&MetaRequest::UpdateMigrationProgress {
            progress: 2,
            total: 10,
        })
        .unwrap_err();
    assert!(matches!(err, ClusterError::InvalidState(_)));
}
