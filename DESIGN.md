# AiDb 设计决策

本文回答 **为什么** 这样设计: 选型理由、放弃的替代方案、已知限制. **是什么、怎么分层、数据怎么走** 见 [ARCHITECTURE.md](ARCHITECTURE.md); 实现细节与入口见 [docs/modules/](docs/modules/).

## 阅读导航

| 域 | 深入阅读 |
|----|----------|
| 写路径 / WAL / MemTable / MVCC | [engine.md](docs/modules/01-engine.md) |
| SSTable / compaction / Bloom / cache / checkpoint | [engine-storage.md](docs/modules/02-engine-storage.md) |
| MetaRaft / Multi-Raft / Router / 迁移 | [cluster.md](docs/modules/03-cluster.md) |
| 全量备份 / 恢复 | [backup.md](docs/modules/04-backup.md) |
| tracing / Prometheus | [observability.md](docs/modules/05-observability.md) |

## 产品形态与横切取舍

### 为什么是嵌入式 lib, 而不是独立服务?

AiDb 是 **lib crate**: 同步 `DB` API, 无网络 listener. [AiKv](../aikv/docs/modules/03-storage.md) 在其上实现 RESP、Cluster 重定向与 HTTP `/metrics`. 这样 LSM 与 Raft 基础设施可复用, 协议层独立演进.

### 为什么 sync API + feature gate?

- **Sync**: 存储路径以 `parking_lot` / 后台线程为主; 嵌入方 (AiKv) 用 `spawn_blocking` 桥接 async.
- **Feature**: `backup` (默认)、`cluster`、`monitoring` 按需启用; 核心 `engine` 不硬依赖 tonic/OpenRaft/Prometheus.

### 与 RocksDB / LevelDB: 借鉴什么, 避免什么?

**借鉴** (思路与算法, 非 C++ 绑定):

- LSM 分层、Leveled compaction、WAL Record 分片、MemTable flush、SSTable 块索引 + Bloom、WriteBatch 原子写、MVCC snapshot.

**避免**:

- 200+ 配置项与庞大 API surface (Column Family、多 `Get` 重载等).
- 复杂事务、特性膨胀、C++ 编译链.
- 过度抽象层 (Env/FileSystem 多级包装).

**现况**: `Options` 约 25 个可调字段 + `for_testing` / `for_high_*` preset — 相对 RocksDB 仍精简; 注释「≤20 项」为早期目标, 已随 stall/subcompaction 等调优项扩展. 公共 API 控制在较小 surface (`lib.rs` 注释: ≤30 函数量级).

### 刻意不实现 (YAGNI)

