//! Peer node implementation for decentralized cluster architecture
//!
//! A Peer node is an equal participant in the cluster that:
//! - Maintains full LSM-tree database with local storage
//! - Has optional LRU cache for frequently accessed data
//! - Participates in consistent hashing for data distribution
//! - Discovers and communicates with other peers
//! - Routes requests to appropriate peers
//! - No centralized coordinator needed

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use super::consistent_hash::ConsistentHashRing;
use super::replica::LruCache;
use super::rpc::{
    self, proto,
    proto::storage_client::StorageClient,
    storage_server::{Storage, StorageServer},
};
use crate::error::{Error, Result};
use crate::DB;

/// Information about a peer in the cluster
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Unique identifier for the peer
    pub id: String,
    /// Network address (e.g., "127.0.0.1:50051")
    pub address: String,
    /// Whether the peer is currently healthy
    pub healthy: bool,
    /// Number of requests routed to this peer
    pub request_count: u64,
}

impl PeerInfo {
    /// Create a new peer info
    pub fn new(id: String, address: String) -> Self {
        Self { id, address, healthy: true, request_count: 0 }
    }
}

/// Statistics for Peer node
#[derive(Debug, Default, Clone)]
pub struct PeerStats {
    /// Total number of local requests received
    pub local_requests: u64,
    /// Total number of forwarded requests
    pub forwarded_requests: u64,
    /// Number of GET requests
    pub get_requests: u64,
    /// Number of PUT requests
    pub put_requests: u64,
    /// Number of DELETE requests
    pub delete_requests: u64,
    /// Number of cache hits (for local cache)
    pub cache_hits: u64,
    /// Number of cache misses (for local cache)
    pub cache_misses: u64,
    /// Number of errors encountered
    pub errors: u64,
}

impl PeerStats {
    /// Calculate cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }
}

/// Peer node that participates in a decentralized cluster
pub struct PeerNode {
    /// Unique identifier for this peer
    id: String,
    /// Network address of this peer
    address: String,
    /// Local database instance
    db: Arc<DB>,
    /// Optional LRU cache for frequently accessed data
    cache: Arc<RwLock<Option<LruCache>>>,
    /// Consistent hash ring for routing (shared view of cluster topology)
    hash_ring: Arc<RwLock<ConsistentHashRing>>,
    /// Known peers in the cluster
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    /// Connection pool for peer clients
    clients: Arc<RwLock<HashMap<String, StorageClient<Channel>>>>,
    /// Statistics
    stats: Arc<RwLock<PeerStats>>,
}

impl PeerNode {
    /// Create a new peer node
    ///
    /// # Arguments
    /// * `id` - Unique identifier for this peer
    /// * `address` - Network address of this peer (e.g., "127.0.0.1:50051")
    /// * `db` - Local database instance
    /// * `cache_capacity` - Optional cache capacity (number of entries). If Some, enables caching
    /// * `virtual_nodes` - Number of virtual nodes per peer in the hash ring
    pub fn new(
        id: String,
        address: String,
        db: Arc<DB>,
        cache_capacity: Option<usize>,
        virtual_nodes: usize,
    ) -> Self {
        let cache = if let Some(capacity) = cache_capacity {
            Some(LruCache::new(capacity))
        } else {
            None
        };

        let mut hash_ring = ConsistentHashRing::new(virtual_nodes);
        // Add self to the hash ring
        hash_ring.add_node(id.clone());

        Self {
            id: id.clone(),
            address,
            db,
            cache: Arc::new(RwLock::new(cache)),
            hash_ring: Arc::new(RwLock::new(hash_ring)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            clients: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(PeerStats::default())),
        }
    }

    /// Get this peer's ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get this peer's address
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Get statistics
    pub fn stats(&self) -> PeerStats {
        self.stats.read().clone()
    }

