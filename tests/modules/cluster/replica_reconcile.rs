//! 集成测试: 集群副本自动对账.
//!
//! 验证 LifecycleManager + MultiRaftNode 的 drift 检测和成员变更流程.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use aidb::cluster::lifecycle_manager::{LifecycleManager, MetaRaftProvider};
use aidb::cluster::meta_types::{
    default_slot_table, ClusterMeta, GroupMeta, NodeInfo, NodeRole, NodeStatus, ReplicaInfo,
    SlotMigrationState, SlotTable,
};
use aidb::cluster::router::Router;
use aidb::cluster::types::NodeId;

// ============================================================================
// Mock infrastructure
// ============================================================================

struct MockMetaRaft {
    meta: Mutex<ClusterMeta>,
}

impl MockMetaRaft {
    fn new(meta: ClusterMeta) -> Self {
        Self {
            meta: Mutex::new(meta),
        }
    }

    fn update_meta(&self, f: impl FnOnce(&mut ClusterMeta)) {
        f(&mut self.meta.lock().unwrap());
    }
}

impl MetaRaftProvider for MockMetaRaft {
    fn get_cluster_meta(&self) -> ClusterMeta {
        self.meta.lock().unwrap().clone()
    }

    fn get_slot_table(&self) -> SlotTable {
        default_slot_table()
    }

    fn get_migration_state(&self) -> Option<SlotMigrationState> {
        None
    }
}

fn make_test_meta(node_ids: &[u64], groups: HashMap<u64, Vec<u64>>) -> ClusterMeta {
    let mut nodes = HashMap::new();
    for &nid in node_ids {
        nodes.insert(
            nid,
            NodeInfo {
                node_id: nid,
                rpc_addr: format!("127.0.0.1:{}", 7000 + nid),
                client_addr: Some(format!("127.0.0.1:{}", 9000 + nid)),
                role: NodeRole::Voter,
                status: NodeStatus::Online,
                registered_at: 0,
                tags: HashMap::new(),
            },
        );
    }

    let mut group_metas = HashMap::new();
    for (gid, replica_ids) in &groups {
        group_metas.insert(
            *gid,
            GroupMeta {
                group_id: *gid,
                replicas: replica_ids
                    .iter()
                    .enumerate()
                    .map(|(i, &nid)| ReplicaInfo {
                        node_id: nid,
                        is_leader: i == 0,
                    })
                    .collect(),
                slot_ranges: vec![(0, 100)],
                config_version: 1,
            },
        );
    }

    ClusterMeta {
        cluster_id: "test".into(),
        nodes,
        groups: group_metas,
        version: 1,
        format_version: 1,
    }
}

fn make_router() -> Arc<Router> {
    Arc::new(Router::new(
        default_slot_table(),
        HashMap::new(),
        HashMap::new(),
    ))
}

// ============================================================================
// tick() expected_memberships 测试
// ============================================================================

#[test]
fn tick_returns_expected_memberships_for_local_groups() {
    let node_id = 1u64;
    let mut groups = HashMap::new();
    groups.insert(10, vec![1, 2]); // 包含 node 1
    groups.insert(20, vec![3, 4]); // 不包含 node 1
    let meta = make_test_meta(&[1, 2, 3, 4], groups);

    let mock = Arc::new(MockMetaRaft::new(meta));
    let router = make_router();
    let mgr = LifecycleManager::new(node_id, router, mock);

    let result = mgr.tick();

    // group 10 包含 node 1 — 应有期望成员.
    let members = result.expected_memberships.get(&10).unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.contains(&1));
    assert!(members.contains(&2));

    // group 20 不含 node 1 — 不应出现.
    assert!(!result.expected_memberships.contains_key(&20));
}

#[test]
fn tick_expected_memberships_empty_when_no_local_groups() {
    let node_id = 1u64;
    let mut groups = HashMap::new();
    groups.insert(10, vec![2, 3]); // node 1 不在任何 group 中
    let meta = make_test_meta(&[1, 2, 3], groups);

    let mock = Arc::new(MockMetaRaft::new(meta));
    let router = make_router();
    let mgr = LifecycleManager::new(node_id, router, mock);

    let result = mgr.tick();
    assert!(result.expected_memberships.is_empty());
    assert!(result.groups_to_create.is_empty());
}

#[test]
fn tick_handles_empty_replicas_in_group() {
    let node_id = 1u64;
    // Create a group where node 1 is the sole replica (single-member group).
    let mut groups = HashMap::new();
    groups.insert(
        10,
        GroupMeta {
            group_id: 10,
            replicas: vec![ReplicaInfo {
                node_id: 1,
                is_leader: true,
            }],
            slot_ranges: vec![(0, 100)],
            config_version: 1,
        },
    );
    let meta = make_test_meta(&[1, 2], HashMap::new());
    // Override with our custom group
    let mut meta = meta;
    meta.groups = groups;

    let mock = Arc::new(MockMetaRaft::new(meta));
    let router = make_router();
    let mgr = LifecycleManager::new(node_id, router, mock);

    let result = mgr.tick();
    assert!(result.expected_memberships.contains_key(&10));
}

