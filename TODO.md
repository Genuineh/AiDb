# AiDb 开发任务清单

> 本清单用于跟踪开发进度，AI agent应按顺序完成各项任务

## 📋 当前阶段：阶段一 - 项目基础设施搭建

---

## ✅ 已完成

- [x] 创建实施计划文档
- [x] 创建任务清单

---

## 🚀 待办任务

### 阶段一：项目基础设施搭建

#### 1.1 初始化Rust项目
- [ ] 创建Cargo工作空间（根目录Cargo.toml）
- [ ] 创建aidb库项目（src/lib.rs）
- [ ] 配置基础依赖包
- [ ] 创建项目目录结构（src/, tests/, benches/, examples/）
- [ ] 创建.gitignore文件
- [ ] 更新README.md（项目介绍、快速开始）

#### 1.2 核心数据类型定义
- [ ] 实现src/error.rs（Error和Result类型）
- [ ] 实现src/config.rs（Config配置结构）
- [ ] 实现src/types.rs（Key/Value基础类型）
- [ ] 实现src/key.rs（InternalKey格式）
- [ ] 编写单元测试：测试序列化和反序列化

---

### 阶段二：WAL实现

#### 2.1 日志格式设计
- [ ] 创建src/wal/目录
- [ ] 实现src/wal/format.rs（定义Record格式）
- [ ] 实现CRC32校验函数
- [ ] 编写单元测试：Record编码解码

#### 2.2 WAL核心功能
- [ ] 实现src/wal/writer.rs（WALWriter）
- [ ] 实现src/wal/reader.rs（WALReader）
- [ ] 实现src/wal/mod.rs（WAL主接口）
- [ ] 实现日志追加写入
- [ ] 实现日志fsync
- [ ] 实现日志恢复功能
- [ ] 编写集成测试：写入-崩溃-恢复测试

---

### 阶段三：MemTable实现

#### 3.1 SkipList实现
- [ ] 创建src/memtable/目录
- [ ] 实现src/memtable/skiplist.rs（并发SkipList）
- [ ] 实现SkipList的put方法
- [ ] 实现SkipList的get方法
- [ ] 实现SkipList的迭代器
- [ ] 编写单元测试：并发读写测试

#### 3.2 MemTable封装
- [ ] 实现src/memtable/mod.rs（MemTable接口）
- [ ] 实现插入、查询、删除操作
- [ ] 实现大小统计
- [ ] 实现转换为Immutable状态
- [ ] 编写测试：基本操作测试

---

### 阶段四：SSTable实现

#### 4.1 Block格式
- [ ] 创建src/sstable/目录
- [ ] 实现src/sstable/block.rs（Block结构）
- [ ] 实现Block编码器
- [ ] 实现Block解码器
- [ ] 实现Block迭代器
- [ ] 编写测试：Block读写测试

#### 4.2 SSTable Builder
- [ ] 实现src/sstable/builder.rs（SSTableBuilder）
- [ ] 实现数据块写入
- [ ] 实现索引块构建
- [ ] 实现Footer写入
- [ ] 实现完整SSTable生成
- [ ] 编写测试：构建SSTable测试

#### 4.3 SSTable Reader
- [ ] 实现src/sstable/reader.rs（SSTableReader）
- [ ] 实现打开SSTable文件
- [ ] 实现读取Footer和Index
- [ ] 实现按Key查询
- [ ] 实现数据块读取
- [ ] 编写测试：读取SSTable测试

---

### 阶段五：Bloom Filter

- [ ] 创建src/filter/目录
- [ ] 实现src/filter/bloom.rs（Bloom Filter）
- [ ] 实现多哈希函数
- [ ] 实现插入和查询操作
- [ ] 集成到SSTable Builder
- [ ] 集成到SSTable Reader
- [ ] 编写测试：误判率测试

---

### 阶段六：版本管理

#### 6.1 Version实现
- [ ] 创建src/version/目录
- [ ] 实现src/version/mod.rs（Version结构）
- [ ] 实现多层级SSTable管理
- [ ] 实现Version查询操作
- [ ] 编写测试：多层查询测试

#### 6.2 VersionEdit和Manifest
- [ ] 实现src/version/edit.rs（VersionEdit）
- [ ] 实现VersionEdit序列化
- [ ] 实现src/manifest.rs（Manifest日志）
- [ ] 实现Manifest写入和读取
- [ ] 实现Version恢复
- [ ] 编写测试：恢复测试

---

### 阶段七：Compaction

#### 7.1 Compaction策略
- [ ] 创建src/compaction/目录
- [ ] 实现src/compaction/picker.rs（文件选择器）
- [ ] 实现Level 0触发条件
- [ ] 实现Level N触发条件
- [ ] 实现文件选择算法
- [ ] 编写测试：选择器测试

