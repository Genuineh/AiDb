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
use super::meta_types::{ClusterMeta, MetaRequest, MetaResponse, NodeStatus, SlotMigrationState};
use super::raft_network::RaftNetworkClientFactory;
use super::raft_storage::{NodeId, OpenRaftStorage, Request, Response, TypeConfig};
use super::sharded_storage::GroupId;
use crate::config::Options;
use crate::error::{Error, Result};
use crate::DB;
use tracing::{info, warn};

/// MetaRaft Node for managing cluster metadata
///
/// This is a specialized Raft node (Group 0) that manages global cluster metadata
/// including slot-to-group mappings, group membership, and node information.
///
/// Like `OpenRaftNode`, this node maintains a reference to the network factory,
/// allowing callers to pre-populate node addresses before calling Raft membership
/// operations like `add_learner` or `change_membership`.
pub struct MetaRaftNode {
    /// Node ID
    node_id: NodeId,

    /// OpenRaft instance for MetaRaft group (Group 0)
    raft: Arc<Raft<TypeConfig>>,

    /// Metadata state machine
    meta_state: Arc<MetaStateMachine>,

    /// Network factory for managing node addresses
    ///
    /// This is stored so that callers can add node addresses before Raft
    /// membership changes. The factory is shared with the Raft instance.
    network_factory: Arc<RwLock<RaftNetworkClientFactory>>,

    /// Data directory (reserved for future use)
    #[allow(dead_code)]
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
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = Config::default();
    /// let node = MetaRaftNode::new(1, "./data/meta", config).await?;
    /// # Ok(())
    /// # }
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
        let db = DB::open(&db_dir, options)?;
        let storage = OpenRaftStorage::with_meta_state(db, meta_state.clone())?;

        // Use Adaptor to split storage into log_store and state_machine
        let (log_store, state_machine) = Adaptor::new(storage);

