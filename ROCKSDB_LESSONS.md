# 从RocksDB中学习什么、避免什么

## 🎯 核心问题

**如何在自研Rust实现中，借鉴RocksDB优点，避免其问题？**

---

## ✅ 从RocksDB借鉴的优点

### 1. 成熟的LSM-Tree架构

**RocksDB的设计**：
```
Level 0: 新flush的SSTable（可能重叠）
Level 1: 10MB, 有序不重叠
Level 2: 100MB, 有序不重叠
Level 3: 1GB, 有序不重叠
...
```

**AiDb借鉴**：
- ✅ 采用相同的分层策略
- ✅ Level 0允许重叠，Level 1+不重叠
- ✅ 按大小阈值触发compaction

**价值**：经过大规模验证的架构，平衡读写放大

---

### 2. 高效的WAL设计

**RocksDB的WAL格式**：
```
Record:
  [checksum: 4 bytes]  // CRC32校验
  [length: 2 bytes]    // 数据长度
  [type: 1 byte]       // FULL/FIRST/MIDDLE/LAST
  [data: N bytes]      // 实际数据
```

**AiDb借鉴**：
```rust
// src/wal/record.rs
pub struct Record {
    checksum: u32,      // ✅ CRC32保证完整性
    length: u16,        // ✅ 变长记录
    record_type: u8,    // ✅ 支持大记录分块
    data: Vec<u8>,
}
```

**价值**：
- 数据完整性保证
- 高效的顺序写入
- 支持崩溃恢复

---

### 3. Bloom Filter优化

**RocksDB的实现**：
- 每个SSTable一个Bloom Filter
- 存储在meta block
- 查询前先检查BF，避免无效读取

**AiDb借鉴**：
```rust
// src/sstable/builder.rs
pub struct SSTableBuilder {
    bloom_filter: BloomFilter,  // ✅ 每个SSTable独立BF
    // ...
}

impl SSTableBuilder {
    pub fn add(&mut self, key: &[u8], value: &[u8]) {
        self.bloom_filter.insert(key);  // ✅ 构建时插入
        self.data_block.add(key, value);
    }
}

// src/sstable/reader.rs
impl SSTableReader {
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // ✅ 先查BF，快速判断key不存在
        if !self.bloom_filter.may_contain(key) {
            return Ok(None);
        }
        // 再查索引和数据块
        self.read_data_block(key)
    }
}
```

**价值**：
- 大幅减少磁盘读取（可减少80%+无效读）
- 低内存开销（1% FP率只需10 bits/key）

---

### 4. 分块索引设计

**RocksDB的SSTable格式**：
```
[Data Block 1]  ← 4KB块
[Data Block 2]  ← 4KB块
...
[Data Block N]
[Index Block]   ← 指向每个数据块的索引
[Footer]        ← 固定48字节，指向Index Block
```

**AiDb借鉴**：
```rust
// 两级索引
// 1. Footer -> Index Block (O(1))
// 2. Index Block -> Data Block (二分查找 O(log n))
// 3. Data Block内部查找 (O(log n))

pub struct SSTable {
    index: IndexBlock,      // ✅ 内存中的索引
    data_blocks: Vec<...>,  // ✅ 按需加载数据块
}
```

**价值**：
- 减少内存占用（只需索引在内存）
- 快速查找（二级索引）
- 支持范围查询

---

### 5. Block Cache策略

**RocksDB的缓存**：
```cpp
// LRU缓存热数据块
BlockCache cache(8MB);  // 8MB缓存
```

**AiDb借鉴**：
```rust
// src/cache/lru.rs
pub struct BlockCache {
    cache: LruCache<BlockHandle, Block>,
    capacity: usize,  // ✅ 可配置大小
}

// 阶段B实现
impl DB {
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 查MemTable
        // 2. 查Block Cache ✅ 命中则直接返回
        if let Some(block) = self.cache.get(block_handle) {
            return block.get(key);
        }
        // 3. 从磁盘读取
    }
}
```

**价值**：
- 显著提升热数据读取性能
- 减少磁盘I/O
- 可配置缓存大小

---

### 6. Compaction策略

