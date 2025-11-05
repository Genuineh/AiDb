# AiDb 优化实施计划 - 自研Rust实现

## 🎯 核心目标

**自研纯Rust LSM-Tree存储引擎，借鉴RocksDB优点，避免其问题**

### 从RocksDB借鉴的优点

| RocksDB优点 | 如何借鉴到AiDb |
|-----------|--------------|
| **成熟的LSM架构** | 采用经过验证的分层合并策略 |
| **优秀的写入性能** | WAL + MemTable批处理优化 |
| **高效的Compaction** | Leveled compaction算法 |
| **Bloom Filter优化** | 减少无效磁盘访问 |
| **Block Cache** | LRU缓存热数据 |
| **批量写入优化** | WriteBatch原子操作 |
| **压缩支持** | 集成Snappy/LZ4 |
| **多线程Compaction** | 后台异步压缩 |

### 避免RocksDB的问题

| RocksDB问题 | AiDb解决方案 |
|-----------|------------|
| **C++代码，Rust绑定复杂** | ✅ 纯Rust实现，原生集成 |
| **API复杂，配置项过多** | ✅ 简化API，合理默认值 |
| **编译慢，依赖多** | ✅ 纯Rust依赖，编译快 |
| **代码库庞大，难以理解** | ✅ 清晰架构，模块化设计 |
| **过度设计** | ✅ 务实实现，按需添加功能 |
| **Binary体积大** | ✅ 模块化特性，按需编译 |

---

## 🚀 优化后的实施策略

### 核心原则

1. **简单优先** (Simplicity First)
   - 先实现核心功能，避免过度设计
   - 清晰的代码结构，易于理解和维护

2. **渐进验证** (Incremental Validation)
   - 每个阶段都能独立运行和测试
   - 早期发现问题，快速迭代

3. **性能导向** (Performance Oriented)
   - 从设计开始就考虑性能
   - 借鉴RocksDB的优化技巧

4. **Rust优势** (Rust Strengths)
   - 利用零成本抽象
   - 类型安全，避免内存问题
   - 优秀的并发原语

---

## 📅 三阶段实施路线

### 阶段 A：简化MVP (4-6周) - **推荐先做这个**

**目标**：快速实现可用的存储引擎，验证核心架构

**范围**：
- ✅ 基础WAL（无优化）
- ✅ 简单MemTable（单个，固定大小）
- ✅ 基础SSTable（无压缩，简单格式）
- ✅ 单线程Flush
- ✅ 基本读写功能
- ✅ 简单的崩溃恢复

**简化点**（相比完整RocksDB）：
- 不实现Compaction（只做flush）
- 单层SSTable结构
- 无Bloom Filter
- 无Block Cache
- 无压缩
- 单线程操作

**价值**：
- 2周内可以运行基础功能
- 验证整体架构设计
- 建立测试框架
- 理解LSM-Tree核心

**成功标准**：
```rust
let db = DB::open("./data", Options::default())?;
db.put(b"key", b"value")?;
assert_eq!(db.get(b"key")?, Some(b"value".to_vec()));
// 重启后数据仍在
```

---

### 阶段 B：性能优化 (6-8周)

**目标**：接近RocksDB 50-60%性能

**新增功能**：
- ✅ Leveled Compaction（核心优化）
- ✅ Bloom Filter（减少读放大）
- ✅ Block Cache（LRU缓存）
- ✅ 压缩支持（Snappy）
- ✅ WriteBatch（批量写入）
- ✅ 多MemTable支持
- ✅ 后台线程（Flush + Compaction）

**借鉴RocksDB的优化**：
1. **Compaction策略**
   - Level 0: 重叠文件
   - Level 1+: 有序无重叠
   - 大小阈值触发

2. **Bloom Filter**
   - 每个SSTable一个
   - 可配置误判率
   - 大幅减少磁盘读取

3. **缓存策略**
   - Block级别缓存
   - LRU淘汰策略
   - 可配置大小

4. **并发优化**
   - 读写分离
   - 后台异步Compaction
   - 无锁或细粒度锁

**成功标准**：
- 顺序写入：>50K ops/s
- 随机写入：>30K ops/s
- 随机读取：>50K ops/s

