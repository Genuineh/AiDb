# AiDb — 待核实与问题跟踪

> 位于 aidb 仓库根目录. module 内 **一行引用** 本文件条目 (见 `AiKv-Workflow/backup/design.md` 模板).

**图例**: 状态 = `open` | `confirmed-bug` | `doc-only` | `closed`

---

## 如何使用

1. 文档整理 **步 2–3** 发现设计偏离、实现疑点、oldmain 行为差异时, 在此新增条目.
2. 在对应 module 的 **「待核实」** 小节写: `见 ISSUES.md#ISSUE-NNN — 一句话`
3. 文档整理 **不阻塞** 于修复; 确认要修的 bug 另开开发任务.
4. 关闭条目时更新状态, 必要时回写 module 删除或改写引用.

整理流程中新增 ISSUES 条目, 须在 **步 2–3 确认门控** 内讨论后再写入.

---

## 条目模板 (复制后填写)

```markdown
### ISSUE-NNN: 标题

- **状态**: open
- **发现于**: PROGRESS 步 N / 章节 `docs/modules/xxx.md`
- **相关 src**: `src/...`
- **旧文档**: `aidb-oldmain/docs/...` (可选)
- **oldmain 代码**: `aidb-oldmain/src/...` (可选)
- **现象**: 当前实现 vs 旧设计/旧代码 的差异
- **影响**: 文档应如何描述 / 是否可能是 bug
- **下一步**: 待核实 | 需写测试 | 需开 issue 修代码
```

---

## 条目列表

<!-- 按 ISSUE-NNN 倒序追加 -->

### ISSUE-020: SSTable 压缩 block CRC 损坏 (P0)

- **状态**: closed (fixed 2026-07-02, commit `702b929`)
- **发现于**: 生产集群性能优化排查 (`aidb::config::Options::default()` 默认 `CompressionType::Snap`)
- **相关 src**: `src/engine/sstable/builder.rs` (`SSTableBuilder::flush_data_block`), `src/engine/sstable/block_io.rs` (`write_block`/`compress_block`/`decompress_block`)
- **现象**: `flush_data_block` 用压缩**前** `block_data.len()` 计算 `BlockHandle.size` 并累加 `data_block_offset`, 但落盘的是压缩**后**的 payload; 只要一张 SST 出现第 2 个 data block, 读侧就按错误偏移拆 trailer, 读到垃圾 `compression_type`/`crc` 字段, 报 `Error::Corruption("block CRC mismatch")`. 同一批代码里还有一个被这个 bug 掩盖的独立问题: LZ4 compress 用 `prepend_size=false` 写入却用 `decompress(data, None)` 解压 (要求前缀带长度), 二者不匹配导致 LZ4 解压必然失败.
- **影响**: `Options::default()`/`for_high_write_throughput()` 默认启用 Snap 压缩, 是生产/aikv 默认路径; 此前所有 SSTable 测试都只覆盖 `CompressionType::None`, 从未真正测过多 block 场景下的 Snap/Lz4 读写, 掩盖了两个 bug 长期存在.
- **修复**: `write_block` 改为返回实际写入的 payload 长度, 调用方 (`flush_data_block` 及 `finish()` 中 meta_index/index block) 均改用该返回值计算 `handle.size`; LZ4 统一为 `prepend_size=true` 配 `decompress(_, None)`.
- **回归**: `tests/modules/sstable/function.rs::test_multi_block_read_with_snap_compression` / `test_multi_block_read_with_lz4_compression`; 另在本地 6 节点集群用修复后镜像跑压测, 验证真实落盘的 ~12000 block/文件 SST 读写无 `Corruption`.
- **下一步**: 已关闭. 详见 `CHANGELOG.md` Unreleased/Fixed.

### ISSUE-019: Block Cache capacity 未导出 OTel gauge

- **状态**: closed
- **发现于**: Grafana 面板重设计 / `docs/modules/observability.md`
- **相关 src**: `src/metrics.rs`, `src/engine/cache/block_cache.rs`
- **现象**: `DB::block_cache_capacity()` API 存在; 仅有 `aidb_block_cache_size_bytes`, 无 capacity gauge
- **修复**: 新增 `aidb_block_cache_capacity_bytes`; `BlockCache::new` 写入配置容量
- **影响**: Grafana Engine 可画 cache 使用率 (size/capacity)