**RocksDB的Leveled Compaction**：
```
触发条件：
- Level 0: 文件数 >= 4
- Level N: 总大小 >= 10^N MB

选择策略：
- Level 0 -> Level 1: 选择所有重叠文件
- Level N -> Level N+1: 选择一个文件 + 下层重叠文件

合并过程：
- 多路归并排序
- 丢弃旧版本和删除的key
```

**AiDb借鉴**：
```rust
// src/compaction/picker.rs
pub struct CompactionPicker {
    levels: Vec<Level>,
}

impl CompactionPicker {
    pub fn pick(&self) -> Option<CompactionTask> {
        // ✅ 相同的触发条件
        if self.levels[0].files.len() >= 4 {
            return Some(self.pick_level0_compaction());
        }
        
        // ✅ 相同的选择策略
        for level in 1..self.levels.len() {
            if self.levels[level].total_size > target_size(level) {
                return Some(self.pick_level_compaction(level));
            }
        }
        None
    }
}
```

**价值**：
- 控制读写放大
- 空间回收
- 保持查询性能

---

### 7. 批量写入优化

**RocksDB的WriteBatch**：
```cpp
WriteBatch batch;
batch.Put("key1", "value1");
batch.Put("key2", "value2");
batch.Delete("key3");
db->Write(WriteOptions(), &batch);  // 原子性写入
```

**AiDb借鉴**：
```rust
// src/batch.rs
pub struct WriteBatch {
    ops: Vec<WriteOp>,  // ✅ 批量操作
    size: usize,
}

impl DB {
    pub fn write(&self, batch: WriteBatch) -> Result<()> {
        // ✅ 一次WAL写入
        self.wal.append_batch(&batch)?;
        
        // ✅ 批量写入MemTable
        for op in batch.ops {
            match op {
                WriteOp::Put(k, v) => self.memtable.put(k, v),
                WriteOp::Delete(k) => self.memtable.delete(k),
            }
        }
        Ok(())
    }
}
```

**价值**：
- 减少系统调用
- 保证原子性
- 提升写入吞吐

---

### 8. 并发控制设计

**RocksDB的读写分离**：
- 读操作：几乎无锁（MVCC）
- 写操作：写锁保护MemTable
- 后台任务：独立线程

**AiDb借鉴**：
```rust
pub struct DB {
    memtable: Arc<RwLock<MemTable>>,     // ✅ 读写锁
    imm_memtables: Arc<RwLock<Vec<...>>>, // ✅ 读多写少
    sstables: Arc<RwLock<VersionSet>>,   // ✅ 版本管理
}

impl DB {
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // ✅ 只需读锁，多个读操作可并发
        let memtable = self.memtable.read();
        if let Some(v) = memtable.get(key) {
            return Ok(Some(v));
        }
        // ...
    }
}
```

**价值**：
- 高并发读取
- 写操作不阻塞读
- 充分利用多核

---

## ❌ 避免RocksDB的问题

### 1. 配置复杂度

**RocksDB的问题**：
```cpp
// 200+ 配置选项！
DBOptions db_options;
db_options.max_background_jobs = 4;
db_options.max_subcompactions = 2;
db_options.bytes_per_sync = 1048576;
db_options.wal_bytes_per_sync = 1048576;
// ... 还有190+个配置
```

**AiDb解决方案**：
```rust
// < 20个核心配置，其余使用合理默认值
pub struct Options {
    // 必需
    pub create_if_missing: bool,           // 默认: true
    
    // 性能相关（80%场景使用默认值）
    pub memtable_size: usize,              // 默认: 4MB
    pub block_size: usize,                 // 默认: 4KB
    pub block_cache_size: usize,           // 默认: 8MB
    pub max_background_jobs: usize,        // 默认: 2
    
    // 高级（可选）
    pub use_bloom_filter: bool,            // 默认: true
    pub compression: Compression,          // 默认: Snappy
    
    // 仅此而已！
}

// 大多数用户这样用即可
let db = DB::open("./data", Options::default())?;
```

**价值**：
- 降低学习曲线
- 减少配置错误
- 80%场景开箱即用

---

### 2. API复杂度

**RocksDB的问题**：
```cpp
// API数量巨大，学习成本高
class DB {
  Status Get(const ReadOptions&, const Slice&, std::string*);
  Status Get(const ReadOptions&, ColumnFamilyHandle*, const Slice&, std::string*);
  Status Get(const ReadOptions&, ColumnFamilyHandle*, const Slice&, PinnableSlice*);
  // ... 100+ 方法
};
```

