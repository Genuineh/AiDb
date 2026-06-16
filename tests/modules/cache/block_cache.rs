//! BlockCache 单元测试 (Phase7.3) — sharded LRU.

use std::sync::Arc;
use std::thread;

use aidb::engine::cache::{shard_index, BlockCache, CacheKey, CacheStats};
use bytes::Bytes;

fn key(file_number: u64, offset: u64) -> CacheKey {
    CacheKey {
        file_number,
        offset,
    }
}

fn bytes(s: &[u8]) -> Bytes {
    Bytes::from(s.to_vec())
}

/// Generate `count` keys that all hash to the same shard (shard 0).
fn colliding_keys(count: usize, base: u64) -> Vec<CacheKey> {
    let mut keys = Vec::new();
    for off in 0..(count * 100) {
        let k = CacheKey {
            file_number: base,
            offset: off as u64,
        };
        if shard_index(&k) == 0 {
            keys.push(k);
            if keys.len() == count {
                return keys;
            }
        }
    }
    panic!("could not find {count} colliding keys starting at base={base}");
}

#[test]
fn test_cache_basic_operations() {
    let cache = BlockCache::new(1024);
    let k = key(1, 100);
    cache.insert(k.clone(), bytes(b"hello"));

    assert_eq!(cache.get(k.clone()).unwrap(), bytes(b"hello"));
    let stats = cache.stats();
    assert_eq!(stats.lookups, 1);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.insertions, 1);

    assert!(cache.get(key(1, 200)).is_none());
    let stats = cache.stats();
    assert_eq!(stats.lookups, 2);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

#[test]
fn test_cache_lru_eviction() {
    let cache = BlockCache::new(800); // per_shard = 50
    let ks = colliding_keys(3, 1);
    cache.insert(ks[0].clone(), bytes(&[0u8; 20]));
    cache.insert(ks[1].clone(), bytes(&[1u8; 20]));
    cache.insert(ks[2].clone(), bytes(&[2u8; 20]));

    assert!(cache.get(ks[0].clone()).is_none());
    assert!(cache.get(ks[1].clone()).is_some());
    assert!(cache.get(ks[2].clone()).is_some());
}

#[test]
fn test_cache_touch_updates_lru() {
    let cache = BlockCache::new(800); // per_shard = 50
    let ks = colliding_keys(3, 2);
    cache.insert(ks[0].clone(), bytes(&[0u8; 20]));
    cache.insert(ks[1].clone(), bytes(&[1u8; 20]));

    // Touch ks[0] — promotes it to most recent
    assert!(cache.get(ks[0].clone()).is_some());

    // Insert ks[2] — should evict ks[1] (the oldest, since ks[0] was touched)
    cache.insert(ks[2].clone(), bytes(&[2u8; 20]));

    assert!(cache.get(ks[0].clone()).is_some());
    assert!(cache.get(ks[1].clone()).is_none());
}

#[test]
fn test_cache_update_existing_key() {
    let cache = BlockCache::new(256); // per_shard = 16
    let k = key(1, 0);
    cache.insert(k.clone(), bytes(&[0u8; 10]));
    assert_eq!(cache.size(), 10);
    cache.insert(k.clone(), bytes(&[1u8; 14]));
    assert_eq!(cache.size(), 14);
    assert_eq!(cache.get(k).unwrap(), bytes(&[1u8; 14]));
    assert_eq!(cache.len(), 1);
}

#[test]
fn test_cache_clear() {
    let cache = BlockCache::new(1024);
    cache.insert(key(1, 0), bytes(b"data"));
    cache.get(key(1, 0));
    cache.clear();

    assert!(cache.get(key(1, 0)).is_none());
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.size(), 0);
    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
}

#[test]
fn test_cache_disabled_when_capacity_zero() {
    let cache = BlockCache::new(0);
    cache.insert(key(1, 0), bytes(b"data"));
    assert!(cache.get(key(1, 0)).is_none());
    assert_eq!(cache.stats().insertions, 0);
}

#[test]
fn test_cache_large_value_not_cached() {
    let cache = BlockCache::new(100);
    cache.insert(key(1, 0), bytes(&[0u8; 200]));
    assert!(cache.get(key(1, 0)).is_none());
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.stats().insertions, 0);
}

