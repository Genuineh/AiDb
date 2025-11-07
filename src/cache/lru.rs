//! LRU (Least Recently Used) cache implementation for block caching.
//!
//! This module provides a thread-safe LRU cache specifically designed for
//! caching SSTable data blocks.

use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique identifier for a cached block.
///
/// Combines the file path and block offset to uniquely identify a block
/// across all SSTables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// SSTable file number or path identifier
    pub file_id: u64,
    /// Block offset in the file
    pub offset: u64,
}

impl CacheKey {
    /// Create a new cache key
    pub fn new(file_id: u64, offset: u64) -> Self {
        Self { file_id, offset }
    }
}

/// Statistics for cache performance monitoring.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total number of cache lookups
    pub lookups: u64,
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of insertions
    pub insertions: u64,
    /// Number of evictions
    pub evictions: u64,
}

impl CacheStats {
    /// Calculate the cache hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            0.0
        } else {
            self.hits as f64 / self.lookups as f64
        }
    }

    /// Reset all statistics to zero
    pub fn reset(&mut self) {
        self.lookups = 0;
        self.hits = 0;
        self.misses = 0;
        self.insertions = 0;
        self.evictions = 0;
    }
}

/// Thread-safe LRU cache for SSTable blocks.
///
/// Uses a combination of HashMap for O(1) lookups and VecDeque for
/// maintaining LRU order.
///
/// # Thread Safety
///
/// This cache is thread-safe and can be shared across multiple threads
/// using `Arc<BlockCache>`.
#[derive(Debug)]
pub struct BlockCache {
    /// Maximum cache capacity in bytes
    capacity: usize,
    /// Current cache size in bytes
    current_size: AtomicU64,
    /// Cache entries stored by key
    cache: RwLock<HashMap<CacheKey, Bytes>>,
    /// LRU queue (most recently used at the back)
    lru_queue: RwLock<VecDeque<CacheKey>>,
    /// Cache statistics
    stats: RwLock<CacheStats>,
}

