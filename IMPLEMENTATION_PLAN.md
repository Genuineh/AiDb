# AiDb 存储引擎实施计划

## 项目概述
构建一个基于LSM-Tree架构的高性能KV存储引擎（类似RocksDB），使用Rust语言实现。

## 技术架构
- **语言**: Rust
- **架构**: LSM-Tree (Log-Structured Merge-Tree)
- **核心特性**: 高写入吞吐、范围查询、数据压缩、持久化

---

## 阶段一：项目基础设施搭建

### 1.1 初始化Rust项目
- [ ] 创建Cargo工作空间结构
- [ ] 配置Cargo.toml（依赖、编译选项）
- [ ] 设置项目目录结构
- [ ] 配置CI/CD基础（GitHub Actions）
- [ ] 添加基础文档（README、CONTRIBUTING）

**依赖包**:
```toml
bytes = "1.5"
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
crc32fast = "1.4"
parking_lot = "0.12"
crossbeam = "0.8"
```

**目录结构**:
```
aidb/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── error.rs
│   ├── config.rs
│   └── ...
├── tests/
├── benches/
└── examples/
```

### 1.2 核心数据类型定义
- [ ] 定义Key/Value类型
- [ ] 定义错误类型（Error枚举）
- [ ] 定义配置结构（Config）
- [ ] 实现基础序列化/反序列化
- [ ] 定义内部键格式（InternalKey: user_key + sequence + type）

---

## 阶段二：WAL（Write-Ahead Log）实现

### 2.1 日志格式设计
- [ ] 设计日志记录格式（Record Format）
  - Header: checksum(4B) + length(2B) + type(1B)
  - Data: key_len + key + value_len + value
- [ ] 实现日志写入器（WALWriter）
- [ ] 实现日志读取器（WALReader）
- [ ] 实现CRC32校验

### 2.2 WAL核心功能
- [ ] 实现追加写入（append）
- [ ] 实现日志同步（sync/fsync）
- [ ] 实现日志恢复（recovery）
- [ ] 实现日志轮转（rotation）
- [ ] 单元测试：写入/读取/恢复测试

**文件**: `src/wal/mod.rs`, `src/wal/writer.rs`, `src/wal/reader.rs`

---

## 阶段三：MemTable实现

### 3.1 MemTable数据结构
- [ ] 选择跳表（SkipList）作为内存索引结构
- [ ] 实现线程安全的SkipList
  - 使用Arc<RwLock>或crossbeam
- [ ] 实现Put操作
- [ ] 实现Get操作
- [ ] 实现Delete操作（墓碑标记）
- [ ] 实现迭代器（Iterator）

### 3.2 MemTable管理
- [ ] 实现MemTable大小限制
- [ ] 实现Immutable MemTable转换
- [ ] 实现多MemTable查询（mutable + immutable list）
- [ ] 单元测试：并发读写测试

**文件**: `src/memtable/mod.rs`, `src/memtable/skiplist.rs`

---

## 阶段四：SSTable实现

### 4.1 SSTable格式设计
- [ ] 设计SSTable文件格式
  ```
  [Data Block 1]
  [Data Block 2]
  ...
  [Data Block N]
  [Meta Block]      // Filter block (Bloom Filter)
  [Meta Index Block]
  [Index Block]     // Data block索引
  [Footer]          // 固定48字节
  ```
- [ ] 实现Block格式（数据块）
  - KV pairs + restart points + num_restarts
- [ ] 实现Footer格式

### 4.2 SSTable写入
- [ ] 实现SSTableBuilder
  - 写入数据块（分块，默认4KB）
  - 构建索引块
  - 构建Bloom Filter
- [ ] 实现数据压缩（可选：Snappy/LZ4）
- [ ] 实现SSTable落盘
- [ ] 单元测试：构建SSTable

### 4.3 SSTable读取
- [ ] 实现SSTableReader
- [ ] 实现索引块解析
- [ ] 实现数据块读取
- [ ] 实现Block Cache（LRU缓存）
- [ ] 实现两级查询：
  - 先查Bloom Filter
  - 再查Index Block
  - 最后读Data Block
- [ ] 单元测试：读取测试

**文件**: `src/sstable/mod.rs`, `src/sstable/builder.rs`, `src/sstable/reader.rs`, `src/sstable/block.rs`

---

## 阶段五：Bloom Filter实现

### 5.1 Bloom Filter
- [ ] 实现Bloom Filter数据结构
- [ ] 实现哈希函数（多个hash）
- [ ] 实现插入操作
- [ ] 实现查询操作
- [ ] 配置误判率（默认1%）
- [ ] 单元测试：误判率验证

**文件**: `src/filter/bloom.rs`

---

## 阶段六：版本管理与Manifest

