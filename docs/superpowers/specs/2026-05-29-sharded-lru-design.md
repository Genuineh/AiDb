---
name: sharded-lru-design
description: Block Cache 分片 (sharded LRU) 减少锁竞争
---

# Sharded LRU Block Cache 设计规格

## 动机

当前 `BlockCache` 是单分片架构：全局一份 `RwLock<HashMap>` + `RwLock<VecDeque>`。

- 每次 cache hit 都获取 `lru_queue.write()`，串行化所有并发读
- `touch()` 执行 O(n) `retain` 扫描整个队列
- 读密集 workload 下不同 SST / 不同 block 的缓存访问互相竞争

分片后，不同 shard 的读取完全无竞争，将锁粒度从全局降为 `1/N`。

## 设计

### 分片策略

- 固定 16 shard（power of 2，`DEFAULT_NUM_SHARDS = 16`）
- 路由：`shard_idx = hash(key) & (NUM_SHARDS - 1)`
- 总容量均分：`capacity_per_shard = total_capacity / NUM_SHARDS`
- 每个 shard 独立 `Mutex` + `HashMap` + `VecDeque`，独立 LRU evict

### 结构

```rust
pub struct BlockCache {
    shards: Box<[CacheShard; NUM_SHARDS]>,
    // 原子计数器由所有 shard 共享
    lookups: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    insertions: AtomicU64,
    evictions: AtomicU64,
}

struct CacheShard {
    inner: Mutex<ShardInner>,
}

struct ShardInner {
    cache: HashMap<CacheKey, Bytes>,
    lru_queue: VecDeque<CacheKey>,
    current_size: usize,
}
```

### API 变化

- `BlockCache::new(capacity)` — 保持签名不变，内部创建 16 shard
- `get(&self, key)` → `self.shards[shard_idx].inner.lock()` → lookup + touch
- `insert(&self, key, value)` → 同 shard → 存 + evict
- `clear(&self)` → 遍历所有 shard 逐个清空
- `stats(&self)` → 汇总所有 shard 的计数器
- `current_size()` → 遍历求和（仅用于诊断，不在 hot path 调用）
- 所有 `RwLock` → `Mutex`（shard 内写远多于读，Mutex 更轻量）

### 兼容性

- `CacheKey`、`CacheStats` — 不变
- 调用方（`SSTableReader::get()`、`SSTableIterator::load_block()`）— 不感知变化
- `Options.block_cache_size` 配置项不变
- 现有 16 个缓存测试断言语义不变

### 不改的部分

- O(n) LRU `retain` 晋升 — 本设计不涉及。Sharding 本身已大幅降低竞争
- 解压 / CRC 校验 — 不碰
- Bloom filter 快路径 — 不碰

## 改动文件

| 文件 | 改动 |
|------|------|
| `src/engine/cache/block_cache.rs` | 重构为 sharded 架构 |
| `tests/modules/cache/block_cache.rs` | 现有测试保持通过，可能追加 shard 隔离性测试 |

## 测试

- 现有 11 个缓存测试保持通过
- 新增 `test_shard_isolation` — 多个 shard 并发写入不互相影响
- 新增 `test_concurrent_shard_access` — 随机 shard 高并发无竞争
