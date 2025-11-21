//! Multi-Raft Node implementation for managing multiple Raft groups
//!
//! This module implements a node that can participate in multiple independent Raft groups
//! simultaneously, enabling horizontal scaling and data sharding.

use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "raft-cluster")]
use openraft::{storage::Adaptor, BasicNode, Config, Raft};

use super::meta_raft_node::MetaRaftNode;
use super::meta_types::ClusterMeta;
use super::raft_network::RaftNetworkClientFactory;
use super::raft_storage::{NodeId, Request, TypeConfig};
use super::router::Router;
use super::sharded_state_machine::ShardedStateMachine;
use super::sharded_storage::{GroupId, ShardedRaftStorage};
use crate::config::Options;
use crate::error::{Error, Result};

/// Multi-Raft Node managing multiple independent Raft groups
///
/// This node can participate in multiple Raft groups simultaneously, with each group
/// managing a subset of the data. This enables horizontal scaling where adding more
/// nodes increases both storage capacity and throughput.
///
/// # Architecture
///
/// ```text
/// MultiRaftNode
/// ├── MetaRaft (Group 0) - Manages cluster metadata
/// ├── Data Group 1 - Manages slots [0, 100)
/// ├── Data Group 2 - Manages slots [100, 200)
/// └── Data Group N - Manages slots [...]
/// ```
pub struct MultiRaftNode {
    /// Node ID
    node_id: NodeId,

    /// MetaRaft instance for cluster metadata management
    meta_raft: Option<Arc<MetaRaftNode>>,

    /// Active Raft groups (group_id -> Raft instance)
    groups: Arc<RwLock<HashMap<GroupId, Arc<Raft<TypeConfig>>>>>,

    /// Sharded storage managing per-group storage
    storage: Arc<ShardedRaftStorage>,

    /// Network factory for creating group-specific clients
    network_factory: Arc<RaftNetworkClientFactory>,

    /// Router for key-to-group mapping
    router: Option<Arc<Router>>,

    /// Sharded state machine managing per-group AiDb instances
    state_machine: Option<Arc<ShardedStateMachine>>,

    /// Data directory
    data_dir: PathBuf,

    /// Default Raft configuration
    raft_config: Config,
}

impl MultiRaftNode {
    /// Create a new Multi-Raft node
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of this node
    /// * `data_dir` - Base data directory
    /// * `config` - Default Raft configuration for data groups
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use aidb::cluster::MultiRaftNode;
    /// use openraft::Config;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = Config::default();
    /// let node = MultiRaftNode::new(1, "./data", config).await?;
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

        // Create sharded storage
        let groups_dir = data_dir.join("groups");
        let storage = Arc::new(ShardedRaftStorage::new(groups_dir, node_id)?);

        // Create network factory
        let network_factory = Arc::new(RaftNetworkClientFactory::new(node_id));

