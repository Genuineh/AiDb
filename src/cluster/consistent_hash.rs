//! Consistent hashing implementation for distributed key routing
//!
//! This module implements a consistent hash ring with virtual nodes for
//! distributed load balancing across multiple shards.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

/// Unique identifier for a shard
pub type ShardId = String;

/// Consistent hash ring for routing keys to shards
pub struct ConsistentHashRing {
    /// Hash ring mapping hash values to shard IDs
    ring: BTreeMap<u64, ShardId>,
    /// Number of virtual nodes per physical shard
    virtual_nodes: usize,
}

impl ConsistentHashRing {
    /// Create a new consistent hash ring
    ///
    /// # Arguments
    /// * `virtual_nodes` - Number of virtual nodes per physical shard (default: 150)
    pub fn new(virtual_nodes: usize) -> Self {
        Self {
            ring: BTreeMap::new(),
            virtual_nodes: if virtual_nodes == 0 {
                150
            } else {
                virtual_nodes
            },
        }
    }

    /// Add a shard to the ring
    ///
    /// # Arguments
    /// * `shard_id` - Unique identifier for the shard
    pub fn add_node(&mut self, shard_id: ShardId) {
        for i in 0..self.virtual_nodes {
            let virtual_key = format!("{}:{}", shard_id, i);
            let hash = self.hash(&virtual_key);
            self.ring.insert(hash, shard_id.clone());
        }
    }

    /// Remove a shard from the ring
    ///
    /// # Arguments
    /// * `shard_id` - Identifier of the shard to remove
    pub fn remove_node(&mut self, shard_id: &str) {
        let mut keys_to_remove = Vec::new();

        for (hash, id) in &self.ring {
            if id == shard_id {
                keys_to_remove.push(*hash);
            }
        }

        for key in keys_to_remove {
            self.ring.remove(&key);
        }
    }

    /// Get the shard responsible for a given key
    ///
    /// # Arguments
    /// * `key` - The key to route
    ///
    /// # Returns
    /// * `Some(ShardId)` - The shard responsible for this key
    /// * `None` - If no shards are available
    pub fn get_node(&self, key: &[u8]) -> Option<ShardId> {
        if self.ring.is_empty() {
            return None;
        }

        let hash = self.hash(key);

        // Find the first node with hash >= key hash (clockwise on ring)
        if let Some((_node_hash, shard_id)) = self.ring.range(hash..).next() {
            return Some(shard_id.clone());
        }

        // Wrap around to the first node
        self.ring.values().next().cloned()
    }

    /// Get all shards in the ring
    pub fn get_all_nodes(&self) -> Vec<ShardId> {
        let mut nodes = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for shard_id in self.ring.values() {
            if seen.insert(shard_id.clone()) {
                nodes.push(shard_id.clone());
            }
        }

        nodes
    }

    /// Get the number of physical nodes in the ring
    pub fn node_count(&self) -> usize {
        self.get_all_nodes().len()
    }

    /// Hash a key using DefaultHasher
    fn hash<T: Hash + ?Sized>(&self, key: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Get distribution statistics for testing
    pub fn get_distribution(&self, test_keys: &[Vec<u8>]) -> BTreeMap<ShardId, usize> {
        let mut distribution = BTreeMap::new();

        for key in test_keys {
            if let Some(shard_id) = self.get_node(key) {
                *distribution.entry(shard_id).or_insert(0) += 1;
            }
        }

        distribution
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consistent_hash_ring_creation() {
        let ring = ConsistentHashRing::new(150);
        assert_eq!(ring.virtual_nodes, 150);
        assert_eq!(ring.node_count(), 0);
    }

    #[test]
    fn test_add_and_get_node() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());

        let shard = ring.get_node(b"test_key");
        assert_eq!(shard, Some("shard1".to_string()));
    }

