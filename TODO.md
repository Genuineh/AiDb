# AiDb 开发任务清单

> 本清单跟踪所有开发任务。更新时间：2025-11-06
>
> **最新更新**: Flush功能实现完成！(Day 19-21) - 所有22个子任务完成，8个新测试全部通过

## 📋 当前Sprint

**阶段A (MVP)** - Week 1-6

### 🚀 本周任务 (Week 1)

- [x] 项目初始化
- [x] 基础架构设计
- [x] WAL实现
- [x] MemTable实现 ✅ **已完成**
- [x] SSTable实现 ✅ **已完成**

---

## ✅ 已完成

### 项目基础
- [x] Cargo工作空间配置
- [x] 项目目录结构
- [x] 错误类型定义 (error.rs)
- [x] 配置结构 (config.rs)
- [x] 基础文档结构
- [x] 架构设计文档
- [x] 实施计划文档

---

## 📅 待办任务

### 阶段A: MVP (Week 1-6)

#### Week 1-2: WAL + MemTable

**WAL实现** (Day 3-5)
- [x] 定义Record格式
- [x] 实现CRC32校验
- [x] 实现WALWriter
- [x] 实现WALReader
- [x] 实现追加写入
- [x] 实现fsync
- [x] 实现恢复功能
- [x] WAL单元测试

**MemTable实现** (Day 6-9) ✅
- [x] 集成crossbeam-skiplist
- [x] 实现Put操作
- [x] 实现Get操作
- [x] 实现Delete操作（墓碑）
- [x] 实现Iterator
- [x] 实现大小统计
- [x] 并发读写测试

**SSTable基础** (Day 10-14) ✅
- [x] 设计Block格式
- [x] 实现BlockBuilder
- [x] 实现BlockReader
- [x] 实现SSTableBuilder
- [x] 实现SSTableReader
- [x] 实现Index Block
- [x] 实现Footer
- [x] SSTable测试

#### Week 3-4: DB引擎整合 ⭐ **当前阶段**

**状态**: 未开始 | **优先级**: P0 | **预计**: 14天

**DB核心逻辑** (Day 15-18) - 预计4天 ✅ **已完成**
- [x] **DB结构设计与实现**
  - [x] 添加 DB 内部字段（MemTable, WAL, SSTables, Options等）
  - [x] 实现线程安全机制（Arc, RwLock）
  - [x] 设计序列号管理（SequenceNumber）
  - [x] 添加目录结构管理
  
- [x] **DB::open() 实现**
  - [x] 创建/验证数据库目录
  - [x] 恢复 WAL 日志
  - [x] 初始化 MemTable
  - [x] 加载现有 SSTables
  - [x] 单元测试
  
- [x] **DB::put() 实现**
  - [x] 写入 WAL（持久化）
  - [x] 插入 MemTable
  - [x] 序列号递增
  - [x] 检查 MemTable 大小
  - [x] 单元测试
  
- [x] **DB::get() 实现**
  - [x] 从 MemTable 查找
  - [x] 从 Immutable MemTable 查找
  - [x] 从 SSTables 查找（Level 0 → Level N）
  - [x] 处理删除标记
  - [x] 单元测试
  
- [x] **DB::delete() 实现**
  - [x] 写入墓碑到 WAL
  - [x] 插入墓碑到 MemTable
  - [x] 单元测试
  
- [x] **集成测试**
  - [x] 测试完整的写入-读取流程
  - [x] 测试删除操作
  - [x] 测试重启恢复（基础实现）
  - [x] 测试错误处理

**Flush实现** (Day 19-21) - 预计3天 ✅ **已完成**
- [x] **MemTable→SSTable转换**
  - [x] 实现 flush_memtable() 方法
  - [x] 遍历 MemTable 所有键值对
  - [x] 使用 SSTableBuilder 构建 SSTable
  - [x] 生成新的 SSTable 文件
  - [x] 单元测试
  
- [x] **Immutable MemTable管理**
  - [x] 添加 immutable_memtables 字段（Vec 或 VecDeque）
  - [x] 实现 MemTable freeze 操作
  - [x] 实现切换逻辑（mutable → immutable）
  - [x] 更新 get() 查询路径
  - [x] 单元测试
  
- [x] **后台Flush线程**
  - [x] 实现同步flush机制（暂不需要后台线程）
  - [x] 实现 flush 完成后清理
  - [x] 测试并发写入+flush
  