        Ok(Self {
            node_id,
            meta_raft: None,
            groups: Arc::new(RwLock::new(HashMap::new())),
            storage,
            network_factory,
            router: None,
            state_machine: None,
            data_dir,
            raft_config: config,
        })
    }

    /// Initialize MetaRaft on this node
    ///
    /// This should be called once to set up the MetaRaft group (Group 0) for managing
    /// cluster metadata.
    pub async fn init_meta_raft(&mut self, config: Config) -> Result<()> {
        let meta_dir = self.data_dir.join("meta");
        let meta_raft = Arc::new(MetaRaftNode::new(self.node_id, meta_dir, config).await?);
        self.meta_raft = Some(meta_raft);
        Ok(())
    }

    /// Initialize MetaRaft cluster (bootstrap)
    ///
    /// This should be called on the first node to bootstrap the MetaRaft cluster.
    pub async fn initialize_meta_cluster(&self, members: Vec<(NodeId, String)>) -> Result<()> {
        match &self.meta_raft {
            Some(meta) => meta.initialize(members).await,
            None => Err(Error::Internal("MetaRaft not initialized".to_string())),
        }
    }

    /// Create a new Raft group
    ///
    /// This dynamically creates a new Raft group that will participate in consensus
    /// independently from other groups.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Unique identifier for the group
    /// * `replicas` - Initial replica nodes for this group
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use aidb::cluster::MultiRaftNode;
    /// # use openraft::Config;
    /// # async fn example(node: &MultiRaftNode) -> Result<(), Box<dyn std::error::Error>> {
    /// let replicas = vec![1, 2, 3];
    /// node.create_raft_group(100, replicas).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_raft_group(
        &self,
        group_id: GroupId,
        replicas: Vec<NodeId>,
    ) -> Result<Arc<Raft<TypeConfig>>> {
        // Check if group already exists
        {
            let groups = self.groups.read();
            if let Some(group) = groups.get(&group_id) {
                return Ok(Arc::clone(group));
            }
        }

        // Create storage for this group
        let group_storage = self.storage.create_group(group_id)?;

        // Use Adaptor to split storage
        let (log_store, state_machine) = Adaptor::new((*group_storage).clone());

        // Create network factory (clone for this group)
        let network = RaftNetworkClientFactory::new(self.node_id);

        // Validate config
        let config = self
            .raft_config
            .clone()
            .validate()
            .map_err(|e| Error::Internal(format!("Invalid Raft config: {:?}", e)))?;

        // Create Raft instance
        let raft = Raft::new(self.node_id, Arc::new(config), network, log_store, state_machine)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create Raft: {:?}", e)))?;

        let raft = Arc::new(raft);

        // Initialize if this node is in the replica list
        if replicas.contains(&self.node_id) {
            let mut members = BTreeMap::new();
            for replica in &replicas {
                // Use default address - in production, these should be configured
                let addr = format!("127.0.0.1:{}", 50051 + replica);
                members.insert(*replica, BasicNode { addr });
            }

            // Only initialize if we're the first node (node_id is minimum)
            if self.node_id == *replicas.iter().min().unwrap_or(&self.node_id) {
                raft.initialize(members)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to initialize group: {:?}", e)))?;
            }
        }

        // Store in groups map
        {
            let mut groups = self.groups.write();
            groups.insert(group_id, Arc::clone(&raft));
        }

        Ok(raft)
    }

    /// Get an existing Raft group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Some(Raft instance) if the group exists, None otherwise
    pub fn get_raft_group(&self, group_id: GroupId) -> Option<Arc<Raft<TypeConfig>>> {
        let groups = self.groups.read();
        groups.get(&group_id).map(Arc::clone)
    }

    /// Remove a Raft group
    ///
    /// This stops the Raft group and removes it from active groups.
    /// The storage is not deleted to allow for recovery.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    pub async fn remove_raft_group(&self, group_id: GroupId) -> Result<bool> {
        // Remove from active groups
        let raft = {
            let mut groups = self.groups.write();
            groups.remove(&group_id)
        };

        // Shutdown the Raft instance if it existed
        if let Some(raft) = raft {
            raft.shutdown()
                .await
                .map_err(|e| Error::Internal(format!("Failed to shutdown group: {:?}", e)))?;

            // Remove from storage
            self.storage.remove_group(group_id)?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// List all active group IDs
    pub fn list_groups(&self) -> Vec<GroupId> {
        let groups = self.groups.read();
        groups.keys().copied().collect()
    }

    /// Get the number of active groups
    pub fn group_count(&self) -> usize {
        let groups = self.groups.read();
        groups.len()
    }

    /// Check if a group exists
    pub fn has_group(&self, group_id: GroupId) -> bool {
        let groups = self.groups.read();
        groups.contains_key(&group_id)
    }

    /// Get MetaRaft instance
    pub fn meta_raft(&self) -> Option<&Arc<MetaRaftNode>> {
        self.meta_raft.as_ref()
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get storage instance
    pub fn storage(&self) -> &Arc<ShardedRaftStorage> {
        &self.storage
    }

    /// Add a node address to the network factory
    ///
    /// This is required before creating groups that include this node.
    pub fn add_node_address(&self, node_id: NodeId, addr: String) {
        self.network_factory.add_node(node_id, addr);
    }

    /// Load existing groups from disk
    ///
    /// This scans the storage directory and creates Raft instances for all
    /// existing groups. Useful for recovery after restart.
    pub async fn load_existing_groups(&self) -> Result<usize> {
        // Load storage groups
        let loaded = self.storage.load_existing_groups()?;

        // For each loaded storage group, create a Raft instance
        let group_ids: Vec<GroupId> = self.storage.list_groups();

        for group_id in group_ids {
            if !self.has_group(group_id) {
                let group_storage = self
                    .storage
                    .get_group(group_id)
                    .ok_or_else(|| Error::Internal(format!("Group {} not found", group_id)))?;

                // Use Adaptor to split storage
                let (log_store, state_machine) = Adaptor::new((*group_storage).clone());

                let network = RaftNetworkClientFactory::new(self.node_id);

                let config = self
                    .raft_config
                    .clone()
                    .validate()
                    .map_err(|e| Error::Internal(format!("Invalid Raft config: {:?}", e)))?;

                let raft =
                    Raft::new(self.node_id, Arc::new(config), network, log_store, state_machine)
                        .await
                        .map_err(|e| Error::Internal(format!("Failed to create Raft: {:?}", e)))?;

                let mut groups = self.groups.write();
                groups.insert(group_id, Arc::new(raft));
            }
        }

        Ok(loaded)
    }

    /// Shutdown all Raft groups gracefully
    pub async fn shutdown(&self) -> Result<()> {
        // Shutdown MetaRaft
        if let Some(meta) = &self.meta_raft {
            meta.shutdown().await?;
        }

        // Shutdown all data groups
        let group_ids: Vec<GroupId> = self.list_groups();
        for group_id in group_ids {
            if let Some(raft) = self.get_raft_group(group_id) {
                raft.shutdown()
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to shutdown group: {:?}", e)))?;
            }
        }

        // Shutdown state machine
        if let Some(state_machine) = &self.state_machine {
            state_machine.shutdown()?;
        }

        Ok(())
    }

    /// Initialize the router with metadata from MetaRaft
    ///
    /// This should be called after MetaRaft is initialized to enable
    /// automatic key routing.
    pub fn init_router(&mut self) -> Result<()> {
        let meta_raft = self
            .meta_raft
            .as_ref()
            .ok_or_else(|| Error::Internal("MetaRaft not initialized".to_string()))?;

        let meta = meta_raft.get_cluster_meta();
        self.router = Some(Arc::new(Router::with_meta_client(meta, Arc::clone(meta_raft))));

        Ok(())
    }

    /// Initialize the sharded state machine
    ///
    /// This creates per-group AiDb instances for data storage.
    ///
    /// # Arguments
    ///
    /// * `options` - Database options template for creating DBs
    pub fn init_state_machine(&mut self, options: Options) -> Result<()> {
        let db_dir = self.data_dir.join("state_machine");
        std::fs::create_dir_all(&db_dir)?;

        let state_machine = if let Some(router) = &self.router {
            ShardedStateMachine::with_router(db_dir, options, Arc::clone(router))
        } else {
            ShardedStateMachine::new(db_dir, options)
        };

        self.state_machine = Some(Arc::new(state_machine));
        Ok(())
    }

    /// Start the node and join the cluster
    ///
    /// This method performs the complete node startup sequence:
    /// 1. Joins MetaRaft (if not bootstrap node)
    /// 2. Gets cluster metadata
    /// 3. Loads groups that this node should participate in
    ///
    /// # Arguments
    ///
    /// * `is_bootstrap` - Whether this is the bootstrap node
    /// * `meta_leader_addr` - Address of MetaRaft leader (required if not bootstrap)
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub async fn start(&self, is_bootstrap: bool, meta_leader_addr: Option<String>) -> Result<()> {
        // If not bootstrap node, join MetaRaft
        if !is_bootstrap {
            if let Some(addr) = meta_leader_addr {
                self.join_meta_raft(&addr).await?;
            } else {
                return Err(Error::Internal(
                    "MetaRaft leader address required for non-bootstrap node".to_string(),
                ));
            }
        }

        // Get cluster metadata
        if let Some(meta_raft) = &self.meta_raft {
            let meta = meta_raft.get_cluster_meta();

            // Load groups that this node should participate in
            self.load_groups_from_meta(&meta).await?;
        }

        Ok(())
    }

    /// Join an existing MetaRaft cluster
    ///
    /// This method adds the node as a learner to the MetaRaft cluster
    /// and waits for promotion to voter.
    ///
    /// # Arguments
    ///
    /// * `leader_addr` - Address of the current MetaRaft leader
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub async fn join_meta_raft(&self, _leader_addr: &str) -> Result<()> {
        let meta_raft = self
            .meta_raft
            .as_ref()
            .ok_or_else(|| Error::Internal("MetaRaft not initialized".to_string()))?;

        // TODO: Connect to leader and add ourselves as learner
        // For now, this is a placeholder that assumes MetaRaft is already initialized

        // Add node to cluster metadata through MetaRaft
        let node_addr = format!("127.0.0.1:{}", 50051 + self.node_id);
        let _response = meta_raft.add_node(self.node_id, node_addr).await?;

        // In a full implementation, we would:
        // 1. Connect to the MetaRaft leader via gRPC
        // 2. Call add_learner() on the leader
        // 3. Wait for log replication to catch up
        // 4. Call change_membership() to promote to voter

        Ok(())
    }

    /// Load groups from cluster metadata
    ///
    /// Creates Raft instances for all groups that this node should participate in.
    ///
    /// # Arguments
    ///
    /// * `meta` - Cluster metadata
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    async fn load_groups_from_meta(&self, meta: &ClusterMeta) -> Result<()> {
        // Find all groups where this node is a replica
        for (group_id, group_meta) in &meta.groups {
            if group_meta.is_replica(self.node_id) {
                // Create the group if it doesn't exist
                if !self.has_group(*group_id) {
                    self.create_raft_group(*group_id, group_meta.replicas.clone()).await?;
                }
            }
        }

        Ok(())
    }

    /// Start watching MetaRaft for metadata updates
    ///
    /// This spawns a background task that keeps the router's metadata cache up-to-date.
    ///
    /// # Arguments
    ///
    /// * `poll_interval_ms` - How often to check for updates (milliseconds)
    ///
    /// # Returns
    ///
    /// A join handle for the background task
    pub async fn start_metadata_watcher(
        &self,
        poll_interval_ms: u64,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not initialized".to_string()))?;

        router.clone().start_watching(poll_interval_ms).await
    }

    /// Put a key-value pair with automatic routing
    ///
    /// This routes the key to the appropriate group and proposes a write.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to insert
    /// * `value` - Value to insert
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not initialized".to_string()))?;

        let group_id = router.route(&key)?;
        let raft = self
            .get_raft_group(group_id)
            .ok_or_else(|| Error::NotFound(format!("Group {} not found", group_id)))?;

        let request = Request::Put { key, value };
        raft.client_write(request)
            .await
            .map_err(|e| Error::Internal(format!("Raft write failed: {:?}", e)))?;

        Ok(())
    }

    /// Get a value with automatic routing
    ///
    /// # Arguments
    ///
    /// * `key` - Key to retrieve
    ///
    /// # Returns
    ///
    /// The value if found, `None` otherwise
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let state_machine = self
            .state_machine
            .as_ref()
            .ok_or_else(|| Error::Internal("State machine not initialized".to_string()))?;

        state_machine.get_routed(key)
    }

    /// Delete a key with automatic routing
    ///
    /// # Arguments
    ///
    /// * `key` - Key to delete
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub async fn delete(&self, key: &[u8]) -> Result<()> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not initialized".to_string()))?;

        let group_id = router.route(key)?;
        let raft = self
            .get_raft_group(group_id)
            .ok_or_else(|| Error::NotFound(format!("Group {} not found", group_id)))?;

        let request = Request::Delete { key: key.to_vec() };
        raft.client_write(request)
            .await
            .map_err(|e| Error::Internal(format!("Raft write failed: {:?}", e)))?;

        Ok(())
    }

    /// Get the router instance
    pub fn router(&self) -> Option<&Arc<Router>> {
        self.router.as_ref()
    }

    /// Get the state machine instance
    pub fn state_machine(&self) -> Option<&Arc<ShardedStateMachine>> {
        self.state_machine.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_multi_raft_node() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();
        assert_eq!(node.node_id(), 1);
        assert_eq!(node.group_count(), 0);
    }

    #[tokio::test]
    async fn test_create_raft_group() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Create a group
        let replicas = vec![1];
        let group = node.create_raft_group(100, replicas).await.unwrap();
        assert!(Arc::strong_count(&group) >= 1);

        assert_eq!(node.group_count(), 1);
        assert!(node.has_group(100));
        assert!(!node.has_group(200));
    }

    #[tokio::test]
    async fn test_create_multiple_groups() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Create multiple groups
        for i in 1..=10 {
            node.create_raft_group(i, vec![1]).await.unwrap();
        }

        assert_eq!(node.group_count(), 10);

        // Verify all groups exist
        for i in 1..=10 {
            assert!(node.has_group(i));
        }
    }

    #[tokio::test]
    async fn test_create_group_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Create same group twice
        let group1 = node.create_raft_group(100, vec![1]).await.unwrap();
        let group2 = node.create_raft_group(100, vec![1]).await.unwrap();

        // Should return the same instance
        assert!(Arc::ptr_eq(&group1, &group2));
        assert_eq!(node.group_count(), 1);
    }

    #[tokio::test]
    async fn test_remove_group() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Create and remove a group
        node.create_raft_group(100, vec![1]).await.unwrap();
        assert!(node.has_group(100));

        let removed = node.remove_raft_group(100).await.unwrap();
        assert!(removed);
        assert!(!node.has_group(100));
        assert_eq!(node.group_count(), 0);

        // Try to remove again
        let removed = node.remove_raft_group(100).await.unwrap();
        assert!(!removed);
    }

    #[tokio::test]
    async fn test_list_groups() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        // Create groups
        node.create_raft_group(1, vec![1]).await.unwrap();
        node.create_raft_group(5, vec![1]).await.unwrap();
        node.create_raft_group(3, vec![1]).await.unwrap();

        let mut groups = node.list_groups();
        groups.sort();

        assert_eq!(groups, vec![1, 3, 5]);
    }

    #[tokio::test]
    async fn test_add_node_address() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config).await.unwrap();

        node.add_node_address(2, "127.0.0.1:50052".to_string());
        node.add_node_address(3, "127.0.0.1:50053".to_string());

        // Addresses are stored in network factory
        // Successfully added
    }

    #[tokio::test]
    async fn test_load_existing_groups() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().to_path_buf();

        // Create some groups
        {
            let config = Config::default();
            let node = MultiRaftNode::new(1, &storage_path, config).await.unwrap();
            node.create_raft_group(1, vec![1]).await.unwrap();
            node.create_raft_group(2, vec![1]).await.unwrap();
            node.create_raft_group(3, vec![1]).await.unwrap();
        }

        // Create new node instance and load existing groups
        let config = Config::default();
        let node = MultiRaftNode::new(1, &storage_path, config).await.unwrap();
        let loaded = node.load_existing_groups().await.unwrap();

        assert_eq!(loaded, 3);
        assert_eq!(node.group_count(), 3);
        assert!(node.has_group(1));
        assert!(node.has_group(2));
        assert!(node.has_group(3));
    }
}