---

### 阶段 C：生产就绪 (4-6周)

**目标**：功能完整，可用于生产

**新增功能**：
- ✅ Snapshot（MVCC）
- ✅ Iterator（完整支持）
- ✅ 监控和指标
- ✅ 配置优化
- ✅ 错误处理完善
- ✅ 完整的测试覆盖
- ✅ 性能基准测试
- ✅ 文档完善

**避免RocksDB的复杂性**：
- 配置项<20个（vs RocksDB的200+）
- API方法<30个（vs RocksDB的100+）
- 单一数据库实例（不支持Column Families）
- 简化的事务模型

**成功标准**：
- 功能测试通过率100%
- 崩溃恢复测试通过
- 性能达到RocksDB 60-70%
- 文档完整

---

## 📋 阶段A详细计划（立即开始）

### Week 1-2: WAL + MemTable

#### Day 1-2: 项目基础
```bash
任务清单：
✅ 1. 验证当前项目结构
✅ 2. 确认依赖配置合理
✅ 3. 设置测试框架
✅ 4. 创建examples/目录结构
```

#### Day 3-5: WAL实现
```rust
// 目标：简单但可靠的WAL

src/wal/
├── mod.rs          // WAL接口
├── record.rs       // Record格式
└── writer.rs       // 写入器

// Record格式（从RocksDB学习）
[checksum: u32][length: u16][type: u8][data: bytes]

// 简化点：
- 不支持多文件
- 不支持压缩
- 简单的顺序写入
```

**关键优化**（从RocksDB学习）：
- 使用缓冲写入减少syscall
- CRC32校验保证数据完整性
- fsync确保持久化

#### Day 6-9: MemTable实现
```rust
// 目标：高性能的内存表

src/memtable/
├── mod.rs          // MemTable接口
└── skiplist.rs     // 跳表实现

// 使用crossbeam-skiplist（经过优化的实现）
// 简化点：
- 单个MemTable
- 固定大小（4MB）
- 简单的并发控制
```

**关键优化**（从RocksDB学习）：
- 使用SkipList保持有序
- 支持并发读写
- 内存使用监控

#### Day 10-14: SSTable基础

```rust
// 目标：简单的SSTable格式

src/sstable/
├── mod.rs          // SSTable接口
├── builder.rs      // 构建器
├── reader.rs       // 读取器
└── block.rs        // Block格式

// SSTable格式（简化版）
[Data Blocks...]
[Index Block]
[Footer: 48 bytes]

// 简化点：
- 无压缩
- 无Bloom Filter
- 固定Block大小（4KB）
- 简单索引
```

**关键设计**（从RocksDB学习）：
- Block级别读取
- 二级索引（Index Block + Data Block）
- Footer固定位置，快速定位

---

### Week 3-4: DB引擎整合

#### Day 15-18: DB核心逻辑

```rust
// 目标：整合所有组件

src/db/
├── mod.rs          // DB主结构
├── write.rs        // 写入路径
└── read.rs         // 读取路径

pub struct DB {
    wal: WAL,
    memtable: Arc<RwLock<MemTable>>,
    sstables: Vec<Arc<SSTable>>,  // 简化：单层
    options: Options,
}

impl DB {
    // 写入路径（从RocksDB学习）
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 写WAL（持久化）
        // 2. 写MemTable（内存）
        // 3. 检查是否需要flush
    }
    
    // 读取路径（从RocksDB学习）
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 查MemTable（最新）
        // 2. 查SSTable（从新到旧）
    }
}
```

#### Day 19-21: Flush实现

```rust
// 目标：MemTable到SSTable的转换

// Flush触发条件（从RocksDB学习）
- MemTable大小超过阈值
- 手动触发
- 关闭数据库

// Flush过程
1. 切换MemTable为Immutable
2. 创建新的MemTable
3. 后台线程写SSTable
4. 更新元数据
5. 删除旧WAL
```

#### Day 22-28: 测试和修复

```rust
// 集成测试
tests/
├── basic_test.rs           // 基础读写
├── recovery_test.rs        // 崩溃恢复
└── stress_test.rs          // 压力测试

// 测试场景（从RocksDB学习）
1. 基础CRUD
2. 大量数据写入
3. 崩溃恢复
4. 并发读写
5. 边界条件
```