#### 7.2 Compaction执行
- [ ] 实现src/compaction/mod.rs（Compaction主逻辑）
- [ ] 实现多路归并迭代器
- [ ] 实现Key合并逻辑
- [ ] 实现新SSTable生成
- [ ] 实现后台线程执行
- [ ] 编写测试：Compaction正确性测试

---

### 阶段八：DB引擎

#### 8.1 DB结构
- [ ] 创建src/db.rs
- [ ] 定义DB主结构
- [ ] 实现Open函数
- [ ] 实现Close函数
- [ ] 实现Recovery恢复逻辑

#### 8.2 写入路径
- [ ] 实现Put操作（WAL + MemTable）
- [ ] 实现Delete操作
- [ ] 实现WriteBatch批量写入
- [ ] 实现MemTable切换逻辑
- [ ] 触发后台Flush
- [ ] 编写测试：写入测试

#### 8.3 读取路径
- [ ] 实现Get操作（多层查询）
- [ ] 实现MultiGet批量读取
- [ ] 实现范围扫描（Scan）
- [ ] 编写测试：读取测试

#### 8.4 Flush操作
- [ ] 实现MemTable Flush到SSTable
- [ ] 更新Version
- [ ] 删除旧WAL
- [ ] 触发Compaction检查
- [ ] 编写测试：Flush测试

---

### 阶段九：迭代器

- [ ] 创建src/iterator/目录
- [ ] 定义Iterator trait
- [ ] 实现MemTable Iterator
- [ ] 实现SSTable Iterator
- [ ] 实现MergingIterator
- [ ] 实现DB Iterator
- [ ] 支持Seek操作
- [ ] 编写测试：迭代器测试

---

### 阶段十：高级特性

#### 10.1 Snapshot
- [ ] 实现src/snapshot.rs
- [ ] 基于Sequence Number实现
- [ ] 实现快照读取
- [ ] 编写测试：快照隔离测试

#### 10.2 压缩
- [ ] 创建src/compress.rs
- [ ] 集成Snappy压缩
- [ ] 在SSTable中支持压缩
- [ ] 编写测试：压缩解压测试

---

### 阶段十一：缓存优化

- [ ] 创建src/cache/目录
- [ ] 实现src/cache/lru.rs（LRU缓存）
- [ ] 实现Block Cache
- [ ] 实现Table Cache
- [ ] 集成到读取路径
- [ ] 编写测试：缓存命中测试

---

### 阶段十二：测试

#### 12.1 单元测试
- [ ] 补充所有模块单元测试
- [ ] 边界条件测试
- [ ] 错误处理测试

#### 12.2 集成测试
- [ ] tests/integration_test.rs（完整读写流程）
- [ ] tests/crash_recovery_test.rs（崩溃恢复）
- [ ] tests/concurrency_test.rs（并发测试）

#### 12.3 基准测试
- [ ] benches/write_bench.rs（写入性能）
- [ ] benches/read_bench.rs（读取性能）
- [ ] benches/compaction_bench.rs（Compaction性能）

---

### 阶段十三：文档

- [ ] 完善所有公共API的Rustdoc
- [ ] 创建examples/basic.rs（基础示例）
- [ ] 创建examples/batch.rs（批量操作示例）
- [ ] 创建examples/iterator.rs（迭代器示例）
- [ ] 更新README.md（完整使用文档）
- [ ] 创建docs/architecture.md（架构文档）

---

### 阶段十四：工具

- [ ] 创建src/bin/aidb-tool.rs（命令行工具）
- [ ] 实现数据导入导出
- [ ] 实现数据库检查（fsck）
- [ ] 实现性能分析
- [ ] 实现统计信息收集

---

## 📊 进度统计

- **总任务数**: ~120
- **已完成**: 2
- **进行中**: 0
- **待开始**: 118
- **完成度**: 1.7%

---

## 🎯 当前优先级

**P0 - 立即执行**:
1. 初始化Rust项目结构
2. 实现核心数据类型

**P1 - 高优先级**:
3. 实现WAL
4. 实现MemTable

**P2 - 中优先级**:
5. 实现SSTable
6. 实现版本管理

**P3 - 低优先级**:
7. 高级特性
8. 文档和工具

---

## 💡 AI Agent使用说明

1. **按顺序执行**: 严格按照阶段顺序完成任务
2. **验证测试**: 每完成一个任务，立即运行相关测试
3. **保持可编译**: 确保代码始终可以通过`cargo build`
4. **代码质量**: 定期运行`cargo clippy`和`cargo fmt`
5. **更新清单**: 完成任务后，标记为已完成并更新进度

---

*最后更新: 2025-11-04*
