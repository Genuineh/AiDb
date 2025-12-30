//! OpenRaft-based Raft node implementation for AiDb
//!
//! This module provides a Raft node implementation using the openraft library.
//! It manages the Raft consensus protocol and provides APIs for proposing changes
//! and reading data with strong consistency guarantees.

use parking_lot::RwLock;
use std::sync::Arc;

#[cfg(feature = "raft-cluster")]
use openraft::{storage::Adaptor, Config, Raft, RaftMetrics};

use crate::cluster::raft_network::RaftNetworkClientFactory;
use crate::cluster::raft_storage::{NodeId, OpenRaftStorage, Request, Response, TypeConfig};
use crate::error::{Error, Result};
use crate::DB;

/// Configuration for the Raft node
#[derive(Debug, Clone)]
pub struct RaftNodeConfig {
    /// Node ID
    pub node_id: NodeId,
    /// Minimum election timeout in milliseconds
    pub election_timeout_min: u64,
    /// Maximum election timeout in milliseconds
    pub election_timeout_max: u64,
    /// Heartbeat interval in milliseconds
    pub heartbeat_interval: u64,
    /// Maximum number of log entries per append
    pub max_payload_entries: u64,
    /// Snapshot threshold (number of logs)
    pub snapshot_logs_since_last: u64,
}

impl Default for RaftNodeConfig {
    fn default() -> Self {
        Self {
            node_id: 1,
            election_timeout_min: 150,
            election_timeout_max: 300,
            heartbeat_interval: 50,
            max_payload_entries: 300,
            snapshot_logs_since_last: 1000,
        }
    }
}

/// Raft node using openraft
#[cfg(feature = "raft-cluster")]
pub struct OpenRaftNode {
    /// Node ID
    node_id: NodeId,
    /// Raft instance
    raft: Arc<Raft<TypeConfig>>,
    /// Network factory
    network_factory: Arc<RwLock<RaftNetworkClientFactory>>,
}

#[cfg(feature = "raft-cluster")]
impl OpenRaftNode {
    /// Create a new Raft node
    pub async fn new(
        config: RaftNodeConfig,
        db: Arc<DB>,
        network_factory: RaftNetworkClientFactory,
    ) -> Result<Self> {
        // Create storage
        let storage = OpenRaftStorage::new(db)?;

        // Wrap storage in Adaptor for compatibility with openraft 0.9
        let (log_store, state_machine) = Adaptor::new(storage);

        // Create openraft config
        let raft_config = Config {
            cluster_name: "aidb-cluster".to_string(),
            election_timeout_min: config.election_timeout_min,
            election_timeout_max: config.election_timeout_max,
            heartbeat_interval: config.heartbeat_interval,
            max_payload_entries: config.max_payload_entries,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(
                config.snapshot_logs_since_last,
            ),
            ..Default::default()
        };

        // validate() takes ownership, so we call it and get back the validated config
        let raft_config = raft_config
            .validate()
            .map_err(|e| Error::ClusterError(format!("Invalid Raft config: {}", e)))?;

        // Store network factory in Arc for shared use
        let network_factory_arc = Arc::new(RwLock::new(network_factory));

        // Clone the factory to share the underlying Arc<RwLock<HashMap>> of node addresses
        // This ensures both the Raft instance and stored factory reference the same node map
        let network_factory_for_raft = network_factory_arc.read().clone();

        let raft = Raft::new(
            config.node_id,
            Arc::new(raft_config),
            network_factory_for_raft,
            log_store,
            state_machine,
        )
        .await
        .map_err(|e| Error::ClusterError(format!("Failed to create Raft: {}", e)))?;

        Ok(Self {
            node_id: config.node_id,
            raft: Arc::new(raft),
            network_factory: network_factory_arc,
        })
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Initialize the cluster (should be called on the first node only)
    pub async fn initialize(&self, nodes: Vec<(NodeId, String)>) -> Result<()> {
        let mut members = std::collections::BTreeMap::new();

        for (node_id, addr) in nodes {
            // Store the address in BasicNode so openraft's RaftNetworkFactory can use it
            members.insert(node_id, openraft::BasicNode { addr: addr.clone() });
            self.network_factory.write().add_node(node_id, addr);
        }

        self.raft
            .initialize(members)
            .await
            .map_err(|e| Error::ClusterError(format!("Failed to initialize cluster: {}", e)))?;

        Ok(())
    }

    /// Add a learner node to the cluster
    pub async fn add_learner(&self, node_id: NodeId, address: String) -> Result<()> {
        self.network_factory.write().add_node(node_id, address.clone());

        self.raft
            .add_learner(node_id, openraft::BasicNode { addr: address }, true)
            .await
            .map_err(|e| Error::ClusterError(format!("Failed to add learner: {}", e)))?;

        Ok(())
    }

    /// Change membership (promote learner to voter or remove node)
    pub async fn change_membership(&self, members: Vec<NodeId>) -> Result<()> {
        let members_set: std::collections::BTreeSet<NodeId> = members.into_iter().collect();

        self.raft
            .change_membership(members_set, false)
            .await
            .map_err(|e| Error::ClusterError(format!("Failed to change membership: {}", e)))?;

        Ok(())
    }

    /// Propose a write operation
    pub async fn propose(&self, request: Request) -> Result<Response> {
        // In openraft 0.9, client_write takes the app_data directly
        let response = self
            .raft
            .client_write(request)
            .await
            .map_err(|e| Error::ClusterError(format!("Failed to propose: {}", e)))?;

        Ok(response.data)
    }

    /// Write a batch of operations (Thin Replication)
    ///
    /// This method writes a batch of operations to the Raft log. In thin replication,
    /// only these WAL entries are replicated, not the full SSTable files. This reduces
    /// replication cost by 90%+ while maintaining strong consistency.
    ///
    /// # Arguments
    ///
    /// * `batch` - The batch of write operations to replicate
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok if successful, Error otherwise
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::cluster::{OpenRaftNode, ThinWriteBatch};
    ///
    /// # async fn example(node: OpenRaftNode) -> Result<(), Box<dyn std::error::Error>> {
    /// let mut batch = ThinWriteBatch::new();
    /// batch.put(b"key1".to_vec(), b"value1".to_vec());
    /// batch.put(b"key2".to_vec(), b"value2".to_vec());
    /// batch.delete(b"key3".to_vec());
    ///
    /// node.write_batch(batch).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write_batch(
        &self,
        batch: crate::cluster::thin_replication::WriteBatch,
    ) -> Result<()> {
        let request = Request::WriteBatch(batch);
        let response = self.propose(request).await?;

        match response {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(Error::ClusterError(e)),
            _ => Err(Error::ClusterError("Unexpected response".to_string())),
        }
    }

