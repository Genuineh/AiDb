//! Router for sharded key routing in Multi-Raft clusters
//!
//! This module provides the routing logic that maps keys to Raft groups using
//! slot-based sharding. It maintains a local cache of cluster metadata and
//! watches for updates from MetaRaft.
//!
//! # Routing Logic
//!
//! 1. Compute slot from key: `slot = crc16(key) % 16384`
//! 2. Look up group from slot: `group_id = meta.slots[slot]`
//! 3. Get group replicas: `replicas = meta.groups[group_id].replicas`
//!
//! # Example
//!
//! ```
//! # use aidb::cluster::{Router, ClusterMeta};
//! let meta = ClusterMeta::new();
//! let router = Router::new(meta);
//! 
//! // Route a key to its group
//! let key = b"user:12345";
//! let group_id = router.route(key).unwrap();
//! 
//! // Get the slot for a key
//! let slot = Router::key_to_slot(key);
//! ```

use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::meta_raft_node::MetaRaftNode;
use super::meta_types::ClusterMeta;
use super::raft_storage::NodeId;
use super::sharded_storage::GroupId;
use crate::error::{Error, Result};

/// CRC-16/XMODEM polynomial (used in Redis Cluster)
const CRC16_XMODEM: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_XMODEM);

/// Number of slots in the cluster (Redis Cluster compatible)
pub const SLOT_COUNT: usize = 16384;

/// Router for mapping keys to Raft groups
///
/// The Router maintains a local cache of cluster metadata and provides
/// fast key-to-group lookups. It can optionally watch MetaRaft for
/// metadata updates to keep the cache fresh.
///
/// # Thread Safety
///
/// The Router is thread-safe and can be shared across multiple threads
/// using `Arc<Router>`.
pub struct Router {
    /// Local cache of cluster metadata
    meta_cache: Arc<RwLock<ClusterMeta>>,

    /// MetaRaft client for fetching updates (optional)
    meta_client: Option<Arc<MetaRaftNode>>,

    /// Cached configuration version for quick staleness checks
    cached_version: AtomicU64,
}

impl Router {
    /// Create a new Router with initial metadata
    ///
    /// # Arguments
    ///
    /// * `initial_meta` - Initial cluster metadata
    pub fn new(initial_meta: ClusterMeta) -> Self {
        let version = initial_meta.config_version;
        Self {
            meta_cache: Arc::new(RwLock::new(initial_meta)),
            meta_client: None,
            cached_version: AtomicU64::new(version),
        }
    }

    /// Create a new Router with MetaRaft client for automatic updates
    ///
    /// # Arguments
    ///
    /// * `initial_meta` - Initial cluster metadata
    /// * `meta_client` - MetaRaft client for fetching updates
    pub fn with_meta_client(initial_meta: ClusterMeta, meta_client: Arc<MetaRaftNode>) -> Self {
        let version = initial_meta.config_version;
        Self {
            meta_cache: Arc::new(RwLock::new(initial_meta)),
            meta_client: Some(meta_client),
            cached_version: AtomicU64::new(version),
        }
    }

    /// Compute the slot number for a key
    ///
    /// Uses CRC-16/XMODEM hash (compatible with Redis Cluster) and takes
    /// modulo 16384 to get the slot number.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to compute slot for
    ///
    /// # Returns
    ///
    /// Slot number in range [0, 16384)
    ///
    /// # Examples
    ///
    /// ```
    /// use aidb::cluster::router::Router;
    ///
    /// let slot = Router::key_to_slot(b"user:12345");
    /// assert!(slot < 16384);
    /// ```
    pub fn key_to_slot(key: &[u8]) -> u16 {
        let hash = CRC16_XMODEM.checksum(key);
        (hash % SLOT_COUNT as u16) as u16
    }

    /// Look up the group ID for a given slot
    ///
    /// # Arguments
    ///
    /// * `slot` - Slot number
    ///
    /// # Returns
    ///
    /// Group ID that owns this slot
    pub fn slot_to_group(&self, slot: u16) -> Result<GroupId> {
        if slot >= SLOT_COUNT as u16 {
            return Err(Error::InvalidArgument(format!("Slot {} out of range", slot)));
        }

        let meta = self.meta_cache.read();
        Ok(meta.slot_to_group(slot))
    }

    /// Route a key to its owning group
    ///
    /// This is the primary routing method. It computes the slot from the key
    /// and looks up which group owns that slot.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to route
    ///
    /// # Returns
    ///
    /// Group ID that should handle this key
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use aidb::cluster::Router;
    /// # fn example(router: &Router) -> Result<(), Box<dyn std::error::Error>> {
    /// let group_id = router.route(b"user:12345")?;
    /// println!("Key routed to group {}", group_id);
    /// # Ok(())
    /// # }
    /// ```
    pub fn route(&self, key: &[u8]) -> Result<GroupId> {
        let slot = Self::key_to_slot(key);
        self.slot_to_group(slot)
    }