        // Create network factory and wrap in Arc<RwLock<>> for shared access
        let network_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(node_id)));

        // Clone the factory to share the underlying Arc<RwLock<HashMap>> of node addresses
        // This ensures both the Raft instance and stored factory reference the same node map
        let network_for_raft = network_factory.read().clone();

        // Validate and build config
        let config = config
            .validate()
            .map_err(|e| Error::Internal(format!("Invalid Raft config: {:?}", e)))?;

        // Create Raft instance
        let raft = Raft::new(node_id, Arc::new(config), network_for_raft, log_store, state_machine)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create Raft: {:?}", e)))?;

        Ok(Self { node_id, raft: Arc::new(raft), meta_state, network_factory, data_dir })
    }

    /// Initialize a new MetaRaft cluster
    ///
    /// This should be called once on the first node to bootstrap the cluster.
    /// If the Raft cluster is already initialized (e.g. after a restart), this
    /// is a no-op.
    ///
    /// # Arguments
    ///
    /// * `members` - Initial cluster members with their addresses
    pub async fn initialize(&self, members: Vec<(NodeId, String)>) -> Result<()> {
        let mut nodes = std::collections::BTreeMap::new();
        for (member_id, addr) in members {
            // Store the address in network factory so openraft can reach peers
            self.network_factory.write().add_node(member_id, addr.clone());
            nodes.insert(member_id, BasicNode { addr });
        }

        // Skip if already initialized (e.g. after restart) to avoid NotAllowed error
        if self
            .raft
            .is_initialized()
            .await
            .map_err(|e| Error::Internal(format!("Failed to check MetaRaft init status: {:?}", e)))?
        {
            return Ok(());
        }

        self.raft
            .initialize(nodes)
            .await
            .map_err(|e| Error::Internal(format!("Failed to initialize MetaRaft: {:?}", e)))?;

        Ok(())
    }

    /// Add a learner node to the cluster
    pub async fn add_learner(&self, node_id: NodeId, node: BasicNode) -> Result<()> {
        // Store the address in network factory so openraft can reach this peer
        self.network_factory.write().add_node(node_id, node.addr.clone());

        self.raft
            .add_learner(node_id, node, true)
            .await
            .map_err(|e| Error::Internal(format!("Failed to add learner: {:?}", e)))?;

        Ok(())
    }

    /// Change cluster membership
    pub async fn change_membership(&self, members: BTreeSet<NodeId>, retain: bool) -> Result<()> {
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

    /// Set migration state for a slot.
    pub async fn set_slot_migration_state(
        &self,
        slot: u16,
        state: SlotMigrationState,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::SetSlotMigrationState { slot, state };
        self.propose_meta_change(request).await
    }

    /// Clear migration metadata for a slot.
    pub async fn clear_slot_migration(&self, slot: u16) -> Result<MetaResponse> {
        let request = MetaRequest::ClearSlotMigration { slot };
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

    /// Update node status in cluster metadata.
    pub async fn update_node_status(
        &self,
        node_id: NodeId,
        status: NodeStatus,
    ) -> Result<MetaResponse> {
        let request = MetaRequest::UpdateNodeStatus { node_id, status };
        self.propose_meta_change(request).await
    }

    /// Get current cluster metadata
    ///
    /// This reads from the local state machine without going through Raft.
    pub fn get_cluster_meta(&self) -> ClusterMeta {
        self.meta_state.get_cluster_meta()
    }

    /// See [`MetaStateMachine::get_config_version`].
    pub fn get_config_version(&self) -> u64 {
        self.meta_state.get_config_version()
    }

    /// Propose a metadata change through Raft consensus
    ///
    /// This serializes the MetaRequest and proposes it as a Raft log entry.
    async fn propose_meta_change(&self, request: MetaRequest) -> Result<MetaResponse> {
        let t0 = std::time::Instant::now();
        info!(
            diag_event = "metaraft_propose_attempt",
            requester_node_id = self.node_id,
            request_type = %match &request {
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
            },
            "MetaRaft propose attempt"
        );
        // Create a Request::Meta directly
        let meta_request = Request::Meta(request);

        // Propose through Raft
        let cw = self
            .raft
            .client_write(meta_request)
            .await
            .map_err(|e| {
                let emsg = format!("{:?}", e);
                if emsg.contains("ForwardToLeader") {
                    warn!(
                        diag_event = "metaraft_propose_forward_to_leader",
                        requester_node_id = self.node_id,
                        duration_ms = t0.elapsed().as_millis() as u64,
                        error = %emsg,
                        "MetaRaft propose returned ForwardToLeader"
                    );
                } else {
                    warn!(
                        diag_event = "metaraft_propose_failed",
                        requester_node_id = self.node_id,
                        duration_ms = t0.elapsed().as_millis() as u64,
                        error = %emsg,
                        "MetaRaft propose failed"
                    );
                }
                Error::Internal(format!("Failed to propose change: {:?}", e))
            })?;

        match cw.data {
            Response::Ok => {
                info!(
                    diag_event = "metaraft_propose_success",
                    requester_node_id = self.node_id,
                    duration_ms = t0.elapsed().as_millis() as u64,
                    "MetaRaft propose applied"
                );
                Ok(MetaResponse::Ok)
            }
            Response::Error(msg) => {
                warn!(
                    diag_event = "metaraft_apply_rejected",
                    requester_node_id = self.node_id,
                    duration_ms = t0.elapsed().as_millis() as u64,
                    error = %msg,
                    "MetaRaft state machine rejected metadata change"
                );
                Err(Error::Internal(msg))
            }
            Response::Value(v) => Err(Error::Internal(format!(
                "Unexpected MetaRaft client_write payload: {:?}",
                v
            ))),
        }
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        self.raft.ensure_linearizable().await.is_ok()
    }

    /// Get the current leader ID
    pub async fn get_leader(&self) -> Option<NodeId> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader
    }

    /// Look up a peer's gRPC address from the replicated MetaRaft membership.
    ///
    /// OpenRaft replicates `BasicNode { addr }` to every voter/learner, so even
    /// nodes that never received the `ADDLEARNER` command directly will know the
    /// correct address after log-replay.
    pub fn get_member_address(&self, node_id: NodeId) -> Option<String> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics
            .membership_config
            .membership()
            .get_node(&node_id)
            .map(|n| n.addr.clone())
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get Raft instance (for advanced operations)
    pub fn raft(&self) -> &Arc<Raft<TypeConfig>> {
        &self.raft
    }

    /// Add a known node address to the network factory without changing membership.
    ///
    /// This is used to pre-populate peer addresses from configuration so the node can
    /// contact other nodes for elections and replication. Call this method before
    /// `add_learner` or `change_membership` to ensure the network factory knows how
    /// to reach the target nodes.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The ID of the node to register
    /// * `address` - The network address (e.g., "http://127.0.0.1:50051")
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use aidb::cluster::MetaRaftNode;
    /// use openraft::Config;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = Config::default();
    /// let node = MetaRaftNode::new(1, "./data/meta", config).await?;
    ///
    /// // Pre-populate peer addresses before membership changes
    /// node.add_node_address(2, "http://127.0.0.1:50052".to_string());
    /// node.add_node_address(3, "http://127.0.0.1:50053".to_string());
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_node_address(&self, node_id: NodeId, address: String) {
        self.network_factory.write().add_node(node_id, address);
    }

    /// Remove a known node address from the network factory.
    ///
    /// This removes the address mapping for a node. Call this after removing a node
    /// from the cluster to clean up the network factory.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The ID of the node to remove
    pub fn remove_node_address(&self, node_id: NodeId) {
        self.network_factory.write().remove_node(node_id);
    }

    /// Return current node addresses known to the network factory.
    ///
    /// This returns all node addresses that have been registered via `add_node_address`,
    /// `initialize`, or `add_learner`.
    ///
    /// # Returns
    ///
    /// A vector of (NodeId, address) tuples for all known nodes.
    pub fn node_addresses(&self) -> Vec<(NodeId, String)> {
        self.network_factory.read().list_nodes()
    }

    /// Get the network factory instance (for advanced operations).
    ///
    /// This provides direct access to the network factory, which can be useful
    /// for integrating with other components that need to share node address
    /// information.
    pub fn network_factory(&self) -> &Arc<RwLock<RaftNetworkClientFactory>> {
        &self.network_factory
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

        // Verify that initialize also registered the node address
        let addresses = node.node_addresses();
        assert!(addresses.iter().any(|(id, _)| *id == 1));
    }

    #[tokio::test]
    async fn test_add_node_address() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Initially no addresses (except possibly self)
        let initial_count = node.node_addresses().len();

        // Add node addresses
        node.add_node_address(2, "http://127.0.0.1:50052".to_string());
        node.add_node_address(3, "http://127.0.0.1:50053".to_string());

        let addresses = node.node_addresses();
        assert_eq!(addresses.len(), initial_count + 2);

        // Verify addresses are correct
        let addr_map: std::collections::HashMap<_, _> = addresses.into_iter().collect();
        assert_eq!(addr_map.get(&2), Some(&"http://127.0.0.1:50052".to_string()));
        assert_eq!(addr_map.get(&3), Some(&"http://127.0.0.1:50053".to_string()));
    }

    #[tokio::test]
    async fn test_remove_node_address() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Add and then remove a node address
        node.add_node_address(2, "http://127.0.0.1:50052".to_string());
        assert!(node.node_addresses().iter().any(|(id, _)| *id == 2));

        node.remove_node_address(2);
        assert!(!node.node_addresses().iter().any(|(id, _)| *id == 2));
    }

    #[tokio::test]
    async fn test_network_factory_access() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MetaRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Verify network_factory() returns a valid reference
        let factory = node.network_factory();
        factory.write().add_node(5, "http://127.0.0.1:50055".to_string());

        // Verify the address is accessible via node_addresses()
        let addresses = node.node_addresses();
        assert!(addresses.iter().any(|(id, _)| *id == 5));
    }
}