    /// Put a key-value pair (internally uses WriteBatch)
    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        // Use WriteBatch for consistency with thin replication
        let mut batch = crate::cluster::thin_replication::WriteBatch::new();
        batch.put(key, value);
        self.write_batch(batch).await
    }

    /// Delete a key (internally uses WriteBatch)
    pub async fn delete(&self, key: Vec<u8>) -> Result<()> {
        // Use WriteBatch for consistency with thin replication
        let mut batch = crate::cluster::thin_replication::WriteBatch::new();
        batch.delete(key);
        self.write_batch(batch).await
    }

    /// Read with linearizable consistency (must go through Raft)
    pub async fn linearizable_read(&self, _key: Vec<u8>) -> Result<Option<Vec<u8>>> {
        // For linearizable reads in openraft, we need to ensure we're the leader
        // and that our state is up to date

        let metrics = self.raft.metrics().borrow().clone();

        if metrics.current_leader != Some(self.node_id) {
            return Err(Error::ClusterError("Not the leader".to_string()));
        }

        // For now, return unimplemented - linearizable reads require additional
        // implementation to access the state machine safely
        Err(Error::ClusterError(
            "Linearizable read not fully implemented - use local read after ensuring leader"
                .to_string(),
        ))
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader == Some(self.node_id)
    }

    /// Get current leader ID
    pub async fn get_leader(&self) -> Option<NodeId> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader
    }

    /// Get metrics
    pub async fn metrics(&self) -> RaftMetrics<NodeId, openraft::BasicNode> {
        self.raft.metrics().borrow().clone()
    }

    /// Get the Raft instance (for server setup)
    pub fn raft(&self) -> Arc<openraft::Raft<TypeConfig>> {
        self.raft.clone()
    }

    /// Add a known node address to the network factory without changing membership.
    /// This is used to pre-populate peer addresses from configuration so the node can
    /// contact other nodes for elections and replication.
    pub fn add_node_address(&self, node_id: NodeId, address: String) {
        self.network_factory.write().add_node(node_id, address);
    }

    /// Remove a known node address from the network factory.
    pub fn remove_node_address(&self, node_id: NodeId) {
        self.network_factory.write().remove_node(node_id);
    }

    /// Return current node addresses known to the network factory
    pub async fn node_addresses(&self) -> Vec<(NodeId, String)> {
        // Access the underlying RaftNetworkClientFactory's nodes map
        let factory = self.network_factory.read();
        factory.list_nodes()
    }

    /// Start RPC server on the given address
    ///
    /// This starts a gRPC server that listens for Raft protocol messages
    /// from other nodes in the cluster.
    ///
    /// # Arguments
    ///
    /// * `addr` - The socket address to bind to (e.g., "127.0.0.1:50001")
    ///
    /// # Returns
    ///
    /// Returns a future that will run the server until it's shut down.
    /// The future should be spawned as a background task.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::cluster::{OpenRaftNode, RaftNodeConfig, RaftNetworkClientFactory};
    /// # use aidb::{DB, Options};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let db = DB::open("./data", Options::default())?;
    /// # let network_factory = RaftNetworkClientFactory::new(1);
    /// # let config = RaftNodeConfig { node_id: 1, ..Default::default() };
    /// let node = OpenRaftNode::new(config, Arc::new(db), network_factory).await?;
    ///
    /// // Start server in background
    /// let addr = "127.0.0.1:50001".parse()?;
    /// tokio::spawn(async move {
    ///     node.start_server(addr).await
    /// });
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start_server(&self, addr: std::net::SocketAddr) -> Result<()> {
        use crate::cluster::raft_network::raft_rpc::raft_service_server::RaftServiceServer;
        use crate::cluster::raft_network::RaftServiceImpl;

        let service = RaftServiceImpl::new(self.raft.clone());
        let server = RaftServiceServer::new(service);

        tonic::transport::Server::builder()
            .add_service(server)
            .serve(addr)
            .await
            .map_err(|e| Error::ClusterError(format!("Server error: {}", e)))?;

        Ok(())
    }

    /// Shutdown the node
    pub async fn shutdown(&self) -> Result<()> {
        self.raft
            .shutdown()
            .await
            .map_err(|e| Error::ClusterError(format!("Failed to shutdown: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_raft_node_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let network_factory = RaftNetworkClientFactory::new(1);

        let config = RaftNodeConfig { node_id: 1, ..Default::default() };

        let node = OpenRaftNode::new(config, Arc::new(db), network_factory).await;
        assert!(node.is_ok());
    }
}
