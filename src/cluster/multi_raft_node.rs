//! Multi-Raft Node implementation for managing multiple Raft groups
//!
//! This module implements a node that can participate in multiple independent Raft groups
//! simultaneously, enabling horizontal scaling and data sharding.

use parking_lot::RwLock;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "raft-cluster")]
use openraft::{
    error::{InitializeError, RaftError},
    storage::Adaptor,
    BasicNode, Config, Raft,
};

use super::meta_raft_node::MetaRaftNode;
use super::meta_types::{ClusterMeta, GroupMeta, NodeStatus};
use super::raft_network::RaftNetworkClientFactory;
use super::raft_storage::{NodeId, Request, TypeConfig};
use super::router::Router;
use super::sharded_state_machine::ShardedStateMachine;
use super::sharded_storage::{GroupId, ShardedRaftStorage};
use crate::config::Options;
use crate::error::{Error, Result};
use tokio::sync::Mutex as AsyncGroupInitMutex;

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

    /// Ensures only one task runs `Raft::new` for a local data group at a time.
    ///
    /// Without this, [`Self::load_groups_from_meta`] (user command) and the background
    /// metadata watcher can both pass [`Self::has_group`] before either inserts into
    /// [`Self::groups`], constructing two Raft cores on the same on-disk storage.
    group_init_lock: Arc<AsyncGroupInitMutex<()>>,

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

    /// Last MetaRaft metadata version applied by [`MultiRaftNode::sync_data_groups_from_meta`].
    ///
    /// Slot/group changes are usually proposed on the bootstrap node; other nodes replicate MetaRaft
    /// but would otherwise never create local data-group storage. Data-plane entrypoints bump this
    /// via [`MultiRaftNode::ensure_data_groups_for_current_meta`].
    groups_synced_for_config_version: Arc<AtomicU64>,

    /// Debounce state for leader sync: (group_id -> (last_proposed_leader, last_proposal_time))
    /// prevents thundering-herd proposals when multiple nodes observe the same Data Raft election.
    leader_sync_debounce: Arc<RwLock<HashMap<GroupId, (Option<NodeId>, Instant)>>>,
}

