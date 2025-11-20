//! MetaRaft Node implementation for managing cluster metadata
//!
//! This module implements a specialized Raft node for managing global cluster metadata,
//! including slot mappings, group information, and node status.

use parking_lot::RwLock;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "raft-cluster")]
use openraft::{storage::Adaptor, BasicNode, Config, Raft};

use super::meta_state_machine::MetaStateMachine;
use super::meta_types::{ClusterMeta, MetaRequest, MetaResponse};
use super::raft_network::RaftNetworkClientFactory;
use super::raft_storage::{NodeId, OpenRaftStorage, Request, TypeConfig};
use super::sharded_storage::GroupId;
use crate::config::Options;
use crate::error::{Error, Result};
use crate::DB;

/// MetaRaft Node for managing cluster metadata
///
/// This is a specialized Raft node (Group 0) that manages global cluster metadata
/// including slot-to-group mappings, group membership, and node information.
pub struct MetaRaftNode {
    /// Node ID
    node_id: NodeId,

    /// OpenRaft instance for MetaRaft group (Group 0)
    raft: Arc<Raft<TypeConfig>>,

    /// Metadata state machine
    meta_state: Arc<MetaStateMachine>,

    /// Data directory
    data_dir: PathBuf,
}

impl MetaRaftNode {
    /// Create a new MetaRaft node
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of this node
    /// * `data_dir` - Data directory for metadata storage
    /// * `config` - Raft configuration
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use aidb::cluster::MetaRaftNode;
    /// use openraft::Config;
    ///
    /// let config = Config::default();
    /// let node = MetaRaftNode::new(1, "./data/meta", config).await.unwrap();
    /// ```
    pub async fn new<P: Into<PathBuf>>(
        node_id: NodeId,
        data_dir: P,
        config: Config,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)?;

        // Create metadata state machine
        let meta_dir = data_dir.join("meta");
        let meta_state = Arc::new(MetaStateMachine::new(&meta_dir)?);

        // Create storage for MetaRaft (Group 0)
        let db_dir = data_dir.join("db");
        let options = Options::default();
        let db = Arc::new(DB::open(&db_dir, options)?);
        let storage = OpenRaftStorage::new(db)?;

        // Use Adaptor to split storage into log_store and state_machine
        let (log_store, state_machine) = Adaptor::new(storage);

        // Create network factory
        let network = RaftNetworkClientFactory::new(node_id);

        // Validate and build config
        let config = config
            .validate()
            .map_err(|e| Error::Internal(format!("Invalid Raft config: {:?}", e)))?;

        // Create Raft instance
        let raft = Raft::new(node_id, Arc::new(config), network, log_store, state_machine)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create Raft: {:?}", e)))?;

