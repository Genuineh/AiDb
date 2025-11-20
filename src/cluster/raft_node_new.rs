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

        // Store network factory for later use
        let network_factory_clone = RaftNetworkClientFactory::new(config.node_id);
        let network_factory_arc = Arc::new(RwLock::new(network_factory));

        // Create Raft instance - pass network factory directly (not wrapped in Arc)
        let raft = Raft::new(
            config.node_id,
            Arc::new(raft_config),
            network_factory_clone,
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
            members.insert(node_id, openraft::BasicNode::default());
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
        self.network_factory.write().add_node(node_id, address);

        self.raft
            .add_learner(node_id, openraft::BasicNode::default(), true)
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

    /// Put a key-value pair
    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let request = Request::Put { key, value };
        let response = self.propose(request).await?;

        match response {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(Error::ClusterError(e)),
            _ => Err(Error::ClusterError("Unexpected response".to_string())),
        }
    }

    /// Delete a key
    pub async fn delete(&self, key: Vec<u8>) -> Result<()> {
        let request = Request::Delete { key };
        let response = self.propose(request).await?;

        match response {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(Error::ClusterError(e)),
            _ => Err(Error::ClusterError("Unexpected response".to_string())),
        }
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