    /// Join the cluster by registering a peer
    ///
    /// # Arguments
    /// * `peer_id` - Unique identifier for the peer
    /// * `peer_address` - Network address of the peer
    pub async fn join_peer(&self, peer_id: String, peer_address: String) -> Result<()> {
        // Add to hash ring
        {
            let mut ring = self.hash_ring.write();
            ring.add_node(peer_id.clone());
        }

        // Create peer info
        let peer_info = PeerInfo::new(peer_id.clone(), peer_address.clone());

        // Store peer info
        {
            let mut peers = self.peers.write();
            peers.insert(peer_id.clone(), peer_info);
        }

        // Create client connection
        let client = StorageClient::connect(peer_address.clone()).await.map_err(|e| {
            Error::ClusterError(format!("Failed to connect to peer: {}", e))
        })?;

        {
            let mut clients = self.clients.write();
            clients.insert(peer_id.clone(), client);
        }

        log::info!("Peer {} joined cluster: {} at {}", self.id, peer_id, peer_address);
        Ok(())
    }

    /// Remove a peer from the cluster
    ///
    /// # Arguments
    /// * `peer_id` - Identifier of the peer to remove
    pub fn leave_peer(&self, peer_id: &str) {
        // Remove from hash ring
        {
            let mut ring = self.hash_ring.write();
            ring.remove_node(peer_id);
        }

        // Remove peer info
        {
            let mut peers = self.peers.write();
            peers.remove(peer_id);
        }

        // Remove client connection
        {
            let mut clients = self.clients.write();
            clients.remove(peer_id);
        }

        log::info!("Peer {} left cluster: {}", self.id, peer_id);
    }

    /// Get the peer responsible for a given key
    ///
    /// # Arguments
    /// * `key` - The key to route
    ///
    /// # Returns
    /// The peer ID responsible for this key, or None if no peers available
    pub fn route_key(&self, key: &[u8]) -> Option<String> {
        let ring = self.hash_ring.read();
        ring.get_node(key)
    }

    /// Get a client for a specific peer
    fn get_client(&self, peer_id: &str) -> Option<StorageClient<Channel>> {
        let clients = self.clients.read();
        clients.get(peer_id).cloned()
    }

    /// Get list of all known peers (including self)
    pub fn list_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read();
        let mut result: Vec<PeerInfo> = peers.values().cloned().collect();
        
        // Add self to the list
        result.push(PeerInfo {
            id: self.id.clone(),
            address: self.address.clone(),
            healthy: true,
            request_count: 0, // Self stats are tracked separately
        });
        
