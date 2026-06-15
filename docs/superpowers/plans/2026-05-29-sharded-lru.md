# Sharded LRU Block Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Split `BlockCache` into 16 independent shards to reduce lock contention on concurrent reads.

**Architecture:** 16 `Mutex`-protected shards, each with own `HashMap` + `VecDeque`. Route by `hash(key) & 0xF`. Capacity divided evenly. Public API unchanged.

**Tech Stack:** Rust, same crate. No new deps.

---

### Task 1: Refactor BlockCache to sharded + adapt existing tests

**Files:**
- Modify: `src/engine/cache/block_cache.rs`
- Modify: `tests/modules/cache/block_cache.rs`

- [ ] **Step 1: Understand current code and tests**

Read these files:
- `aidb/src/engine/cache/block_cache.rs` — current single-shard cache
- `aidb/tests/modules/cache/block_cache.rs` — existing tests

Note which tests rely on exact LRU eviction order (they'll need adaptation for sharding):
- `test_cache_lru_eviction` — depends on 3 keys in 100-byte cache triggering LRU eviction
- `test_cache_bulk_eviction` — 20 entries of 32 bytes in 256-byte cache, expects evictions
- `test_cache_touch_updates_lru` — LRU order after touch

- [ ] **Step 2: Write the new sharded BlockCache**

Key implementation:

```rust
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;  // parking_lot::Mutex or std::sync::Mutex

const NUM_SHARDS: usize = 16;
const SHARD_MASK: usize = NUM_SHARDS - 1;

struct ShardInner {
    cache: HashMap<CacheKey, Bytes>,
    lru_queue: VecDeque<CacheKey>,
    current_size: usize,
}

struct CacheShard {
    inner: Mutex<ShardInner>,
}

pub struct BlockCache {
    shards: Box<[CacheShard; NUM_SHARDS]>,
    capacity_per_shard: usize,
    lookups: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    insertions: AtomicU64,
    evictions: AtomicU64,
}
```

`BlockCache::new(capacity: usize)`:
- `capacity_per_shard = max(1, capacity / NUM_SHARDS)`
- Create 16 empty shards, init counters to 0

`fn shard_index(key: &CacheKey) -> usize`:
```rust
fn shard_index(key: &CacheKey) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) & SHARD_MASK
}
```

**`get`:** lock shard → lookup → `lru_queue.retain(|k| k != key); lru_queue.push_back(*key)` → return clone

**`insert`:** lock shard → evict loop while `current_size + value_len > capacity_per_shard` → update existing size if re-insert → insert → `current_size += value_len`

**`clear`:** iterate all shards, lock each, clear all

**`current_size`:** iterate all shards, lock each, sum `current_size`

**`stats`:** return struct from atomic counters (unchanged)

- [ ] **Step 3: Adapt existing tests for sharded semantics**

The key insight: with 16 shards, eviction tests need keys that HASH TO THE SAME SHARD. Create a helper:

```rust
/// Find `count` keys that all hash to shard 0 (or any single shard).
fn colliding_keys(count: usize, base_file: u64) -> Vec<CacheKey> {
    let mut keys = Vec::new();
    for offset in 0..(count * 100) {
        let k = CacheKey { file_number: base_file, offset: offset as u64 };
        if shard_index(&k) == 0 {
            keys.push(k);
            if keys.len() == count {
                return keys;
            }
        }
    }
    panic!("could not find {count} colliding keys");
}
```

Use this helper in LRU-dependent tests like `test_cache_lru_eviction`, `test_cache_bulk_eviction`, `test_cache_touch_updates_lru`.

For tests that don't depend on LRU order (basic ops, disabled, large value, concurrent access), they should work with minimal to no changes.

- [ ] **Step 4: Run tests and iterate**

```bash
cd aidb && cargo test --test cache -- --test-threads=1
```

Fix any failures:
- If `test_concurrent_touch_race` fails, verify the Mutex/lock behavior is correct
- If `test_cache_touch_updates_lru` fails, adapt to use `colliding_keys`
- If `test_concurrent_access` size checks fail, adjust for per-shard capacity rounding
- If `test_cache_bulk_eviction` fails, use `colliding_keys` to target same shard

- [ ] **Step 5: Run full test suite**

```bash
cd aidb && cargo test --test sstable cache --test db cache --test cache -- --test-threads=1
```

- [ ] **Step 6: Clippy + fmt**

```bash
cd aidb && RUSTFLAGS='-D warnings' cargo clippy --all-targets && cargo fmt --check
```

- [ ] **Step 7: Commit**

```bash
cd aidb && git add src/engine/cache/block_cache.rs tests/modules/cache/block_cache.rs docs/superpowers/specs/2026-05-29-sharded-lru-design.md docs/superpowers/plans/2026-05-29-sharded-lru.md && git commit -m "feat: sharded LRU block cache

Split single-shard BlockCache into 16 independent shards to reduce lock
contention. Each shard has its own Mutex + HashMap + VecDeque, routed by
hash(key) & 0xF. Total capacity divided evenly. Public API unchanged.

Existing tests adapted to use same-shard keys for LRU eviction tests.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```
