use std::time::{SystemTime, UNIX_EPOCH};

use crate::cluster::meta_types::{
    ClusterMeta, GroupMeta, MetaRequest, NodeInfo, NodeRole, NodeStatus, ReplicaInfo,
    SlotMigrationState, SlotStatus, SlotTable, SLOT_COUNT,
};

use super::MetaSmResult;

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

pub(super) fn apply_mutate(
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
                // Frozen / ReadyToCommit: validate rejects; keep exhaustive.
                Some(SlotMigrationState::Frozen { .. })
                | Some(SlotMigrationState::ReadyToCommit { .. }) => unreachable!(),
            }
        }
        MetaRequest::FreezeSlotMigration => {
            let (source_group, target_group, slots) = match migration_state.take() {
                Some(SlotMigrationState::Migrating {
                    source_group,
                    target_group,
                    slots,
                    ..
                }) => (source_group, target_group, slots),
                _ => unreachable!(),
            };
            *migration_state = Some(SlotMigrationState::Frozen {
                source_group,
                target_group,
                slots,
            });
        }
        MetaRequest::MarkMigrationReady => {
            let (source_group, target_group, slots) = match migration_state.take() {
                Some(SlotMigrationState::Frozen {
                    source_group,
                    target_group,
                    slots,
                }) => (source_group, target_group, slots),
                _ => unreachable!(),
            };
            *migration_state = Some(SlotMigrationState::ReadyToCommit {
                source_group,
                target_group,
                slots,
            });
        }
        MetaRequest::CommitSlotMigration => {
            let (source_group, target_group, slots) = match migration_state.take() {
                Some(SlotMigrationState::ReadyToCommit {
                    source_group,
                    target_group,
                    slots,
                }) => (source_group, target_group, slots),
                _ => unreachable!(),
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
                Some(SlotMigrationState::Frozen {
                    source_group,
                    target_group,
                    slots,
                })
                | Some(SlotMigrationState::ReadyToCommit {
                    source_group,
                    target_group,
                    slots,
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
