//! Coordinator for distributed cluster management
//!
//! The coordinator is responsible for:
//! - Routing requests to appropriate shards using consistent hashing
//! - Managing shard registration and discovery
//! - Load balancing across shards
//! - Health checking and failure detection

use super::consistent_hash::{ConsistentHashRing, ShardId};
use super::rpc::proto::{
    storage_client::StorageClient, GetRequest, GetResponse, PutRequest, PutResponse,
    DeleteRequest, DeleteResponse,
};
use crate::error::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::transport::Channel;

/// Information about a registered shard
#[derive(Debug, Clone)]
pub struct ShardInfo {
    /// Unique identifier for the shard
    pub id: ShardId,
    /// Network address (e.g., "127.0.0.1:50051")
    pub address: String,
    /// Whether the shard is currently healthy
    pub healthy: bool,
    /// Number of requests routed to this shard
    pub request_count: u64,
}

impl ShardInfo {
    /// Create a new shard info
    pub fn new(id: ShardId, address: String) -> Self {
        Self {
            id,
            address,
            healthy: true,
            request_count: 0,
        }
    }
}

/// Coordinator for managing distributed shards
pub struct Coordinator {
    /// Consistent hash ring for routing
    hash_ring: Arc<RwLock<ConsistentHashRing>>,
    /// Registered shards mapped by their ID
    shards: Arc<RwLock<HashMap<ShardId, ShardInfo>>>,
    /// Connection pool for shard clients
    clients: Arc<RwLock<HashMap<ShardId, StorageClient<Channel>>>>,
}

