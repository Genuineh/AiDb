//! Membership Change Coordinator
//!
//! This module provides automatic membership change coordination when groups need to
//! add or remove replicas. It works with MetaRaft to trigger openraft change_membership
//! calls on the appropriate data groups.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::meta_raft_node::MetaRaftNode;
use super::multi_raft_node::MultiRaftNode;
use super::raft_storage::NodeId;
use super::sharded_storage::GroupId;
use crate::error::{Error, Result};

/// Coordinator for automatic membership changes
///
/// This coordinator listens for metadata changes from MetaRaft and applies
/// membership changes to the corresponding data groups.
pub struct MembershipCoordinator {
    /// Node instance to apply changes to
    node: Arc<MultiRaftNode>,

    /// MetaRaft instance to watch for changes
    meta_raft: Arc<MetaRaftNode>,
}

impl MembershipCoordinator {
    /// Create a new membership coordinator
    ///
    /// # Arguments
    ///
    /// * `node` - MultiRaftNode instance
    /// * `meta_raft` - MetaRaft instance
    pub fn new(node: Arc<MultiRaftNode>, meta_raft: Arc<MetaRaftNode>) -> Self {
        Self { node, meta_raft }
    }

    /// Apply a membership change to a specific group
    ///
    /// This method uses openraft's change_membership API to safely update
    /// the replica set for a group. It supports Joint Consensus for zero-downtime
    /// changes.
    ///
    /// # Arguments
    ///
    /// * `group_id` - ID of the group to update
    /// * `new_members` - New set of replica node IDs
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    ///
    /// # Notes
    ///
    /// This uses openraft's change_membership which implements Joint Consensus,
    /// ensuring zero downtime during membership changes.
    pub async fn apply_membership_change(
        &self,
        group_id: GroupId,
        new_members: Vec<NodeId>,
    ) -> Result<()> {
        // Get the Raft group
        let raft = self
            .node
            .get_raft_group(group_id)
            .ok_or_else(|| Error::Internal(format!("Group {} not found", group_id)))?;

        // Convert to BTreeSet as required by openraft
        let members: BTreeSet<NodeId> = new_members.into_iter().collect();

        // Use openraft's change_membership
        // retain=true means keep learners that are not in the new member set
        raft.change_membership(members, true)
            .await
            .map_err(|e| Error::Internal(format!("Failed to change membership: {:?}", e)))?;

        Ok(())
    }

    /// Apply multiple membership changes
    ///
    /// This is a batch operation that applies membership changes to multiple groups.
    ///
    /// # Arguments
    ///
    /// * `changes` - Vector of (group_id, new_members) tuples
    ///
    /// # Returns
    ///
    /// `Ok(())` if all changes succeed, or the first error encountered
    pub async fn apply_membership_changes(
        &self,
        changes: Vec<(GroupId, Vec<NodeId>)>,
    ) -> Result<()> {
        for (group_id, new_members) in changes {
            self.apply_membership_change(group_id, new_members).await?;
        }
        Ok(())
    }

    /// Add a node as learner to a group
    ///
    /// This is the first step in adding a new replica. The node is added as a learner
    /// first to catch up with the log, then promoted to voter via change_membership.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group to add learner to
    /// * `node_id` - ID of the node to add
    /// * `addr` - Address of the node
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub async fn add_learner(
        &self,
        group_id: GroupId,
        node_id: NodeId,
        addr: String,
    ) -> Result<()> {
        let raft = self
            .node
            .get_raft_group(group_id)
            .ok_or_else(|| Error::Internal(format!("Group {} not found", group_id)))?;

        // Add node address to network factory
        self.node.add_node_address(node_id, addr.clone());

        // Add as learner
        use openraft::BasicNode;
        let node = BasicNode { addr };
        raft.add_learner(node_id, node, true)
            .await
            .map_err(|e| Error::Internal(format!("Failed to add learner: {:?}", e)))?;

        Ok(())
    }

    /// Promote a learner to voter
    ///
    /// After a learner has caught up with the log, this promotes it to a voting member.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group ID
    /// * `new_members` - Complete new member set (including the promoted learner)
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub async fn promote_learner(&self, group_id: GroupId, new_members: Vec<NodeId>) -> Result<()> {
        self.apply_membership_change(group_id, new_members).await
    }

    /// Check if a group is ready for membership change
    ///
    /// This checks if the group's leader is available and the group is healthy.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group to check
    ///
    /// # Returns
    ///
    /// `true` if ready for membership change
    pub async fn is_group_ready(&self, group_id: GroupId) -> bool {
        if let Some(raft) = self.node.get_raft_group(group_id) {
            // Check if we can get metrics (indicates group is functioning)
            let metrics = raft.metrics().borrow().clone();
            metrics.current_leader.is_some()
        } else {
            false
        }
    }

    /// Get MetaRaft instance
    pub fn meta_raft(&self) -> &Arc<MetaRaftNode> {
        &self.meta_raft
    }

    /// Get node instance
    pub fn node(&self) -> &Arc<MultiRaftNode> {
        &self.node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::Config;

    #[tokio::test]
    async fn test_membership_coordinator_creation() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let config = Config::default();
        let node = Arc::new(MultiRaftNode::new(1, temp_dir.path(), config.clone(), None).await.unwrap());

        let meta_dir = temp_dir.path().join("meta");
        let meta_raft = Arc::new(MetaRaftNode::new(1, meta_dir, config).await.unwrap());

        let _coordinator = MembershipCoordinator::new(node, meta_raft);
        // Just test creation succeeds
    }

    #[tokio::test]
    async fn test_is_group_ready_nonexistent() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let config = Config::default();
        let node = Arc::new(MultiRaftNode::new(1, temp_dir.path(), config.clone(), None).await.unwrap());

        let meta_dir = temp_dir.path().join("meta");
        let meta_raft = Arc::new(MetaRaftNode::new(1, meta_dir, config).await.unwrap());

        let coordinator = MembershipCoordinator::new(node, meta_raft);

        // Group 999 doesn't exist
        let ready = coordinator.is_group_ready(999).await;
        assert!(!ready);
    }
}
