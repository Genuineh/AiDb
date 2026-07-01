//! Sharded LRU Block Cache with O(1) operations and hit/miss statistics.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
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
    let mut hasher = rustc_hash::FxHasher::default();
    key.hash(&mut hasher);
    (hasher.finish() as usize) & SHARD_MASK
}

struct ShardInner {
    /// value (Bytes) + access_counter. 计数器用于检测 lru_queue 中的 stale entry.
    cache: HashMap<CacheKey, (Bytes, u64)>,
    /// LRU 顺序: (key, 插入/访问时的 access_counter).
    /// 可能包含 stale entry (counter 不匹配), 在 eviction 时惰性跳过.
    lru_queue: VecDeque<(CacheKey, u64)>,
    current_size: usize,
    /// 单调递增计数器, 每次 get hit 或 insert 时递增.
    next_counter: u64,
}

impl ShardInner {
    /// Evict the LRU entry, skipping stale entries. Returns true if something was evicted.
    fn evict_one(&mut self, evictions: &AtomicU64, look_for: Option<&CacheKey>) -> bool {
        loop {
            let Some((key, counter)) = self.lru_queue.pop_front() else {
                return false;
            };
            // 跳过显式排除的 key (用于 insert 中覆盖旧 key 的场景)
            if let Some(exclude) = look_for {
                if &key == exclude {
                    continue;
                }
            }
            // counter 匹配才说明这是真正的 LRU entry
            if let Some((_, ref stored_counter)) = self.cache.get(&key) {
                if *stored_counter == counter {
                    if let Some((bytes, _)) = self.cache.remove(&key) {
                        self.current_size -= bytes.len();
                        evictions.fetch_add(1, Ordering::Relaxed);
                        tracing::debug!(target: "cache", evicted_count = 1, "cache.evict");
                        return true;
                    }
                }
            }
            // stale entry 或已删除: 跳过
        }
    }
}

struct CacheShard {
    inner: Mutex<ShardInner>,
}

/// Thread-safe sharded LRU cache for SSTable Data Block payloads.
/// 使用双计数器方案实现 O(1) 的 get/insert/eviction 操作.
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
                    next_counter: 0,
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
        if let Some((value, _)) = guard.cache.get_mut(&key) {
            let result = value.clone();
            // O(1) LRU touch: 递增 counter 并追加到队尾
            guard.next_counter += 1;
            let new_counter = guard.next_counter;
            // 更新 cache 中的 counter
            if let Some((_, ref mut stored_counter)) = guard.cache.get_mut(&key) {
                *stored_counter = new_counter;
            }
            guard.lru_queue.push_back((key.clone(), new_counter));

            // 惰性清理 stale entry: 从队头移除 counter 不匹配的过时条目.
            // 每个 get hit 最多清理一个, 均摊 O(1).
            while let Some((ref qkey, qcounter)) = guard.lru_queue.pop_front() {
                let stale = match guard.cache.get(qkey) {
                    Some((_, stored_counter)) => *stored_counter != qcounter,
                    None => true,
                };
                if !stale {
                    // 遇到活跃 entry, 放回队头并停止清理
                    guard.lru_queue.push_front((qkey.clone(), qcounter));
                    break;
                }
                // stale entry 已移除, 继续检查下一个
            }

            drop(guard);
            self.hits.fetch_add(1, Ordering::Relaxed);
            #[cfg(feature = "monitoring")]
            crate::metrics::record_block_cache_hit();
            tracing::Span::current().record("hit", true);
            tracing::debug!(target: "cache", "cache.hit");
            Some(result)
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

        // 覆盖已有 key: 先清理旧 LRU entry (惰性, evict_one 会跳过 stale)
        if let Some((old, _)) = guard.cache.remove(&key) {
            guard.current_size -= old.len();
        }

        guard.next_counter += 1;
        let counter = guard.next_counter;

        // Best-effort eviction loop
        let mut eviction_count = 0usize;
        while guard.current_size + value_len > self.capacity_per_shard
            && eviction_count < max_evictions
        {
            if guard.evict_one(&self.evictions, Some(&key)) {
                eviction_count += 1;
            } else {
                break;
            }
        }

        // Guaranteed eviction (locked)
        while guard.current_size + value_len > self.capacity_per_shard {
            if !guard.evict_one(&self.evictions, None) {
                break;
            }
        }

        let inserted = guard
            .cache
            .insert(key.clone(), (value, counter))
            .is_none();
        if inserted {
            guard.current_size += value_len;
        }
        // 无论之前是否存在, 追加新的 LRU entry
        guard.lru_queue.push_back((key, counter));
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