impl BlockCache {
    /// Create a new BlockCache with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum cache size in bytes. Set to 0 to disable caching.
    ///
    /// # Examples
    ///
    /// ```
    /// use aidb::cache::BlockCache;
    ///
    /// // Create a 8MB cache
    /// let cache = BlockCache::new(8 * 1024 * 1024);
    /// ```
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            current_size: AtomicU64::new(0),
            cache: RwLock::new(HashMap::new()),
            lru_queue: RwLock::new(VecDeque::new()),
            stats: RwLock::new(CacheStats::default()),
        }
    }

    /// Get a block from the cache.
    ///
    /// Returns `Some(block)` if the block is in the cache (cache hit),
    /// or `None` if not found (cache miss).
    ///
    /// This operation updates the LRU order, moving the accessed item
    /// to the most recently used position.
    pub fn get(&self, key: &CacheKey) -> Option<Bytes> {
        // Update lookup count
        {
            let mut stats = self.stats.write();
            stats.lookups += 1;
        }

        // Check if disabled
        if self.capacity == 0 {
            return None;
        }

        // Try to get from cache
        let cache = self.cache.read();
        if let Some(value) = cache.get(key) {
            // Cache hit - update LRU order
            let result = value.clone();
            drop(cache); // Release read lock before acquiring write lock

            // Move to end of LRU queue (most recently used)
            self.touch(key);

            // Update hit count
            {
                let mut stats = self.stats.write();
                stats.hits += 1;
            }

            Some(result)
        } else {
            // Cache miss
            drop(cache);
            {
                let mut stats = self.stats.write();
                stats.misses += 1;
            }
            None
        }
    }

    /// Insert a block into the cache.
    ///
    /// If the cache is at capacity, evicts the least recently used blocks
    /// to make room for the new entry.
    pub fn insert(&self, key: CacheKey, value: Bytes) {
        // Check if disabled
        if self.capacity == 0 {
            return;
        }

        let value_size = value.len();

        // Don't cache blocks larger than capacity
        if value_size > self.capacity {
            return;
        }

        // Evict until we have space
        while self.current_size.load(Ordering::Relaxed) as usize + value_size > self.capacity {
            self.evict_one();
        }

        // Insert into cache
        let mut cache = self.cache.write();
        let mut lru_queue = self.lru_queue.write();

        // Check if key already exists
        if let Some(old_value) = cache.get(&key) {
            // Update size
            let old_size = old_value.len();
            self.current_size.fetch_sub(old_size as u64, Ordering::Relaxed);

            // Remove old position in LRU queue
            lru_queue.retain(|k| k != &key);
        }

        // Insert new entry
        cache.insert(key.clone(), value);
        lru_queue.push_back(key);

        // Update size
        self.current_size.fetch_add(value_size as u64, Ordering::Relaxed);

        // Update stats
        drop(cache);
        drop(lru_queue);
        {
            let mut stats = self.stats.write();
            stats.insertions += 1;
        }
    }

    /// Touch a key to mark it as recently used.
    ///
    /// Moves the key to the end of the LRU queue without changing its value.
    ///
    /// # Performance Note
    ///
    /// This operation is O(n) due to linear search in VecDeque. For typical cache
    /// sizes (default 8MB ≈ 2000 blocks), this is acceptable. For very large caches
    /// (>10K entries), consider using a more efficient data structure.
    fn touch(&self, key: &CacheKey) {
        let mut lru_queue = self.lru_queue.write();

        // Find and remove the key from its current position
        if let Some(pos) = lru_queue.iter().position(|k| k == key) {
            lru_queue.remove(pos);
        }

        // Add to the end (most recently used)
        lru_queue.push_back(key.clone());
    }

    /// Evict the least recently used entry from the cache.
    fn evict_one(&self) {
        let mut lru_queue = self.lru_queue.write();

        if let Some(key) = lru_queue.pop_front() {
            drop(lru_queue); // Release lock before acquiring cache lock

            let mut cache = self.cache.write();
            if let Some(value) = cache.remove(&key) {
                // Update size
                let size = value.len();
                self.current_size.fetch_sub(size as u64, Ordering::Relaxed);

                // Update stats
                drop(cache);
                {
                    let mut stats = self.stats.write();
                    stats.evictions += 1;
                }
            }
        }
    }

    /// Get current cache statistics.
    pub fn stats(&self) -> CacheStats {
        self.stats.read().clone()
    }

    /// Reset cache statistics to zero.
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        stats.reset();
    }

    /// Clear all entries from the cache.
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        let mut lru_queue = self.lru_queue.write();

        cache.clear();
        lru_queue.clear();
        self.current_size.store(0, Ordering::Relaxed);
    }

    /// Get the current size of cached data in bytes.
    pub fn size(&self) -> usize {
        self.current_size.load(Ordering::Relaxed) as usize
    }

    /// Get the cache capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_cache_basic_operations() {
        let cache = BlockCache::new(1024);

        let key1 = CacheKey::new(1, 0);
        let value1 = Bytes::from(vec![1, 2, 3, 4]);

        // Initially empty
        assert_eq!(cache.get(&key1), None);

        // Insert and retrieve
        cache.insert(key1.clone(), value1.clone());
        assert_eq!(cache.get(&key1), Some(value1.clone()));

        // Stats should reflect operations
        let stats = cache.stats();
        assert_eq!(stats.lookups, 2); // 2 gets
        assert_eq!(stats.hits, 1); // 1 hit
        assert_eq!(stats.misses, 1); // 1 miss
        assert_eq!(stats.insertions, 1);
    }

    #[test]
    fn test_cache_lru_eviction() {
        // Small cache that holds ~3 entries of size 4
        let cache = BlockCache::new(12);

        let key1 = CacheKey::new(1, 0);
        let key2 = CacheKey::new(2, 0);
        let key3 = CacheKey::new(3, 0);
        let key4 = CacheKey::new(4, 0);

        let value = Bytes::from(vec![1, 2, 3, 4]);

        // Insert 3 entries (fills cache)
        cache.insert(key1.clone(), value.clone());
        cache.insert(key2.clone(), value.clone());
        cache.insert(key3.clone(), value.clone());

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.size(), 12);

        // Insert 4th entry should evict key1 (LRU)
        cache.insert(key4.clone(), value.clone());

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&key1), None); // Evicted
        assert_eq!(cache.get(&key2), Some(value.clone()));
        assert_eq!(cache.get(&key3), Some(value.clone()));
        assert_eq!(cache.get(&key4), Some(value.clone()));

        let stats = cache.stats();
        assert_eq!(stats.evictions, 1);
    }

    #[test]
    fn test_cache_touch_updates_lru() {
        // Small cache
        let cache = BlockCache::new(12);

        let key1 = CacheKey::new(1, 0);
        let key2 = CacheKey::new(2, 0);
        let key3 = CacheKey::new(3, 0);
        let key4 = CacheKey::new(4, 0);

        let value = Bytes::from(vec![1, 2, 3, 4]);

        // Insert 3 entries
        cache.insert(key1.clone(), value.clone());
        cache.insert(key2.clone(), value.clone());
        cache.insert(key3.clone(), value.clone());

        // Access key1 to make it most recently used
        assert_eq!(cache.get(&key1), Some(value.clone()));

        // Insert 4th entry should evict key2 (now LRU), not key1
        cache.insert(key4.clone(), value.clone());

        assert_eq!(cache.get(&key1), Some(value.clone())); // Still there
        assert_eq!(cache.get(&key2), None); // Evicted
        assert_eq!(cache.get(&key3), Some(value.clone()));
        assert_eq!(cache.get(&key4), Some(value.clone()));
    }

    #[test]
    fn test_cache_update_existing_key() {
        let cache = BlockCache::new(1024);

        let key = CacheKey::new(1, 0);
        let value1 = Bytes::from(vec![1, 2, 3, 4]);
        let value2 = Bytes::from(vec![5, 6, 7, 8, 9]);

        // Insert initial value
        cache.insert(key.clone(), value1.clone());
        assert_eq!(cache.get(&key), Some(value1));
        assert_eq!(cache.size(), 4);

        // Update with new value
        cache.insert(key.clone(), value2.clone());
        assert_eq!(cache.get(&key), Some(value2));
        assert_eq!(cache.size(), 5);
        assert_eq!(cache.len(), 1); // Still only 1 entry
    }

    #[test]
    fn test_cache_clear() {
        let cache = BlockCache::new(1024);

        let key1 = CacheKey::new(1, 0);
        let key2 = CacheKey::new(2, 0);
        let value = Bytes::from(vec![1, 2, 3, 4]);

        cache.insert(key1.clone(), value.clone());
        cache.insert(key2.clone(), value.clone());

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.size(), 8);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert_eq!(cache.size(), 0);
        assert_eq!(cache.get(&key1), None);
        assert_eq!(cache.get(&key2), None);
    }

    #[test]
    fn test_cache_disabled_when_capacity_zero() {
        let cache = BlockCache::new(0);

        let key = CacheKey::new(1, 0);
        let value = Bytes::from(vec![1, 2, 3, 4]);

        cache.insert(key.clone(), value.clone());

        // Should not cache when capacity is 0
        assert_eq!(cache.get(&key), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let cache = BlockCache::new(1024);

        let key1 = CacheKey::new(1, 0);
        let key2 = CacheKey::new(2, 0);
        let value = Bytes::from(vec![1, 2, 3, 4]);

        cache.insert(key1.clone(), value.clone());

        // 2 hits, 1 miss
        cache.get(&key1); // hit
        cache.get(&key1); // hit
        cache.get(&key2); // miss

        let stats = cache.stats();
        assert_eq!(stats.lookups, 3);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_cache_reset_stats() {
        let cache = BlockCache::new(1024);

        let key = CacheKey::new(1, 0);
        let value = Bytes::from(vec![1, 2, 3, 4]);

        cache.insert(key.clone(), value.clone());
        cache.get(&key);

        let stats = cache.stats();
        assert!(stats.lookups > 0);

        cache.reset_stats();

        let stats = cache.stats();
        assert_eq!(stats.lookups, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_cache_large_value_not_cached() {
        let cache = BlockCache::new(10); // Very small cache

        let key = CacheKey::new(1, 0);
        let value = Bytes::from(vec![0u8; 100]); // Larger than capacity

        cache.insert(key.clone(), value);

        // Should not be cached
        assert_eq!(cache.get(&key), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let cache = Arc::new(BlockCache::new(1024));
        let mut handles = vec![];

        // Spawn multiple threads
        for i in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                let key = CacheKey::new(i, 0);
                let value = Bytes::from(vec![i as u8; 10]);

                cache_clone.insert(key.clone(), value.clone());
                assert_eq!(cache_clone.get(&key), Some(value));
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // All entries should be present (1024 bytes capacity, 10 threads × 10 bytes each)
        assert_eq!(cache.len(), 10);
    }
}
