# AiDb 设计决策

## 存储引擎

### 为什么选择 LSM-Tree 而非 B-Tree?

LSM-Tree 在写密集场景下优势明显:
- 顺序写入 (WAL + MemTable) 替代随机写
- 后台 compaction 异步整理, 不阻塞前台
- Bloom Filter 有效控制读放大

与 RocksDB/LevelDB 思路一致, 适合 AiKv 等写负载高的服务.

### 为什么选择 Leveled Compaction 而非 Tiered/Universal?

- Leveled 读放大可控 (O(log N) 层次)
- 空间放大较小 (L 越大文件占比越高)
- LevelDB 生态验证充分, 实现参考丰富
- 适合点查为主的 workload

### WAL 格式: 为什么使用 Record 分片?

单条记录最大 32KB+ (CRC32 + type + data). 超过 block size (32KB) 时自动分片为 First/Middle/Last, 兼容大 Value 写入场景. RocksDB 同款设计, 工程验证充分.

### MemTable: 为什么选择 Crossbeam SkipMap?

- 无锁并发读写, 不阻塞 reader/writer
- `crossbeam-skiplist` 提供有序 map, freeze 后 immutable 共享安全
- 相比 `std::sync::RwLock<BTreeMap>` 写性能更好

### SSTable Block: restart points 的作用

每 16 个 key 记录一次完整 key (restart point), 其后 key 仅存储与前者的公共前缀差量. 这样:
- 空间利用率高 (短 key 压缩比显著)
- 二分查找在 restart points 上进行 (O(log(num_restarts)))
- 查到 restart point 后线性扫描, cache locality 好

## 集群

### 为什么使用 OpenRaft?

- Rust 生态最成熟的 Raft 实现
- 支持 joint consensus (成员变更安全)
- 内置 snapshot 机制
- 活跃维护, API 稳定

### 为什么控制平面 (MetaRaft) 和数据平面 (Multi-Raft) 分离?

- 控制平面变更频率低 (成员/路由/迁移状态)
- 数据平面需要高吞吐 (KV 写入)
- 分离避免控制面写入影响数据面延迟
- MetaRaft 单 Group, Multi-Raft 按 slot 拆分

### 为什么使用 gRPC (tonic) 而非自建协议?

- 成熟的序列化 (protobuf) + 传输
- tonic 原生 async, 与 tokio 集成
- 跨语言兼容 (未来可能引入其他语言节点)

### 为什么 slot 数量固定为 16384?

- Redis Cluster 兼容 (CLUSTER ADDSLOTS/NODES/SLOTS)
- 远大于节点数, 支持细粒度迁移
- hash tag `{...}` 允许用户控制 key 分布

## 备份

### 为什么基于 Checkpoint?

- LSM-Tree flush 后 SSTable 不可变, Checkpoint 通过 hardlink/copy 获得稳定快照
- 与 LSM-Tree flush-compaction 自然对齐
- 无需全局锁, compaction 可继续运行

### 为什么恢复用临时目录 + 原子 rename?

- 保证恢复操作的原子性
- 恢复中途失败不损坏已有数据
- EXDEV 跨文件系统时 fallback 到 copy + remove

## 可观测性

### 为什么选择 tracing 而非 log crate?

- `#[instrument]` 自动 span 上下文传递
- 结构化 event 支持 (字段 + 级别)
- 一个 `tracing-subscriber` 同时输出日志和 tracing 数据
- 零开销在未启用时

### 为什么 Prometheus 指标使用 Gauge/Counter/Histogram 区分类型?

- Counter: 只增不减 (操作总数, flush 次数, compaction 次数)
- Gauge: 可增可减 (SSTable 数量/大小, memtable 大小)
- Histogram: 延迟分布 (compaction 耗时, 备份耗时)
- Histogram 需关注 bucket 配置, 避免过大开销