### 6.1 Version（版本）
- [ ] 设计Version结构
  - 多层级SSTable列表（Level 0-6）
  - Level 0: 新刷盘的SSTable（可能重叠）
  - Level 1+: 有序且不重叠
- [ ] 实现Version查询（多层查找）
- [ ] 实现Version迭代器

### 6.2 VersionEdit（版本变更）
- [ ] 定义VersionEdit结构
  - 新增文件列表
  - 删除文件列表
  - Compaction指针
- [ ] 实现序列化/反序列化

### 6.3 Manifest（元数据日志）
- [ ] 实现Manifest写入
- [ ] 实现Manifest恢复
- [ ] 实现Version链表管理
- [ ] 单元测试：版本恢复

**文件**: `src/version/mod.rs`, `src/version/edit.rs`, `src/manifest.rs`

---

## 阶段七：Compaction实现

### 7.1 Compaction策略
- [ ] 实现Level 0 Compaction触发条件
  - 文件数量阈值（默认4个）
- [ ] 实现Level 1+ Compaction触发条件
  - 层级大小阈值（Level N: 10^N MB）
- [ ] 实现文件选择算法
  - Level 0: 选择所有重叠文件
  - Level N: 选择一个文件 + Level N+1重叠文件

### 7.2 Compaction执行
- [ ] 实现多路归并排序
- [ ] 实现重复Key合并（保留最新）
- [ ] 实现删除墓碑处理
- [ ] 实现新SSTable生成
- [ ] 实现原子性更新Version
- [ ] 后台线程执行Compaction
- [ ] 单元测试：Compaction正确性

**文件**: `src/compaction/mod.rs`, `src/compaction/picker.rs`

---

## 阶段八：核心DB引擎实现

### 8.1 DB结构设计
- [ ] 定义DB主结构
  ```rust
  pub struct DB {
      config: Config,
      wal: WAL,
      memtable: Arc<RwLock<MemTable>>,
      imm_memtables: Arc<RwLock<Vec<MemTable>>>,
      versions: Arc<RwLock<VersionSet>>,
      cache: Arc<BlockCache>,
  }
  ```

### 8.2 写入路径
- [ ] 实现Put操作
  1. 写WAL
  2. 写MemTable
  3. 检查MemTable大小
  4. 触发Flush（如需要）
- [ ] 实现Delete操作
- [ ] 实现Batch Write（批量写入）
- [ ] 实现写入限流（backpressure）

### 8.3 读取路径
- [ ] 实现Get操作
  1. 查MemTable
  2. 查Immutable MemTables
  3. 查各层SSTable（Level 0 -> Level N）
- [ ] 实现范围查询（Scan）
- [ ] 实现MultiGet（批量读取）

### 8.4 Flush操作
- [ ] 实现MemTable到SSTable的转换
- [ ] 生成新的SSTable文件
- [ ] 更新Manifest
- [ ] 删除旧的WAL
- [ ] 后台线程执行Flush

### 8.5 DB管理
- [ ] 实现Open/Close
- [ ] 实现Recovery（崩溃恢复）
  - 读取Manifest
  - 重放WAL
  - 重建MemTable
- [ ] 实现统计信息收集
- [ ] 集成测试：完整读写流程

**文件**: `src/db.rs`, `src/db/write.rs`, `src/db/read.rs`

---

## 阶段九：迭代器实现

### 9.1 基础迭代器
- [ ] 定义Iterator trait
- [ ] 实现MemTable Iterator
- [ ] 实现SSTable Iterator
- [ ] 实现Block Iterator

### 9.2 合并迭代器
- [ ] 实现MergingIterator（多路归并）
- [ ] 实现TwoLevelIterator（Index + Data）
- [ ] 实现DB Iterator（整合所有层级）
- [ ] 支持正向/反向迭代
- [ ] 单元测试：迭代器正确性

**文件**: `src/iterator/mod.rs`, `src/iterator/merging.rs`

---

## 阶段十：高级特性

### 10.1 快照（Snapshot）
- [ ] 实现快照机制
- [ ] 基于Sequence Number实现MVCC
- [ ] 快照读取隔离
- [ ] 单元测试：快照一致性

### 10.2 事务支持（可选）
- [ ] 实现简单事务接口
- [ ] 实现乐观锁
- [ ] 实现回滚机制

### 10.3 压缩算法
- [ ] 集成Snappy压缩
- [ ] 集成LZ4压缩
- [ ] 可配置压缩算法
- [ ] 性能对比测试

**文件**: `src/snapshot.rs`, `src/transaction.rs`, `src/compress.rs`

---

## 阶段十一：性能优化