### ISSUE-018: scan/close 未计入 aidb_operations_total

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/observability.md`
- **相关 src**: `src/engine/db/inner.rs` (`scan`, `close`)
- **旧文档**: `backup/aidb/DEPLOYMENT.md` §可观测性 — 列 `put/get/delete/scan/close`
- **现象**: `record_operation` 覆盖 put/get/delete/write_batch/snapshot/stall_*; `db_scan`/`db_close` 有 span 但无 counter
- **影响**: module 指标表不写 scan/close; 或已知限制一句
- **下一步**: 已关闭 (doc-only)

### ISSUE-017: compaction 指标 counter/histogram label 名不一致

- **状态**: fixed (2026-06-24)
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/observability.md`
- **相关 src**: `src/metrics.rs`
- **现象**: counter 曾用 `type`, histogram 用 `phase`
- **修复**: 统一为 `aidb.compaction.phase` (PromQL: `aidb_compaction_phase`)
- **下一步**: 已关闭

### ISSUE-016: 旧设计若干 Prometheus 系列未实现

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/observability.md`
- **相关 src**: `src/metrics.rs`
- **旧文档**: `backup/aidb/docs/observability.md` Metrics 表; `aidb-oldmain/docs/monitoring/MONITORING_GUIDE.md`
- **oldmain 代码**: `aidb-oldmain/src/monitoring/metrics.rs` — 独立 requests/errors/cluster 指标
- **现象**: 现码无 `wal_sync_duration`, `cache_hit_rate` gauge, `snapshot_count`, `cluster_nodes`, `errors_total`, `restore_duration` 等
- **影响**: 已知限制; Dashboard 用 PromQL 派生 (如 hit rate) 或链 aikv 指标
- **下一步**: 已关闭 (doc-only; OTel 迁移后, 未实现指标的已知限制见 `docs/modules/05-observability.md`)

### ISSUE-015: 旧 observability 指标表与 span 名大量过时

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/observability.md`
- **相关 src**: `src/metrics.rs`, 各模块 `#[instrument]`
- **旧文档**: `backup/aidb/docs/observability.md` §2 矩阵
- **现象**: 旧稿 `aidb_puts_total`、`compaction_pick`、`raft_propose`、`memtable_insert` 等与现 `aidb_operations_total{op}`、`cmp_pick`、`meta_propose`、`mem_put` 不符
- **影响**: 正文以现码 grep 为准; 不回迁旧表
- **下一步**: 已关闭 (doc-only)

