//! MetaRaft state machine — cluster metadata apply path.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use tracing::instrument;

use crate::cluster::meta_types::{
    default_slot_table, ClusterMeta, GroupMeta, MetaRequest, NodeInfo, NodeRole, NodeStatus,
    ReplicaInfo, SlotMigrationState, SlotStatus, SlotTable, SLOT_COUNT,
};
use crate::cluster::storage::keys::{
    meta_cluster_meta_key, meta_migration_state_key, meta_slot_table_key,
};
use crate::cluster::types::{ClusterError, NodeId};
use crate::error::{Error, Result};

type MetaSmResult<T> = std::result::Result<T, ClusterError>;

use crate::DB;

/// MetaRaft apply output — persisted atomically by outer storage apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutput {
    pub kv_pairs: Vec<(Vec<u8>, Vec<u8>)>,
}

pub struct MetaStateMachine {
    db: Arc<DB>,
    cluster_meta: RwLock<ClusterMeta>,
    slot_table: RwLock<SlotTable>,
    migration_state: RwLock<Option<SlotMigrationState>>,
}

impl MetaStateMachine {
    pub fn new(db: Arc<DB>) -> Result<Self> {
        let sm = Self {
            db,
            cluster_meta: RwLock::new(ClusterMeta::default()),
            slot_table: RwLock::new(default_slot_table()),
            migration_state: RwLock::new(None),
        };
        sm.reload_from_db()?;
        Ok(sm)
    }

    pub fn reload_from_db(&self) -> Result<()> {
        let cluster_meta = match self.db.get(&meta_cluster_meta_key())? {
            Some(bytes) => {
                let meta: ClusterMeta = rmp_serde::from_slice(&bytes)
                    .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;
                if meta.format_version > 1 {
                    return Err(Error::Corruption(format!(
                        "unsupported meta format_version {}",
                        meta.format_version
                    )));
                }
                meta
            }
            None => ClusterMeta::default(),
        };

        let slot_table = match self.db.get(&meta_slot_table_key())? {
            Some(bytes) => {
                let table: SlotTable = rmp_serde::from_slice(&bytes)
                    .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;
                if table.len() != SLOT_COUNT {
                    return Err(Error::Corruption(format!(
                        "invalid slot_table length {}",
                        table.len()
                    )));
                }
                table
            }
            None => default_slot_table(),
        };

        let migration_state = match self.db.get(&meta_migration_state_key())? {
            Some(bytes) => rmp_serde::from_slice(&bytes)
                .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?,
            None => None,
        };

        *self.cluster_meta.write() = cluster_meta;
        *self.slot_table.write() = slot_table;
        *self.migration_state.write() = migration_state;
        Ok(())
    }

    pub fn get_cluster_meta(&self) -> ClusterMeta {
        self.cluster_meta.read().clone()
    }

    pub fn get_slot_table(&self) -> SlotTable {
        self.slot_table.read().clone()
    }

    pub fn get_migration_state(&self) -> Option<SlotMigrationState> {
        self.migration_state.read().clone()
    }

    /// Directly set migration state (for testing).
    /// Skips Raft consensus — only use in test scenarios.
    pub fn set_migration_state(&self, state: Option<SlotMigrationState>) {
        *self.migration_state.write() = state;
    }

    /// Directly set slot table (for testing).
    /// Skips Raft consensus — only use in test scenarios.
    pub fn set_slot_table(&self, table: SlotTable) {
        *self.slot_table.write() = table;
    }

    pub fn validate_meta_request(&self, request: &MetaRequest) -> MetaSmResult<()> {
        let cluster_meta = self.cluster_meta.read();
        let slot_table = self.slot_table.read();
        let migration_state = self.migration_state.read();
        validate_with_state(request, &cluster_meta, &slot_table, &migration_state)
    }

