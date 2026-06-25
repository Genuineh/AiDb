//! Sharded LRU Block Cache with hit/miss statistics.

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use parking_lot::Mutex;

const NUM_SHARDS: usize = 16;
const SHARD_MASK: usize = NUM_SHARDS - 1;
const MIN_MAX_EVICTIONS: usize = 16;
const ASSUMED_BLOCK_SIZE: usize = 8 * 1024;

/// Cache lookup key: SST file number + Data Block file offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub file_number: u64,
    pub offset: u64,
}

/// Snapshot of cache statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
    pub insertions: u64,
    pub evictions: u64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            0.0
        } else {
            self.hits as f64 / self.lookups as f64
        }
    }
}

/// Compute shard index for a given cache key.
pub fn shard_index(key: &CacheKey) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) & SHARD_MASK
}

struct ShardInner {
    cache: HashMap<CacheKey, Bytes>,
    lru_queue: VecDeque<CacheKey>,
    current_size: usize,
}

impl ShardInner {
    /// Evict the LRU entry. Returns true if something was evicted.
    fn evict_one(&mut self, evictions: &AtomicU64) -> bool {
        let Some(victim) = self.lru_queue.pop_front() else {
            return false;
        };
        if let Some(bytes) = self.cache.remove(&victim) {
            self.current_size -= bytes.len();
            evictions.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(target: "cache", evicted_count = 1, "cache.evict");
            let _span = tracing::trace_span!("cache_evict", evicted_count = 1).entered();
            true
        } else {
            false
        }
    }
}

struct CacheShard {
    inner: Mutex<ShardInner>,
}

/// Thread-safe sharded LRU cache for SSTable Data Block payloads.
pub struct BlockCache {
    shards: Box<[CacheShard; NUM_SHARDS]>,
    /// Per-shard capacity (total capacity divided by NUM_SHARDS; each shard has its
    /// own independent budget).
    capacity_per_shard: usize,
    lookups: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    insertions: AtomicU64,
    evictions: AtomicU64,
}

impl BlockCache {
    pub fn new(capacity: usize) -> Self {
        let capacity_per_shard = if capacity == 0 {
            0
        } else {
            (capacity / NUM_SHARDS).max(1)
        };
        let cache = Self {
            shards: Box::new([(); NUM_SHARDS].map(|_| CacheShard {
                inner: Mutex::new(ShardInner {
                    cache: HashMap::new(),
                    lru_queue: VecDeque::new(),
                    current_size: 0,
                }),
            })),
            capacity_per_shard,
            lookups: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            insertions: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        };
        #[cfg(feature = "monitoring")]
        crate::metrics::set_block_cache_capacity(cache.capacity() as u64);
        cache
    }

    #[tracing::instrument(
    name = "cache_get",
    skip(self),
    fields(file_number = key.file_number, offset = key.offset, hit)
  )]
    pub fn get(&self, key: CacheKey) -> Option<Bytes> {
        self.lookups.fetch_add(1, Ordering::Relaxed);
        if self.capacity_per_shard == 0 {
            self.misses.fetch_add(1, Ordering::Relaxed);
            #[cfg(feature = "monitoring")]
            crate::metrics::record_block_cache_miss();
            tracing::Span::current().record("hit", false);
            tracing::debug!(target: "cache", "cache.miss");
            return None;
        }

        let idx = shard_index(&key);
        let mut guard = self.shards[idx].inner.lock();
        let value = guard.cache.get(&key).cloned();

        if let Some(bytes) = value {
            // LRU touch: remove and push back
            guard.lru_queue.retain(|k| k != &key);
            guard.lru_queue.push_back(key);
            drop(guard);
            self.hits.fetch_add(1, Ordering::Relaxed);
            #[cfg(feature = "monitoring")]
            crate::metrics::record_block_cache_hit();
            tracing::Span::current().record("hit", true);
            tracing::debug!(target: "cache", "cache.hit");
            Some(bytes)
        } else {
            drop(guard);
            self.misses.fetch_add(1, Ordering::Relaxed);
            #[cfg(feature = "monitoring")]
            crate::metrics::record_block_cache_miss();
            tracing::Span::current().record("hit", false);
            tracing::debug!(target: "cache", "cache.miss");
            None
        }
    }

    #[tracing::instrument(
    name = "cache_insert",
    skip(self, value),
    fields(file_number = key.file_number, offset = key.offset, bytes = value.len())
  )]
    pub fn insert(&self, key: CacheKey, value: Bytes) {
        if self.capacity_per_shard == 0 {
            return;
        }
        let value_len = value.len();
        if value_len > self.capacity_per_shard {
            return;
        }

        let max_evictions = (self.capacity_per_shard / ASSUMED_BLOCK_SIZE).max(MIN_MAX_EVICTIONS);
        let idx = shard_index(&key);
        let mut guard = self.shards[idx].inner.lock();

        // Best-effort eviction loop
        let mut eviction_count = 0usize;
        while guard.current_size + value_len > self.capacity_per_shard
            && eviction_count < max_evictions
        {
            if guard.evict_one(&self.evictions) {
                eviction_count += 1;
            } else {
                break;
            }
        }

        // Guaranteed eviction (locked)
        while guard.current_size + value_len > self.capacity_per_shard {
            if !guard.evict_one(&self.evictions) {
                break;
            }
        }

        if let Some(old) = guard.cache.insert(key.clone(), value) {
            guard.current_size -= old.len();
            guard.lru_queue.retain(|k| k != &key);
        }
        guard.lru_queue.push_back(key);
        guard.current_size += value_len;
        self.insertions.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(target: "cache", "cache.insert");
        drop(guard);
        #[cfg(feature = "monitoring")]
        crate::metrics::set_block_cache_size(self.size());
    }

    pub fn clear(&self) {
        for shard in self.shards.iter() {
            let mut guard = shard.inner.lock();
            guard.cache.clear();
            guard.lru_queue.clear();
            guard.current_size = 0;
        }
        #[cfg(feature = "monitoring")]
        crate::metrics::set_block_cache_size(0);
    }

    pub fn len(&self) -> usize {
        let mut total = 0usize;
        for shard in self.shards.iter() {
            let guard = shard.inner.lock();
            total += guard.cache.len();
        }
        total
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn size(&self) -> u64 {
        let mut total = 0usize;
        for shard in self.shards.iter() {
            let guard = shard.inner.lock();
            total += guard.current_size;
        }
        total as u64
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            lookups: self.lookups.load(Ordering::Relaxed),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            insertions: self.insertions.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }

    pub fn reset_stats(&self) {
        self.lookups.store(0, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.insertions.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }

    pub fn capacity(&self) -> usize {
        self.capacity_per_shard * NUM_SHARDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lib_cache_basic() {
        let cache = BlockCache::new(256);
        let k = CacheKey {
            file_number: 1,
            offset: 0,
        };
        cache.insert(k.clone(), Bytes::from_static(b"hello"));
        assert_eq!(cache.get(k).unwrap(), Bytes::from_static(b"hello"));
    }
}