        result
    }

    /// Mark a peer as unhealthy
    pub fn mark_unhealthy(&self, peer_id: &str) {
        let mut peers = self.peers.write();
        if let Some(peer_info) = peers.get_mut(peer_id) {
            peer_info.healthy = false;
            log::warn!("Marked peer {} as unhealthy", peer_id);
        }
    }

    /// Mark a peer as healthy
    pub fn mark_healthy(&self, peer_id: &str) {
        let mut peers = self.peers.write();
        if let Some(peer_info) = peers.get_mut(peer_id) {
            peer_info.healthy = true;
            log::info!("Marked peer {} as healthy", peer_id);
        }
    }

    /// Handle a GET request (either locally or forward to appropriate peer)
    async fn handle_get_request(&self, key: &[u8]) -> Result<proto::GetResponse> {
        // Check which peer is responsible for this key
        let responsible_peer = self
            .route_key(key)
            .ok_or_else(|| Error::ClusterError("No peers available".to_string()))?;

        // If this peer is responsible, handle locally
        if responsible_peer == self.id {
            self.handle_local_get(key)
        } else {
            // Forward to the responsible peer
            self.forward_get(&responsible_peer, key).await
        }
    }

    /// Handle a local GET request
    fn handle_local_get(&self, key: &[u8]) -> Result<proto::GetResponse> {
        let mut stats = self.stats.write();
        stats.local_requests += 1;
        stats.get_requests += 1;
        drop(stats);

        // Try cache first if enabled
        if let Some(ref mut cache) = *self.cache.write() {
            if let Some(value) = cache.get(key) {
                self.stats.write().cache_hits += 1;
                return Ok(proto::GetResponse { found: true, value });
            }
            self.stats.write().cache_misses += 1;
        }

        // Get from local DB
        match self.db.get(key) {
            Ok(Some(value)) => {
                // Update cache if enabled
                if let Some(ref mut cache) = *self.cache.write() {
                    cache.put(key.to_vec(), value.clone());
                }
                Ok(proto::GetResponse { found: true, value })
            }
            Ok(None) => Ok(proto::GetResponse { found: false, value: vec![] }),
            Err(e) => {
                self.stats.write().errors += 1;
                Err(e)
            }
        }
    }

    /// Forward a GET request to another peer
    async fn forward_get(&self, peer_id: &str, key: &[u8]) -> Result<proto::GetResponse> {
        self.stats.write().forwarded_requests += 1;

        let mut client = self.get_client(peer_id).ok_or_else(|| {
            Error::ClusterError(format!("Client not found for peer: {}", peer_id))
        })?;

        let request = tonic::Request::new(proto::GetRequest { key: key.to_vec() });

        let response = client
            .get(request)
            .await
            .map_err(|e| Error::ClusterError(format!("RPC error: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Handle a PUT request (either locally or forward to appropriate peer)
    async fn handle_put_request(&self, key: &[u8], value: &[u8]) -> Result<proto::PutResponse> {
        // Check which peer is responsible for this key
        let responsible_peer = self
            .route_key(key)
            .ok_or_else(|| Error::ClusterError("No peers available".to_string()))?;

        // If this peer is responsible, handle locally
        if responsible_peer == self.id {
            self.handle_local_put(key, value)
        } else {
            // Forward to the responsible peer
            self.forward_put(&responsible_peer, key, value).await
        }
    }

    /// Handle a local PUT request
    fn handle_local_put(&self, key: &[u8], value: &[u8]) -> Result<proto::PutResponse> {
        let mut stats = self.stats.write();
        stats.local_requests += 1;
        stats.put_requests += 1;
        drop(stats);

        match self.db.put(key, value) {
            Ok(()) => {
                // Update cache if enabled
                if let Some(ref mut cache) = *self.cache.write() {
                    cache.put(key.to_vec(), value.to_vec());
                }
                Ok(proto::PutResponse { success: true, error: String::new() })
            }
            Err(e) => {
                self.stats.write().errors += 1;
                Err(e)
            }
        }
    }

    /// Forward a PUT request to another peer
    async fn forward_put(
        &self,
        peer_id: &str,
        key: &[u8],
        value: &[u8],
    ) -> Result<proto::PutResponse> {
        self.stats.write().forwarded_requests += 1;

        let mut client = self.get_client(peer_id).ok_or_else(|| {
            Error::ClusterError(format!("Client not found for peer: {}", peer_id))
        })?;

        let request =
            tonic::Request::new(proto::PutRequest { key: key.to_vec(), value: value.to_vec() });

        let response = client
            .put(request)
            .await
            .map_err(|e| Error::ClusterError(format!("RPC error: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Handle a DELETE request (either locally or forward to appropriate peer)
    async fn handle_delete_request(&self, key: &[u8]) -> Result<proto::DeleteResponse> {
        // Check which peer is responsible for this key
        let responsible_peer = self
            .route_key(key)
            .ok_or_else(|| Error::ClusterError("No peers available".to_string()))?;

        // If this peer is responsible, handle locally
        if responsible_peer == self.id {
            self.handle_local_delete(key)
        } else {
            // Forward to the responsible peer
            self.forward_delete(&responsible_peer, key).await
        }
    }

    /// Handle a local DELETE request
    fn handle_local_delete(&self, key: &[u8]) -> Result<proto::DeleteResponse> {
        let mut stats = self.stats.write();
        stats.local_requests += 1;
        stats.delete_requests += 1;
        drop(stats);

        // Invalidate cache if enabled
        if let Some(ref mut cache) = *self.cache.write() {
            cache.invalidate(key);
        }

        match self.db.delete(key) {
            Ok(()) => Ok(proto::DeleteResponse { success: true, error: String::new() }),
            Err(e) => {
                self.stats.write().errors += 1;
                Err(e)
            }
        }
    }

    /// Forward a DELETE request to another peer
    async fn forward_delete(&self, peer_id: &str, key: &[u8]) -> Result<proto::DeleteResponse> {
        self.stats.write().forwarded_requests += 1;

        let mut client = self.get_client(peer_id).ok_or_else(|| {
            Error::ClusterError(format!("Client not found for peer: {}", peer_id))
        })?;

        let request = tonic::Request::new(proto::DeleteRequest { key: key.to_vec() });

        let response = client
            .delete(request)
            .await
            .map_err(|e| Error::ClusterError(format!("RPC error: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Create a gRPC server
    pub fn into_server(self) -> StorageServer<Self> {
        StorageServer::new(self)
    }

    /// Start the RPC server on the given address
    pub async fn serve(self, addr: std::net::SocketAddr) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let server = self.into_server();

        tonic::transport::Server::builder().add_service(server).serve(addr).await?;

        Ok(())
    }
}

#[tonic::async_trait]
impl Storage for PeerNode {
    async fn get(
        &self,
        request: Request<proto::GetRequest>,
    ) -> std::result::Result<Response<proto::GetResponse>, Status> {
        let req = request.into_inner();

        match self.handle_get_request(&req.key).await {
            Ok(response) => Ok(Response::new(response)),
            Err(e) => Err(rpc::to_status(e)),
        }
    }

    async fn put(
        &self,
        request: Request<proto::PutRequest>,
    ) -> std::result::Result<Response<proto::PutResponse>, Status> {
        let req = request.into_inner();

        match self.handle_put_request(&req.key, &req.value).await {
            Ok(response) => Ok(Response::new(response)),
            Err(e) => Err(rpc::to_status(e)),
        }
    }

    async fn delete(
        &self,
        request: Request<proto::DeleteRequest>,
    ) -> std::result::Result<Response<proto::DeleteResponse>, Status> {
        let req = request.into_inner();

        match self.handle_delete_request(&req.key).await {
            Ok(response) => Ok(Response::new(response)),
            Err(e) => Err(rpc::to_status(e)),
        }
    }

    async fn batch_get(
        &self,
        request: Request<proto::BatchGetRequest>,
    ) -> std::result::Result<Response<proto::BatchGetResponse>, Status> {
        let req = request.into_inner();
        let mut results = Vec::new();

        for key in req.keys {
            match self.handle_get_request(&key).await {
                Ok(response) => {
                    results.push(proto::KeyValue {
                        key: key.clone(),
                        found: response.found,
                        value: response.value,
                    });
                }
                Err(_) => {
                    results.push(proto::KeyValue {
                        key: key.clone(),
                        found: false,
                        value: vec![],
                    });
                }
            }
        }

        Ok(Response::new(proto::BatchGetResponse { results }))
    }

    async fn write(
        &self,
        request: Request<proto::WriteRequest>,
    ) -> std::result::Result<Response<proto::WriteResponse>, Status> {
        let req = request.into_inner();

        // Process all operations in the batch
        for op in req.operations {
            match proto::write_op::OpType::try_from(op.op_type) {
                Ok(proto::write_op::OpType::Put) => {
                    if let Err(e) = self.handle_put_request(&op.key, &op.value).await {
                        return Err(rpc::to_status(e));
                    }
                }
                Ok(proto::write_op::OpType::Delete) => {
                    if let Err(e) = self.handle_delete_request(&op.key).await {
                        return Err(rpc::to_status(e));
                    }
                }
                Err(_) => {
                    return Err(Status::invalid_argument("Invalid operation type"));
                }
            }
        }

        Ok(Response::new(proto::WriteResponse { success: true, error: String::new() }))
    }

    async fn scan(
        &self,
        _request: Request<proto::ScanRequest>,
    ) -> std::result::Result<Response<Self::ScanStream>, Status> {
        // TODO: Implement scan for peer-to-peer model
        // This would require coordinating with multiple peers
        Err(Status::unimplemented("Scan not yet implemented for peer-to-peer mode"))
    }

    type ScanStream = tokio_stream::wrappers::ReceiverStream<
        std::result::Result<proto::ScanResponse, Status>,
    >;

    async fn health_check(
        &self,
        _request: Request<proto::HealthCheckRequest>,
    ) -> std::result::Result<Response<proto::HealthCheckResponse>, Status> {
        Ok(Response::new(proto::HealthCheckResponse {
            status: proto::health_check_response::ServingStatus::Serving as i32,
        }))
    }

    async fn get_stats(
        &self,
        _request: Request<proto::GetStatsRequest>,
    ) -> std::result::Result<Response<proto::GetStatsResponse>, Status> {
        let stats = self.stats();
        
        // Get cache stats from DB if available
        let cache_stats = self.db.cache_stats();

        Ok(Response::new(proto::GetStatsResponse {
            total_keys: 0,    // Would need to add this to DB
            total_size: 0,    // Would need to add this to DB
            memtable_size: 0, // Would need to add this to DB
            num_sstables: 0,  // Would need to add this to DB
            cache_stats: Some(proto::CacheStats {
                hits: cache_stats.hits + stats.cache_hits,
                misses: cache_stats.misses + stats.cache_misses,
                total_requests: cache_stats.lookups + stats.cache_hits + stats.cache_misses,
                hit_rate: if stats.cache_hits + stats.cache_misses > 0 {
                    stats.hit_rate()
                } else {
                    cache_stats.hit_rate()
                },
            }),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, DB};
    use tempfile::TempDir;

    fn create_test_peer(
        id: &str,
        address: &str,
        cache_capacity: Option<usize>,
    ) -> (PeerNode, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let peer = PeerNode::new(
            id.to_string(),
            address.to_string(),
            Arc::new(db),
            cache_capacity,
            150,
        );
        (peer, temp_dir)
    }

    #[test]
    fn test_peer_creation() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", Some(100));
        assert_eq!(peer.id(), "peer1");
        assert_eq!(peer.address(), "127.0.0.1:50051");
    }

    #[test]
    fn test_peer_routing() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", None);

        // With only self in the ring, all keys should route to self
        let routed_peer = peer.route_key(b"test_key");
        assert_eq!(routed_peer, Some("peer1".to_string()));
    }

    #[test]
    fn test_local_get_put() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", None);

        // Put a value
        let put_result = peer.handle_local_put(b"key1", b"value1");
        assert!(put_result.is_ok());

        // Get the value
        let get_result = peer.handle_local_get(b"key1");
        assert!(get_result.is_ok());
        let response = get_result.unwrap();
        assert!(response.found);
        assert_eq!(response.value, b"value1");
    }

    #[test]
    fn test_local_delete() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", None);

        // Put and then delete
        peer.handle_local_put(b"key1", b"value1").unwrap();
        let delete_result = peer.handle_local_delete(b"key1");
        assert!(delete_result.is_ok());

        // Verify deletion
        let get_result = peer.handle_local_get(b"key1");
        assert!(get_result.is_ok());
        assert!(!get_result.unwrap().found);
    }

    #[test]
    fn test_cache_enabled() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", Some(100));

        // Put a value (this will also populate the cache)
        peer.handle_local_put(b"key1", b"value1").unwrap();

        // First get should hit cache (since put populated it)
        peer.handle_local_get(b"key1").unwrap();
        let stats = peer.stats();
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 0);

        // Get a non-existent key should miss cache
        peer.handle_local_get(b"key2").unwrap();
        let stats = peer.stats();
        assert_eq!(stats.cache_misses, 1);
        assert_eq!(stats.cache_hits, 1);
    }

    #[test]
    fn test_list_peers() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", None);

        // Initially only self
        let peers = peer.list_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].id, "peer1");
    }

    #[test]
    fn test_mark_peer_health() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", None);

        // Add a peer info manually for testing
        {
            let mut peers = peer.peers.write();
            peers.insert(
                "peer2".to_string(),
                PeerInfo::new("peer2".to_string(), "127.0.0.1:50052".to_string()),
            );
        }

        // Initially healthy
        let peers = peer.peers.read();
        assert!(peers.get("peer2").unwrap().healthy);
        drop(peers);

        // Mark unhealthy
        peer.mark_unhealthy("peer2");
        let peers = peer.peers.read();
        assert!(!peers.get("peer2").unwrap().healthy);
        drop(peers);

        // Mark healthy again
        peer.mark_healthy("peer2");
        let peers = peer.peers.read();
        assert!(peers.get("peer2").unwrap().healthy);
    }

    #[test]
    fn test_stats_tracking() {
        let (peer, _temp_dir) = create_test_peer("peer1", "127.0.0.1:50051", None);

        // Perform operations
        peer.handle_local_put(b"key1", b"value1").unwrap();
        peer.handle_local_get(b"key1").unwrap();
        peer.handle_local_delete(b"key1").unwrap();

        let stats = peer.stats();
        assert_eq!(stats.local_requests, 3);
        assert_eq!(stats.put_requests, 1);
        assert_eq!(stats.get_requests, 1);
        assert_eq!(stats.delete_requests, 1);
    }
}