    #[instrument(name = "meta_apply", skip(self))]
    pub fn apply_meta_request(&self, request: MetaRequest) -> MetaSmResult<ApplyOutput> {
        self.validate_meta_request(&request)?;

        let mut cluster_meta = self.cluster_meta.write();
        let mut slot_table = self.slot_table.write();
        let mut migration_state = self.migration_state.write();

        apply_mutate(
            request,
            &mut cluster_meta,
            &mut slot_table,
            &mut migration_state,
        )?;

        cluster_meta.version += 1;

        let kv_pairs = vec![
            (
                meta_cluster_meta_key(),
                rmp_serde::to_vec(&*cluster_meta)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            ),
            (
                meta_slot_table_key(),
                rmp_serde::to_vec(&*slot_table)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            ),
            (
                meta_migration_state_key(),
                rmp_serde::to_vec(&*migration_state)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            ),
        ];

        Ok(ApplyOutput { kv_pairs })
    }
}

pub fn rebuild_slot_ranges(slot_table: &SlotTable, group_id: u64) -> Vec<(u16, u16)> {
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < SLOT_COUNT {
        if slot_table[i] == SlotStatus::Assigned(group_id) {
            let start = i as u16;
            while i < SLOT_COUNT && slot_table[i] == SlotStatus::Assigned(group_id) {
                i += 1;
            }
            ranges.push((start, (i - 1) as u16));
        } else {
            i += 1;
        }
    }
    ranges
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn validate_with_state(
    request: &MetaRequest,
    cluster_meta: &ClusterMeta,
    slot_table: &SlotTable,
    migration_state: &Option<SlotMigrationState>,
) -> MetaSmResult<()> {
    match request {
        MetaRequest::RegisterNode {
            node_id, rpc_addr, ..
        } => {
            if cluster_meta.nodes.contains_key(node_id) {
                return Err(ClusterError::InvalidState("node already exists".into()));
            }
            if rpc_addr.is_empty() {
                return Err(ClusterError::InvalidConfig("empty rpc_addr".into()));
            }
        }
        MetaRequest::UpdateNodeStatus { node_id, status } => {
            let node = cluster_meta
                .nodes
                .get(node_id)
                .ok_or_else(|| ClusterError::InvalidState("node not found".into()))?;
            if !valid_status_transition(&node.status, status) {
                return Err(ClusterError::InvalidState(
                    "invalid status transition".into(),
                ));
            }
        }
        MetaRequest::ChangeNodeRole { node_id, .. }
        | MetaRequest::UpdateNodeTags { node_id, .. }
        | MetaRequest::UpdateNodeClientAddr { node_id, .. } => {
            if !cluster_meta.nodes.contains_key(node_id) {
                return Err(ClusterError::InvalidState("node not found".into()));
            }
        }
        MetaRequest::RemoveNode { node_id } => {
            if !cluster_meta.nodes.contains_key(node_id) {
                return Err(ClusterError::InvalidState("node not found".into()));
            }
            if cluster_meta
                .groups
                .values()
                .any(|g| g.replicas.iter().any(|r| r.node_id == *node_id))
            {
                return Err(ClusterError::InvalidState(
                    "node still has active groups".into(),
                ));
            }
            if node_in_active_migration(*node_id, migration_state, cluster_meta) {
                return Err(ClusterError::InvalidState(
                    "node involved in active migration".into(),
                ));
            }
        }
        MetaRequest::CreateGroup {
            group_id,
            initial_replicas,
        } => {
            if cluster_meta.groups.contains_key(group_id) {
                return Err(ClusterError::InvalidState("group already exists".into()));
            }
            if initial_replicas.is_empty() {
                return Err(ClusterError::InvalidConfig("empty initial_replicas".into()));
            }
            validate_replicas(cluster_meta, initial_replicas)?;
        }
        MetaRequest::RemoveGroup { group_id } => {
            if !cluster_meta.groups.contains_key(group_id) {
                return Err(ClusterError::InvalidState("group not found".into()));
            }
            if slot_table.iter().any(|s| match s {
                SlotStatus::Assigned(g) | SlotStatus::Migrating(g) => *g == *group_id,
                SlotStatus::Unallocated => false,
            }) {
                return Err(ClusterError::InvalidState("group still has slots".into()));
            }
            if group_in_active_migration(*group_id, migration_state) {
                return Err(ClusterError::InvalidState(
                    "group involved in active migration".into(),
                ));
            }
        }
        MetaRequest::ChangeGroupMembership {
            group_id,
            new_replicas,
            config_version,
        } => {
            let group = cluster_meta
                .groups
                .get(group_id)
                .ok_or_else(|| ClusterError::InvalidState("group not found".into()))?;
            validate_replicas(cluster_meta, new_replicas)?;
            if *config_version < group.config_version {
                return Err(ClusterError::InvalidState("config version too old".into()));
            }
        }
        MetaRequest::AssignSlots { group_id, slots } => {
            if !cluster_meta.groups.contains_key(group_id) {
                return Err(ClusterError::InvalidState("group not found".into()));
            }
            validate_slots(slots)?;
            for slot in slots {
                if slot_table[*slot as usize] != SlotStatus::Unallocated {
                    return Err(ClusterError::InvalidState("slot already assigned".into()));
                }
            }
        }
        MetaRequest::UnassignSlots { slots } => {
            validate_slots(slots)?;
            for slot in slots {
                if !matches!(slot_table[*slot as usize], SlotStatus::Assigned(_)) {
                    return Err(ClusterError::InvalidState("slot not assigned".into()));
                }
            }
        }
        MetaRequest::BeginSlotMigration {
            source_group,
            target_group,
            slots,
        } => {
            if source_group == target_group {
                return Err(ClusterError::InvalidConfig(
                    "source and target group must differ".into(),
                ));
            }
            if migration_state.is_some() {
                return Err(ClusterError::InvalidState(
                    "migration already in progress".into(),
                ));
            }
            if !cluster_meta.groups.contains_key(source_group) {
                return Err(ClusterError::InvalidState("group not found".into()));
            }
            if !cluster_meta.groups.contains_key(target_group) {
                return Err(ClusterError::InvalidState("group not found".into()));
            }
            validate_slots(slots)?;
            for slot in slots {
                if slot_table[*slot as usize] != SlotStatus::Assigned(*source_group) {
                    return Err(ClusterError::InvalidState(
                        "slot not assigned to source group".into(),
                    ));
                }
            }
        }
        MetaRequest::UpdateMigrationProgress { progress, total } => {
            match migration_state.as_ref() {
                Some(SlotMigrationState::Prepare { .. })
                | Some(SlotMigrationState::Migrating { .. }) => {}
                _ => return Err(ClusterError::InvalidState("no active migration".into())),
            }
            if progress > total {
                return Err(ClusterError::InvalidConfig("progress exceeds total".into()));
            }
        }
        MetaRequest::CommitSlotMigration => match migration_state.as_ref() {
            Some(SlotMigrationState::Prepare { .. })
            | Some(SlotMigrationState::Migrating { .. }) => {}
            _ => return Err(ClusterError::InvalidState("no active migration".into())),
        },
        MetaRequest::CancelSlotMigration => {
            if migration_state.is_none() {
                return Err(ClusterError::InvalidState("no active migration".into()));
            }
        }
        MetaRequest::BumpEpoch => {
            // No validation required — always allowed
        }
    }
    Ok(())
}

fn apply_mutate(
    request: MetaRequest,
    cluster_meta: &mut ClusterMeta,
    slot_table: &mut SlotTable,
    migration_state: &mut Option<SlotMigrationState>,
) -> MetaSmResult<()> {
    match request {
        MetaRequest::RegisterNode {
            node_id,
            rpc_addr,
            client_addr,
            tags,
        } => {
            cluster_meta.nodes.insert(
                node_id,
                NodeInfo {
                    node_id,
                    rpc_addr,
                    client_addr,
                    role: NodeRole::Learner,
                    status: NodeStatus::Online,
                    registered_at: now_millis(),
                    tags,
                },
            );
        }
        MetaRequest::UpdateNodeStatus { node_id, status } => {
            cluster_meta.nodes.get_mut(&node_id).unwrap().status = status;
        }
        MetaRequest::ChangeNodeRole { node_id, role } => {
            cluster_meta.nodes.get_mut(&node_id).unwrap().role = role;
        }
        MetaRequest::UpdateNodeTags { node_id, tags } => {
            cluster_meta.nodes.get_mut(&node_id).unwrap().tags = tags;
        }
        MetaRequest::UpdateNodeClientAddr {
            node_id,
            client_addr,
        } => {
            cluster_meta.nodes.get_mut(&node_id).unwrap().client_addr = client_addr;
        }
        MetaRequest::RemoveNode { node_id } => {
            cluster_meta.nodes.remove(&node_id);
        }
        MetaRequest::CreateGroup {
            group_id,
            initial_replicas,
        } => {
            let replicas = initial_replicas
                .into_iter()
                .map(|(node_id, is_leader)| ReplicaInfo { node_id, is_leader })
                .collect();
            cluster_meta.groups.insert(
                group_id,
                GroupMeta {
                    group_id,
                    replicas,
                    slot_ranges: vec![],
                    config_version: 0,
                },
            );
        }
        MetaRequest::RemoveGroup { group_id } => {
            cluster_meta.groups.remove(&group_id);
        }
        MetaRequest::ChangeGroupMembership {
            group_id,
            new_replicas,
            config_version,
        } => {
            let group = cluster_meta.groups.get_mut(&group_id).unwrap();
            group.replicas = new_replicas
                .into_iter()
                .map(|(node_id, is_leader)| ReplicaInfo { node_id, is_leader })
                .collect();
            group.config_version = config_version;
        }
        MetaRequest::AssignSlots { group_id, slots } => {
            for slot in &slots {
                slot_table[*slot as usize] = SlotStatus::Assigned(group_id);
            }
            let ranges = rebuild_slot_ranges(slot_table, group_id);
            cluster_meta.groups.get_mut(&group_id).unwrap().slot_ranges = ranges;
        }
        MetaRequest::UnassignSlots { slots } => {
            // 收集受影响的 group_ids, 更新 slot_ranges
            let mut affected = Vec::new();
            for &slot in &slots {
                let idx = slot as usize;
                let prev = std::mem::replace(&mut slot_table[idx], SlotStatus::Unallocated);
                if let SlotStatus::Assigned(gid) = prev {
                    if !affected.contains(&gid) {
                        affected.push(gid);
                    }
                }
            }
            for gid in &affected {
                let ranges = rebuild_slot_ranges(slot_table, *gid);
                if let Some(group) = cluster_meta.groups.get_mut(gid) {
                    group.slot_ranges = ranges;
                }
            }
        }
        MetaRequest::BeginSlotMigration {
            source_group,
            target_group,
            slots,
        } => {
            for slot in &slots {
                slot_table[*slot as usize] = SlotStatus::Migrating(source_group);
            }
            *migration_state = Some(SlotMigrationState::Prepare {
                source_group,
                target_group,
                slots,
            });
        }
        MetaRequest::UpdateMigrationProgress { progress, total } => {
            match migration_state.as_mut() {
                Some(SlotMigrationState::Prepare {
                    source_group,
                    target_group,
                    slots,
                }) => {
                    *migration_state = Some(SlotMigrationState::Migrating {
                        source_group: *source_group,
                        target_group: *target_group,
                        slots: slots.clone(),
                        progress,
                        total,
                    });
                }
                Some(SlotMigrationState::Migrating {
                    progress: p,
                    total: t,
                    ..
                }) => {
                    *p = progress;
                    *t = total;
                }
                None => unreachable!(),
            }
        }
        MetaRequest::CommitSlotMigration => {
            let (source_group, target_group, slots) = match migration_state.take() {
                Some(SlotMigrationState::Prepare {
                    source_group,
                    target_group,
                    slots,
                }) => (source_group, target_group, slots),
                Some(SlotMigrationState::Migrating {
                    source_group,
                    target_group,
                    slots,
                    ..
                }) => (source_group, target_group, slots),
                None => unreachable!(),
            };
            for slot in &slots {
                slot_table[*slot as usize] = SlotStatus::Assigned(target_group);
            }
            if let Some(g) = cluster_meta.groups.get_mut(&source_group) {
                g.slot_ranges = rebuild_slot_ranges(slot_table, source_group);
            }
            if let Some(g) = cluster_meta.groups.get_mut(&target_group) {
                g.slot_ranges = rebuild_slot_ranges(slot_table, target_group);
            }
        }
        MetaRequest::CancelSlotMigration => {
            let (source_group, target_group, slots) = match migration_state.take() {
                Some(SlotMigrationState::Prepare {
                    source_group,
                    target_group,
                    slots,
                }) => (source_group, target_group, slots),
                Some(SlotMigrationState::Migrating {
                    source_group,
                    target_group,
                    slots,
                    ..
                }) => (source_group, target_group, slots),
                None => unreachable!(),
            };
            for slot in &slots {
                slot_table[*slot as usize] = SlotStatus::Assigned(source_group);
            }
            if let Some(g) = cluster_meta.groups.get_mut(&source_group) {
                g.slot_ranges = rebuild_slot_ranges(slot_table, source_group);
            }
            if let Some(g) = cluster_meta.groups.get_mut(&target_group) {
                g.slot_ranges = rebuild_slot_ranges(slot_table, target_group);
            }
        }
        MetaRequest::BumpEpoch => {
            // Increment version by 1 here; the caller will increment it again,
            // resulting in a net +2 for epoch bumps (distinguishable from normal mutations).
            cluster_meta.version += 1;
        }
    }
    Ok(())
}

fn validate_replicas(cluster_meta: &ClusterMeta, replicas: &[(NodeId, bool)]) -> MetaSmResult<()> {
    let mut seen = std::collections::HashSet::new();
    for (node_id, _) in replicas {
        if !cluster_meta.nodes.contains_key(node_id) {
            return Err(ClusterError::InvalidState("node not found".into()));
        }
        if !seen.insert(*node_id) {
            return Err(ClusterError::InvalidConfig(
                "duplicate replica node_id".into(),
            ));
        }
    }
    Ok(())
}

fn validate_slots(slots: &[u16]) -> MetaSmResult<()> {
    if slots.is_empty() {
        return Err(ClusterError::InvalidConfig(
            "empty or duplicate slots".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for slot in slots {
        if *slot >= SLOT_COUNT as u16 {
            return Err(ClusterError::InvalidConfig("slot out of range".into()));
        }
        if !seen.insert(*slot) {
            return Err(ClusterError::InvalidConfig(
                "empty or duplicate slots".into(),
            ));
        }
    }
    Ok(())
}

fn valid_status_transition(from: &NodeStatus, to: &NodeStatus) -> bool {
    use NodeStatus::*;
    matches!(
        (from, to),
        (Online, Offline)
            | (Offline, Online)
            | (Online, Draining)
            | (Draining, Offline)
            | (Online, Online)
            | (Offline, Offline)
            | (Draining, Draining)
    )
}

fn group_in_active_migration(group_id: u64, migration: &Option<SlotMigrationState>) -> bool {
    match migration {
        Some(SlotMigrationState::Prepare {
            source_group,
            target_group,
            ..
        })
        | Some(SlotMigrationState::Migrating {
            source_group,
            target_group,
            ..
        }) => *source_group == group_id || *target_group == group_id,
        None => false,
    }
}

fn node_in_active_migration(
    node_id: NodeId,
    migration: &Option<SlotMigrationState>,
    cluster_meta: &ClusterMeta,
) -> bool {
    let (source, target) = match migration {
        Some(SlotMigrationState::Prepare {
            source_group,
            target_group,
            ..
        })
        | Some(SlotMigrationState::Migrating {
            source_group,
            target_group,
            ..
        }) => (*source_group, *target_group),
        None => return false,
    };
    for gid in [source, target] {
        if let Some(g) = cluster_meta.groups.get(&gid) {
            if g.replicas.iter().any(|r| r.node_id == node_id) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Options;
    use std::collections::HashMap;
    use tempfile::TempDir;

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
        assert!(matches!(err, ClusterError::InvalidState(s) if s.contains("no active migration")));
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
}