/// Result of a streaming scan operation on a single group.
pub struct GroupScanResult {
    /// The keys found in this scan batch
    pub keys: Vec<Vec<u8>>,
    /// Whether this group has been fully exhausted (no more keys)
    pub exhausted: bool,
    /// Last key seen (for cursor resume)
    pub last_key: Option<Vec<u8>>,
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
    /// let node = MultiRaftNode::new(1, "./data", config, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new<P: Into<PathBuf>>(
        node_id: NodeId,
        data_dir: P,
        config: Config,
        storage_options: Option<Options>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)?;

        // Create sharded storage (use custom options if provided, e.g. use_wal: false for cluster mode)
        let groups_dir = data_dir.join("groups");
        let storage = if let Some(opts) = storage_options {
            Arc::new(ShardedRaftStorage::with_options(groups_dir, node_id, opts)?)
        } else {
            Arc::new(ShardedRaftStorage::new(groups_dir, node_id)?)
        };

        // Create network factory
        let network_factory = Arc::new(RaftNetworkClientFactory::new(node_id));

        Ok(Self {
            node_id,
            meta_raft: None,
            groups: Arc::new(RwLock::new(HashMap::new())),
            group_init_lock: Arc::new(AsyncGroupInitMutex::new(())),
            storage,
            network_factory,
            router: None,
            state_machine: None,
            data_dir,
            raft_config: config,
            groups_synced_for_config_version: Arc::new(AtomicU64::new(0)),
            leader_sync_debounce: Arc::new(RwLock::new(HashMap::new())),
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

        let _init_guard = self.group_init_lock.lock().await;
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

        // Create a group-specific network factory so RPCs carry the correct group_id.
        let network = self.network_factory.as_ref().with_group_id(group_id);

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

        Self::maybe_initialize_raft(&raft, self.node_id, &replicas, None, || {
            self.peer_raft_grpc_addr(self.node_id)
        })
        .await?;

        // Store in groups map
        {
            let mut groups = self.groups.write();
            groups.insert(group_id, Arc::clone(&raft));
        }

        Ok(raft)
    }

    /// Like [`Self::create_raft_group`] but honours `designated_leader` from
    /// [`ClusterMeta`] so that only the meta-designated leader initialises the
    /// Raft group, avoiding split-brain when a replica's hash-based node-id
    /// happens to be smaller than the master's.
    async fn create_raft_group_with_leader(
        &self,
        group_id: GroupId,
        replicas: Vec<NodeId>,
        designated_leader: Option<NodeId>,
    ) -> Result<Arc<Raft<TypeConfig>>> {
        {
            let groups = self.groups.read();
            if let Some(group) = groups.get(&group_id) {
                return Ok(Arc::clone(group));
            }
        }

        let _init_guard = self.group_init_lock.lock().await;
        {
            let groups = self.groups.read();
            if let Some(group) = groups.get(&group_id) {
                return Ok(Arc::clone(group));
            }
        }

        let group_storage = self.storage.create_group(group_id)?;
        let (log_store, state_machine) = Adaptor::new((*group_storage).clone());
        let network = self.network_factory.as_ref().with_group_id(group_id);
        let config = self
            .raft_config
            .clone()
            .validate()
            .map_err(|e| Error::Internal(format!("Invalid Raft config: {:?}", e)))?;
        let raft = Raft::new(self.node_id, Arc::new(config), network, log_store, state_machine)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create Raft: {:?}", e)))?;
        let raft = Arc::new(raft);

        Self::maybe_initialize_raft(&raft, self.node_id, &replicas, designated_leader, || {
            self.peer_raft_grpc_addr(self.node_id)
        })
        .await?;

        {
            let mut groups = self.groups.write();
            groups.insert(group_id, Arc::clone(&raft));
        }

        Ok(raft)
    }

    /// Conditionally initialise a Raft instance as a single-voter group.
    ///
    /// When `designated_leader` is `Some`, only that node initialises.
    /// Otherwise fall back to the smallest node-id in `replicas`.
    async fn maybe_initialize_raft<F>(
        raft: &Raft<TypeConfig>,
        self_node_id: NodeId,
        replicas: &[NodeId],
        designated_leader: Option<NodeId>,
        self_addr_fn: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Result<String>,
    {
        if !replicas.contains(&self_node_id) {
            return Ok(());
        }
        if raft
            .is_initialized()
            .await
            .map_err(|e| Error::Internal(format!("Raft is_initialized failed: {:?}", e)))?
        {
            return Ok(());
        }
        let should_init = match designated_leader {
            Some(leader) => self_node_id == leader,
            None => self_node_id == *replicas.iter().min().unwrap_or(&self_node_id),
        };
        if should_init {
            let self_addr = self_addr_fn()?;
            let members = BTreeMap::from([(self_node_id, BasicNode { addr: self_addr })]);
            raft.initialize(members).await.or_else(|e| match e {
                RaftError::APIError(InitializeError::NotAllowed(_)) => Ok(()),
                other => Err(Error::Internal(format!(
                    "Failed to initialize group: {:?}",
                    other
                ))),
            })?;
        }
        Ok(())
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

    /// Sync actual Data Raft leaders back to MetaRaft metadata cache.
    ///
    /// For each local Data Raft group, reads `metrics.current_leader` from openraft
    /// and proposes an update to MetaRaft if it differs from the cached value.
    ///
    /// A per-group debounce prevents redundant proposals: the same leader for the
    /// same group is only proposed once per underlying sync interval (typically 2s).
    /// `ForwardToLeader` errors are silently skipped — the MetaRaft-leader node's
    /// own watcher will pick up the change on its next tick.
    pub async fn sync_data_group_leaders_to_meta(&self) -> Result<()> {
        let Some(meta_raft) = self.meta_raft() else {
            return Ok(());
        };
        let meta = meta_raft.get_cluster_meta();
        let now = Instant::now();

        // Minimum interval between proposals for the same (group, leader) pair.
        // Shorter than the watcher interval (2s) to allow one proposal per cycle.
        const DEBOUNCE_MS: u64 = 1500;

        for &group_id in &self.list_groups() {
            let Some(raft) = self.get_raft_group(group_id) else {
                continue;
            };
            let current_leader = raft.metrics().borrow().current_leader;

            // Do not propose None — missing cache is better than wrong cache.
            let Some(actual_leader) = current_leader else {
                continue;
            };

            let cached_leader = meta.groups.get(&group_id).and_then(|g| g.leader);
            if Some(actual_leader) == cached_leader {
                continue;
            }

            // Debounce: skip if we already proposed this exact leader recently.
            {
                let debounce = self.leader_sync_debounce.read();
                if let Some((prev_leader, prev_time)) = debounce.get(&group_id) {
                    if *prev_leader == Some(actual_leader)
                        && (now.duration_since(*prev_time).as_millis() as u64)
                            < DEBOUNCE_MS
                    {
                        continue;
                    }
                }
            }

            tracing::info!(
                group_id = group_id,
                actual_leader = actual_leader,
                cached_leader = ?cached_leader,
                "Syncing Data Raft leader to MetaRaft cache"
            );

            match meta_raft.update_group_leader(group_id, actual_leader).await {
                Ok(_) => {
                    let mut debounce = self.leader_sync_debounce.write();
                    debounce.insert(group_id, (Some(actual_leader), now));
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("ForwardToLeader") {
                        // Not the MetaRaft leader — the leader's watcher will sync.
                        tracing::debug!(
                            group_id = group_id,
                            "Leader sync skipped: this node is not MetaRaft leader"
                        );
                    } else {
                        tracing::warn!(
                            group_id = group_id,
                            error = %msg,
                            "Leader sync proposal failed"
                        );
                    }
                }
            }
        }
        Ok(())
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

    /// Resolve a peer's Raft gRPC URL for OpenRaft membership (`initialize` / replication).
    ///
    /// Uses the shared [`RaftNetworkClientFactory`] (e.g. `CLUSTER METARAFT ADDLEARNER`),
    /// then falls back to [`ClusterMeta::nodes`] from MetaRaft (e.g. `CLUSTER MEET` Redis `host:port`).
    pub fn peer_raft_grpc_addr(&self, peer: NodeId) -> Result<String> {
        // 1. Directly registered addresses (ADDLEARNER on this node)
        if let Some((_, mut a)) = self
            .network_factory
            .list_nodes()
            .into_iter()
            .find(|(id, _)| *id == peer)
        {
            if !a.starts_with("http://") && !a.starts_with("https://") {
                a = format!("http://{}", a);
            }
            return Ok(a);
        }

        // 2. MetaRaft replicated membership (correct Docker hostnames from ADDLEARNER,
        //    available on ALL members after log replication — not just the bootstrap node)
        if let Some(meta_raft) = self.meta_raft.as_ref() {
            if let Some(mut a) = meta_raft.get_member_address(peer) {
                if !a.starts_with("http://") && !a.starts_with("https://") {
                    a = format!("http://{}", a);
                }
                return Ok(a);
            }
        }

        // 3. Infer from CLUSTER MEET Redis addresses (may be wrong in Docker)
        if let Some(meta_raft) = self.meta_raft.as_ref() {
            let meta = meta_raft.get_cluster_meta();
            if let Some(node) = meta.nodes.get(&peer) {
                return Ok(Self::infer_raft_grpc_url(&node.addr));
            }
        }

        // 4. Unit tests without MetaRaft
        if self.meta_raft.is_none() {
            let port = 50051u64.saturating_add(peer);
            if port <= u16::MAX as u64 {
                return Ok(format!("http://127.0.0.1:{}", port));
            }
        }

        Err(Error::Internal(format!(
            "No cluster address for node {}; register via CLUSTER MEET / METARAFT ADDLEARNER",
            peer
        )))
    }

    /// `CLUSTER MEET` stores Redis `host:data_port`; bridge to inter-node Raft listener
    /// (`50051 + data_port - 6379` for the default layout). Values that already look like
    /// gRPC URLs are passed through.
    fn infer_raft_grpc_url(addr: &str) -> String {
        let addr = addr.trim();
        if addr.starts_with("http://") || addr.starts_with("https://") {
            return addr.to_string();
        }
        if let Some((host, port_s)) = addr.rsplit_once(':') {
            if let Ok(port) = port_s.parse::<u16>() {
                if (50_050..=50_200).contains(&port) {
                    return format!("http://{}:{}", host, port);
                }
                if port >= 6379 {
                    let raft = 50051u32 + u32::from(port.saturating_sub(6379));
                    return format!("http://{}:{}", host, raft);
                }
            }
        }
        format!("http://{}", addr)
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
            if self.has_group(group_id) {
                continue;
            }
            let _init_guard = self.group_init_lock.lock().await;
            if self.has_group(group_id) {
                continue;
            }

            let group_storage = self
                .storage
                .get_group(group_id)
                .ok_or_else(|| Error::Internal(format!("Group {} not found", group_id)))?;

            // Use Adaptor to split storage
            let (log_store, state_machine) = Adaptor::new((*group_storage).clone());

            let network = self.network_factory.as_ref().with_group_id(group_id);

            let config = self
                .raft_config
                .clone()
                .validate()
                .map_err(|e| Error::Internal(format!("Invalid Raft config: {:?}", e)))?;

            let raft = Raft::new(self.node_id, Arc::new(config), network, log_store, state_machine)
                .await
                .map_err(|e| Error::Internal(format!("Failed to create Raft: {:?}", e)))?;

            let mut groups = self.groups.write();
            groups.insert(group_id, Arc::new(raft));
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
        // TODO: Make node address configurable instead of hard-coded format
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
                    tracing::info!(
                        diag_event = "multi_raft_create_group_from_meta",
                        node_id = self.node_id,
                        group_id = *group_id,
                        replica_count = group_meta.replicas.len(),
                        meta_group_count = meta.groups.len(),
                        "creating local data Raft group from cluster metadata"
                    );
                    self.create_raft_group_with_leader(
                        *group_id,
                        group_meta.replicas.clone(),
                        group_meta.leader,
                    )
                    .await?;
                } else if let Some(raft) = self.get_raft_group(*group_id) {
                    // Group exists (e.g. loaded from disk by load_existing_groups).
                    // Check if it is initialized with a valid membership for this node.
                    let metrics = raft.metrics().borrow().clone();
                    let is_initialized = metrics.membership_config.membership().voter_ids().next().is_some();
                    let is_voter = metrics.membership_config.membership().voter_ids().any(|id| id == self.node_id);
                    let should_be_in_group = group_meta.replicas.contains(&self.node_id);

                    if is_initialized && !is_voter {
                        // This node is in the group's replica list but is currently a
                        // learner, not a voter. This is normal during early cluster
                        // provisioning, or during failover when the leader hasn't yet
                        // reconciled membership.
                        //
                        // DO NOT destroy and re-create the group — that would erase the
                        // local Raft log. The leader's reconcile_data_group_membership
                        // (or the failover self-healing path) will promote this node.
                        tracing::warn!(
                            diag_event = "multi_raft_learner_not_voter",
                            node_id = self.node_id,
                            group_id = *group_id,
                            has_leader = metrics.current_leader.is_some(),
                            current_voters = ?metrics.membership_config.membership().voter_ids().collect::<Vec<_>>(),
                            meta_replicas = ?group_meta.replicas,
                            "Data Raft group has initialized membership but local node is a learner, not a voter; keeping existing group (leader will reconcile, or failover will self-heal)"
                        );
                    }
                    // Safe no-op for already-initialized groups; initializes uninitialized
                    // learners (e.g. groups freshly created from disk).
                    Self::maybe_initialize_raft(
                        &raft,
                        self.node_id,
                        &group_meta.replicas,
                        group_meta.leader,
                        || self.peer_raft_grpc_addr(self.node_id),
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Ensure local Raft data groups exist for every group in cluster metadata that includes
    /// this node in `replicas`. Call after MetaRaft topology changes (ADDSLOTS, REPLICATE, etc.)
    /// or on startup once MetaRaft is available.
    pub async fn sync_data_groups_from_meta(&self) -> Result<()> {
        let t0 = std::time::Instant::now();
        let meta_raft = self
            .meta_raft
            .as_ref()
            .ok_or_else(|| Error::Internal("MetaRaft not initialized".to_string()))?;
        let cv_before = meta_raft.get_config_version();
        let meta = meta_raft.get_cluster_meta();
        let local_groups_before = self.list_groups().len();
        tracing::info!(
            diag_event = "sync_data_groups_from_meta_start",
            node_id = self.node_id,
            meta_config_version = cv_before,
            meta_group_count = meta.groups.len(),
            local_data_group_count = local_groups_before,
            "sync_data_groups_from_meta start"
        );
        self.load_groups_from_meta(&meta).await?;
        self.reconcile_data_group_membership(&meta).await;
        let cv_after = meta_raft.get_config_version();
        self.groups_synced_for_config_version
            .store(cv_after, Ordering::Release);
        tracing::info!(
            diag_event = "sync_data_groups_from_meta_done",
            node_id = self.node_id,
            duration_ms = t0.elapsed().as_millis() as u64,
            meta_config_version = cv_after,
            local_data_group_count = self.list_groups().len(),
            "sync_data_groups_from_meta done"
        );
        Ok(())
    }

    /// Voter set we want for a data group: every replica in [`GroupMeta::replicas`] that is not
    /// [`NodeStatus::Offline`] in [`ClusterMeta::nodes`].
    ///
    /// Without promoting all live replicas to voters, only the initial leader may be a voter; after
    /// it fails, remaining nodes stay learners and cannot elect a Raft leader — Redis failover alone
    /// is not enough.
    fn raft_voter_goal_for_group(meta: &ClusterMeta, group_meta: &GroupMeta) -> BTreeSet<NodeId> {
        group_meta
            .replicas
            .iter()
            .copied()
            .filter(|id| {
                meta.nodes
                    .get(id)
                    .map(|n| n.status != NodeStatus::Offline)
                    .unwrap_or(true)
            })
            .collect()
    }

    /// For each data Raft group where this node is the leader, add any replicas
    /// from [`ClusterMeta`] that are missing from the Raft membership as learners.
    /// Learners receive full log replication so their state machines stay up-to-date,
    /// enabling reads after a failover even without a Raft leader election.
    ///
    /// Then, if every **live** (non-Offline) meta replica is already known to Raft, call
    /// [`Raft::change_membership`] so all of them become **voters**. That gives a 3-node shard
    /// a 2-of-3 quorum after one node is lost.
    async fn reconcile_data_group_membership(&self, meta: &ClusterMeta) {
        for (group_id, group_meta) in &meta.groups {
            let raft = match self.get_raft_group(*group_id) {
                Some(r) => r,
                None => continue,
            };
            let metrics = raft.metrics().borrow().clone();
            if metrics.current_leader != Some(self.node_id) {
                continue;
            }
            for replica_id in &group_meta.replicas {
                if *replica_id == self.node_id {
                    continue;
                }
                let already_known = metrics
                    .membership_config
                    .membership()
                    .get_node(replica_id)
                    .is_some();
                if already_known {
                    continue;
                }
                let addr = match self.peer_raft_grpc_addr(*replica_id) {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                if let Err(e) =
                    raft.add_learner(*replica_id, BasicNode { addr }, false).await
                {
                    tracing::warn!(
                        "reconcile: add learner {} to group {} failed: {}",
                        replica_id,
                        group_id,
                        e
                    );
                }
            }

            let latest_metrics = raft.metrics().borrow().clone();
            let membership = latest_metrics.membership_config.membership();
            let current_voters: BTreeSet<NodeId> = membership.voter_ids().collect();
            let known_nodes: BTreeSet<NodeId> = membership.nodes().map(|(id, _)| *id).collect();
            let desired_voters: BTreeSet<NodeId> = Self::raft_voter_goal_for_group(meta, group_meta);

            if desired_voters.is_empty() && !group_meta.replicas.is_empty() {
                tracing::warn!(
                    group_id = *group_id,
                    leader_id = self.node_id,
                    meta_replicas = ?group_meta.replicas,
                    "All group replicas are Offline in ClusterMeta; skip change_membership"
                );
                continue;
            }

            let ready_for_membership_change = !desired_voters.is_empty()
                && current_voters != desired_voters
                && desired_voters.is_subset(&known_nodes);
            if ready_for_membership_change {
                let change_timeout_ms = std::env::var("AIKV_DATA_GROUP_MEMBERSHIP_TIMEOUT_MS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(5000);
                tracing::info!(
                    group_id = *group_id,
                    leader_id = self.node_id,
                    current_voters = ?current_voters,
                    desired_voters = ?desired_voters,
                    "Reconciling data-group voters to live ClusterMeta replicas (retain old nodes as learners)"
                );
                match tokio::time::timeout(
                    std::time::Duration::from_millis(change_timeout_ms),
                    raft.change_membership(desired_voters.clone(), true),
                )
                .await
                {
                    Err(_) => {
                        tracing::warn!(
                            group_id = *group_id,
                            leader_id = self.node_id,
                            timeout_ms = change_timeout_ms,
                            "Timed out change_membership for data group"
                        );
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            group_id = *group_id,
                            leader_id = self.node_id,
                            error = ?e,
                            "change_membership failed for data group"
                        );
                    }
                    Ok(Ok(_)) => {
                        tracing::info!(
                            group_id = *group_id,
                            leader_id = self.node_id,
                            voters = ?desired_voters,
                            "Data-group voter membership reconciled"
                        );
                    }
                }
            } else if !desired_voters.is_empty() && current_voters != desired_voters {
                tracing::debug!(
                    group_id = *group_id,
                    leader_id = self.node_id,
                    current_voters = ?current_voters,
                    desired_voters = ?desired_voters,
                    known_nodes = ?known_nodes,
                    "Defer change_membership until all desired replicas are Raft learners"
                );
            }
        }
    }

    /// Async version: creates full Raft instances for any missing groups.
    /// Use from `async fn` paths (`put`, `delete`, `write_batch_for_route_key`).
    pub async fn ensure_data_groups_async(&self) -> Result<()> {
        let Some(meta_raft) = self.meta_raft.as_ref() else {
            return Ok(());
        };
        let live_v = meta_raft.get_config_version();
        if live_v == self.groups_synced_for_config_version.load(Ordering::Acquire) {
            return Ok(());
        }
        self.sync_data_groups_from_meta().await
    }

    /// Synchronous, **non-blocking** equivalent of [`Self::sync_data_groups_from_meta`].
    ///
    /// Safe to call from inside a tokio `block_on` (e.g. `ClusterRaftEngine::set_value`).
    /// Only creates **storage** for missing groups (pure sync); the full Raft instance will be
    /// created by the background watcher or next `sync_data_groups_from_meta` call.
    pub fn ensure_data_groups_for_current_meta(&self) -> Result<()> {
        let Some(meta_raft) = self.meta_raft.as_ref() else {
            return Ok(());
        };
        let live_v = meta_raft.get_config_version();
        if live_v == self.groups_synced_for_config_version.load(Ordering::Acquire) {
            return Ok(());
        }
        let meta = meta_raft.get_cluster_meta();
        for (group_id, group_meta) in &meta.groups {
            if group_meta.is_replica(self.node_id) && !self.storage.has_group(*group_id) {
                self.storage.create_group(*group_id)?;
            }
        }
        // IMPORTANT:
        // This sync path only ensures local storage exists. It does NOT create in-memory
        // Raft instances (`self.groups`) for missing groups.
        //
        // Therefore we must NOT advance `groups_synced_for_config_version` here, otherwise
        // async write paths may skip `sync_data_groups_from_meta()` and then fail with:
        // "No local storage for Raft group ...".
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

        self.ensure_data_groups_async().await?;
        router.sync_cache_from_meta_if_stale()?;
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
    /// Reads from the same on-disk state as Raft apply (`OpenRaftStorage`), not the optional
    /// `ShardedStateMachine` cache (which is a separate path).
    ///
    /// # Arguments
    ///
    /// * `key` - Key to retrieve
    ///
    /// # Returns
    ///
    /// The value if found, `None` otherwise
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not initialized".to_string()))?;

        self.ensure_data_groups_for_current_meta()?;
        router.sync_cache_from_meta_if_stale()?;
        let group_id = router.route(key)?;
        let group_storage = self.storage.get_group(group_id).ok_or_else(|| {
            Error::NotFound(format!("No local storage for Raft group {}", group_id))
        })?;

        group_storage.get_state_machine_value(key)
    }

    /// Read a key from the Raft group that **owns `slot`**, without deriving slot from `key` bytes.
    ///
    /// Expiration sidecar keys are stored under a `__exp__:` prefix; [`Self::get`] would route them
    /// by CRC16 of the full key and hit the wrong shard. Callers must pass the **data key's** slot.
    pub fn get_in_slot(&self, slot: u16, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not initialized".to_string()))?;

        self.ensure_data_groups_for_current_meta()?;
        router.sync_cache_from_meta_if_stale()?;
        let group_id = router.slot_to_group(slot)?;
        let group_storage = self.storage.get_group(group_id).ok_or_else(|| {
            Error::NotFound(format!("No local storage for Raft group {}", group_id))
        })?;

        group_storage.get_state_machine_value(key)
    }

    /// Read a raw state-machine key from a specific local Raft group, without slot routing.
    ///
    /// While a slot is `IMPORTING` on this node, [`Self::get`] still routes by `meta.slots`
    /// (current owner). Callers that must observe the target group's data (e.g. RESTORE
    /// `BUSYKEY` checks) use this to read only the local group that is receiving the slot.
    pub fn get_from_local_group(&self, group_id: GroupId, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.ensure_data_groups_for_current_meta()?;
        let group_storage = self.storage.get_group(group_id).ok_or_else(|| {
            Error::NotFound(format!("No local storage for Raft group {}", group_id))
        })?;
        group_storage.get_state_machine_value(key)
    }

    /// Scan this node's local Raft group DB for `sm:` keys that hash to `slot` (Redis `GETKEYSINSLOT`).
    pub fn scan_local_group_slot_keys_sync(&self, group_id: GroupId, slot: u16) -> Result<Vec<Vec<u8>>> {
        self.ensure_data_groups_for_current_meta()?;
        let group_storage = self.storage.get_group(group_id).ok_or_else(|| {
            Error::NotFound(format!("No local storage for Raft group {}", group_id))
        })?;
        group_storage.scan_state_machine_keys_in_slot(slot)
    }

    /// Scan all keys in this node's local Raft group DB (for SCAN command in cluster mode).
    ///
    /// Unlike [`Self::scan_local_group_slot_keys_sync`], this returns ALL keys in the group
    /// without slot filtering.
    pub fn scan_local_group_all_keys_sync(&self, group_id: GroupId) -> Result<Vec<Vec<u8>>> {
        self.ensure_data_groups_for_current_meta()?;
        let group_storage = self.storage.get_group(group_id).ok_or_else(|| {
            Error::NotFound(format!("No local storage for Raft group {}", group_id))
        })?;
        group_storage.scan_all_state_machine_keys()
    }

    /// Scan a single group with streaming and early termination.
    ///
    /// Returns up to `limit` keys and whether the group is exhausted.
    /// Uses seek-based positioning (RocksDB-style) for efficient cursor resumption.
    ///
    /// If `start_key` is provided, seeks to the first key strictly after it,
    /// enabling correct cursor-based resumption without scanning from the beginning.
    pub async fn scan_group_streaming(
        &self,
        group_id: GroupId,
        limit: usize,
        start_key: Option<&[u8]>,
    ) -> Result<GroupScanResult> {
        self.ensure_data_groups_for_current_meta()?;
        let group_storage = self.storage.get_group(group_id).ok_or_else(|| {
            Error::NotFound(format!("No local storage for Raft group {}", group_id))
        })?;

        let mut iter = group_storage.scan_state_machine_keys_streaming_limited(limit)?;

        // If resuming from a cursor, seek after the last-returned key
        if let Some(start) = start_key {
            iter.seek_after(start)?;
        }

        let mut keys = Vec::new();
        let mut last_key = None;

        for key in iter {
            keys.push(key.clone());
            last_key = Some(key);
            if keys.len() >= limit {
                break;
            }
        }

        // exhausted if we got fewer keys than requested (DB ran out)
        let exhausted = keys.len() < limit;

        Ok(GroupScanResult {
            keys,
            exhausted,
            last_key,
        })
    }

    /// Cross-group streaming scan with cursor support.
    ///
    /// cursor format: "group_idx:last_key_base64" or empty for start
    /// Returns: (next_cursor, keys)
    pub async fn scan_groups_streaming(
        &self,
        cursor: Option<&str>,
        count: usize,
    ) -> Result<(String, Vec<Vec<u8>>)> {
        let mut groups = self.list_groups();
        // Sort for deterministic ordering — the cursor encodes a group index,
        // so the order must be stable across calls.
        groups.sort();
        let (start_group_idx, resume_key) = Self::parse_scan_cursor(cursor);

        if count == 0 || start_group_idx >= groups.len() {
            return Ok((String::new(), Vec::new()));
        }

        let mut result_keys = Vec::new();
        let mut pending_cursor: Option<String> = None;

        for (group_idx, group_id) in groups.iter().enumerate() {
            if group_idx < start_group_idx {
                continue;
            }

            let remaining = count - result_keys.len();
            let resume = if group_idx == start_group_idx { resume_key.as_deref() } else { None };
            let group_result = self.scan_group_streaming(*group_id, remaining, resume).await?;

            result_keys.extend(group_result.keys.clone());

            if result_keys.len() >= count {
                if !group_result.exhausted {
                    // Continue from the same group, resume at last returned key.
                    if let Some(last_key) = group_result.last_key {
                        pending_cursor = Some(format!("{}:{}", group_idx, base64::encode(last_key)));
                    } else {
                        pending_cursor = Some(format!("{}:", group_idx));
                    }
                } else {
                    // Current group exhausted; continue from next group.
                    let next_group = group_idx + 1;
                    if next_group < groups.len() {
                        pending_cursor = Some(format!("{}:", next_group));
                    }
                }
                break;
            }
        }

        let next_cursor = if result_keys.len() < count {
            String::new()
        } else {
            pending_cursor.unwrap_or_default()
        };

        Ok((next_cursor, result_keys))
    }

    /// Parse scan cursor: (group_idx, resume_key)
    fn parse_scan_cursor(cursor: Option<&str>) -> (usize, Option<Vec<u8>>) {
        match cursor {
            None => (0, None),
            Some("") => (0, None),
            Some(c) => {
                let parts: Vec<&str> = c.splitn(2, ':').collect();
                let group_idx = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let resume_key = parts.get(1).and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        base64::decode(s).ok()
                    }
                });
                (group_idx, resume_key)
            }
        }
    }
    pub async fn write_batch_for_route_key(
        &self,
        route_key: &[u8],
        batch: crate::cluster::thin_replication::WriteBatch,
    ) -> Result<()> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not initialized".to_string()))?;

        self.ensure_data_groups_async().await?;
        router.sync_cache_from_meta_if_stale()?;
        let group_id = router.route(route_key)?;
        let raft = if let Some(r) = self.get_raft_group(group_id) {
            r
        } else {
            // Some startup/order races can route to a group before local in-memory
            // raft instances are fully materialized. Force a sync and retry once.
            log::warn!(
                "diag_event=db_write_batch_resync_retry group_id={} detail=local_raft_missing_before_sync",
                group_id
            );
            self.sync_data_groups_from_meta().await?;
            self.get_raft_group(group_id).ok_or_else(|| {
                log::error!(
                    "diag_event=db_write_batch_no_group_after_sync group_id={} detail=still_no_local_raft",
                    group_id
                );
                Error::NotFound(format!("No local storage for Raft group {}", group_id))
            })?
        };

        let request = Request::WriteBatch(batch);
        raft.client_write(request)
            .await
            .map_err(|e| Error::Internal(format!("Raft write batch failed: {:?}", e)))?;

        Ok(())
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

        self.ensure_data_groups_async().await?;
        router.sync_cache_from_meta_if_stale()?;
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

    /// Aggregate AiDb MemTable / WAL / block-cache stats for all local Raft group databases.
    ///
    /// Uses [`ShardedRaftStorage`] (the same DBs AiKv cluster writes to), not the optional
    /// [`ShardedStateMachine`] helper from [`Self::init_state_machine`].
    pub fn aggregate_aidb_storage_stats(&self) -> (u64, u64, u64, u64) {
        self.storage.aggregate_aidb_storage_stats()
    }

    /// Aggregate dbsize() across all local Raft groups for DBSIZE command.
    pub fn aggregate_dbsize(&self) -> usize {
        self.storage.aggregate_dbsize()
    }

    /// Reset key count to zero on all local Raft groups.
    ///
    /// This is used after flush operations to ensure accurate key counting.
    pub fn reset_all_key_counts(&self) {
        self.storage.reset_all_key_counts();
    }

    /// Clear all SSTable data in all groups.
    ///
    /// This should be called when flushing the database to ensure all SSTable data
    /// is deleted and not just the keys in the state machine.
    pub fn clear_all_data(&self) -> Result<()> {
        self.storage.clear_all_data()
    }

    /// Collect and update per-group metrics
    ///
    /// This method collects metrics from all active Raft groups and updates
    /// Prometheus metrics. Should be called periodically (e.g., every 5-10 seconds).
    ///
    /// # Returns
    ///
    /// Number of groups for which metrics were collected
    #[cfg(feature = "monitoring")]
    pub async fn collect_group_metrics(&self) -> Result<usize> {
        use crate::monitoring::MetricsCollector;

        let metrics = MetricsCollector::new();
        let group_ids = self.list_groups();
        let mut collected = 0;

        for group_id in group_ids {
            // Get Raft instance
            let raft = match self.get_raft_group(group_id) {
                Some(r) => r,
                None => continue,
            };

            // Get metrics from Raft
            let raft_metrics = raft.metrics().borrow().clone();

            // Update replication lag (committed - applied)
            let committed = raft_metrics.last_log_index.unwrap_or(0);
            let applied = raft_metrics.last_applied.map(|id| id.index).unwrap_or(0);
            let lag = committed.saturating_sub(applied);
            metrics.update_raft_group_replication_lag(group_id, lag);

            // Get log statistics
            if let Some(storage) = self.storage.get_group(group_id) {
                if let Ok((total_entries, total_bytes, _oldest, _newest)) = storage.get_log_stats()
                {
                    metrics.update_raft_group_log_size(group_id, total_entries);

                    // Store for potential size-based cleanup
                    if total_bytes > 0 {
                        // Log size is available for cleanup decisions
                        let _ = total_bytes; // Used in cleanup_group_logs
                    }
                }
            }

            collected += 1;
        }

        Ok(collected)
    }

    /// Cleanup logs for all groups based on cluster configuration
    ///
    /// This method runs log cleanup on all active Raft groups based on
    /// the retention policy specified in ClusterConfig.
    ///
    /// # Arguments
    ///
    /// * `cluster_config` - Cluster configuration with retention settings
    ///
    /// # Returns
    ///
    /// Total number of log entries purged across all groups
    pub async fn cleanup_all_group_logs(
        &self,
        cluster_config: &crate::config::ClusterConfig,
    ) -> Result<u64> {
        let group_ids = self.list_groups();
        let mut total_purged = 0u64;

        for group_id in group_ids {
            match self
                .cleanup_group_logs(
                    group_id,
                    cluster_config.max_log_entries,
                    cluster_config.max_log_size_bytes,
                )
                .await
            {
                Ok(purged) => {
                    total_purged += purged;
                    if purged > 0 {
                        #[cfg(feature = "monitoring")]
                        {
                            use crate::monitoring::MetricsCollector;
                            let metrics = MetricsCollector::new();
                            metrics.record_raft_group_log_compaction(group_id);
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "event=raft_cleanup_logs_failed group_id={} error={}",
                        group_id,
                        e
                    );
                }
            }
        }

        Ok(total_purged)
    }

    /// Cleanup logs for a specific group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    /// * `max_entries` - Maximum number of log entries to retain
    /// * `max_size_bytes` - Maximum log size in bytes
    ///
    /// # Returns
    ///
    /// Number of entries purged
    pub async fn cleanup_group_logs(
        &self,
        group_id: GroupId,
        max_entries: u64,
        max_size_bytes: u64,
    ) -> Result<u64> {
        let storage = self
            .storage
            .get_group(group_id)
            .ok_or_else(|| Error::Internal(format!("Group {} not found", group_id)))?;

        storage.cleanup_logs(max_entries, max_size_bytes)
    }

    /// Create snapshot for a specific group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Ok if snapshot was created successfully
    pub async fn create_group_snapshot(&self, group_id: GroupId) -> Result<()> {
        self.storage.create_group_snapshot(group_id).await?;

        #[cfg(feature = "monitoring")]
        {
            use crate::monitoring::MetricsCollector;
            let metrics = MetricsCollector::new();
            metrics.record_raft_group_snapshot(group_id);
        }

        Ok(())
    }

    /// Create snapshots for all groups
    ///
    /// # Returns
    ///
    /// Number of snapshots created
    pub async fn create_all_group_snapshots(&self) -> Result<usize> {
        let group_ids = self.list_groups();
        let mut created = 0;

        for group_id in group_ids {
            match self.create_group_snapshot(group_id).await {
                Ok(_) => created += 1,
                Err(e) => {
                    log::warn!(
                        "event=raft_create_snapshot_failed group_id={} error={}",
                        group_id,
                        e
                    );
                }
            }
        }

        Ok(created)
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

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();
        assert_eq!(node.node_id(), 1);
        assert_eq!(node.group_count(), 0);
    }

    #[tokio::test]
    async fn test_create_raft_group() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

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

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

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

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

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

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

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

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

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

        let node = MultiRaftNode::new(1, temp_dir.path(), config, None).await.unwrap();

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
            let node = MultiRaftNode::new(1, &storage_path, config, None).await.unwrap();
            node.create_raft_group(1, vec![1]).await.unwrap();
            node.create_raft_group(2, vec![1]).await.unwrap();
            node.create_raft_group(3, vec![1]).await.unwrap();
        }

        // Create new node instance and load existing groups
        let config = Config::default();
        let node = MultiRaftNode::new(1, &storage_path, config, None).await.unwrap();
        let loaded = node.load_existing_groups().await.unwrap();

        assert_eq!(loaded, 3);
        assert_eq!(node.group_count(), 3);
        assert!(node.has_group(1));
        assert!(node.has_group(2));
        assert!(node.has_group(3));
    }
}