---

## 🔑 关键设计决策

### 1. 数据结构选择

| 组件 | RocksDB | AiDb选择 | 理由 |
|------|---------|----------|------|
| MemTable | SkipList | crossbeam-skiplist | Rust生态成熟实现 |
| WAL格式 | 自定义 | 简化版RocksDB格式 | 经过验证的设计 |
| SSTable索引 | 二级索引 | 相同 | 高效查找 |
| 锁策略 | 细粒度 | parking_lot::RwLock | 简单+高性能 |

### 2. 并发模型

**RocksDB的问题**：
- 复杂的锁层次
- 多种同步原语
- 难以理解

**AiDb方案**：
```rust
// 简化的并发模型
1. 读写锁保护MemTable
2. Arc + RwLock管理SSTable列表
3. 后台线程独立运行
4. 消息传递协调

// 利用Rust优势
- 编译期检查数据竞争
- 类型安全的并发
- 清晰的所有权模型
```

### 3. 错误处理

**RocksDB的问题**：
- C++异常+错误码混用
- 错误信息不够清晰

**AiDb方案**：
```rust
// Rust风格的错误处理
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Corruption detected: {0}")]
    Corruption(String),
    
    #[error("Key not found: {0}")]
    NotFound(String),
}

// 优势
- 类型安全
- 强制错误处理
- 清晰的错误传播
```

### 4. 配置简化

**RocksDB**：200+ 配置选项

**AiDb**：< 20 个核心配置

```rust
pub struct Options {
    // 必需配置
    pub create_if_missing: bool,        // 默认: true
    
    // 性能相关（合理默认值）
    pub memtable_size: usize,           // 默认: 4MB
    pub block_size: usize,              // 默认: 4KB
    pub block_cache_size: usize,        // 默认: 8MB
    
    // 高级配置（可选）
    pub use_bloom_filter: bool,         // 默认: true
    pub compression: Compression,       // 默认: Snappy
    pub max_background_jobs: usize,     // 默认: 2
}

// 80%场景使用默认值即可
let db = DB::open("./data", Options::default())?;
```

---

## 🎨 架构对比

### RocksDB架构（复杂）

```
┌─────────────────────────────────────┐
│  Column Families                     │
├─────────────────────────────────────┤
│  Version Set                         │
│  ├─ Version 1                        │
│  ├─ Version 2                        │
│  └─ Version 3                        │
├─────────────────────────────────────┤
│  Compaction Picker (多种策略)        │
│  ├─ Universal                        │
│  ├─ Leveled                          │
│  └─ FIFO                            │
├─────────────────────────────────────┤
│  多种MemTable实现                    │
│  ├─ SkipList                         │
│  ├─ HashSkipList                     │
│  └─ Vector                           │
└─────────────────────────────────────┘
```

### AiDb架构（简化）

```
┌─────────────────────────────────────┐
│  DB (单一实例)                       │
├─────────────────────────────────────┤
│  WAL + MemTable                     │
├─────────────────────────────────────┤
│  SSTable Manager                     │
│  ├─ Level 0: [SST, SST, SST]       │
│  ├─ Level 1: [SST, SST, ...]       │
│  └─ Level N: [SST, ...]             │
├─────────────────────────────────────┤
│  Background Jobs                     │
│  ├─ Flush                            │
│  └─ Compaction                       │
└─────────────────────────────────────┘

核心原则：
- 单一实现，避免多态
- 清晰的层次
- 简单的生命周期
```

---

## 📊 性能目标

### 阶段A（MVP）

| 操作 | 目标 | RocksDB | 占比 |
|------|------|---------|------|
| 顺序写 | 30K ops/s | 200K ops/s | 15% |
| 随机写 | 20K ops/s | 100K ops/s | 20% |
| 随机读 | 30K ops/s | 200K ops/s | 15% |

*可接受，重点验证功能*

### 阶段B（优化）

| 操作 | 目标 | RocksDB | 占比 |
|------|------|---------|------|
| 顺序写 | 100K ops/s | 200K ops/s | 50% |
| 随机写 | 50K ops/s | 100K ops/s | 50% |
| 随机读 | 120K ops/s | 200K ops/s | 60% |