        Ok(Self { node_id, raft: Arc::new(raft), meta_state, data_dir })
    }

    /// Initialize a new MetaRaft cluster
    ///
    /// This should be called once on the first node to bootstrap the cluster.
    ///
    /// # Arguments
    ///
    /// * `members` - Initial cluster members with their addresses
    pub async fn initialize(&self, members: Vec<(NodeId, String)>) -> Result<()> {
        let mut nodes = std::collections::BTreeMap::new();
        for (member_id, addr) in members {
            nodes.insert(member_id, BasicNode { addr });
        }

        self.raft
            .initialize(nodes)
            .await
            .map_err(|e| Error::Internal(format!("Failed to initialize MetaRaft: {:?}", e)))?;

        Ok(())
    }

    /// Add a learner node to the cluster
    pub async fn add_learner(&self, node_id: NodeId, node: BasicNode) -> Result<()> {
        self.raft
            .add_learner(node_id, node, true)
            .await
            .map_err(|e| Error::Internal(format!("Failed to add learner: {:?}", e)))?;

        Ok(())
    }

    /// Change cluster membership
    pub async fn change_membership(
        &self,
        members: BTreeSet<NodeId>,
        retain: bool,
    ) -> Result<()> {
        self.raft
            .change_membership(members, retain)
            .await
            .map_err(|e| Error::Internal(format!("Failed to change membership: {:?}", e)))?;

        Ok(())
    }

    /// Add a node to the cluster metadata
    ///
    /// This proposes a metadata change through Raft consensus.
    pub async fn add_node(&self, node_id: NodeId, addr: String) -> Result<MetaResponse> {
        let request = MetaRequest::AddNode { node_id, addr };
        self.propose_meta_change(request).await
    }

    /// Remove a node from the cluster metadata
    pub async fn remove_node(&self, node_id: NodeId) -> Result<MetaResponse> {
        let request = MetaRequest::RemoveNode { node_id };
        self.propose_meta_change(request).await
    }

    /// Create a new Raft group
    pub async fn create_group(
        &self,
        group_id: GroupId,
        replicas: Vec<NodeId>,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::CreateGroup { group_id, replicas };
        self.propose_meta_change(request).await
    }

    /// Update slot mappings
    pub async fn update_slots(
        &self,
        start: u16,
        end: u16,
        group_id: GroupId,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::UpdateSlots { start, end, group_id };
        self.propose_meta_change(request).await
    }

    /// Update group membership
    pub async fn update_group_members(
        &self,
        group_id: GroupId,
        replicas: Vec<NodeId>,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::UpdateGroupMembers { group_id, replicas };
        self.propose_meta_change(request).await
    }

    /// Start a slot migration
    pub async fn start_migration(
        &self,
        slot: u16,
        from_group: GroupId,
        to_group: GroupId,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::StartMigration { slot, from_group, to_group };
        self.propose_meta_change(request).await
    }

    /// Complete a slot migration
    pub async fn complete_migration(&self, slot: u16) -> Result<MetaResponse> {
        let request = MetaRequest::CompleteMigration { slot };
        self.propose_meta_change(request).await
    }

    /// Update group leader
    pub async fn update_group_leader(
        &self,
        group_id: GroupId,
        leader: NodeId,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::UpdateGroupLeader { group_id, leader };
        self.propose_meta_change(request).await
    }

    /// Get current cluster metadata
    ///
    /// This reads from the local state machine without going through Raft.
    pub fn get_cluster_meta(&self) -> ClusterMeta {
        self.meta_state.get_cluster_meta()
    }

    /// Propose a metadata change through Raft consensus
    ///
    /// This serializes the MetaRequest and proposes it as a Raft log entry.
    async fn propose_meta_change(&self, request: MetaRequest) -> Result<MetaResponse> {
        // Serialize the MetaRequest
        let data = bincode::serialize(&request)
            .map_err(|e| Error::Internal(format!("Failed to serialize request: {}", e)))?;

        // Wrap in a Request::WriteBatch for compatibility with existing state machine
        let batch_request = Request::Put {
            key: b"meta:request".to_vec(),
            value: data,
        };

        // Propose through Raft
        let response = self
            .raft
            .client_write(batch_request)
            .await
            .map_err(|e| Error::Internal(format!("Failed to propose change: {:?}", e)))?;

        // For now, return Ok - in a full implementation, we'd decode the response
        Ok(MetaResponse::Ok)
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        self.raft
            .ensure_linearizable()
            .await
            .is_ok()
    }

    /// Get the current leader ID
    pub async fn get_leader(&self) -> Option<NodeId> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get Raft instance (for advanced operations)
    pub fn raft(&self) -> &Arc<Raft<TypeConfig>> {
        &self.raft
    }

    /// Shutdown the node gracefully
    pub async fn shutdown(&self) -> Result<()> {
        self.raft
            .shutdown()
            .await
            .map_err(|e| Error::Internal(format!("Failed to shutdown: {:?}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_meta_raft_node() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();
        assert_eq!(node.node_id(), 1);
    }

    #[tokio::test]
    async fn test_get_cluster_meta() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();
        let meta = node.get_cluster_meta();

        assert_eq!(meta.config_version, 0);
        assert_eq!(meta.groups.len(), 0);
        assert_eq!(meta.nodes.len(), 0);
    }

    #[tokio::test]
    async fn test_initialize_cluster() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        let members = vec![(1, "127.0.0.1:50051".to_string())];

        // Initialize should succeed
        let result = node.initialize(members).await;
        assert!(result.is_ok());
    }
}