| 项 | 说明 |
|----|------|
| Column Family / 多 CF | 单 keyspace; aikv 用 `{db_index}:` 前缀区分逻辑库 |
| 复杂事务 | 仅 WriteBatch + Snapshot MVCC |
| ThinReplication | oldmain 曾探索; **当前全量 Raft log 复制**; inventory 列为未来考量 |
| 内置 HTTP `/metrics` | 见 [可观测性](#可观测性) 与 ISSUE-014 |
| `aidb-admin` CLI | 用库 API 或 `examples/` |

### 磁盘格式

与早期 `aidb-oldmain` **不兼容** (WAL 逻辑、InternalKey SST、Bloom/Block CRC 语义等已演进). 不做向后读取; 详情见 engine / engine-storage 模块「已知限制」.

---

## 存储引擎

### 为什么选择 LSM-Tree 而非 B-Tree?

写密集场景下顺序写 (WAL + MemTable) 优于随机写; 后台 compaction 异步整理; Bloom Filter 控制读放大. 与 LevelDB/RocksDB 路线一致, 适合 AiKv 等高写负载嵌入.

### 为什么选择 Leveled Compaction 而非 Tiered/Universal?

- Leveled 点查读放大可控 (O(log N) 层次).
- 空间放大相对 Tiered 更小.
- LevelDB 生态验证充分. **放弃** Universal/Tiered 多种策略并存 — 只维护一种 compaction.

### WAL 格式: 为什么使用 Record 分片?

物理 record 含 CRC32 + length + type + data. 超过 block size (~32KB) 时拆为 First/Middle/Last, 兼容大 value. RocksDB 同款, 工程验证充分. 逻辑层 `WalEntry` + `BatchStart` 保证 batch 崩溃原子性 (见 [engine.md](docs/modules/01-engine.md)).

### MemTable: 为什么选择 Crossbeam SkipMap?

无锁并发读写; freeze 后 immutable 可安全共享; 相比 `RwLock<BTreeMap>` 写路径更轻. oldmain 已采用 SkipMap; 重构延续该决策.

### SSTable Block: restart points 的作用

默认每 16 个 key 一个 restart point (完整 key), 其余存前缀差量 — 省空间; restart 点上二分 + 块内线性扫描, cache locality 好. 细节见 [engine-storage.md](docs/modules/02-engine-storage.md).

### Block Cache: 为什么 16-shard LRU?

重构新增: 总容量均分 16 shard, hash 选 shard, 降低单锁竞争. **trade-off**: hash 偏斜时单 shard 先满, 不借用其它 shard 配额.

### Write stall: 为什么 L0 堆积时 sleep / 停写?

重构新增: `level0_slowdown_writes_trigger` / `level0_stop_writes_trigger` 在 L0 文件过多时渐进 sleep 或轮询等待 compaction — 避免 L0 无限增长导致读放大失控. **放弃** 无界堆 L0.

### Subcompaction: 为什么大 job 按 key range 并行?

重构新增: 输入超过 `subcompaction_min_size` (默认 64MB) 且多线程可用时, `std::thread::scope` 分裂子任务 — 缩短大 compaction 墙钟时间. `0` 禁用.

### MVCC 与语义取舍

- `get` 用当前 sequence; `iter`/`scan` 用 `K_MAX_SEQUENCE` — **intentional**, 见 engine 模块.
- 并发 `get` 可能在 WriteBatch 写入 MemTable 期间看到 batch **部分** 效果 (LevelDB 一致); Snapshot 在 `write_lock` 下创建, 无此问题.

---

## 集群

### 为什么 Multi-Raft, 而非单 Raft / P2P / Paxos?

| 方案 | 一致性 | 扩展性 | 复杂度 | 结论 |
|------|--------|--------|--------|------|
| 单 Raft | 强一致 | 差 (单 Leader 瓶颈) | 低 | 不适合水平扩展 |
| **Multi-Raft** | 强一致 | 好 (多 Group 并行 Leader) | 中 | **选用** |
| 无共识 P2P | 最终一致 | 好 | 低 | 不满足强一致目标 |
| Paxos | 强一致 | 中 | 高 | Rust 生态与工程成本偏高 |

OpenRaft 提供成熟 Raft + joint consensus + snapshot; 与 16384 slot 分片模型配合, 兼容 Redis Cluster **槽** 语义 (协议命令在 AiKv).

### 为什么 OpenRaft?

Rust 生态最成熟的 Raft 实现之一; joint consensus 安全成员变更; 内置 snapshot; API 与社区活跃.

### 为什么控制平面 (MetaRaft) 和数据平面 (Multi-Raft) 分离?

- MetaRaft (`group_id = 0`): 节点、Group、SlotTable、迁移状态 — 变更低频.
- Multi-Raft (`group_id ≥ 1`): 每 Group 独立 `ShardedStorage` + `OpenRaftNode`, 目录 `data/group_{id}/` — 数据面高吞吐.
- 分离避免元数据写入与 KV propose 争用同一 Raft 队列.

### 为什么每个节点可参与多个 Group?

3 节点 × 3 Group 时, Leader 可分布在不同节点 — 提高 CPU/磁盘利用率; 单节点故障只影响其 Leader 的 Group, 非整集群瘫痪.

### 状态机 key: 为什么前缀隔离?

用户 key 经 `sm_key(group_id, user_key)` 编码为 `\x01sm/{gid}/...`; Raft 元数据 `\x00raft/{gid}/...`. 与用户 keyspace、Raft 日志键分离, 便于 scan/snapshot/调试. 演进自 oldmain 的 `sm:` 字符串前缀, 现按 Group 二进制隔离.

### 为什么 gRPC (tonic) 而非自建协议?

protobuf + HTTP/2 成熟; tonic 与 tokio 集成; grpcurl 等工具可调试; 跨语言节点扩展成本低. 自定义二进制协议的性能优势不足以抵消开发与生态成本.

### 为什么 slot 数量固定为 16384?

Redis Cluster 兼容槽模型 (`CLUSTER SLOTS` / hash tag `{...}`); 槽数远大于典型节点数, 支持细粒度迁移; CRC16 `% 16384` 计算高效.

### aidb 与 AiKv 分工

| 能力 | AiDb | AiKv |
|------|------|------|
| Slot 路由 / Raft propose | ✅ | 调用 aidb cluster API |
| `SlotStatus` / `NotLeader` | ✅ | — |
| MOVED / ASK / CLUSTER 子命令 | — | ✅ |
| HTTP `/metrics` | `register_into` only | ✅ 暴露 |

### 已知限制 (摘要)

- **无 ThinReplication**: 全量 Raft log 复制.
- **无跨 Group `write_batch`**: 调用方 `Router::group_ops` 分组后逐 Group `propose`.
- **Migrating 为 slot 级 ASK**, 非 per-key.
- 数据 Group apply 逐 entry 更新 `last_applied` — 见 [ISSUES.md#ISSUE-005](ISSUES.md#issue-005--数据-group-apply-仍逐-entry-写-last_applied).

---

## 备份

### 为什么基于 Checkpoint?

LSM flush 后 SST 不可变; `Checkpoint::create` 在 flush + pin SST 后 link/copy 目录快照 — 与 compaction 协议对齐 (checkpoint 期间阻止危险 compaction). 重构将 Checkpoint 提升为一等模块; oldmain 备份直拷贝, 现强制经 Checkpoint, 一致性边界更清晰.

### 为什么恢复用临时目录 + 原子 rename?

写到 `restore_tmp_*` → 逐文件 SHA256 → `DB::open` 冒烟 → `rename` 到目标 — 中途失败不损坏已有数据. 跨文件系统 `rename` 失败 (EXDEV) 时 fallback `copy_dir_all`.

### trade-off: 全量 only

**放弃** 增量、压缩、远程 S3、backup_id 碰撞重试 — 实现与运维简单; 大库备份 I/O 成本高 (含 checkpoint 后二次 copy, 见 backup 模块). 集群多 Group 协调备份在 AiKv `cluster_adapter`, 非 aidb 单 `DB` API.

---

## 可观测性

### 为什么选择 tracing 而非 log crate?

`#[instrument]` 传递 span 上下文; 结构化 field; 与 `tracing-subscriber` 统一; 未订阅时零开销. **始终编译**, 不依赖 `monitoring` feature.

### 为什么 Prometheus 区分 Counter / Gauge / Histogram?

- **Counter**: 只增 (操作次数, flush/compaction 次数).
- **Gauge**: 可增减 (memtable/SST 大小, WAL 字节).
- **Histogram**: 延迟分布 (操作/compaction/备份耗时); bucket 需控制 cardinality.

### 为什么库内注册、无内置 HTTP?

`monitoring` feature 启用 `aidb::metrics` 与 `register_into(registry)` — 嵌入方 (AiKv) 将 aidb 系列挂到同一 Prometheus registry 并在 HTTP 暴露. oldmain 的 `MetricsServer` 已移除; **职责分离**: 库只产出指标, 进程决定 scrape 端点.

**放弃 / 精简**: 旧设计多项指标未实现 (`wal_sync_duration`, `cache_hit_rate` gauge 等); 无进程级 memory/disk — 见 [observability.md](docs/modules/05-observability.md).

---

## 决策总表

| 决策 | 选择 | 理由 | 放弃 / 限制 |
|------|------|------|-------------|
| 产品形态 | 嵌入式 lib | 复用 LSM/Raft; 协议在 AiKv | 非独立 DB 服务 |
| 存储模型 | LSM-Tree | 写密集、顺序 I/O | B-Tree 随机写 |
| Compaction | Leveled | 点查读放大可控 | Tiered/Universal |
| MemTable | Crossbeam SkipMap | 无锁并发 | RwLock+BTreeMap |
| WAL | Record 分片 | 大 value | 单 record 上限 |
| Block Cache | 16-shard LRU | 降锁竞争 | 跨 shard 不借配额 |
| L0 背压 | write stall | 控 L0 堆积 | 无界 L0 |
| 大 compaction | subcompaction | 并行缩短耗时 | `min_size=0` 禁用 |
| 共识 | OpenRaft | 成熟 Raft + snapshot | 自研 Paxos |
| 集群拓扑 | MetaRaft + Multi-Raft | 控制/数据分离 | 单全局 Raft |
| RPC | gRPC (tonic) | 生态与工具 | 自建二进制 |
| 分片 | 16384 slot | Redis 槽兼容 | 动态改槽数 |
| SM 隔离 | `\x01sm/{gid}/` 前缀 | 命名空间 + 多 Group DB | 裸 user key 进 Raft DB |
| 复制 | 全量 Raft log | 实现简单、正确性优先 | ThinReplication |
| 备份 | Checkpoint 全量 | 与 LSM 对齐 | 增量/S3/CLI |
| 指标 | tracing + Prom 注册 | 库进程分离 | 内置 HTTP scrape |

---

## 进一步阅读

- [ARCHITECTURE.md](ARCHITECTURE.md) — 分层、数据流、feature 边界
- [AGENTS.md](AGENTS.md) — AI 助手入口与参考项目
- [docs/modules/](docs/modules/) — 域级实现与常见任务
- [DEPLOYMENT.md](DEPLOYMENT.md) — 构建、feature、运行 (步 15)
- [ISSUES.md](ISSUES.md) — 待核实与跟踪

## 已知限制 (根文档摘要)

- 数据 Group apply 逐 entry 写 `last_applied` — 见 [ISSUES.md#ISSUE-005](ISSUES.md#issue-005--数据-group-apply-仍逐-entry-写-last_applied).

## 待核实

- HTTP `/metrics` 与 OTel 运行在嵌入方 (AiKv), 非 aidb 库内 — 见 [ISSUES.md#ISSUE-014](ISSUES.md#issue-014--httpoteljson-log-运行在嵌入方-aidb-仅库内指标).
