use crate::cluster::meta_types::{
    ClusterMeta, MetaRequest, NodeStatus, SlotMigrationState, SlotStatus, SlotTable, SLOT_COUNT,
};
use crate::cluster::types::{ClusterError, NodeId};

use super::MetaSmResult;

pub(super) fn validate_with_state(
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
                Some(SlotMigrationState::Frozen { .. })
                | Some(SlotMigrationState::ReadyToCommit { .. }) => {
                    return Err(ClusterError::InvalidState(
                        "cannot update progress in frozen/ready phase".into(),
                    ));
                }
                None => return Err(ClusterError::InvalidState("no active migration".into())),
            }
            if progress > total {
                return Err(ClusterError::InvalidConfig("progress exceeds total".into()));
            }
        }
        MetaRequest::FreezeSlotMigration => match migration_state.as_ref() {
            Some(SlotMigrationState::Migrating { .. }) => {}
            _ => {
                return Err(ClusterError::InvalidState(
                    "freeze requires Migrating state".into(),
                ));
            }
        },
        MetaRequest::MarkMigrationReady => match migration_state.as_ref() {
            Some(SlotMigrationState::Frozen { .. }) => {}
            _ => {
                return Err(ClusterError::InvalidState(
                    "mark ready requires Frozen state".into(),
                ));
            }
        },
        MetaRequest::CommitSlotMigration => match migration_state.as_ref() {
            Some(SlotMigrationState::ReadyToCommit { .. }) => {}
            _ => {
                return Err(ClusterError::InvalidState(
                    "commit requires ReadyToCommit state".into(),
                ));
            }
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
        })
        | Some(SlotMigrationState::Frozen {
            source_group,
            target_group,
            ..
        })
        | Some(SlotMigrationState::ReadyToCommit {
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
        })
        | Some(SlotMigrationState::Frozen {
            source_group,
            target_group,
            ..
        })
        | Some(SlotMigrationState::ReadyToCommit {
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