**AiDb解决方案**：
```rust
// < 30个核心API
pub trait DB {
    // 基础操作（6个）
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn delete(&self, key: &[u8]) -> Result<()>;
    fn write(&self, batch: WriteBatch) -> Result<()>;
    fn open(path: &str, options: Options) -> Result<Self>;
    fn close(&self) -> Result<()>;
    
    // 高级操作（阶段B/C添加）
    fn iter(&self) -> Iterator;
    fn snapshot(&self) -> Snapshot;
    fn compact_range(&self, start: &[u8], end: &[u8]) -> Result<()>;
    
    // 仅此而已！
}

// 简单易用
let db = DB::open("./data", Options::default())?;
db.put(b"key", b"value")?;  // 就这么简单
```

**价值**：
- API清晰简洁
- 容易上手
- 减少使用错误

---

### 3. 特性膨胀

**RocksDB的问题**：
- Column Families（多数用户不需要）
- Transaction（复杂的事务支持）
- TTL（Time To Live）
- Backup/Restore
- BlobDB
- ... 各种特性

**AiDb解决方案**：
```
阶段A (MVP):
- ✅ 基础CRUD
- ✅ WAL + 崩溃恢复
- ❌ 不实现高级特性

阶段B (优化):
- ✅ Compaction
- ✅ Bloom Filter
- ✅ Cache
- ❌ 仍不实现非核心特性

阶段C (生产):
- ✅ Snapshot
- ✅ Iterator
- ❌ 不实现Column Families
- ❌ 不实现复杂事务
```

**原则**：
- 只实现80%用户需要的功能
- 避免特性膨胀
- 保持代码简洁

**价值**：
- 代码易于维护
- Binary体积小
- 编译快速

---

### 4. 代码复杂度

**RocksDB的问题**：
```
rocksdb/
├── 500+ C++文件
├── 200,000+ 行代码
├── 复杂的类层次
└── 难以理解的代码流程
```

**AiDb解决方案**：
```
aidb/
├── 清晰的模块划分
│   ├── wal/        (< 500 行)
│   ├── memtable/   (< 500 行)
│   ├── sstable/    (< 1000 行)
│   ├── compaction/ (< 1000 行)
│   └── db/         (< 1000 行)
├── 总计 < 10,000 行
└── 清晰的文档和注释
```

**设计原则**：
```rust
// 1. 单一职责
pub struct WAL {
    // 只负责日志写入和恢复
}

// 2. 组合优于继承
pub struct DB {
    wal: WAL,               // 组合
    memtable: MemTable,     // 组合
    sstables: SSTableSet,   // 组合
}

// 3. 清晰的抽象
pub trait Iterator {
    fn next(&mut self) -> Option<(Vec<u8>, Vec<u8>)>;
}
```

**价值**：
- 新人1天能理解架构
- 容易定位问题
- 方便扩展功能

---

### 5. C++依赖问题

**RocksDB的问题**：
```bash
# 编译RocksDB需要
- C++ 编译器
- CMake
- zlib, snappy, lz4等C库
- 编译时间长（10+ 分钟）
- 跨平台编译困难
```

**AiDb解决方案**：
```bash
# 纯Rust，只需
cargo build        # 30秒内完成
cargo build --release  # 2分钟

# 依赖
bytes = "1.5"          # 纯Rust
parking_lot = "0.12"   # 纯Rust
crossbeam = "0.8"      # 纯Rust
snap = "1.1"           # 纯Rust的Snappy实现

# 跨平台
cargo build --target x86_64-pc-windows-gnu
cargo build --target aarch64-apple-darwin
# 一键编译，无需配置
```

**价值**：
- 编译快速
- 无C++依赖
- 跨平台简单
- 容易集成

---

### 6. 过度抽象

**RocksDB的问题**：
```cpp
// 过多的抽象层
class Env { ... };              // 环境抽象
class FileSystem { ... };       // 文件系统抽象
class SequentialFile { ... };   // 顺序文件抽象
class RandomAccessFile { ... }; // 随机访问抽象
class WritableFile { ... };     // 可写文件抽象
// ... 10+ 层抽象
```