- [x] **WAL轮转**
  - [x] Flush 完成后删除旧 WAL
  - [x] 创建新 WAL 文件
  - [x] 实现 WAL 文件管理
  - [x] 测试 WAL 轮转
  
- [x] **Flush触发条件**
  - [x] 实现大小检查（MemTable size >= threshold）
  - [x] 实现手动 flush API
  - [x] 实现关闭时自动 flush
  - [x] 测试各种触发场景
  
- [x] **Flush集成测试**
  - [x] 测试自动flush
  - [x] 测试手动flush
  - [x] 测试flush后读取
  - [x] 测试并发写入+flush
  - [x] 测试多次flush
  
完成详情：见 [FLUSH_COMPLETION_SUMMARY.md](FLUSH_COMPLETION_SUMMARY.md)

**测试和修复** (Day 22-28) - 预计7天 ✅ **已完成**
- [x] **端到端测试**
  - [x] 完整 CRUD 流程测试
  - [x] 大量数据写入测试（10万+条）
  - [x] 顺序写入+随机读取
  - [x] 随机写入+随机读取
  - [x] 覆盖写入测试
  
- [x] **崩溃恢复测试**
  - [x] 写入中途崩溃恢复
  - [x] Flush中途崩溃恢复
  - [x] WAL损坏处理
  - [x] 部分写入恢复
  - [x] 验证数据一致性
  
- [x] **并发测试**
  - [x] 多线程并发写入
  - [x] 多线程并发读取
  - [x] 并发读写混合
  - [x] 并发写入+flush
  - [x] 死锁检测
  - [x] 数据竞争检测（TSAN/Miri）
  
- [x] **压力测试**
  - [x] 高频写入测试（100k ops/s）
  - [x] 高频读取测试
  - [x] 大value写入（1MB+）
  - [x] 内存压力测试
  - [x] 磁盘空间测试
  - [x] 长时间运行测试（1小时+）
  - ℹ️  注：压力测试通过手动触发的 GitHub Actions workflow 运行
  
- [x] **Bug修复**
  - [x] 修复测试中发现的问题
    - [x] 修复 WAL 恢复逻辑（实现完整的 WAL 条目解析）
    - [x] 实现 Drop trait 确保自动 flush
    - [x] 修复多次重启后的数据恢复问题（扫描最新 WAL 文件）
  - [x] 代码审查
  - [x] 内存泄漏检查
  - [x] 边界条件处理
  
- [x] **性能初测**
  - [x] 写入性能基准测试框架
  - [x] 读取性能基准测试框架
  - [x] 内存使用统计
  - [x] 磁盘IO统计
  - [x] 生成性能报告
  - ℹ️  注：基准测试通过手动触发的 GitHub Actions workflow 运行

完成详情：见 [TESTING_COMPLETION_SUMMARY.md](TESTING_COMPLETION_SUMMARY.md)

### 阶段B: 性能优化 (Week 7-14)

#### Week 7-8: Compaction ✅ **已完成**
- [x] Level 0 Compaction
- [x] Level N Compaction
- [x] 文件选择策略
- [x] 多路归并实现
- [x] Version管理
- [x] Manifest实现
- [x] Compaction测试
- [x] Tombstone处理
- [ ] 后台Compaction线程（简化为同步实现）

完成详情：见 [COMPACTION_COMPLETION_SUMMARY.md](COMPACTION_COMPLETION_SUMMARY.md)

#### Week 9-10: Bloom Filter ✅ **已完成**
- [x] BloomFilter数据结构
- [x] 哈希函数实现
- [x] 插入和查询
- [x] 集成到SSTableBuilder
- [x] 集成到SSTableReader
- [x] 误判率测试

完成详情：见 [BLOOM_FILTER_COMPLETION_SUMMARY.md](BLOOM_FILTER_COMPLETION_SUMMARY.md)

#### Week 11-12: Block Cache ✅ **已完成**
- [x] LRU Cache实现
- [x] 集成到读取路径
- [x] 缓存统计
- [x] 缓存大小配置
- [x] 性能测试

完成详情：见 [BLOCK_CACHE_COMPLETION_SUMMARY.md](BLOCK_CACHE_COMPLETION_SUMMARY.md)

#### Week 13-14: 压缩和优化
- [ ] Snappy压缩集成
- [ ] WriteBatch实现
- [ ] 批量写入优化
- [ ] 并发优化
- [ ] 读写分离
- [ ] 完整基准测试
- [ ] 性能报告