*接近实用，大部分场景够用*

### 阶段C（生产）

| 操作 | 目标 | RocksDB | 占比 |
|------|------|---------|------|
| 顺序写 | 140K ops/s | 200K ops/s | 70% |
| 随机写 | 70K ops/s | 100K ops/s | 70% |
| 随机读 | 140K ops/s | 200K ops/s | 70% |

*生产可用，满足大部分需求*

---

## 🛠️ 开发工具链

### 性能分析

```bash
# 使用cargo-flamegraph
cargo install flamegraph
cargo flamegraph --bench write_bench

# 使用criterion做基准测试
cargo bench

# 使用perf分析
perf record -g ./target/release/examples/basic
perf report
```

### 测试工具

```bash
# 单元测试
cargo test

# 集成测试
cargo test --test '*'

# 压力测试
cargo test --release -- --ignored stress

# 内存检查（Linux）
valgrind --leak-check=full ./target/debug/examples/basic
```

### 代码质量

```bash
# 静态分析
cargo clippy -- -D warnings

# 格式化
cargo fmt

# 测试覆盖率
cargo tarpaulin --out Html
```

---

## 🎯 成功标准

### 技术指标

- ✅ 功能完整：基础CRUD + 崩溃恢复
- ✅ 性能合格：达到RocksDB 60%+
- ✅ 质量保证：测试覆盖率 > 80%
- ✅ 代码清晰：平均函数 < 50行
- ✅ 文档完善：所有公共API有文档

### 实用指标

- ✅ 编译快速：< 30秒（release build）
- ✅ Binary小：< 5MB（stripped）
- ✅ 易于使用：< 10行代码完成基本操作
- ✅ 易于理解：新人 < 1天理解架构

---

## 🚧 风险和应对

### 风险1：性能未达预期

**应对**：
1. 早期建立性能基准
2. 每个阶段做性能测试
3. 使用profiler定位瓶颈
4. 参考RocksDB的优化技巧

### 风险2：正确性问题

**应对**：
1. 完善的测试覆盖
2. 使用proptest做属性测试
3. 参考LevelDB论文和实现
4. 代码审查

### 风险3：时间超期

**应对**：
1. 严格控制范围
2. MVP优先
3. 功能分阶段
4. 避免过度设计

---

## 📚 参考资源

### 必读

1. **论文**
   - "The Log-Structured Merge-Tree (LSM-Tree)"
   - "Bigtable: A Distributed Storage System"

2. **代码**
   - RocksDB源码（参考设计）
   - sled项目（Rust实现参考）
   - mini-lsm教程

3. **文档**
   - RocksDB Wiki（设计文档）
   - LevelDB源码注释

### 推荐工具

- **性能分析**：flamegraph, perf, criterion
- **测试**：proptest, quickcheck
- **调试**：rust-gdb, valgrind

---

## 📝 后续计划

### 阶段A完成后评估

1. 功能是否达到预期？
2. 性能是否在可接受范围？
3. 代码质量如何？
4. 是否需要调整后续计划？

### 可能的方向

**如果进展顺利**：
- 继续阶段B，添加优化功能
- 扩展API（snapshot, iterator）
- 性能调优

**如果遇到困难**：
- 回顾设计，寻找简化空间
- 参考更多成功项目
- 调整性能预期

---

## 总结

### 核心思路

1. **分阶段**：MVP → 优化 → 生产
2. **学习借鉴**：从RocksDB学习设计和优化
3. **避免陷阱**：不照搬RocksDB的复杂性
4. **Rust优势**：利用语言特性提升质量
5. **务实优先**：功能>完美，实用>花哨

### 下一步行动

**立即开始**：
1. ✅ 确认项目结构
2. 🚀 实现WAL（Day 3-5）
3. 🚀 实现MemTable（Day 6-9）
4. 🚀 实现SSTable（Day 10-14）

**预期成果**：
- 4-6周后有可运行的MVP
- 12-16周后有可用于生产的版本
- 持续优化和改进

---

*最后更新: 2025-11-04*
*状态: 等待确认后开始实施*