### ISSUE-014: HTTP/OTel/JSON log 运行在嵌入方, aidb 仅库内指标

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/observability.md`
- **相关 src**: `src/metrics.rs` (`init` / `init_otel`); `aikv/src/server/otel.rs`
- **旧文档**: `backup/aidb/DEPLOYMENT.md` §可观测性 (`--metrics-port`, `AIDB_OTLP_ENDPOINT`); `backup/aidb/docs/observability.md` OTel 拓扑
- **oldmain 代码**: `aidb-oldmain/src/monitoring/{server,metrics}.rs` — 内置 MetricsServer + Collector
- **现象**: 现 aidb 无 HTTP 端点、无 OTel Layer 初始化、无 `AIDB_*` env; `monitoring` 依赖含 opentelemetry 但接线在嵌入方; aikv `otel.rs` 设 global `MeterProvider` 后调 `aidb::metrics::init()`, 与 `aikv_*` 共用 OTLP 出口
- **影响**: module 写库侧职责边界; HTTP/OTel 链 aikv observability (步 12)
- **下一步**: 已关闭 (doc-only)

### ISSUE-013: list_backups 与 get_backup_info 对损坏 manifest 行为不一致

- **状态**: doc-only
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/backup.md`
- **相关 src**: `src/backup/manager.rs` (`list_backups`, `get_backup_info`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/13-backup-bench.md` §1 — 未区分两种查询路径
- **现象**: `list_backups` 遇损坏 manifest 仅 `tracing::warn` 并跳过; `get_backup_info` 返回 `Error::Corruption`
- **影响**: module「已知限制」一句; 非 bug
- **下一步**: 已关闭 (doc-only)

### ISSUE-012: 无 backup_id 碰撞重试与压缩/增量/S3

- **状态**: doc-only
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/backup.md`
- **相关 src**: `src/backup/manager.rs`, `src/backup/storage.rs`
- **旧文档**: `WiQunTools/.../13-backup-bench.md` §1 (碰撞 while 循环)、「未来考量」; `aidb-oldmain/src/backup/metadata.rs` (`BackupType::Incremental`)
- **现象**: 现码 `timestamp_nanos()` 单次取值无重试; 无压缩、增量备份、S3 等远程 `BackupStorage` 实现
- **影响**: module「已知限制」列举; 非阻塞文档
- **下一步**: 已关闭 (doc-only)

### ISSUE-011: 创建路径为 Checkpoint 组合而非 inventory 直连 pin_sstables

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/backup.md`
- **相关 src**: `src/backup/manager.rs` (`create_backup_with_description`), `src/engine/checkpoint/mod.rs`
- **旧文档**: `WiQunTools/.../13-backup-bench.md` §1.3 — 逐步 `pin_sstables` + 分 SST/MANIFEST 复制
- **oldmain 代码**: `aidb-oldmain/src/backup/manager.rs` — `list_sstable_files` / `list_wal_files` 分目录复制, 无 Checkpoint
- **现象**: 现码 `Checkpoint::create` 得全目录快照后再 `BackupStorage::store` 到 `backup_{id}`; 二次 I/O
- **影响**: 正文写实际流程并链 engine-storage; 已知限制可提双重复制开销
- **下一步**: 已关闭 (doc-only)

### ISSUE-010: MembershipCoordinator 无空节点 60s 超时清理

- **状态**: doc-only
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/cluster.md`
- **相关 src**: `src/cluster/membership_coordinator.rs`
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/12-cluster-ops.md` — add_node 后 60s 无 Group 则 RemoveNode
- **现象**: inventory 描述超时清理; 现码 `add_node` 注册后即返回, 无后台 spawn
- **影响**: module 已知限制一句; 非阻塞文档
- **下一步**: 已关闭 (doc-only)

### ISSUE-009: router.rs CRC 注释与 Redis 向量不一致

- **状态**: doc-only
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/cluster.md`
- **相关 src**: `src/cluster/router.rs`, `tests/modules/multi_raft/unit.rs`
- **现象**: 源码注释写 CRC16-CCITT; 测试 `crc16(b"123456789")==0x31C3` 为 Redis/XMODEM 标准向量, 行为与 Redis Cluster 一致
- **影响**: module 写「Redis 兼容 slot 算法」, 不复制注释歧义
- **下一步**: 已关闭 (doc-only)

### ISSUE-008: get_ttl_from_group 恒返回 None

- **状态**: doc-only
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/cluster.md`
- **相关 src**: `src/cluster/multi_raft_node.rs` (`get_ttl_from_group`), `src/cluster/slot_migration.rs` (`verify_migration`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/12-cluster-ops.md` — 迁移验证含 TTL 对比
- **现象**: 函数注释「AiDb 引擎层不支持逐 key TTL」; verify 中 TTL 分支为 no-op
- **影响**: module 已知限制; 迁移验证仅比对 value
- **下一步**: 已关闭 (doc-only)

### ISSUE-007: 无 MultiRaftNode 级 write_batch / resolve_ask_redirect

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/cluster.md`
- **相关 src**: `src/cluster/multi_raft_node.rs`, `src/cluster/router.rs` (`group_ops`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/11-multi-raft.md`
- **oldmain 代码**: oldmain MultiRaftNode 亦无完整 inventory 伪代码级 API; 跨 Group batch 由调用方 + Router 分组
- **现象**: inventory 描述 `MultiRaftNode::write_batch`; 现码 batch 在 `OpenRaftNode` + `Router::group_ops`, 无 MultiRaft 聚合入口
- **影响**: aikv 层自行按 Group 分组 propose; module 说明边界即可
- **下一步**: 已关闭 (doc-only)

### ISSUE-006: Migrating/ASK 重定向不在 aidb MultiRaftNode 内完成

- **状态**: doc-only
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/cluster.md`
- **相关 src**: `src/cluster/multi_raft_node.rs` (`propose_key`, `get_key`), `src/cluster/router.rs` (`route_key` 返回 `SlotStatus`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/11-multi-raft.md` — `resolve_ask_redirect` + `NotLeader { is_ask: true }`
- **oldmain 代码**: oldmain 亦无 is_ask 字段 (旧 `ClusterError` 为字符串)
- **现象**: aidb 路由到 Migrating slot 的 source group 后直 propose; **不**填充 `ClusterError::NotLeader.is_ask`. MOVED/ASK 由 aikv `cluster` 模块读 `SlotStatus` + `migration_state` 实现 (步 11)
- **影响**: module 明确 aidb/aikv 分工; 非 aidb bug
- **下一步**: 已关闭 (doc-only); aikv cluster.md 覆盖

### ISSUE-005: 数据 Group apply 仍逐 entry 写 last_applied

- **状态**: closed
- **发现于**: PROGRESS 步 2–3 / 章节 `docs/modules/cluster.md`
- **相关 src**: `src/cluster/storage/apply.rs` (`apply_entries_internal`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/09-raft.md` — ⚠️ 数据 Group 原子 WriteBatch 待统一
- **oldmain 代码**: `raft_storage.rs` 同为逐 entry apply (与现码同类)
- **现象**: Meta 分支已单 WriteBatch 原子 (apply_meta_entry); 数据 Group 每条 entry 单独 `persist_last_applied` + SM 写入
- **影响**: 崩溃窗口与 inventory 警告一致; module 待核实一行
- **修复**: 数据 Group / Membership / Blank 逐 entry 单 WriteBatch (SM ops + last_applied); 对齐 `apply_meta_entry`
- **回归**: `tests/modules/cluster/group_apply_batch.rs`, `src/cluster/storage/apply.rs` unit tests
- **下一步**: 已关闭

### ISSUE-004: inventory 称 compaction 不保护 Snapshot

- **状态**: closed
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/engine-storage.md`
- **相关 src**: `src/engine/compaction/job.rs`, `src/engine/db/snapshot.rs`, `src/engine/db/inner.rs` (`run_compaction_once`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/05-compaction.md`, `08-snapshot.md` — 「不保护 / 弱化语义 / 预留分支」
- **现象**: 当前 `SnapshotList::min_snapshot_sequence` → `CompactionJob::with_snapshot_threshold`, dedup 时 `snapshot_protected` 保留旧版本
- **影响**: 旧 inventory 设计决策已过时; 代码无 bug. `engine-storage.md` 已写现行保护语义
- **下一步**: 已关闭 (doc-only)

### ISSUE-003: inventory 仍写 Block 压缩未实现

- **状态**: closed
- **发现于**: PROGRESS 步 2 / 章节 `docs/modules/engine-storage.md`
- **相关 src**: `src/engine/sstable/block_io.rs`, `Cargo.toml` feature `compression`
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/03-sstable.md` — `known_limitation`
- **现象**: inventory 称 Snap/LZ4 未接线; 源码在 `compression` feature 下已实现读写
- **影响**: 旧 inventory limitation 已过时; 代码无 bug. `engine-storage.md` 已写 feature gate
- **下一步**: 已关闭 (doc-only)

### ISSUE-002: 大 WriteBatch 与 max_wal_size 轮转交互

- **状态**: closed
- **发现于**: PROGRESS 步 1 / 章节 `docs/modules/engine.md` (步 2)
- **相关 src**: `src/engine/db/inner.rs` (`DB::write`), `src/engine/wal/manager.rs` (`append` → `rotate`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/01-wal.md` — batch 超 `max_wal_size` 时禁止 rotate
- **现象**: inventory 规定大 batch 可临时超过文件上限且写入期间禁止 rotate; 当前 `append` 每条后检查 `max_wal_size` 并可能 rotate, 无 batch 临界区
- **影响**: 与 ISSUE-001 同源
- **修复**: `ensure_space_for_batch` 预 rotate; `append_in_batch` 写入期间禁用 auto-rotate
- **回归**: `tests/modules/wal/write_batch_boundary.rs`
- **下一步**: 已关闭

### ISSUE-001: WriteBatch 可能跨 WAL 文件边界

- **状态**: closed
- **发现于**: PROGRESS 步 1 / 章节 `docs/modules/engine.md` (步 2)
- **相关 src**: `src/engine/db/inner.rs` (`DB::write`), `src/engine/wal/manager.rs` (`append` → `rotate`)
- **旧文档**: `WiQunTools/docs/wiqun-db-inventory/01-wal.md` — 「Batch 不跨 WAL 文件」
- **现象**: `write()` 循环 `wal.append()`; `append` 在 `size >= max_wal_size` 时自动 `rotate`, batch 中途可切文件. recover 按**文件**独立追踪 batch 边界, 跨文件 batch 语义未定义
- **影响**: 崩溃恢复可能部分 replay batch (已修复)
- **修复**: 同 ISSUE-002 — batch 预检空间 + `append_in_batch` 禁用 mid-batch rotate
- **回归**: `tests/modules/wal/write_batch_boundary.rs`, `tests/engine/wal_write_batch_boundary.rs`
- **下一步**: 已关闭