impl Coordinator {
    /// Create a new coordinator
    ///
    /// # Arguments
    /// * `virtual_nodes` - Number of virtual nodes per shard in the hash ring
    pub fn new(virtual_nodes: usize) -> Self {
        Self {
            hash_ring: Arc::new(RwLock::new(ConsistentHashRing::new(virtual_nodes))),
            shards: Arc::new(RwLock::new(HashMap::new())),
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new shard with the coordinator
    ///
    /// # Arguments
    /// * `shard_id` - Unique identifier for the shard
    /// * `address` - Network address of the shard (e.g., "http://127.0.0.1:50051")
    pub async fn register_shard(&self, shard_id: ShardId, address: String) -> Result<()> {
        // Add to hash ring
        {
            let mut ring = self.hash_ring.write();
            ring.add_node(shard_id.clone());
        }

        // Create shard info
        let shard_info = ShardInfo::new(shard_id.clone(), address.clone());
        
        // Store shard info
        {
            let mut shards = self.shards.write();
            shards.insert(shard_id.clone(), shard_info);
        }

        // Create client connection
        let client = StorageClient::connect(address.clone()).await
            .map_err(|e| crate::error::Error::ClusterError(format!("Failed to connect to shard: {}", e)))?;
        
        {
            let mut clients = self.clients.write();
            clients.insert(shard_id.clone(), client);
        }

        log::info!("Registered shard: {} at {}", shard_id, address);
        Ok(())
    }

    /// Unregister a shard from the coordinator
    ///
    /// # Arguments
    /// * `shard_id` - Identifier of the shard to remove
    pub fn unregister_shard(&self, shard_id: &str) {
        // Remove from hash ring
        {
            let mut ring = self.hash_ring.write();
            ring.remove_node(shard_id);
        }

        // Remove shard info
        {
            let mut shards = self.shards.write();
            shards.remove(shard_id);
        }

        // Remove client connection
        {
            let mut clients = self.clients.write();
            clients.remove(shard_id);
        }

        log::info!("Unregistered shard: {}", shard_id);
    }

    /// Get the shard responsible for a given key
    ///
    /// # Arguments
    /// * `key` - The key to route
    ///
    /// # Returns
    /// The shard ID responsible for this key, or None if no shards available
    pub fn route_key(&self, key: &[u8]) -> Option<ShardId> {
        let ring = self.hash_ring.read();
        ring.get_node(key)
    }

    /// Get a client for a specific shard
    fn get_client(&self, shard_id: &str) -> Option<StorageClient<Channel>> {
        let clients = self.clients.read();
        clients.get(shard_id).cloned()
    }

    /// Forward a GET request to the appropriate shard
    ///
    /// # Arguments
    /// * `key` - The key to get
    ///
    /// # Returns
    /// The response from the shard
    pub async fn get(&self, key: &[u8]) -> Result<GetResponse> {
        let shard_id = self.route_key(key)
            .ok_or_else(|| crate::error::Error::ClusterError("No shards available".to_string()))?;

        // Increment request count
        {
            let mut shards = self.shards.write();
            if let Some(shard_info) = shards.get_mut(&shard_id) {
                shard_info.request_count += 1;
            }
        }

        let mut client = self.get_client(&shard_id)
            .ok_or_else(|| crate::error::Error::ClusterError(format!("Client not found for shard: {}", shard_id)))?;

        let request = tonic::Request::new(GetRequest {
            key: key.to_vec(),
        });

        let response = client.get(request).await
            .map_err(|e| crate::error::Error::ClusterError(format!("RPC error: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Forward a PUT request to the appropriate shard
    ///
    /// # Arguments
    /// * `key` - The key to put
    /// * `value` - The value to store
    ///
    /// # Returns
    /// The response from the shard
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<PutResponse> {
        let shard_id = self.route_key(key)
            .ok_or_else(|| crate::error::Error::ClusterError("No shards available".to_string()))?;

        // Increment request count
        {
            let mut shards = self.shards.write();
            if let Some(shard_info) = shards.get_mut(&shard_id) {
                shard_info.request_count += 1;
            }
        }

        let mut client = self.get_client(&shard_id)
            .ok_or_else(|| crate::error::Error::ClusterError(format!("Client not found for shard: {}", shard_id)))?;

        let request = tonic::Request::new(PutRequest {
            key: key.to_vec(),
            value: value.to_vec(),
        });

        let response = client.put(request).await
            .map_err(|e| crate::error::Error::ClusterError(format!("RPC error: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Forward a DELETE request to the appropriate shard
    ///
    /// # Arguments
    /// * `key` - The key to delete
    ///
    /// # Returns
    /// The response from the shard
    pub async fn delete(&self, key: &[u8]) -> Result<DeleteResponse> {
        let shard_id = self.route_key(key)
            .ok_or_else(|| crate::error::Error::ClusterError("No shards available".to_string()))?;

        // Increment request count
        {
            let mut shards = self.shards.write();
            if let Some(shard_info) = shards.get_mut(&shard_id) {
                shard_info.request_count += 1;
            }
        }

        let mut client = self.get_client(&shard_id)
            .ok_or_else(|| crate::error::Error::ClusterError(format!("Client not found for shard: {}", shard_id)))?;

        let request = tonic::Request::new(DeleteRequest {
            key: key.to_vec(),
        });

        let response = client.delete(request).await
            .map_err(|e| crate::error::Error::ClusterError(format!("RPC error: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Get list of all registered shards
    pub fn list_shards(&self) -> Vec<ShardInfo> {
        let shards = self.shards.read();
        shards.values().cloned().collect()
    }

    /// Get statistics for a specific shard
    pub fn get_shard_stats(&self, shard_id: &str) -> Option<ShardInfo> {
        let shards = self.shards.read();
        shards.get(shard_id).cloned()
    }

    /// Mark a shard as unhealthy
    pub fn mark_unhealthy(&self, shard_id: &str) {
        let mut shards = self.shards.write();
        if let Some(shard_info) = shards.get_mut(shard_id) {
            shard_info.healthy = false;
            log::warn!("Marked shard {} as unhealthy", shard_id);
        }
    }

    /// Mark a shard as healthy
    pub fn mark_healthy(&self, shard_id: &str) {
        let mut shards = self.shards.write();
        if let Some(shard_info) = shards.get_mut(shard_id) {
            shard_info.healthy = true;
            log::info!("Marked shard {} as healthy", shard_id);
        }
    }

    /// Get the number of registered shards
    pub fn shard_count(&self) -> usize {
        let shards = self.shards.read();
        shards.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_creation() {
        let coordinator = Coordinator::new(150);
        assert_eq!(coordinator.shard_count(), 0);
    }

    #[test]
    fn test_route_key() {
        let coordinator = Coordinator::new(150);
        
        // No shards registered yet
        assert_eq!(coordinator.route_key(b"test_key"), None);
    }

    #[test]
    fn test_shard_registration_unregistration() {
        let coordinator = Coordinator::new(150);
        
        // Initially no shards
        assert_eq!(coordinator.shard_count(), 0);
        
        // Manually add to hash ring for testing (without async connect)
        {
            let mut ring = coordinator.hash_ring.write();
            ring.add_node("shard1".to_string());
        }
        {
            let mut shards = coordinator.shards.write();
            shards.insert("shard1".to_string(), ShardInfo::new("shard1".to_string(), "addr1".to_string()));
        }
        
        assert_eq!(coordinator.shard_count(), 1);
        
        // Should route to shard1
        let shard = coordinator.route_key(b"test_key");
        assert_eq!(shard, Some("shard1".to_string()));
        
        // Unregister
        coordinator.unregister_shard("shard1");
        assert_eq!(coordinator.shard_count(), 0);
        assert_eq!(coordinator.route_key(b"test_key"), None);
    }

    #[test]
    fn test_list_shards() {
        let coordinator = Coordinator::new(150);
        
        // Add some shards manually for testing
        {
            let mut shards = coordinator.shards.write();
            shards.insert("shard1".to_string(), ShardInfo::new("shard1".to_string(), "addr1".to_string()));
            shards.insert("shard2".to_string(), ShardInfo::new("shard2".to_string(), "addr2".to_string()));
        }
        
        let shard_list = coordinator.list_shards();
        assert_eq!(shard_list.len(), 2);
    }

    #[test]
    fn test_mark_healthy_unhealthy() {
        let coordinator = Coordinator::new(150);
        
        // Add shard
        {
            let mut shards = coordinator.shards.write();
            shards.insert("shard1".to_string(), ShardInfo::new("shard1".to_string(), "addr1".to_string()));
        }
        
        // Initially healthy
        let info = coordinator.get_shard_stats("shard1").unwrap();
        assert!(info.healthy);
        
        // Mark unhealthy
        coordinator.mark_unhealthy("shard1");
        let info = coordinator.get_shard_stats("shard1").unwrap();
        assert!(!info.healthy);
        
        // Mark healthy again
        coordinator.mark_healthy("shard1");
        let info = coordinator.get_shard_stats("shard1").unwrap();
        assert!(info.healthy);
    }

    #[test]
    fn test_routing_with_multiple_shards() {
        let coordinator = Coordinator::new(150);
        
        // Add multiple shards
        {
            let mut ring = coordinator.hash_ring.write();
            ring.add_node("shard1".to_string());
            ring.add_node("shard2".to_string());
            ring.add_node("shard3".to_string());
        }
        {
            let mut shards = coordinator.shards.write();
            shards.insert("shard1".to_string(), ShardInfo::new("shard1".to_string(), "addr1".to_string()));
            shards.insert("shard2".to_string(), ShardInfo::new("shard2".to_string(), "addr2".to_string()));
            shards.insert("shard3".to_string(), ShardInfo::new("shard3".to_string(), "addr3".to_string()));
        }
        
        // All keys should route to one of the shards
        let shard1 = coordinator.route_key(b"key1");
        let shard2 = coordinator.route_key(b"key2");
        let shard3 = coordinator.route_key(b"key3");
        
        assert!(shard1.is_some());
        assert!(shard2.is_some());
        assert!(shard3.is_some());
    }
}
