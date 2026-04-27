//! Multi-Raft Network layer for routing RPC requests to appropriate groups
//!
//! This module extends the single-group RaftNetwork to support multiple independent
//! Raft groups, routing requests based on group_id.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "raft-cluster")]
use openraft::RaftNetworkFactory;

use crate::cluster::raft_network::RaftNetworkClient;
use crate::cluster::raft_storage::{NodeId, TypeConfig};
use crate::cluster::sharded_storage::GroupId;
use crate::error::Error as AiDbError;

/// Multi-Raft network client that routes requests to specific groups
///
/// This client wraps the single-group RaftNetworkClient and adds group_id
/// routing capability for Multi-Raft architecture.
#[cfg(feature = "raft-cluster")]
pub struct MultiRaftNetworkClient {
    /// Inner single-group client (reserved for future use)
    #[allow(dead_code)]
    inner: RaftNetworkClient,
    /// Group ID for this client
    group_id: GroupId,
}

#[cfg(feature = "raft-cluster")]
impl MultiRaftNetworkClient {
    /// Create a new Multi-Raft network client
    pub fn new(group_id: GroupId, node_id: NodeId, target: NodeId, target_addr: String) -> Self {
        Self { inner: RaftNetworkClient::new(node_id, target, target_addr, group_id), group_id }
    }

    /// Get the group ID
    pub fn group_id(&self) -> GroupId {
        self.group_id
    }
}

// Note: RaftNetwork trait implementation commented out due to openraft 0.9 type complexity
// The factory and client structures are functional for creating network connections
// Full trait implementation will be completed in a future update
//
// #[cfg(feature = "raft-cluster")]
// impl RaftNetwork<TypeConfig> for MultiRaftNetworkClient {
//     async fn append_entries(...) { ... }
//     async fn install_snapshot(...) { ... }
//     async fn vote(...) { ... }
// }

/// Multi-Raft network factory that creates group-aware network clients
///
/// This factory maintains node address mappings and creates clients for
/// specific Raft groups, enabling Multi-Raft architecture.
#[cfg(feature = "raft-cluster")]
#[derive(Clone)]
pub struct MultiRaftNetworkFactory {
    /// Node ID of this node
    node_id: NodeId,
    /// Map of node ID to address
    node_addresses: Arc<RwLock<HashMap<NodeId, String>>>,
}

#[cfg(feature = "raft-cluster")]
impl MultiRaftNetworkFactory {
    /// Create a new Multi-Raft network factory
    ///
    /// # Arguments
    ///
    /// * `node_id` - ID of this node
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id, node_addresses: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Add or update a node's address
    ///
    /// # Arguments
    ///
    /// * `node_id` - Node ID
    /// * `addr` - Node address (e.g., "127.0.0.1:50051")
    pub fn add_node(&self, node_id: NodeId, addr: String) {
        let mut addresses = self.node_addresses.write();
        addresses.insert(node_id, addr);
    }

    /// Remove a node's address
    pub fn remove_node(&self, node_id: NodeId) {
        let mut addresses = self.node_addresses.write();
        addresses.remove(&node_id);
    }

    /// Get a node's address
    pub fn get_node_address(&self, node_id: NodeId) -> Option<String> {
        let addresses = self.node_addresses.read();
        addresses.get(&node_id).cloned()
    }

    /// Create a network client for a specific group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Raft group ID
    /// * `target` - Target node ID
    ///
    /// # Returns
    ///
    /// Network client for the specified group and target node
    ///
    /// Note: Currently returns RaftNetworkClient for compatibility.
    /// In a full Multi-Raft implementation, this would return a group-aware client.
    pub fn create_client(
        &self,
        group_id: GroupId,
        target: NodeId,
    ) -> Result<RaftNetworkClient, AiDbError> {
        let addresses = self.node_addresses.read();
        let target_addr = addresses
            .get(&target)
            .ok_or_else(|| AiDbError::Internal(format!("Node {} address not found", target)))?
            .clone();

        Ok(RaftNetworkClient::new(self.node_id, target, target_addr, group_id))
    }
}

#[cfg(feature = "raft-cluster")]
impl RaftNetworkFactory<TypeConfig> for MultiRaftNetworkFactory {
    type Network = RaftNetworkClient; // Use RaftNetworkClient instead

    async fn new_client(&mut self, target: NodeId, _node: &openraft::BasicNode) -> Self::Network {
        // For backward compatibility, create a simple RaftNetworkClient
        let addresses = self.node_addresses.read();
        let target_addr = addresses
            .get(&target)
            .cloned()
            .unwrap_or_else(|| format!("127.0.0.1:{}", 50051 + target));

        RaftNetworkClient::new(self.node_id, target, target_addr, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_factory() {
        let factory = MultiRaftNetworkFactory::new(1);
        assert_eq!(factory.node_id, 1);
    }

    #[test]
    fn test_add_and_get_node() {
        let factory = MultiRaftNetworkFactory::new(1);

        factory.add_node(2, "127.0.0.1:50052".to_string());
        factory.add_node(3, "127.0.0.1:50053".to_string());

        assert_eq!(factory.get_node_address(2), Some("127.0.0.1:50052".to_string()));
        assert_eq!(factory.get_node_address(3), Some("127.0.0.1:50053".to_string()));
        assert_eq!(factory.get_node_address(4), None);
    }

    #[test]
    fn test_remove_node() {
        let factory = MultiRaftNetworkFactory::new(1);

        factory.add_node(2, "127.0.0.1:50052".to_string());
        assert!(factory.get_node_address(2).is_some());

        factory.remove_node(2);
        assert!(factory.get_node_address(2).is_none());
    }

    #[test]
    fn test_create_client() {
        let factory = MultiRaftNetworkFactory::new(1);
        factory.add_node(2, "127.0.0.1:50052".to_string());

        let _client = factory.create_client(100, 2).unwrap();
        // Client created successfully
    }

    #[test]
    fn test_create_client_missing_node() {
        let factory = MultiRaftNetworkFactory::new(1);

        let result = factory.create_client(100, 999);
        assert!(result.is_err());
    }

    #[test]
    fn test_factory_clone() {
        let factory1 = MultiRaftNetworkFactory::new(1);
        factory1.add_node(2, "127.0.0.1:50052".to_string());

        let factory2 = factory1.clone();
        assert_eq!(factory2.get_node_address(2), Some("127.0.0.1:50052".to_string()));
    }
}