**AiDb解决方案**：
```rust
// 简单直接
use std::fs::File;              // 标准库的File
use std::io::{Read, Write};     // 标准trait

// 只在必要时抽象
pub trait StorageBackend {      // 只有1层抽象
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
}

impl StorageBackend for DB {
    // 直接实现，不绕圈子
}
```

**价值**：
- 代码直接明了
- 减少运行时开销
- 容易调试

---

## 📊 对比总结

### 架构对比

| 方面 | RocksDB | AiDb | 说明 |
|------|---------|------|------|
| **LSM架构** | ✅ Leveled | ✅ 相同 | 借鉴成熟设计 |
| **WAL格式** | ✅ 高效 | ✅ 简化版 | 保留核心优化 |
| **MemTable** | ✅ SkipList | ✅ 相同 | 使用crossbeam |
| **SSTable** | ✅ 复杂格式 | ⚡ 简化版 | 降低复杂度 |
| **Compaction** | ✅ 多种策略 | ⚡ Leveled | 只实现一种 |
| **Bloom Filter** | ✅ 有 | ✅ 有 | 借鉴实现 |
| **Block Cache** | ✅ 有 | ✅ 有 | LRU缓存 |
| **配置项** | ❌ 200+ | ✅ < 20 | 大幅简化 |
| **API数量** | ❌ 100+ | ✅ < 30 | 清晰简洁 |
| **代码行数** | ❌ 200K+ | ✅ < 10K | 易于维护 |
| **编译时间** | ❌ 10+ min | ✅ < 2 min | Rust快速编译 |
| **Column Families** | ✅ 有 | ❌ 无 | 避免复杂性 |
| **事务支持** | ✅ 复杂 | ⚡ 简化 | 基础Snapshot |

### 性能预期

| 操作 | RocksDB | AiDb (阶段A) | AiDb (阶段B) | AiDb (阶段C) |
|------|---------|-------------|-------------|-------------|
| 顺序写 | 200K/s | 30K/s (15%) | 100K/s (50%) | 140K/s (70%) |
| 随机写 | 100K/s | 20K/s (20%) | 50K/s (50%) | 70K/s (70%) |
| 随机读 | 200K/s | 30K/s (15%) | 120K/s (60%) | 140K/s (70%) |

**说明**：
- 阶段A：功能优先，性能可接受
- 阶段B：性能优化，接近实用
- 阶段C：生产就绪，满足大部分需求

---

## 🎯 实施建议

### 学习RocksDB的方法

1. **阅读设计文档**
   - RocksDB Wiki
   - 相关论文
   - 源码注释

2. **理解核心算法**
   - LSM-Tree分层策略
   - Leveled Compaction
   - Bloom Filter原理

3. **但不照搬实现**
   - 理解"为什么"比"怎么做"更重要
   - Rust有更好的实现方式
   - 简化到核心需求

### 开发优先级

**第一优先级**（阶段A）：
- ✅ 正确性 > 性能
- ✅ 简单实现 > 优化
- ✅ 可运行 > 完美

**第二优先级**（阶段B）：
- ✅ 性能 > 功能
- ✅ 核心优化 > 边缘特性
- ✅ 实用 > 完美

**第三优先级**（阶段C）：
- ✅ 稳定性 > 新功能
- ✅ 文档 > 代码
- ✅ 可维护性 > 性能

---

## 总结

### 核心思想

```
借鉴 RocksDB 优点：
  ✅ 成熟的架构设计
  ✅ 经过验证的算法
  ✅ 性能优化技巧

避免 RocksDB 问题：
  ❌ 过度复杂的配置
  ❌ 庞大的API表面
  ❌ 不必要的特性
  ❌ C++依赖和编译问题

采用 Rust 优势：
  ✅ 类型安全
  ✅ 内存安全
  ✅ 零成本抽象
  ✅ 优秀的工具链

保持 简单实用：
  ✅ 清晰的代码
  ✅ 合理的默认值
  ✅ 渐进式实施
  ✅ 按需添加功能
```

### 最终目标

构建一个：
- ✅ **高性能**：达到RocksDB 60-70%性能
- ✅ **易使用**：简洁的API，合理的默认值
- ✅ **易维护**：清晰的代码，完善的文档
- ✅ **纯Rust**：无C++依赖，快速编译
- ✅ **实用性**：满足80%场景需求

的LSM-Tree存储引擎！

---

*这是AiDb项目的核心设计理念*

*最后更新: 2025-11-04*
