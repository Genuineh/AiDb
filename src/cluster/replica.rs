//! Replica node implementation
//!
//! A Replica node provides a cache layer in front of a Primary node.
//! Cache hits are served directly, while cache misses are forwarded to the Primary.

use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::RwLock;
use tonic::transport::Channel;

use crate::cluster::rpc::proto::{self, storage_client::StorageClient};
use crate::error::{Error, Result};

/// LRU Cache entry with access tracking
#[derive(Debug, Clone)]
struct CacheEntry {
    value: Vec<u8>,
    access_count: u64,
}

/// Simple LRU cache implementation
pub struct LruCache {
    capacity: usize,
    cache: HashMap<Vec<u8>, CacheEntry>,
    access_list: Vec<Vec<u8>>, // Most recently used at the end
}

impl LruCache {
    /// Create a new LRU cache with given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cache: HashMap::new(),
            access_list: Vec::new(),
        }
    }

    /// Get a value from the cache
    pub fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(entry) = self.cache.get_mut(key) {
            entry.access_count += 1;
            // Move to end of access list (most recent)
            self.access_list.retain(|k| k != key);
            self.access_list.push(key.to_vec());
            Some(entry.value.clone())
        } else {
            None
        }
    }

    /// Put a value into the cache
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        // If key already exists, update it
        if self.cache.contains_key(&key) {
            self.cache.insert(key.clone(), CacheEntry {
                value,
                access_count: 1,
            });
            self.access_list.retain(|k| k != &key);
            self.access_list.push(key);
            return;
        }

        // If cache is full, evict least recently used
        if self.cache.len() >= self.capacity {
            if let Some(lru_key) = self.access_list.first().cloned() {
                self.cache.remove(&lru_key);
                self.access_list.remove(0);
            }
        }

        // Insert new entry
        self.cache.insert(key.clone(), CacheEntry {
            value,
            access_count: 1,
        });
        self.access_list.push(key);
    }

    /// Invalidate a key in the cache
    pub fn invalidate(&mut self, key: &[u8]) {
        self.cache.remove(key);
        self.access_list.retain(|k| k != key);
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_list.clear();
    }

    /// Get cache statistics
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Statistics for Replica node
#[derive(Debug, Default, Clone)]
pub struct ReplicaStats {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub forwarded_requests: u64,
    pub errors: u64,
}

impl ReplicaStats {
    pub fn hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.total_requests as f64
        }
    }
}

/// Replica node that caches data and forwards misses to Primary
pub struct ReplicaNode {
    cache: Arc<RwLock<LruCache>>,
    primary_client: StorageClient<Channel>,
    stats: Arc<RwLock<ReplicaStats>>,
}

impl ReplicaNode {
    /// Create a new Replica node
    pub async fn new(
        primary_addr: String,
        cache_capacity: usize,
    ) -> Result<Self> {
        let primary_client = StorageClient::connect(primary_addr)
            .await
            .map_err(|e| Error::Network(format!("Failed to connect to primary: {}", e)))?;

        Ok(Self {
            cache: Arc::new(RwLock::new(LruCache::new(cache_capacity))),
            primary_client,
            stats: Arc::new(RwLock::new(ReplicaStats::default())),
        })
    }

    /// Get a value, checking cache first and forwarding to Primary on miss
    pub async fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.stats.write().total_requests += 1;

        // Check cache first
        if let Some(value) = self.cache.write().get(key) {
            self.stats.write().cache_hits += 1;
            return Ok(Some(value));
        }

        // Cache miss - forward to primary
        self.stats.write().cache_misses += 1;
        self.stats.write().forwarded_requests += 1;

        let request = tonic::Request::new(proto::GetRequest {
            key: key.to_vec(),
        });

        match self.primary_client.get(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if resp.found {
                    // Cache the result
                    self.cache.write().put(key.to_vec(), resp.value.clone());
                    Ok(Some(resp.value))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                self.stats.write().errors += 1;
                Err(Error::Network(format!("RPC error: {}", e)))
            }
        }
    }

    /// Put a value - forwards to Primary and invalidates cache
    pub async fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.stats.write().total_requests += 1;
        self.stats.write().forwarded_requests += 1;

        // Invalidate cache entry
        self.cache.write().invalidate(key);

        let request = tonic::Request::new(proto::PutRequest {
            key: key.to_vec(),
            value: value.to_vec(),
        });

        match self.primary_client.put(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if resp.success {
                    Ok(())
                } else {
                    self.stats.write().errors += 1;
                    Err(Error::Network(format!("Put failed: {}", resp.error)))
                }
            }
            Err(e) => {
                self.stats.write().errors += 1;
                Err(Error::Network(format!("RPC error: {}", e)))
            }
        }
    }

    /// Delete a value - forwards to Primary and invalidates cache
    pub async fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.stats.write().total_requests += 1;
        self.stats.write().forwarded_requests += 1;

        // Invalidate cache entry
        self.cache.write().invalidate(key);

        let request = tonic::Request::new(proto::DeleteRequest {
            key: key.to_vec(),
        });

        match self.primary_client.delete(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if resp.success {
                    Ok(())
                } else {
                    self.stats.write().errors += 1;
                    Err(Error::Network(format!("Delete failed: {}", resp.error)))
                }
            }
            Err(e) => {
                self.stats.write().errors += 1;
                Err(Error::Network(format!("RPC error: {}", e)))
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> ReplicaStats {
        self.stats.read().clone()
    }

    /// Clear the cache
    pub fn clear_cache(&self) {
        self.cache.write().clear();
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.cache.read().len()
    }

    /// Warm up the cache with frequently accessed keys
    pub async fn warmup(&mut self, keys: Vec<Vec<u8>>) -> Result<usize> {
        let mut warmed = 0;
        
        for key in keys {
            match self.get(&key).await {
                Ok(Some(_)) => warmed += 1,
                Ok(None) => {},
                Err(_) => {},
            }
        }
        
        Ok(warmed)
    }

    /// Health check - forwards to Primary
    pub async fn health_check(&mut self) -> Result<bool> {
        let request = tonic::Request::new(proto::HealthCheckRequest {
            service: "storage".to_string(),
        });

        match self.primary_client.health_check(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                Ok(resp.status == proto::health_check_response::ServingStatus::Serving as i32)
            }
            Err(_) => Ok(false),
        }
    }
}