    /// Route a key to the list of nodes that own its group
    ///
    /// Returns the replica nodes for the group that owns this key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to route
    ///
    /// # Returns
    ///
    /// Vector of node IDs that are replicas of the owning group
    ///
    /// # Errors
    ///
    /// Returns an error if the group metadata is not found
    pub fn route_to_nodes(&self, key: &[u8]) -> Result<Vec<NodeId>> {
        let group_id = self.route(key)?;
        let meta = self.meta_cache.read();

        meta.groups
            .get(&group_id)
            .map(|group| group.replicas.clone())
            .ok_or_else(|| Error::NotFound(format!("Group {} metadata not found", group_id)))
    }

    /// Get the current cluster metadata
    ///
    /// Returns a clone of the cached metadata.
    pub fn get_metadata(&self) -> ClusterMeta {
        self.meta_cache.read().clone()
    }

    /// Get the cached configuration version
    ///
    /// This is a fast atomic read that doesn't require locking.
    pub fn get_version(&self) -> u64 {
        self.cached_version.load(Ordering::Acquire)
    }

    /// Update the cached metadata
    ///
    /// This should be called when new metadata is received from MetaRaft.
    ///
    /// # Arguments
    ///
    /// * `new_meta` - New cluster metadata
    ///
    /// # Returns
    ///
    /// `true` if the metadata was updated (version is newer), `false` otherwise
    pub fn update_metadata(&self, new_meta: ClusterMeta) -> bool {
        let mut cache = self.meta_cache.write();
        if new_meta.config_version > cache.config_version {
            *cache = new_meta;
            self.cached_version.store(cache.config_version, Ordering::Release);
            true
        } else {
            false
        }
    }

    /// Fetch latest metadata from MetaRaft and update cache
    ///
    /// This requires that the Router was created with a MetaRaft client.
    ///
    /// # Returns
    ///
    /// `Ok(true)` if metadata was updated, `Ok(false)` if already up-to-date
    ///
    /// # Errors
    ///
    /// Returns an error if MetaRaft client is not configured or fetch fails
    pub fn refresh_metadata(&self) -> Result<bool> {
        let meta_client = self
            .meta_client
            .as_ref()
            .ok_or_else(|| Error::Internal("MetaRaft client not configured".to_string()))?;

        let new_meta = meta_client.get_cluster_meta();
        Ok(self.update_metadata(new_meta))
    }

