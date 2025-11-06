# AiDb 实施计划

本文档整合了单机版和集群版的完整实施计划。

## 目录

- [总览](#总览)
- [阶段0: 单机版 (Week 1-20)](#阶段0-单机版-week-1-20)
- [阶段1: RPC网络层 (Week 21-24)](#阶段1-rpc网络层-week-21-24)
- [阶段2: Coordinator (Week 25-28)](#阶段2-coordinator-week-25-28)
- [阶段3: Shard Group (Week 29-34)](#阶段3-shard-group-week-29-34)
- [阶段4: 备份恢复 (Week 35-40)](#阶段4-备份恢复-week-35-40)
- [阶段5: 弹性伸缩 (Week 41-44)](#阶段5-弹性伸缩-week-41-44)
- [阶段6: 监控运维 (Week 45-48)](#阶段6-监控运维-week-45-48)

---

## 总览

### 时间表（48周 ≈ 12个月）

```
阶段0: 单机版 (Week 1-20) ⭐ 当前阶段
  ├─ A: MVP (Week 1-6)
  ├─ B: 性能优化 (Week 7-14)
  └─ C: 生产就绪 (Week 15-20)

阶段1: RPC网络层 (Week 21-24)
  ├─ Week 21: RPC框架
  ├─ Week 22: Primary节点
  ├─ Week 23: Replica节点
  └─ Week 24: 网络优化

阶段2: Coordinator (Week 25-28)
  ├─ Week 25: 一致性哈希
  ├─ Week 26: 路由和负载均衡
  └─ Week 27-28: 健康检查

阶段3: Shard Group (Week 29-34)
  ├─ Week 29-30: ShardGroup管理
  ├─ Week 31-32: 多Shard测试
  └─ Week 33-34: 性能优化

阶段4: 备份恢复 (Week 35-40)
  ├─ Week 35-36: 备份管理器
  ├─ Week 37-38: 恢复机制
  └─ Week 39-40: 集成测试

阶段5: 弹性伸缩 (Week 41-44)
  ├─ Week 41-42: 动态扩展
  └─ Week 43-44: 自动伸缩(可选)

阶段6: 监控运维 (Week 45-48)
  ├─ Week 45-46: Prometheus监控
  └─ Week 47-48: 运维工具
```

### 里程碑

| 里程碑 | 时间 | 交付物 |
|--------|------|--------|
| M1: MVP可运行 | Week 6 | 基础读写+崩溃恢复 |
| M2: 单机性能达标 | Week 14 | 性能达RocksDB 60% |
| M3: 单机生产就绪 | Week 20 | 完整功能+文档 |
| M4: RPC通信完成 | Week 24 | Primary+Replica |
| M5: 集群路由完成 | Week 28 | Coordinator工作 |
| M6: 多Shard运行 | Week 34 | 完整集群功能 |
| M7: 备份恢复完成 | Week 40 | 容灾方案 |
| M8: 生产就绪 | Week 48 | 完整系统上线 |

---

## 阶段0: 单机版 (Week 1-20)

### 阶段A: MVP (Week 1-6)

**目标**：快速验证核心架构

#### Week 1-2: WAL + MemTable

**Day 1-2**: 项目准备
- [x] 确认项目结构
- [x] 配置依赖
- [x] 设置测试框架

**Day 3-5**: WAL实现 ✅
```rust
// src/wal/mod.rs
pub struct WAL {
    writer: WALWriter,
}

// Record格式
[checksum: u32][length: u16][type: u8][data: bytes]

任务：
- [x] Record编码/解码
- [x] CRC32校验
- [x] 追加写入
- [x] fsync支持
- [x] 单元测试
```

**Day 6-9**: MemTable实现
```rust
// src/memtable/mod.rs
pub struct MemTable {
    data: SkipList<InternalKey, Value>,
    size: AtomicUsize,
}

任务：
- [ ] 集成crossbeam-skiplist
- [ ] Put/Get/Delete操作
- [ ] 大小统计
- [ ] 迭代器
- [ ] 并发测试
```

**Day 10-14**: SSTable基础
```rust
// src/sstable/mod.rs  
pub struct SSTable {
    index: IndexBlock,
    bloom_filter: Option<BloomFilter>,
}

任务：
- [ ] Block格式设计
- [ ] SSTableBuilder
- [ ] SSTableReader
- [ ] Footer和Index
- [ ] 基础测试
```

#### Week 3-4: DB引擎整合

**Day 15-18**: DB核心逻辑 ✅ **已完成**
```rust
// src/lib.rs
pub struct DB {
    path: PathBuf,
    options: Options,
    memtable: Arc<RwLock<MemTable>>,
    immutable_memtables: Arc<RwLock<Vec<Arc<MemTable>>>>,
    wal: Arc<RwLock<WAL>>,
    sstables: Arc<RwLock<Vec<Vec<Arc<SSTableReader>>>>>,
    sequence: Arc<AtomicU64>,
}

任务：
- [x] DB::open()实现 - 创建目录、恢复WAL、初始化组件
- [x] Put/Get/Delete - 完整CRUD操作
- [x] 写入路径 - WAL → MemTable
- [x] 读取路径 - MemTable → Immutable → SSTables
- [x] 基础集成测试 - 83个测试全部通过

完成详情：见 [DB_CORE_COMPLETION_SUMMARY.md](../DB_CORE_COMPLETION_SUMMARY.md)
```

**Day 19-21**: Flush实现
```rust
任务：
- [ ] MemTable→SSTable转换
- [ ] Immutable MemTable管理
- [ ] 后台Flush线程
- [ ] WAL轮转
- [ ] Flush测试
```

**Day 22-28**: 测试和修复
```rust
任务：
- [ ] 端到端测试
- [ ] 崩溃恢复测试
- [ ] 并发测试
- [ ] Bug修复
- [ ] 性能初测
```

**阶段A成功标准**：
```rust
// 能稳定运行
let db = DB::open("./data", Options::default())?;
for i in 0..10000 {
    db.put(&format!("key{}", i).as_bytes(), b"value")?;
}
// 性能：20K+ ops/s
```

**当前状态**：
- ✅ DB核心逻辑已完成（Day 15-18）
- 🔄 Flush实现待开始（Day 19-21）
- 🔄 测试和修复待开始（Day 22-28）

---

### 阶段B: 性能优化 (Week 7-14)

**目标**：接近RocksDB 50-60%性能

#### Week 7-8: Compaction实现

```rust
// src/compaction/mod.rs
pub struct CompactionJob {
    level: usize,
    inputs: Vec<SSTable>,
}

任务：
- [ ] Level 0 Compaction
- [ ] Level N Compaction
- [ ] 文件选择策略
- [ ] 多路归并
- [ ] 后台线程
- [ ] Compaction测试
```

#### Week 9-10: Bloom Filter

```rust
// src/filter/bloom.rs
pub struct BloomFilter {
    bits: Vec<u8>,
    num_hashes: u32,
}

任务：
- [ ] Bloom Filter实现
- [ ] 集成到SSTableBuilder
- [ ] 集成到SSTableReader
- [ ] 误判率测试
```

#### Week 11-12: Block Cache

```rust
// src/cache/lru.rs
pub struct BlockCache {
    cache: LruCache<BlockHandle, Block>,
}

任务：
- [ ] LRU Cache实现
- [ ] 集成到读取路径
- [ ] 缓存统计
- [ ] 性能测试
```

#### Week 13-14: 压缩和优化

```rust
任务：
- [ ] Snappy压缩集成
- [ ] WriteBatch实现
- [ ] 并发优化
- [ ] 性能调优
- [ ] 完整基准测试
```

**阶段B成功标准**：
- 顺序写：100K ops/s
- 随机写：50K ops/s
- 随机读：120K ops/s

---

### 阶段C: 生产就绪 (Week 15-20)

**目标**：功能完整，可用于生产

#### Week 15-16: 高级功能

```rust
任务：
- [ ] Snapshot实现
- [ ] Iterator完整支持
- [ ] 范围查询
- [ ] 配置优化
```

#### Week 17-18: 测试完善

```rust
任务：
- [ ] 单元测试覆盖率>80%
- [ ] 集成测试
- [ ] 压力测试
- [ ] 故障注入测试
```

#### Week 19-20: 文档和发布

```rust
任务：
- [ ] API文档完善
- [ ] 使用示例
- [ ] 性能报告
- [ ] 最佳实践文档
```

**阶段C成功标准**：
- 所有测试通过
- 性能达RocksDB 60-70%
- 文档完整

---

## 阶段1: RPC网络层 (Week 21-24)

### Week 21: RPC框架搭建

**技术选型**: tonic (gRPC)

```toml
[dependencies]
tonic = "0.10"
prost = "0.12"
tokio = { version = "1", features = ["full"] }
```

**定义服务接口**:
```protobuf
// proto/aidb.proto
service Storage {
  rpc Get(GetRequest) returns (GetResponse);
  rpc Put(PutRequest) returns (PutResponse);
  rpc Delete(DeleteRequest) returns (DeleteResponse);
  rpc BatchGet(BatchGetRequest) returns (BatchGetResponse);
  rpc Scan(ScanRequest) returns (stream ScanResponse);
  rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
}
```

**任务清单**：
- [ ] Protobuf定义
- [ ] RPC服务端实现
- [ ] RPC客户端实现
- [ ] 连接池
- [ ] 超时和重试
- [ ] 单元测试

### Week 22: Primary节点实现

```rust
// src/cluster/primary.rs
pub struct PrimaryNode {
    db: Arc<DB>,
    rpc_server: RpcServer,
    stats: Arc<RwLock<PrimaryStats>>,
}

任务：
- [ ] 包装DB为Primary
- [ ] RPC服务集成
- [ ] 健康检查端点
- [ ] 统计信息
- [ ] 测试
```

### Week 23: Replica节点实现

```rust
// src/cluster/replica.rs  
pub struct ReplicaNode {
    cache: Arc<RwLock<LruCache<Vec<u8>, Vec<u8>>>>,
    primary_client: Arc<Mutex<StorageClient>>,
}

任务：
- [ ] LRU缓存实现
- [ ] RPC客户端集成
- [ ] 缓存miss转发
- [ ] 预热策略
- [ ] 测试
```

### Week 24: 网络层优化

```rust
任务：
- [ ] 连接池优化
- [ ] 批量请求
- [ ] 压缩传输
- [ ] 性能测试
```

**阶段1交付物**：
- ✅ Primary通过RPC提供服务
- ✅ Replica缓存+转发工作
- ✅ 性能测试通过

---

## 阶段2: Coordinator (Week 25-28)

### Week 25: 一致性哈希实现

```rust
// src/cluster/consistent_hash.rs
pub struct ConsistentHashRing {
    ring: BTreeMap<u64, ShardId>,
    virtual_nodes: usize,
}

任务：
- [ ] 哈希环实现
- [ ] 虚拟节点
- [ ] 节点增删
- [ ] 均衡性测试
```

### Week 26: Coordinator核心逻辑

```rust
// src/cluster/coordinator.rs
pub struct Coordinator {
    hash_ring: Arc<RwLock<ConsistentHashRing>>,
    shard_groups: Arc<RwLock<HashMap<ShardId, ShardGroup>>>,
}

任务：
- [ ] 路由实现
- [ ] Shard注册
- [ ] 负载均衡
- [ ] Get/Put转发
- [ ] 测试
```

### Week 27-28: 健康检查和故障处理

```rust
// src/cluster/health.rs
pub struct HealthChecker {
    coordinator: Arc<Coordinator>,
    check_interval: Duration,
}

任务：
- [ ] 定期健康检查
- [ ] 故障检测
- [ ] 自动剔除
- [ ] 告警集成
- [ ] 测试
```

**阶段2交付物**：
- ✅ Coordinator可路由请求
- ✅ 负载均衡工作
- ✅ 健康检查正常

---

## 阶段3: Shard Group (Week 29-34)

### Week 29-30: ShardGroupManager

```rust
// src/cluster/shard_group.rs
pub struct ShardGroupManager {
    primary: Option<PrimaryNode>,
    replicas: Vec<ReplicaNode>,
    state: ShardState,
}

任务：
- [ ] 生命周期管理
- [ ] 启动/停止
- [ ] 添加/移除Replica
- [ ] 状态管理
- [ ] 测试
```

### Week 31-32: 多Shard集成测试

```rust
任务：
- [ ] 启动多个Shard
- [ ] 数据分布验证
- [ ] 路由正确性
- [ ] 故障场景测试
- [ ] 负载测试
```

### Week 33-34: 性能优化

```rust
任务：
- [ ] 瓶颈识别
- [ ] 优化热点
- [ ] 压力测试
- [ ] 性能报告
```

**阶段3交付物**：
- ✅ 多Shard集群运行
- ✅ 性能达标
- ✅ 稳定性验证

---

## 阶段4: 备份恢复 (Week 35-40)

### Week 35-36: 备份管理器

```rust
// src/backup/manager.rs
pub struct BackupManager {
    db: Arc<DB>,
    storage: Arc<dyn BackupStorage>,
    config: BackupConfig,
}

任务：
- [ ] S3/OSS存储适配
- [ ] 快照创建
- [ ] WAL归档
- [ ] 保留策略
- [ ] 测试
```

### Week 37-38: 恢复机制

```rust
// src/backup/recovery.rs
pub struct RecoveryManager {
    storage: Arc<dyn BackupStorage>,
    target_dir: PathBuf,
}

任务：
- [ ] 快照下载
- [ ] WAL replay
- [ ] 完整恢复流程
- [ ] 测试
```

### Week 39-40: 集成测试

```rust
任务：
- [ ] 端到端备份恢复
- [ ] 故障注入
- [ ] 大数据量测试
- [ ] 灾难恢复演练
```

**阶段4交付物**：
- ✅ 异步备份正常
- ✅ 从备份恢复成功
- ✅ 容灾方案完整

---

## 阶段5: 弹性伸缩 (Week 41-44)

### Week 41-42: 动态扩展

```rust
// src/cluster/scaling.rs
pub struct ScalingManager {
    coordinator: Arc<Coordinator>,
}

任务：
- [ ] 添加Shard
- [ ] 添加Replica
- [ ] 移除节点
- [ ] 安全检查
- [ ] 测试
```

### Week 43-44: 自动伸缩（可选）

```rust
// src/cluster/autoscaler.rs
pub struct AutoScaler {
    scaling_mgr: Arc<ScalingManager>,
    config: AutoScalerConfig,
}

任务：
- [ ] 指标收集
- [ ] 伸缩策略
- [ ] 自动触发
- [ ] 测试
```

**阶段5交付物**：
- ✅ 手动伸缩工作
- ✅ 自动伸缩（可选）
- ✅ 测试通过

---

## 阶段6: 监控运维 (Week 45-48)

### Week 45-46: Prometheus监控

```rust
// src/metrics/mod.rs
pub struct Metrics {
    requests_total: Counter,
    request_duration: Histogram,
    cache_hits: Counter,
    // ...
}

任务：
- [ ] 指标定义
- [ ] 埋点
- [ ] HTTP endpoint
- [ ] Grafana dashboard
- [ ] 告警规则
```

### Week 47-48: 运维工具

```rust
// src/bin/aidb-admin.rs
任务：
- [ ] 命令行工具
- [ ] 集群管理
- [ ] 备份恢复
- [ ] 状态查询
- [ ] 文档
```

**阶段6交付物**：
- ✅ 完整监控系统
- ✅ 运维工具齐全
- ✅ 文档完善
- ✅ 生产就绪

---

## 开发流程

### 每个功能的开发步骤

1. **设计**
   - 参考RocksDB设计
   - 简化到核心需求
   - 确定API接口

2. **实现**
   - TDD：先写测试
   - 实现核心逻辑
   - 处理边界情况

3. **测试**
   - 单元测试
   - 集成测试
   - 性能测试

4. **优化**
   - Profiling分析
   - 针对性优化
   - 验证改进

5. **文档**
   - API文档
   - 示例代码
   - 设计说明

### 代码审查清单

- [ ] 功能正确性
- [ ] 测试覆盖
- [ ] 错误处理
- [ ] 性能考虑
- [ ] 代码清晰度
- [ ] 文档完整性
- [ ] Clippy通过
- [ ] 格式化

---

## 成功标准

### 功能完整性
- ✅ 单机版完整功能
- ✅ 集群分片存储
- ✅ 备份和恢复
- ✅ 弹性伸缩
- ✅ 监控告警

### 性能目标

| 阶段 | 写入 | 读取(缓存) | 读取(miss) |
|------|------|-----------|-----------|
| 单机版 | 70K ops/s | - | 140K ops/s |
| 10 shards | 700K ops/s | 5M ops/s | 300K ops/s |

### 质量目标
- 测试覆盖率 > 80%
- Clippy无警告
- 文档完整
- 性能稳定

---

## 风险和应对

### 技术风险

| 风险 | 影响 | 应对 |
|------|------|------|
| 性能不达标 | 高 | 早期基准测试，持续优化 |
| 数据一致性 | 高 | 完善测试，故障注入 |
| 时间超期 | 中 | 严格控制范围，MVP优先 |
| 复杂度失控 | 中 | 简化设计，避免过度工程 |

### 质量保证

- 每个阶段都能独立验证
- 持续集成和测试
- 定期代码审查
- 性能监控

---

## 总结

本实施计划：
- ✅ 48周完整路线
- ✅ 每周详细任务
- ✅ 清晰的交付物
- ✅ 明确的成功标准

**现在开始**：从阶段A的Day 3开始实现WAL！

更多细节参考：
- [架构设计](ARCHITECTURE.md)
- [设计决策](DESIGN_DECISIONS.md)
- [任务清单](../TODO.md)