#[test]
fn test_cache_value_equals_capacity() {
    let cache = BlockCache::new(64); // per_shard = 4
    cache.insert(key(1, 0), bytes(&[0u8; 4]));
    assert_eq!(cache.get(key(1, 0)).unwrap(), bytes(&[0u8; 4]));
    assert_eq!(cache.size(), 4);
}

#[test]
fn test_cache_stats_hit_rate() {
    let cache = BlockCache::new(1024);
    let k = key(1, 0);
    cache.insert(k.clone(), bytes(b"x"));
    cache.get(k.clone());
    cache.get(k);
    cache.get(key(2, 0));

    let stats = cache.stats();
    assert!((stats.hit_rate() - 2.0 / 3.0).abs() < f64::EPSILON);
}

#[test]
fn test_cache_hit_rate_zero_division() {
    let cache = BlockCache::new(1024);
    assert_eq!(cache.stats().hit_rate(), 0.0);
}

#[test]
fn test_cache_reset_stats() {
    let cache = BlockCache::new(1024);
    cache.insert(key(1, 0), bytes(b"x"));
    cache.get(key(1, 0));
    cache.reset_stats();
    let stats = cache.stats();
    assert_eq!(stats, CacheStats::default());
}

#[test]
fn test_cache_bulk_eviction() {
    let cache = BlockCache::new(4096); // per_shard = 256
    let ks = colliding_keys(40, 3);
    for (i, k) in ks.iter().enumerate() {
        cache.insert(k.clone(), bytes(&[i as u8; 16]));
    }
    assert!(cache.size() <= 4096);
    assert!(cache.stats().evictions > 0);
}

#[test]
fn test_concurrent_access() {
    let cache = Arc::new(BlockCache::new(4096));
    let handles: Vec<_> = (0..10)
        .map(|t| {
            let c = Arc::clone(&cache);
            thread::spawn(move || {
                for i in 0..100u64 {
                    let k = key(t as u64, i);
                    c.insert(k.clone(), bytes(format!("{t}-{i}").as_bytes()));
                    let _ = c.get(k);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert!(cache.size() <= 4096);
}

#[test]
fn test_concurrent_touch_race() {
    let cache = Arc::new(BlockCache::new(512)); // per_shard = 32
    cache.insert(key(1, 0), bytes(&[0u8; 16]));
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let c = Arc::clone(&cache);
            thread::spawn(move || {
                for _ in 0..50 {
                    let _ = c.get(key(1, 0));
                    c.insert(key(2, 0), bytes(&[1u8; 16]));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

/// 多线程并发读 BlockCache — 纯读路径无锁争用验证.
#[test]
fn test_concurrent_read_pure() {
    let cache = Arc::new(BlockCache::new(65536));
    // Pre-populate with diverse keys
    for f in 0..10u64 {
        for o in 0..20u64 {
            cache.insert(key(f, o), bytes(&[(f * 20 + o) as u8; 64]));
        }
    }
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let c = Arc::clone(&cache);
            thread::spawn(move || {
                for _ in 0..500 {
                    let f = (rand() % 10) as u64;
                    let o = (rand() % 20) as u64;
                    let _ = c.get(key(f, o));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

/// 读写混合并发 — 验证缓存不因并发写入而损坏.
#[test]
fn test_concurrent_read_write_mixed() {
    let cache = Arc::new(BlockCache::new(8192));
    let cache_for_writer = Arc::clone(&cache);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s1 = Arc::clone(&stop);

    // Writer: continuously insert
    let writer = thread::spawn(move || {
        let mut i = 0u64;
        while !s1.load(std::sync::atomic::Ordering::Relaxed) {
            cache_for_writer.insert(key(i % 5, i), bytes(&[(i % 256) as u8; 32]));
            i += 1;
            if i > 5000 {
                break;
            }
        }
    });

    // Readers: continuously read
    let readers: Vec<_> = (0..8)
        .map(|_| {
            let c = Arc::clone(&cache);
            thread::spawn(move || {
                let mut i = 0u64;
                while i < 1000 {
                    let _ = c.get(key(i % 5, i));
                    i += 1;
                }
            })
        })
        .collect();

    writer.join().unwrap();
    for h in readers {
        h.join().unwrap();
    }
}

fn rand() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(12345);
    let old = SEED.fetch_add(1103515245, Ordering::Relaxed);
    (old / 65536) % 32768
}
