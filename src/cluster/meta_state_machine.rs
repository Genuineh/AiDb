//! MetaStateMachine implementation for MetaRaft
//!
//! This module implements the state machine for MetaRaft, which manages
//! global cluster metadata including slot mappings, group memberships, and node information.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::meta_types::{ClusterMeta, MetaRequest, MetaResponse, SlotMigration};
use super::raft_storage::NodeId;
use super::replica_allocator::ReplicaAllocator;
use super::sharded_storage::GroupId;
use crate::error::{Error, Result};

/// Type alias for membership change results
/// Returns (MetaResponse, list of (group_id, new_member_list) tuples)
pub type MembershipChangeResult = (MetaResponse, Vec<(GroupId, Vec<NodeId>)>);

/// MetaStateMachine for managing cluster metadata
///
/// This state machine handles all metadata operations through the MetaRaft group,
/// providing strong consistency guarantees for cluster configuration.
pub struct MetaStateMachine {
    /// Current cluster metadata (boxed to avoid stack overflow with large array)
    meta: Arc<RwLock<Box<ClusterMeta>>>,

    /// Last applied log index (reserved for future use with Raft integration)
    #[allow(dead_code)]
    last_applied: Arc<RwLock<u64>>,

    /// Data directory for persistence
    #[allow(dead_code)]
    data_dir: PathBuf,

    /// Replica allocator for automatic replica assignment
    allocator: ReplicaAllocator,
}

impl MetaStateMachine {
    /// Create a new MetaStateMachine
    ///
    /// # Arguments
    ///
    /// * `data_dir` - Directory for storing metadata snapshots
    pub fn new<P: Into<PathBuf>>(data_dir: P) -> Result<Self> {
        Self::with_replication_factor(data_dir, 3)
    }

    /// Create a new MetaStateMachine with custom replication factor
    ///
    /// # Arguments
    ///
    /// * `data_dir` - Directory for storing metadata snapshots
    /// * `replication_factor` - Number of replicas per group (typically 3 or 5)
    pub fn with_replication_factor<P: Into<PathBuf>>(
        data_dir: P,
        replication_factor: usize,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)?;

        // Try to load existing metadata
        let meta = Self::load_metadata(&data_dir).unwrap_or_default();