// ============================================================================
// Drift 计算逻辑测试 (纯函数).
// ============================================================================

fn compute_drift(
    expected: &BTreeSet<NodeId>,
    actual: &BTreeSet<NodeId>,
) -> (Vec<NodeId>, Vec<NodeId>) {
    let to_add: Vec<NodeId> = expected.difference(actual).copied().collect();
    let to_remove: Vec<NodeId> = actual.difference(expected).copied().collect();
    (to_add, to_remove)
}

#[test]
fn drift_no_drift_when_membership_matches() {
    let expected: BTreeSet<NodeId> = [1, 2].iter().copied().collect();
    let actual: BTreeSet<NodeId> = [1, 2].iter().copied().collect();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert!(to_add.is_empty());
    assert!(to_remove.is_empty());
}

#[test]
fn drift_detects_missing_replica() {
    let expected: BTreeSet<NodeId> = [1, 2].iter().copied().collect();
    let actual: BTreeSet<NodeId> = [1].iter().copied().collect();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert_eq!(to_add, vec![2]);
    assert!(to_remove.is_empty());
}

#[test]
fn drift_detects_extra_member() {
    let expected: BTreeSet<NodeId> = [1].iter().copied().collect();
    let actual: BTreeSet<NodeId> = [1, 2].iter().copied().collect();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert!(to_add.is_empty());
    assert_eq!(to_remove, vec![2]);
}

#[test]
fn drift_detects_both_add_and_remove() {
    let expected: BTreeSet<NodeId> = [2, 3].iter().copied().collect();
    let actual: BTreeSet<NodeId> = [1, 2].iter().copied().collect();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert_eq!(to_add, vec![3]);
    assert_eq!(to_remove, vec![1]);
}

#[test]
fn drift_empty_expected_all_remove() {
    let expected: BTreeSet<NodeId> = BTreeSet::new();
    let actual: BTreeSet<NodeId> = [1, 2].iter().copied().collect();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert!(to_add.is_empty());
    assert_eq!(to_remove.len(), 2);
}

#[test]
fn drift_empty_actual_all_missing() {
    let expected: BTreeSet<NodeId> = [1, 2, 3].iter().copied().collect();
    let actual: BTreeSet<NodeId> = BTreeSet::new();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert_eq!(to_add.len(), 3);
    assert!(to_remove.is_empty());
}

#[test]
fn drift_both_empty_sets() {
    let expected: BTreeSet<NodeId> = BTreeSet::new();
    let actual: BTreeSet<NodeId> = BTreeSet::new();
    let (to_add, to_remove) = compute_drift(&expected, &actual);
    assert!(to_add.is_empty());
    assert!(to_remove.is_empty());
}

// ============================================================================
// 端到端: tick() — drift 检测 — 期望成员正确传播
// ============================================================================

#[test]
fn full_reconcile_cycle_detects_topology_change() {
    let node_id = 1u64;
    let mut groups = HashMap::new();
    groups.insert(10, vec![1]); // 初始: group 10 只有 node 1
    let meta = make_test_meta(&[1, 2], groups);

    let mock = Arc::new(MockMetaRaft::new(meta));
    let router = make_router();
    let mgr = LifecycleManager::new(node_id, router.clone(), mock.clone());

    // Tick 1: group 10 期望成员 = {1}, 创建 group 10.
    let result = mgr.tick();
    assert_eq!(result.groups_to_create, vec![10]);
    assert_eq!(result.expected_memberships.get(&10).unwrap().len(), 1);

    // 模拟 MetaRaft 更新: 加入 node 2 作为 replica.
    mock.update_meta(|meta| {
        let g = meta.groups.get_mut(&10).unwrap();
        g.replicas.push(ReplicaInfo {
            node_id: 2,
            is_leader: false,
        });
    });

    // Tick 2: 期望成员现在 = {1, 2}.
    let result = mgr.tick();
    assert!(result.groups_to_create.is_empty()); // 已创建
    let members = result.expected_memberships.get(&10).unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.contains(&1));
    assert!(members.contains(&2));
    // 此时 drift = {1,2} - {1} = node 2 缺失, 但 drift 检测在 start_lifecycle_impl 中执行.
}

#[test]
fn no_false_positive_drift_in_steady_state() {
    let node_id = 1u64;
    let mut groups = HashMap::new();
    groups.insert(10, vec![1, 2, 3]);
    let meta = make_test_meta(&[1, 2, 3], groups);

    let mock = Arc::new(MockMetaRaft::new(meta));
    let router = make_router();
    let mgr = LifecycleManager::new(node_id, router, mock);

    // 运行 100 tick, 确保 expected_memberships 始终一致.
    for _ in 0..100 {
        let result = mgr.tick();
        let members = result.expected_memberships.get(&10).unwrap();
        assert_eq!(members.len(), 3);
        assert!(members.contains(&1));
        assert!(members.contains(&2));
        assert!(members.contains(&3));
    }
}
