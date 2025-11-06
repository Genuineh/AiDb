# AiDb 开发任务清单

> 本清单跟踪所有开发任务。更新时间：2025-11-04

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

#### Week 3-4: DB引擎整合

**DB核心逻辑** (Day 15-18)
- [ ] 实现DB::open()
- [ ] 实现DB::put()
- [ ] 实现DB::get()
- [ ] 实现DB::delete()
- [ ] 写入路径集成
- [ ] 读取路径集成
- [ ] 基础集成测试

**Flush实现** (Day 19-21)
- [ ] MemTable→SSTable转换
- [ ] Immutable MemTable管理
- [ ] 后台Flush线程
- [ ] WAL轮转
- [ ] Flush触发条件
- [ ] Flush测试

**测试和修复** (Day 22-28)
- [ ] 端到端测试
- [ ] 崩溃恢复测试
- [ ] 并发测试
- [ ] 压力测试
- [ ] Bug修复
- [ ] 性能初测

### 阶段B: 性能优化 (Week 7-14)

#### Week 7-8: Compaction
- [ ] Level 0 Compaction
- [ ] Level N Compaction
- [ ] 文件选择策略
- [ ] 多路归并实现
- [ ] 后台Compaction线程
- [ ] Version管理
- [ ] Manifest实现
- [ ] Compaction测试

#### Week 9-10: Bloom Filter
- [ ] BloomFilter数据结构
- [ ] 哈希函数实现
- [ ] 插入和查询
- [ ] 集成到SSTableBuilder
- [ ] 集成到SSTableReader
- [ ] 误判率测试

#### Week 11-12: Block Cache
- [ ] LRU Cache实现
- [ ] 集成到读取路径
- [ ] 缓存统计
- [ ] 缓存大小配置
- [ ] 性能测试

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

- **总任务数**: ~150
- **已完成**: 30
- **进行中**: 0
- **待开始**: 120
- **完成度**: 20%

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