    #[test]
    fn test_add_multiple_nodes() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());
        ring.add_node("shard2".to_string());
        ring.add_node("shard3".to_string());

        assert_eq!(ring.node_count(), 3);

        // All keys should map to one of the shards
        let shard = ring.get_node(b"test_key");
        assert!(shard.is_some());
        let shard_id = shard.unwrap();
        assert!(shard_id == "shard1" || shard_id == "shard2" || shard_id == "shard3");
    }

    #[test]
    fn test_remove_node() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());
        ring.add_node("shard2".to_string());
        ring.add_node("shard3".to_string());

        assert_eq!(ring.node_count(), 3);

        ring.remove_node("shard2");
        assert_eq!(ring.node_count(), 2);

        // Keys should now only map to shard1 or shard3
        let shard = ring.get_node(b"test_key");
        assert!(shard.is_some());
        let shard_id = shard.unwrap();
        assert!(shard_id == "shard1" || shard_id == "shard3");
        assert_ne!(shard_id, "shard2");
    }

    #[test]
    fn test_empty_ring() {
        let ring = ConsistentHashRing::new(150);
        assert_eq!(ring.get_node(b"test_key"), None);
    }

    #[test]
    fn test_get_all_nodes() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());
        ring.add_node("shard2".to_string());
        ring.add_node("shard3".to_string());

        let mut nodes = ring.get_all_nodes();
        nodes.sort();

        assert_eq!(nodes, vec!["shard1", "shard2", "shard3"]);
    }

    #[test]
    fn test_distribution_balance() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());
        ring.add_node("shard2".to_string());
        ring.add_node("shard3".to_string());

        // Generate test keys
        let mut test_keys = Vec::new();
        for i in 0..1000 {
            test_keys.push(format!("key_{}", i).into_bytes());
        }

        let distribution = ring.get_distribution(&test_keys);

        // Check that all shards got some keys
        assert_eq!(distribution.len(), 3);

        // Check rough balance (each shard should get ~333 keys, allowing 20% variance)
        let expected = 1000 / 3;
        for (_, count) in distribution {
            let variance = (count as i32 - expected).abs() as f64 / expected as f64;
            assert!(variance < 0.5, "Distribution variance too high: {}", variance);
        }
    }

    #[test]
    fn test_consistent_mapping() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());
        ring.add_node("shard2".to_string());

        // Same key should always map to same shard
        let key = b"consistent_key";
        let shard1 = ring.get_node(key);
        let shard2 = ring.get_node(key);
        let shard3 = ring.get_node(key);

        assert_eq!(shard1, shard2);
        assert_eq!(shard2, shard3);
    }

    #[test]
    fn test_minimal_redistribution() {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node("shard1".to_string());
        ring.add_node("shard2".to_string());
        ring.add_node("shard3".to_string());

        // Generate test keys and record their mappings
        let mut test_keys = Vec::new();
        for i in 0..1000 {
            test_keys.push(format!("key_{}", i).into_bytes());
        }

        let mut original_mapping = Vec::new();
        for key in &test_keys {
            original_mapping.push(ring.get_node(key));
        }

        // Remove one shard
        ring.remove_node("shard2");

        // Count how many keys changed their mapping
        let mut changed = 0;
        for (i, key) in test_keys.iter().enumerate() {
            let new_mapping = ring.get_node(key);
            if original_mapping[i] != new_mapping {
                changed += 1;
            }
        }

        // Only keys that were on shard2 should have moved
        // That should be roughly 1/3 of keys (allowing some variance)
        let change_rate = changed as f64 / test_keys.len() as f64;
        assert!(
            (0.2..=0.5).contains(&change_rate),
            "Change rate {} is outside expected range",
            change_rate
        );
    }

    #[test]
    fn test_virtual_nodes_configuration() {
        let ring1 = ConsistentHashRing::new(100);
        assert_eq!(ring1.virtual_nodes, 100);

        let ring2 = ConsistentHashRing::new(0); // Should default to 150
        assert_eq!(ring2.virtual_nodes, 150);

        let ring3 = ConsistentHashRing::new(300);
        assert_eq!(ring3.virtual_nodes, 300);
    }
}