### 阶段C: 生产就绪 (Week 15-20)

#### Week 15-16: 高级功能
- [ ] Snapshot实现
- [ ] MVCC支持
- [ ] Iterator完整实现
- [ ] 范围查询
- [ ] 配置优化

#### Week 17-18: 测试完善
- [ ] 单元测试覆盖率>80%
- [ ] 集成测试套件
- [ ] 压力测试
- [ ] 故障注入测试
- [ ] 边界条件测试

#### Week 19-20: 文档和发布
- [ ] API文档完善
- [ ] 代码示例
- [ ] 使用指南
- [ ] 最佳实践文档
- [ ] 性能调优文档
- [ ] 发布v0.1.0

---

### 阶段1: RPC网络层 (Week 21-24)

#### Week 21: RPC框架
- [ ] Protobuf接口定义
- [ ] tonic/gRPC集成
- [ ] RPC服务端实现
- [ ] RPC客户端实现
- [ ] 连接池
- [ ] 超时和重试

#### Week 22: Primary节点
- [ ] PrimaryNode结构
- [ ] RPC服务集成
- [ ] 健康检查端点
- [ ] 统计信息
- [ ] 测试

#### Week 23: Replica节点
- [ ] ReplicaNode结构
- [ ] LRU缓存实现
- [ ] RPC客户端集成
- [ ] 缓存miss转发
- [ ] 预热策略
- [ ] 测试

#### Week 24: 网络优化
- [ ] 连接池优化
- [ ] 批量请求
- [ ] 压缩传输
- [ ] 性能测试

---

### 阶段2: Coordinator (Week 25-28)

#### Week 25: 一致性哈希
- [ ] 哈希环实现
- [ ] 虚拟节点
- [ ] 节点增删
- [ ] 均衡性测试

#### Week 26: Coordinator核心
- [ ] 路由实现
- [ ] Shard注册
- [ ] 负载均衡
- [ ] 转发逻辑
- [ ] 测试

#### Week 27-28: 健康检查
- [ ] 定期检测
- [ ] 故障处理
- [ ] 自动剔除
- [ ] 告警集成

---

### 阶段3-6: 后续功能 (Week 29-48)

详见 [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md)

---

## 📊 进度统计

- **总任务数**: ~200+
- **已完成**: 116 (新增5个Block Cache任务)
- **Week 3-4 任务数**: 67 (详细子任务)
- **Week 7-8 任务数**: 8 (Compaction任务)
- **Week 9-10 任务数**: 6 (Bloom Filter任务)
- **Week 11-12 任务数**: 5 (Block Cache任务)
- **进行中**: 0
- **待开始**: 84+
- **完成度**: 58.0%

### Week 3-4 详细统计
- **DB核心逻辑**: 26/26 任务 ✅ **已完成**
- **Flush实现**: 22/22 任务 ✅ **已完成**
- **测试和修复**: 19/19 任务 ✅ **已完成**

### 测试覆盖统计
- **单元测试**: 133个 ✅ (新增10个Block Cache单元测试)
- **Bloom Filter集成测试**: 7个 ✅
- **Compaction集成测试**: 8个 ✅
- **Block Cache集成测试**: 5个 ✅
- **端到端测试**: 14个 ✅
- **崩溃恢复测试**: 11个 ✅
- **并发测试**: 10个 ✅
- **文档测试**: 19个 ✅
- **压力测试**: 7个 (手动触发) ✅
- **总计**: 207+ 测试

---

## 🎯 里程碑

- [ ] M1: MVP可运行 (Week 6)
- [ ] M2: 单机性能达标 (Week 14)
- [ ] M3: 单机生产就绪 (Week 20)
- [ ] M4: RPC通信完成 (Week 24)
- [ ] M5: 集群路由完成 (Week 28)
- [ ] M6: 多Shard运行 (Week 34)
- [ ] M7: 备份恢复完成 (Week 40)
- [ ] M8: 生产就绪 (Week 48)

---

## 📝 Notes

### 优先级说明
- P0: 阻塞性任务，必须立即完成
- P1: 高优先级，本周完成
- P2: 中优先级，本月完成
- P3: 低优先级，可延后

### 任务状态
- [ ] 未开始
- [x] 已完成
- ⭐ 当前任务

---

*定期更新，保持同步*