    /// Watch MetaRaft for metadata changes and automatically update cache
    ///
    /// This spawns a background task that periodically polls MetaRaft for
    /// updates. The task runs until the Router is dropped.
    ///
    /// # Arguments
    ///
    /// * `poll_interval_ms` - How often to check for updates (milliseconds)
    ///
    /// # Returns
    ///
    /// A join handle for the background task
    ///
    /// # Errors
    ///
    /// Returns an error if MetaRaft client is not configured
    pub async fn start_watching(
        self: Arc<Self>,
        poll_interval_ms: u64,
    ) -> Result<tokio::task::JoinHandle<()>> {
        if self.meta_client.is_none() {
            return Err(Error::Internal("MetaRaft client not configured".to_string()));
        }

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(poll_interval_ms));
            loop {
                interval.tick().await;

                match self.refresh_metadata() {
                    Ok(updated) => {
                        if updated {
                            tracing::debug!(
                                "Router metadata updated to version {}",
                                self.get_version()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to refresh router metadata: {}", e);
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Get the leader node for a group (if known)
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group to query
    ///
    /// # Returns
    ///
    /// Leader node ID if known, `None` otherwise
    pub fn get_group_leader(&self, group_id: GroupId) -> Option<NodeId> {
        let meta = self.meta_cache.read();
        meta.groups.get(&group_id).and_then(|g| g.leader)
    }

    /// Get all replicas for a group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group to query
    ///
    /// # Returns
    ///
    /// Vector of replica node IDs
    pub fn get_group_replicas(&self, group_id: GroupId) -> Vec<NodeId> {
        let meta = self.meta_cache.read();
        meta.groups.get(&group_id).map(|g| g.replicas.clone()).unwrap_or_default()
    }

    /// Get information about a node
    ///
    /// # Arguments
    ///
    /// * `node_id` - Node to query
    ///
    /// # Returns
    ///
    /// Node address if found
    pub fn get_node_address(&self, node_id: NodeId) -> Option<String> {
        let meta = self.meta_cache.read();
        meta.nodes.get(&node_id).map(|n| n.addr.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::meta_types::{GroupMeta, NodeInfo};

    #[test]
    fn test_key_to_slot() {
        // Test that slot is in valid range
        let slot1 = Router::key_to_slot(b"user:12345");
        assert!(slot1 < SLOT_COUNT as u16);

        // Test that same key produces same slot
        let slot2 = Router::key_to_slot(b"user:12345");
        assert_eq!(slot1, slot2);

        // Test that different keys produce different slots (usually)
        let slot3 = Router::key_to_slot(b"user:67890");
        // Note: Can't guarantee different slots due to hash collisions,
        // but we can check it's in range
        assert!(slot3 < SLOT_COUNT as u16);
    }

    #[test]
    fn test_slot_distribution() {
        // Test that slots are reasonably distributed
        use std::collections::HashMap;
        let mut slot_counts = HashMap::new();

        // Generate 1000 keys and count slot distribution
        for i in 0..1000 {
            let key = format!("key:{}", i);
            let slot = Router::key_to_slot(key.as_bytes());
            *slot_counts.entry(slot / 100).or_insert(0) += 1;
        }

        // We should have keys in multiple buckets
        assert!(slot_counts.len() > 10);
    }

    #[test]
    fn test_router_basic() {
        let mut meta = ClusterMeta::new();
        meta.config_version = 1;

        // Set up simple mapping: slots 0-99 -> group 1, 100-199 -> group 2
        for slot in 0..100 {
            meta.slots[slot] = 1;
        }
        for slot in 100..200 {
            meta.slots[slot] = 2;
        }

        let router = Router::new(meta);

        // Test slot lookup
        assert_eq!(router.slot_to_group(50).unwrap(), 1);
        assert_eq!(router.slot_to_group(150).unwrap(), 2);

        // Test version
        assert_eq!(router.get_version(), 1);
    }

    #[test]
    fn test_router_with_groups() {
        let mut meta = ClusterMeta::with_uniform_distribution(4);
        meta.groups.insert(0, GroupMeta::new(0, vec![1, 2, 3]));
        meta.groups.insert(1, GroupMeta::new(1, vec![1, 2, 3]));
        meta.groups.insert(2, GroupMeta::new(2, vec![1, 2, 3]));
        meta.groups.insert(3, GroupMeta::new(3, vec![1, 2, 3]));

        let router = Router::new(meta);

        // Find a key that maps to group 0
        let mut found = false;
        for i in 0..1000 {
            let key = format!("key:{}", i);
            if let Ok(group_id) = router.route(key.as_bytes()) {
                if group_id == 0 {
                    // Verify we can get nodes for this group
                    let nodes = router.route_to_nodes(key.as_bytes()).unwrap();
                    assert_eq!(nodes, vec![1, 2, 3]);
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "Should find at least one key mapping to group 0");
    }

    #[test]
    fn test_router_update_metadata() {
        let mut meta1 = ClusterMeta::new();
        meta1.config_version = 1;
        let router = Router::new(meta1);

        assert_eq!(router.get_version(), 1);

        // Update with newer version
        let mut meta2 = ClusterMeta::new();
        meta2.config_version = 2;
        assert!(router.update_metadata(meta2));
        assert_eq!(router.get_version(), 2);

        // Try to update with older version
        let mut meta3 = ClusterMeta::new();
        meta3.config_version = 1;
        assert!(!router.update_metadata(meta3));
        assert_eq!(router.get_version(), 2);
    }

    #[test]
    fn test_router_get_group_info() {
        let mut meta = ClusterMeta::new();
        let mut group = GroupMeta::new(1, vec![1, 2, 3]);
        group.set_leader(1);
        meta.groups.insert(1, group);

        let router = Router::new(meta);

        // Test get leader
        assert_eq!(router.get_group_leader(1), Some(1));
        assert_eq!(router.get_group_leader(999), None);

        // Test get replicas
        assert_eq!(router.get_group_replicas(1), vec![1, 2, 3]);
        assert_eq!(router.get_group_replicas(999), Vec::<NodeId>::new());
    }

    #[test]
    fn test_router_get_node_address() {
        let mut meta = ClusterMeta::new();
        meta.nodes.insert(1, NodeInfo::new(1, "127.0.0.1:50051".to_string()));

        let router = Router::new(meta);

        assert_eq!(router.get_node_address(1), Some("127.0.0.1:50051".to_string()));
        assert_eq!(router.get_node_address(999), None);
    }

    #[test]
    fn test_slot_out_of_range() {
        let meta = ClusterMeta::new();
        let router = Router::new(meta);

        assert!(router.slot_to_group(16384).is_err());
        assert!(router.slot_to_group(20000).is_err());
    }
}