        Ok(Self {
            meta: Arc::new(RwLock::new(Box::new(meta))),
            last_applied: Arc::new(RwLock::new(0)),
            data_dir,
            allocator: ReplicaAllocator::new(replication_factor),
        })
    }

    /// Load metadata from disk
    fn load_metadata(data_dir: &Path) -> Result<ClusterMeta> {
        let meta_path = data_dir.join("cluster_meta.json");
        if meta_path.exists() {
            let content = std::fs::read_to_string(&meta_path)?;
            let meta: ClusterMeta = serde_json::from_str(&content)?;
            Ok(meta)
        } else {
            Err(Error::Internal("Metadata file not found".to_string()))
        }
    }

    /// Save metadata to disk (reserved for future use with persistence)
    #[allow(dead_code)]
    fn save_metadata(&self) -> Result<()> {
        let meta_path = self.data_dir.join("cluster_meta.json");
        let meta = self.meta.read();
        let content = serde_json::to_string_pretty(&*meta)?;
        std::fs::write(&meta_path, content)?;
        Ok(())
    }

    /// Get current cluster metadata (read-only access)
    pub fn get_cluster_meta(&self) -> ClusterMeta {
        (**self.meta.read()).clone()
    }

    /// Cheap read of [`ClusterMeta::config_version`] (for router cache vs SM staleness checks).
    pub fn get_config_version(&self) -> u64 {
        self.meta.read().config_version
    }

    /// Handle adding a node with automatic replica rebalancing
    ///
    /// This method adds a node and automatically rebalances group replicas
    /// to distribute load evenly across all nodes.
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of the node to add
    /// * `addr` - Address of the node
    ///
    /// # Returns
    ///
    /// A tuple of (MetaResponse, Vec of pending membership changes)
    /// The membership changes should be executed by the caller to update individual groups
    pub fn handle_add_node(&self, node_id: NodeId, addr: String) -> Result<MembershipChangeResult> {
        let mut meta = self.meta.write();

        // Check if node already exists
        if meta.nodes.contains_key(&node_id) {
            return Ok((
                MetaResponse::Error(format!("Node {} already exists", node_id)),
                Vec::new(),
            ));
        }

        // Add the node
        use super::meta_types::NodeInfo;
        let node_info = NodeInfo::new(node_id, addr);
        meta.nodes.insert(node_id, node_info);
        meta.config_version += 1;

        // Get available nodes
        let available_nodes: Vec<NodeId> = meta
            .nodes
            .iter()
            .filter(|(_, info)| {
                // Include online nodes and nodes that are joining
                info.is_online() || info.status == super::meta_types::NodeStatus::Joining
            })
            .map(|(&id, _)| id)
            .collect();

        // Build current allocation map
        let current_allocation: HashMap<GroupId, Vec<NodeId>> =
            meta.groups.iter().map(|(&gid, group)| (gid, group.replicas.clone())).collect();

        // Rebalance replicas
        let new_allocation = self.allocator.rebalance(&available_nodes, current_allocation)?;

        // Collect membership changes
        let mut membership_changes = Vec::new();

        for (group_id, new_replicas) in &new_allocation {
            if let Some(group) = meta.groups.get(group_id) {
                let old_replicas = &group.replicas;

                // Check if replicas changed
                if old_replicas != new_replicas {
                    membership_changes.push((*group_id, new_replicas.clone()));
                }
            }
        }

        // Update metadata with new allocations
        for (group_id, new_replicas) in new_allocation {
            // Clone old replicas before any mutable borrows
            let old_replicas = if let Some(group) = meta.groups.get(&group_id) {
                group.replicas.clone()
            } else {
                continue;
            };

            // Update node group counts for old replicas
            for &old_replica in &old_replicas {
                if let Some(node) = meta.nodes.get_mut(&old_replica) {
                    if node.group_count > 0 {
                        node.group_count -= 1;
                    }
                }
            }

            // Update group replicas
            if let Some(group) = meta.groups.get_mut(&group_id) {
                group.update_replicas(new_replicas.clone());
            }

            // Update node group counts for new replicas
            for &new_replica in &new_replicas {
                if let Some(node) = meta.nodes.get_mut(&new_replica) {
                    node.group_count += 1;
                }
            }
        }

        Ok((MetaResponse::Ok, membership_changes))
    }

    /// Handle removing a node with automatic replica rebalancing
    ///
    /// This method removes a node and automatically rebalances group replicas
    /// to maintain the replication factor.
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of the node to remove
    ///
    /// # Returns
    ///
    /// A tuple of (MetaResponse, Vec of pending membership changes)
    pub fn handle_remove_node(&self, node_id: NodeId) -> Result<MembershipChangeResult> {
        let mut meta = self.meta.write();

        // Check if node exists
        if !meta.nodes.contains_key(&node_id) {
            return Ok((MetaResponse::Error(format!("Node {} not found", node_id)), Vec::new()));
        }

        // Remove the node
        meta.nodes.remove(&node_id);
        meta.config_version += 1;

        // Get remaining available nodes
        let available_nodes: Vec<NodeId> = meta.nodes.keys().copied().collect();

        // Build current allocation map
        let current_allocation: HashMap<GroupId, Vec<NodeId>> =
            meta.groups.iter().map(|(&gid, group)| (gid, group.replicas.clone())).collect();

        // Rebalance replicas
        let new_allocation = self.allocator.rebalance(&available_nodes, current_allocation)?;

        // Collect membership changes
        let mut membership_changes = Vec::new();

        for (group_id, new_replicas) in &new_allocation {
            if let Some(group) = meta.groups.get(group_id) {
                let old_replicas = &group.replicas;

                // Check if replicas changed
                if old_replicas != new_replicas {
                    membership_changes.push((*group_id, new_replicas.clone()));
                }
            }
        }

        // Update metadata with new allocations
        for (group_id, new_replicas) in new_allocation {
            // Clone old replicas before any mutable borrows
            let old_replicas = if let Some(group) = meta.groups.get(&group_id) {
                group.replicas.clone()
            } else {
                continue;
            };

            // Update node group counts for old replicas
            for &old_replica in &old_replicas {
                if let Some(node) = meta.nodes.get_mut(&old_replica) {
                    if node.group_count > 0 {
                        node.group_count -= 1;
                    }
                }
            }

            // Update group replicas
            if let Some(group) = meta.groups.get_mut(&group_id) {
                group.update_replicas(new_replicas.clone());
            }

            // Update node group counts for new replicas
            for &new_replica in &new_replicas {
                if let Some(node) = meta.nodes.get_mut(&new_replica) {
                    node.group_count += 1;
                }
            }
        }

        Ok((MetaResponse::Ok, membership_changes))
    }

    /// Apply a MetaRequest to the state machine
    ///
    /// This is the core method that processes all metadata changes.
    pub fn apply_meta_request(&self, request: MetaRequest) -> Result<MetaResponse> {
        let request_type: &'static str = match &request {
            MetaRequest::AddNode { .. } => "AddNode",
            MetaRequest::RemoveNode { .. } => "RemoveNode",
            MetaRequest::CreateGroup { .. } => "CreateGroup",
            MetaRequest::UpdateSlots { .. } => "UpdateSlots",
            MetaRequest::UpdateGroupMembers { .. } => "UpdateGroupMembers",
            MetaRequest::StartMigration { .. } => "StartMigration",
            MetaRequest::CompleteMigration { .. } => "CompleteMigration",
            MetaRequest::SetSlotMigrationState { .. } => "SetSlotMigrationState",
            MetaRequest::ClearSlotMigration { .. } => "ClearSlotMigration",
            MetaRequest::UpdateGroupLeader { .. } => "UpdateGroupLeader",
            MetaRequest::UpdateNodeStatus { .. } => "UpdateNodeStatus",
        };

        let mut meta = self.meta.write();

        let response = match request {
            MetaRequest::AddNode { node_id, addr } => {
                use super::meta_types::NodeInfo;
                let node_info = NodeInfo::new(node_id, addr);
                meta.nodes.insert(node_id, node_info);
                meta.config_version += 1;
                Ok(MetaResponse::Ok)
            }

            MetaRequest::RemoveNode { node_id } => {
                if meta.nodes.remove(&node_id).is_some() {
                    meta.config_version += 1;
                    Ok(MetaResponse::Ok)
                } else {
                    Ok(MetaResponse::Error(format!("Node {} not found", node_id)))
                }
            }

            MetaRequest::CreateGroup { group_id, replicas } => {
                use super::meta_types::GroupMeta;

                if meta.groups.contains_key(&group_id) {
                    return Ok(MetaResponse::Error(format!("Group {} already exists", group_id)));
                }

                let group_meta = GroupMeta::new(group_id, replicas.clone());
                meta.groups.insert(group_id, group_meta);
                meta.config_version += 1;

                // Update node group counts
                for replica in &replicas {
                    if let Some(node) = meta.nodes.get_mut(replica) {
                        node.group_count += 1;
                    }
                }

                Ok(MetaResponse::Ok)
            }

            MetaRequest::UpdateSlots { start, end, group_id } => {
                if !meta.groups.contains_key(&group_id) {
                    return Ok(MetaResponse::Error(format!("Group {} does not exist", group_id)));
                }

                meta.update_slot_range(start, end, group_id);
                Ok(MetaResponse::Ok)
            }

            MetaRequest::UpdateGroupMembers { group_id, replicas } => {
                if let Some(group) = meta.groups.get(&group_id) {
                    // Clone old replicas to avoid borrow checker issues
                    let old_replicas = group.replicas.clone();

                    // Update node group counts (remove old replicas)
                    for old_replica in &old_replicas {
                        if let Some(node) = meta.nodes.get_mut(old_replica) {
                            if node.group_count > 0 {
                                node.group_count -= 1;
                            }
                        }
                    }

                    // Update group replicas
                    if let Some(group) = meta.groups.get_mut(&group_id) {
                        group.update_replicas(replicas.clone());
                    }

                    // Update node group counts (add new replicas)
                    for new_replica in &replicas {
                        if let Some(node) = meta.nodes.get_mut(new_replica) {
                            node.group_count += 1;
                        }
                    }

                    meta.config_version += 1;
                    Ok(MetaResponse::Ok)
                } else {
                    Ok(MetaResponse::Error(format!("Group {} not found", group_id)))
                }
            }

            MetaRequest::StartMigration { slot, from_group, to_group } => {
                use super::meta_types::SlotMigration;

                // Verify groups exist
                if !meta.groups.contains_key(&from_group) {
                    return Ok(MetaResponse::Error(format!(
                        "Source group {} does not exist",
                        from_group
                    )));
                }
                if !meta.groups.contains_key(&to_group) {
                    return Ok(MetaResponse::Error(format!(
                        "Target group {} does not exist",
                        to_group
                    )));
                }

                // Check if migration already in progress for this slot
                if meta.migrations.iter().any(|m| m.slot == slot && !m.is_complete()) {
                    return Ok(MetaResponse::Error(format!(
                        "Migration already in progress for slot {}",
                        slot
                    )));
                }

                let migration = SlotMigration::new(slot, from_group, to_group);
                meta.migrations.push(migration);
                meta.config_version += 1;

                Ok(MetaResponse::Ok)
            }

            MetaRequest::CompleteMigration { slot } => {
                // Find and mark migration as complete
                let mut found = false;
                for migration in &mut meta.migrations {
                    if migration.slot == slot && !migration.is_complete() {
                        use super::meta_types::SlotMigrationState;
                        migration.state = SlotMigrationState::Complete;
                        found = true;
                        break;
                    }
                }

                if found {
                    meta.config_version += 1;
                    Ok(MetaResponse::Ok)
                } else {
                    Ok(MetaResponse::Error(format!("No active migration found for slot {}", slot)))
                }
            }

            MetaRequest::SetSlotMigrationState { slot, state } => {
                if slot >= 16384 {
                    return Ok(MetaResponse::Error(format!("Invalid slot: {}", slot)));
                }

                use super::meta_types::SlotMigrationState;
                match state {
                    SlotMigrationState::Migrating { from_group, to_group }
                    | SlotMigrationState::Importing { from_group, to_group } => {
                        if !meta.groups.contains_key(&from_group) {
                            return Ok(MetaResponse::Error(format!(
                                "Source group {} does not exist",
                                from_group
                            )));
                        }
                        if !meta.groups.contains_key(&to_group) {
                            return Ok(MetaResponse::Error(format!(
                                "Target group {} does not exist",
                                to_group
                            )));
                        }
                    }
                    SlotMigrationState::Idle | SlotMigrationState::Complete => {}
                }

                if let Some(m) = meta
                    .migrations
                    .iter_mut()
                    .find(|m| m.slot == slot && !m.is_complete())
                {
                    m.state = state;
                } else {
                    let mut m = SlotMigration::new(slot, 0, 0);
                    m.state = state;
                    meta.migrations.push(m);
                }

                meta.config_version += 1;
                Ok(MetaResponse::Ok)
            }

            MetaRequest::ClearSlotMigration { slot } => {
                let before = meta.migrations.len();
                meta.migrations.retain(|m| m.slot != slot);
                if meta.migrations.len() != before {
                    meta.config_version += 1;
                }
                Ok(MetaResponse::Ok)
            }

            MetaRequest::UpdateGroupLeader { group_id, leader } => {
                if let Some(group) = meta.groups.get_mut(&group_id) {
                    group.set_leader(leader);
                    meta.config_version += 1;
                    Ok(MetaResponse::Ok)
                } else {
                    Ok(MetaResponse::Error(format!("Group {} not found", group_id)))
                }
            }
            MetaRequest::UpdateNodeStatus { node_id, status } => {
                if let Some(node) = meta.nodes.get_mut(&node_id) {
                    if node.status != status {
                        node.status = status;
                        meta.config_version += 1;
                    }
                    Ok(MetaResponse::Ok)
                } else {
                    Ok(MetaResponse::Error(format!("Node {} not found", node_id)))
                }
            }
        };

        if matches!(&response, Ok(MetaResponse::Ok)) {
            let m = &**meta;
            let migrations_active = m.migrations.iter().filter(|x| !x.is_complete()).count();
            tracing::info!(
                diag_event = "metaraft_sm_applied",
                request_type,
                config_version = m.config_version,
                node_count = m.nodes.len(),
                group_count = m.groups.len(),
                migrations_total = m.migrations.len(),
                migrations_active,
                "MetaRaft state machine applied request"
            );
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::meta_types::{NodeStatus, SlotMigrationState};
    use tempfile::TempDir;

    // ── helpers ──────────────────────────────────────────────────────────

    fn create_sm() -> (TempDir, MetaStateMachine) {
        let dir = TempDir::new().unwrap();
        let sm = MetaStateMachine::new(dir.path()).unwrap();
        (dir, sm)
    }

    // ── handle_add_node ───────────────────────────────────────────────────

    #[test]
    fn test_add_node_ok() {
        let (_dir, sm) = create_sm();
        let (resp, changes) = sm.handle_add_node(1, "addr:1".into()).unwrap();
        assert_eq!(resp, MetaResponse::Ok);
        assert!(changes.is_empty()); // no groups yet, so no rebalance

        let meta = sm.get_cluster_meta();
        assert!(meta.nodes.contains_key(&1));
        assert_eq!(meta.config_version, 1);
    }

    #[test]
    fn test_add_node_duplicate() {
        let (_dir, sm) = create_sm();
        sm.handle_add_node(1, "addr:1".into()).unwrap();
        let (resp, _) = sm.handle_add_node(1, "addr:1".into()).unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── handle_remove_node ────────────────────────────────────────────────

    #[test]
    fn test_remove_node_ok() {
        let (_dir, sm) = create_sm();
        sm.handle_add_node(1, "addr:1".into()).unwrap();
        let (resp, _) = sm.handle_remove_node(1).unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert!(!meta.nodes.contains_key(&1));
    }

    #[test]
    fn test_remove_node_nonexistent() {
        let (_dir, sm) = create_sm();
        let (resp, _) = sm.handle_remove_node(999).unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: AddNode / RemoveNode ──────────────────────────

    #[test]
    fn test_apply_add_node() {
        let (_dir, sm) = create_sm();
        let req = MetaRequest::AddNode { node_id: 10, addr: "addr:10".into() };
        let resp = sm.apply_meta_request(req).unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert_eq!(meta.nodes.len(), 1);
        assert_eq!(meta.config_version, 1);
    }

    #[test]
    fn test_apply_remove_node() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 10, addr: "addr:10".into() }).unwrap();
        let resp = sm.apply_meta_request(MetaRequest::RemoveNode { node_id: 10 }).unwrap();
        assert_eq!(resp, MetaResponse::Ok);
        assert!(!sm.get_cluster_meta().nodes.contains_key(&10));
    }

    #[test]
    fn test_apply_remove_node_not_found() {
        let (_dir, sm) = create_sm();
        let resp = sm.apply_meta_request(MetaRequest::RemoveNode { node_id: 999 }).unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: CreateGroup ───────────────────────────────────

    #[test]
    fn test_apply_create_group() {
        let (_dir, sm) = create_sm();
        let resp = sm
            .apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1, 2, 3] })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert!(meta.groups.contains_key(&1));
        assert_eq!(meta.config_version, 1);
    }

    #[test]
    fn test_apply_create_group_duplicate() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1] }).unwrap();
        let resp = sm
            .apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![2] })
            .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: UpdateSlots ───────────────────────────────────

    #[test]
    fn test_apply_update_slots() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1] }).unwrap();
        let resp =
            sm.apply_meta_request(MetaRequest::UpdateSlots { start: 0, end: 100, group_id: 1 })
                .unwrap();
        assert_eq!(resp, MetaResponse::Ok);
    }

    #[test]
    fn test_apply_update_slots_nonexistent_group() {
        let (_dir, sm) = create_sm();
        let resp =
            sm.apply_meta_request(MetaRequest::UpdateSlots { start: 0, end: 100, group_id: 999 })
                .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: UpdateGroupMembers ────────────────────────────

    #[test]
    fn test_apply_update_group_members() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 1, addr: "a:1".into() }).unwrap();
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 2, addr: "a:2".into() }).unwrap();
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 3, addr: "a:3".into() }).unwrap();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1, 2] })
            .unwrap();

        let version_before = sm.get_cluster_meta().config_version;
        let resp = sm
            .apply_meta_request(MetaRequest::UpdateGroupMembers {
                group_id: 1,
                replicas: vec![1, 2, 3],
            })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert_eq!(meta.groups.get(&1).unwrap().replicas, vec![1, 2, 3]);
        assert!(meta.config_version > version_before);
    }

    #[test]
    fn test_apply_update_group_members_not_found() {
        let (_dir, sm) = create_sm();
        let resp = sm
            .apply_meta_request(MetaRequest::UpdateGroupMembers {
                group_id: 999,
                replicas: vec![1],
            })
            .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: StartMigration / CompleteMigration ────────────

    #[test]
    fn test_apply_start_and_complete_migration() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1] }).unwrap();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 2, replicas: vec![2] }).unwrap();

        let resp = sm
            .apply_meta_request(MetaRequest::StartMigration { slot: 42, from_group: 1, to_group: 2 })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert_eq!(meta.migrations.len(), 1);
        assert!(!meta.migrations[0].is_complete());

        let resp = sm
            .apply_meta_request(MetaRequest::CompleteMigration { slot: 42 })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert!(meta.migrations[0].is_complete());
    }

    #[test]
    fn test_apply_start_migration_nonexistent_group() {
        let (_dir, sm) = create_sm();
        let resp = sm
            .apply_meta_request(MetaRequest::StartMigration { slot: 42, from_group: 999, to_group: 1 })
            .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    #[test]
    fn test_apply_start_migration_duplicate_slot() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1] }).unwrap();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 2, replicas: vec![2] }).unwrap();
        sm.apply_meta_request(MetaRequest::StartMigration { slot: 42, from_group: 1, to_group: 2 })
            .unwrap();
        let resp = sm
            .apply_meta_request(MetaRequest::StartMigration { slot: 42, from_group: 1, to_group: 2 })
            .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    #[test]
    fn test_apply_complete_migration_not_found() {
        let (_dir, sm) = create_sm();
        let resp = sm.apply_meta_request(MetaRequest::CompleteMigration { slot: 999 }).unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: SetSlotMigrationState ─────────────────────────

    #[test]
    fn test_apply_set_slot_migration_state() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1] }).unwrap();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 2, replicas: vec![2] }).unwrap();

        let resp = sm
            .apply_meta_request(MetaRequest::SetSlotMigrationState {
                slot: 42,
                state: SlotMigrationState::Migrating { from_group: 1, to_group: 2 },
            })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);
    }

    #[test]
    fn test_apply_set_slot_migration_state_invalid_slot() {
        let (_dir, sm) = create_sm();
        let resp = sm
            .apply_meta_request(MetaRequest::SetSlotMigrationState {
                slot: 50000,
                state: SlotMigrationState::Idle,
            })
            .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: ClearSlotMigration ────────────────────────────

    #[test]
    fn test_apply_clear_slot_migration() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1] }).unwrap();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 2, replicas: vec![2] }).unwrap();
        sm.apply_meta_request(MetaRequest::StartMigration { slot: 42, from_group: 1, to_group: 2 })
            .unwrap();
        assert_eq!(sm.get_cluster_meta().migrations.len(), 1);

        let resp =
            sm.apply_meta_request(MetaRequest::ClearSlotMigration { slot: 42 }).unwrap();
        assert_eq!(resp, MetaResponse::Ok);
        assert_eq!(sm.get_cluster_meta().migrations.len(), 0);
    }

    // ── apply_meta_request: UpdateGroupLeader ─────────────────────────────

    #[test]
    fn test_apply_update_group_leader() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1, 2, 3] })
            .unwrap();

        let resp = sm
            .apply_meta_request(MetaRequest::UpdateGroupLeader { group_id: 1, leader: 2 })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);
        assert_eq!(sm.get_cluster_meta().groups.get(&1).unwrap().leader, Some(2));
    }

    #[test]
    fn test_apply_update_group_leader_not_found() {
        let (_dir, sm) = create_sm();
        let resp =
            sm.apply_meta_request(MetaRequest::UpdateGroupLeader { group_id: 999, leader: 1 })
                .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── apply_meta_request: UpdateNodeStatus ──────────────────────────────

    #[test]
    fn test_apply_update_node_status() {
        let (_dir, sm) = create_sm();
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 1, addr: "a:1".into() }).unwrap();

        let resp = sm
            .apply_meta_request(MetaRequest::UpdateNodeStatus {
                node_id: 1,
                status: NodeStatus::Online,
            })
            .unwrap();
        assert_eq!(resp, MetaResponse::Ok);
        assert_eq!(sm.get_cluster_meta().nodes.get(&1).unwrap().status, NodeStatus::Online);
    }

    #[test]
    fn test_apply_update_node_status_not_found() {
        let (_dir, sm) = create_sm();
        let resp = sm
            .apply_meta_request(MetaRequest::UpdateNodeStatus {
                node_id: 999,
                status: NodeStatus::Online,
            })
            .unwrap();
        assert!(matches!(resp, MetaResponse::Error(_)));
    }

    // ── handle_add_node with rebalance ────────────────────────────────────

    #[test]
    fn test_add_node_triggers_rebalance_when_groups_exist() {
        let (_dir, sm) = create_sm();

        // Add 2 nodes and create a group
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 1, addr: "a:1".into() }).unwrap();
        sm.apply_meta_request(MetaRequest::AddNode { node_id: 2, addr: "a:2".into() }).unwrap();
        sm.apply_meta_request(MetaRequest::CreateGroup { group_id: 1, replicas: vec![1, 2] })
            .unwrap();

        // Add a 3rd node — should trigger rebalance
        let (resp, _changes) = sm.handle_add_node(3, "a:3".into()).unwrap();
        assert_eq!(resp, MetaResponse::Ok);

        let meta = sm.get_cluster_meta();
        assert!(meta.nodes.contains_key(&3));
        assert!(meta.config_version >= 2);
    }

    // ── get_cluster_meta / get_config_version ─────────────────────────────

    #[test]
    fn test_get_config_version() {
        let (_dir, sm) = create_sm();
        assert_eq!(sm.get_config_version(), 0);

        sm.apply_meta_request(MetaRequest::AddNode { node_id: 1, addr: "a:1".into() }).unwrap();
        assert_eq!(sm.get_config_version(), 1);
    }

    #[test]
    fn test_get_cluster_meta_isolation() {
        let (_dir, sm) = create_sm();
        let _meta = sm.get_cluster_meta();
        // Mutating the copy does not affect the SM
        assert_eq!(sm.get_cluster_meta().config_version, 0);
    }
}