### 11.1 缓存优化
- [ ] 实现LRU Block Cache
- [ ] 实现Table Cache（缓存打开的SSTable）
- [ ] 调优缓存大小
- [ ] 性能测试

### 11.2 并发优化
- [ ] 优化锁粒度
- [ ] 实现无锁SkipList（可选）
- [ ] 读写分离优化
- [ ] 并发Compaction

### 11.3 IO优化
- [ ] 实现Direct IO（可选）
- [ ] 实现mmap读取（可选）
- [ ] 批量写入优化
- [ ] 预读优化

**文件**: `src/cache/mod.rs`, `src/cache/lru.rs`

---

## 阶段十二：测试与基准测试

### 12.1 单元测试
- [ ] 每个模块完整单元测试覆盖
- [ ] 边界条件测试
- [ ] 错误处理测试
- [ ] 并发测试

### 12.2 集成测试
- [ ] 完整读写测试
- [ ] 崩溃恢复测试
- [ ] 大数据量测试
- [ ] 压力测试

### 12.3 性能基准测试
- [ ] 实现基准测试框架（criterion）
- [ ] 顺序写入性能测试
- [ ] 随机写入性能测试
- [ ] 顺序读取性能测试
- [ ] 随机读取性能测试
- [ ] 与RocksDB性能对比

**文件**: `tests/`, `benches/`

---

## 阶段十三：文档与示例

### 13.1 API文档
- [ ] 完善Rustdoc注释
- [ ] 生成API文档
- [ ] 添加使用示例

### 13.2 用户文档
- [ ] 编写快速开始指南
- [ ] 编写架构设计文档
- [ ] 编写性能调优指南
- [ ] 编写故障排查指南

### 13.3 示例代码
- [ ] 基础读写示例
- [ ] 批量操作示例
- [ ] 迭代器使用示例
- [ ] 快照使用示例

**文件**: `docs/`, `examples/`

---

## 阶段十四：工具与监控

### 14.1 命令行工具
- [ ] 实现数据导入工具
- [ ] 实现数据导出工具
- [ ] 实现数据库检查工具（fsck）
- [ ] 实现性能分析工具

### 14.2 监控指标
- [ ] 实现统计指标收集
  - 读写QPS
  - 延迟分布
  - Compaction统计
  - 缓存命中率
- [ ] 支持Prometheus格式导出（可选）

**文件**: `src/bin/`, `src/stats.rs`

---

## 实施建议

### 开发顺序
1. **先纵向，后横向**: 先实现基础功能的完整链路，再添加高级特性
2. **测试驱动**: 每个阶段完成后立即编写测试
3. **增量开发**: 每个阶段都能独立验证和测试
4. **性能优先**: 在正确性基础上，持续关注性能

### 关键里程碑
- **Milestone 1**: 完成阶段1-3，实现WAL + MemTable
- **Milestone 2**: 完成阶段4-6，实现SSTable + Version
- **Milestone 3**: 完成阶段7-8，实现Compaction + DB引擎
- **Milestone 4**: 完成阶段9-11，实现高级特性和优化
- **Milestone 5**: 完成阶段12-14，完善测试和文档

### AI Agent实施建议
1. 每个阶段从顶向下逐步实施
2. 遇到复杂模块先实现简化版本，再逐步完善
3. 保持代码可编译状态
4. 每完成一个子任务，运行相关测试验证
5. 定期运行`cargo clippy`和`cargo fmt`保持代码质量

### 技术难点
- **并发控制**: MemTable和Version的并发访问
- **Compaction调度**: 多层级文件选择和合并策略
- **崩溃恢复**: 确保数据一致性
- **性能优化**: 减少内存拷贝，优化IO路径

---

## 参考资源

### 开源项目
- [RocksDB](https://github.com/facebook/rocksdb)
- [LevelDB](https://github.com/google/leveldb)
- [mini-lsm](https://github.com/skyzh/mini-lsm) - Rust教学项目
- [sled](https://github.com/spacejam/sled) - Rust实现的嵌入式数据库

### 论文与文章
- "The Log-Structured Merge-Tree (LSM-Tree)" - O'Neil et al.
- "Bigtable: A Distributed Storage System" - Google
- LevelDB源码解析系列文章

### Rust相关
- Rust并发编程最佳实践
- Rust性能优化指南
- Tokio异步运行时（如需要异步版本）

---

## 预期成果

完成本计划后，将得到：
1. ✅ 功能完整的LSM-Tree存储引擎
2. ✅ 高性能的读写能力（接近RocksDB 50-70%性能）
3. ✅ 完善的测试覆盖
4. ✅ 详细的文档和示例
5. ✅ 生产可用的代码质量

**预估工作量**: 2-3个月（全职开发）

---

*本计划持续更新，随着项目进展可能调整优先级和细节*
