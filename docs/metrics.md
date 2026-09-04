---
name: aidb-metrics
description: AiDb 全量 OTel 指标总表 (仅当前已存在指标). 查指标名, 标签与基数, 类型, 单位, 数据源, 代码位置与典型 PromQL 时读本文; 改埋点前定位调用点也读本文.
---

# AiDb Metrics (指标总表)

## 何时读本文

- 查询 `aidb_*` 指标的名称, 标签与基数, 类型, 单位, 埋点位置与典型 PromQL
- bench / 监控面板选指标, 或改埋点前定位代码
- **不覆盖**: tracing span 索引, `metrics::init` 嵌入机制 → [observability.md](modules/05-observability.md)
- **不覆盖**: 待新增指标 (缺口清单) 与复合指标计算方式 → 工作区根目录 `bench-metrics-audit.md`

## 命名口径

- 指标名统一使用 **Prometheus 渲染名** (下划线形式); OTLP 原生属性为点号形式 (`aidb.operation.name`, `db.client.operations` 等), bench 直查 OTLP 后端 (非经 Collector 转 Prometheus) 时名称不同.
- `db.client.*` 为 OTel semconv 双写指标, 其 Prometheus 渲染名 (是否追加 `_total` / 单位后缀) 取决于 Collector 转换配置, 以实际导出为准.
- 类型统一按 Prometheus 口径标注 (Counter / Gauge / Histogram); 单位为代码中声明的 UCUM 单位.
- 全部指标需 `--features monitoring` 编译; 集群指标额外需 `--features cluster`.
- aidb 为嵌入库, 无独立 OTLP 出口; 核心指标体系采用 Atomic-first 架构, 引擎热路径无锁直写实例级原子字典 (`Arc<Statistics>`), 消除全部全局读锁争用与堆内存分配. OTel instrument 集中于 `src/metrics.rs` (`OtelMetrics`), 由嵌入方 (如 aikv) 在后台定时调用 `sync_to_otel` 执行差分同步并导出.
- 表格列说明:
  - **标签与基数**: 标注关键标签名, 枚举值范围及预期基数, 便于防范高基数风险.
  - **数据源与代码位置**: 保留准确的更新方式与源码行号, 便于研发定位调用与开销.
  - **说明 (实现陷阱与口径)**: 重点记录采样失真, compaction 污染缓存, 统计盲区等底层技术陷阱.
  - **用途与典型 PromQL**: 提供直接可用的 PromQL 表达式与监控/压测用途.

## 引擎指标

| 指标名 | 类型 | 单位 | 标签与基数 | 数据源与代码位置 | 说明 (实现陷阱与口径) | 用途与典型 PromQL |
| --- | --- | --- | --- | --- | --- | --- |
| `aidb_wal_size_bytes` | Gauge | By | — (基数 1) | WAL 追加与轮转/清理时刷新原子值<br>`stats.wal_size_bytes`; `engine/wal/manager.rs` | WAL 当前文件大小 | 空间放大 (SA) 分子组成; LSM 形态观测:<br>`aidb_wal_size_bytes` |
| `aidb_memtable_size_bytes` | Gauge | By | `aidb_memtable_state(2: active\|frozen)`<br>基数 2 | MemTable 写入与冻结时原子统计<br>`stats.memtable_size_bytes`; `engine/memtable/table.rs` | 近似大小 (user_key + value 字节) | 内存驻留与刷盘水位观测:<br>`sum by (aidb_memtable_state) (aidb_memtable_size_bytes)` |
| `aidb_sstable_count` | Gauge | 1 | `aidb_sstable_level(7: 0~6)`<br>基数约 7 | DB open / flush 完成 / compaction apply 后现算<br>`stats.sstable_count`; `engine/db/inner/mod.rs` (`update_sstable_metrics`) | 各层 SST 文件数 | L0 文件数 write stall 风险观测:<br>`aidb_sstable_count{aidb_sstable_level="0"}` |
| `aidb_sstable_size_bytes` | Gauge | By | `aidb_sstable_level(7: 0~6)`<br>基数约 7 | 同上<br>同上 | 各层 SST 总大小 | 数据规模与 SA 分子:<br>`sum(aidb_sstable_size_bytes)` |
| `aidb_operations_total` | Counter | 1 | `aidb_operation_name(8: put\|delete\|write_batch\|write_batch_no_wal\|get\|snapshot\|stall_stop\|stall_slowdown)`<br>基数 8 | 引擎热路径原子直写 + `sync_to_otel` 差分同步<br>`stats.operations`; `engine/db/inner/write.rs`, `read.rs` | `stall_*` 与常规操作混在同一指标族, 且仅覆盖 L0 文件数 stall | 引擎各操作 QPS:<br>`sum by (aidb_operation_name) (rate(aidb_operations_total[1m]))`<br>Stall 请求比率素材 |
| `aidb_operation_duration_seconds` | Histogram | s | `aidb_operation_name(同上)`<br>基数 8 | 引擎热路径原子直写 + `sync_to_otel` 差分同步<br>`stats.operation_durations`; 同上 | 引擎层分位数真实; 桶边界 0.0001~1.0 s (超 1 s 落 +Inf); **scan 无埋点, snapshot 只记计数不记时长** | 引擎层真实 P99 延迟 (无 15 s 窗口平均):<br>`histogram_quantile(0.99, sum by (le, aidb_operation_name) (rate(aidb_operation_duration_seconds_bucket[1m])))` |
| `aidb_flush_total` | Counter | 1 | — (基数 1) | flush 完成时原子累加<br>`stats.flush_total`; `engine/db/inner/flush.rs` | 按轮计数 (每轮刷掉的 immutable memtable 批次 +1) | Flush 执行频率:<br>`rate(aidb_flush_total[1m])` |
| `aidb_flush_duration_seconds` | Histogram | s | — (基数 1) | flush 执行耗时原子直写<br>`stats.flush_duration`; `flush.rs` | 按**单个** memtable→SSTable 记录, 含空表; 桶边界 0.001~5.0 s | Flush 耗时分布 / Write stall 归因:<br>`histogram_quantile(0.95, rate(aidb_flush_duration_seconds_bucket[5m]))` |
| `aidb_block_cache_size_bytes` | Gauge | By | — (基数 1) | BlockCache 插入/淘汰时更新原子值<br>`stats.block_cache_size`; `engine/cache/block_cache.rs` | 16 分片 LRU 当前占用 | 缓存内存占用 (MB):<br>`aidb_block_cache_size_bytes / 1024 / 1024` |
| `aidb_block_cache_capacity_bytes` | Gauge | By | — (基数 1) | BlockCache 构造时设置<br>`stats.block_cache_capacity`; `block_cache.rs` | 总容量上限 | 缓存水位 (使用率 %):<br>`aidb_block_cache_size_bytes / aidb_block_cache_capacity_bytes * 100` |
| `aidb_block_cache_hits_total` | Counter | 1 | — (基数 1) | BlockCache 读取命中原子累加<br>`stats.block_cache_hits`; `block_cache.rs` | **命中率口径被 compaction 污染**: compaction 经共享 BlockCache 顺序扫全文件, miss 计入分母且淘汰用户热块 | 读缓存命中率分子:<br>`rate(aidb_block_cache_hits_total[1m]) / (rate(aidb_block_cache_hits_total[1m]) + rate(aidb_block_cache_misses_total[1m]))` |
| `aidb_block_cache_misses_total` | Counter | 1 | — (基数 1) | BlockCache 读取未命中原子累加<br>`stats.block_cache_misses`; `block_cache.rs` | 同上 | 读缓存未命中率分母项 (同上) |
| `aidb_bloom_false_positive_total` | Counter | 1 | — (基数 1) | Bloom 假阳性穿透判定原子累加<br>`stats.bloom_false_positive`; `engine/sstable/reader.rs` | 仅 FP 分子, 无 TN 分母 (FPR 不可精确计算); 活跃 Range Tombstone 下点查走 `point_state()` 无 bloom 检查 | Bloom 假阳性穿透频次:<br>`rate(aidb_bloom_false_positive_total[1m])` |
| `aidb_sequence` | Gauge | {sequence} | — (基数 1) | DB open 与写路径原子设置<br>`stats.sequence`; `engine/db/inner/mod.rs`, `write.rs` | 最新已分配的全局 Sequence | 写入速率交叉验证:<br>`rate(aidb_sequence[1m])` |
| `aidb_total_key_count` | Gauge | {key} | — (基数 1) | put / delete / write_batch 后原子更新<br>`stats.total_key_count`; `write.rs` | 近似存活 key 数 (AtomicUsize 同源计数, 不持久化) | 逻辑数据规模参考 / SA 分母素材:<br>`aidb_total_key_count` |
| `aidb_compaction_total` | Counter | 1 | `aidb_compaction_phase(3: pick\|run\|apply)`<br>基数 3 | Compaction 各阶段原子累加<br>`stats.compaction_phases`; `engine/db/inner/compaction.rs` | **trivial move 只记 `pick` 阶段** (无 run/apply) | Compaction 执行频次:<br>`sum by (aidb_compaction_phase) (rate(aidb_compaction_total[1m]))` |
| `aidb_compaction_duration_seconds` | Histogram | s | `aidb_compaction_phase(同上)`<br>基数 3 | Compaction 各阶段耗时原子记录<br>`stats.compaction_durations`; 同上 | 桶边界 0.001~5.0 s | Compaction 耗时分布 / 后台 I/O 归因:<br>`histogram_quantile(0.95, sum by (le, aidb_compaction_phase) (rate(aidb_compaction_duration_seconds_bucket[5m])))` |
| `aidb_backup_total` | Counter | 1 | `aidb_backup_operation(3: create\|delete\|restore)`<br>基数 3 | 备份/恢复操作完成时原子累加<br>`stats.backup_total`; `backup/manager.rs`, `backup/recovery.rs` | 备份与恢复系统操作统计 | 备份操作量速率:<br>`sum by (aidb_backup_operation) (rate(aidb_backup_total[1m]))` |
| `aidb_backup_size_bytes` | Gauge | By | — (基数 1) | 仅 create 时原子记录<br>`stats.backup_size_bytes`; `backup/manager.rs` | 最近一次备份总大小 | 备份体积观测 (MB):<br>`aidb_backup_size_bytes / 1024 / 1024` |
| `aidb_backup_duration_seconds` | Histogram | s | — (基数 1) | 备份打包耗时原子记录<br>`stats.backup_duration`; `backup/manager.rs` | 桶边界 0.01~10.0 s | 备份耗时分布:<br>`histogram_quantile(0.95, rate(aidb_backup_duration_seconds_bucket[5m]))` |

## OTel semconv 双写指标

与 `aidb_operations_total` / `aidb_operation_duration_seconds` 同数据源同步双写, 仅属性名不同, 用于接入标准 DB 语义监控.

| 指标名 | 类型 | 单位 | 标签与基数 | 数据源与代码位置 | 说明 (实现陷阱与口径) | 用途与典型 PromQL |
| --- | --- | --- | --- | --- | --- | --- |
| `db.client.operations` | Counter | {operation} | `db_system(1: aidb)` · `db_operation_name(8)`<br>基数 8 | `stats.operations` 经 `sync_to_otel` 同步双写<br>`src/metrics.rs` | OTel 标准 DB 客户端语义; Prometheus 渲染名视 Collector 配置而定 | 标准化面板 / 跨库对比 QPS:<br>`rate(db_client_operations_total[1m])` |
| `db.client.operation.duration` | Histogram | s | 同上 (基数 8) | `stats.operation_durations` 经 `sync_to_otel` 同步双写<br>`src/metrics.rs` | 同上 | 标准化延迟分布:<br>`histogram_quantile(0.95, rate(db_client_operation_duration_seconds_bucket[1m]))` |

## 集群指标 (`monitoring` + `cluster`)

全部由 client/server 直接 `fetch_add` 写入共享 `Arc<Statistics>` 实例, 集中由 `src/metrics.rs` 的 `sync_to_otel` 差分同步并导出.

| 指标名 | 类型 | 单位 | 标签与基数 | 数据源与代码位置 | 说明 (实现陷阱与口径) | 用途与典型 PromQL |
| --- | --- | --- | --- | --- | --- | --- |
| `aidb_raft_rpc_total` | Counter | 1 | `aidb_raft_rpc_type(3: vote\|append_entries\|install_snapshot)` · `aidb_raft_direction(2: incoming\|outgoing)`<br>基数 6 | Raft RPC 收发原子累加<br>`stats.raft_rpc`; `cluster/network/client.rs`, `cluster/network/server.rs` | 集群内部 RPC 流量统计 | 集群复制流量观测:<br>`sum by (aidb_raft_rpc_type, aidb_raft_direction) (rate(aidb_raft_rpc_total[1m]))` |
| `aidb_raft_log_entries_total` | Counter | 1 | — (基数 1) | AppendEntries 入站 entry 数原子累加<br>`stats.raft_log_entries`; `cluster/network/server.rs` | **仅入站方向** (`entries.len()`); 出站复制量需乘多数派或看对端入站 | Raft 日志复制吞吐速率:<br>`rate(aidb_raft_log_entries_total[1m])` |
| `aidb_raft_group_fatal_total` | Counter | 1 | `aidb_raft_group_id`<br>基数视分片数而定 | Group 进入 OpenRaft Fatal 状态原子累加<br>`stats.raft_group_fatal`; `cluster/multi_raft_node/lifecycle.rs` | Raft 致命不可恢复异常 | 集群致命错误告警:<br>`rate(aidb_raft_group_fatal_total[1m]) > 0` |
| `aidb_raft_group_restart_total` | Counter | 1 | `aidb_raft_group_id` · `aidb_raft_group_restart_outcome(2: success\|failure)`<br>基数 2 × 分片数 | Group 自愈重启结果原子累加<br>`stats.raft_group_restart`; `lifecycle.rs` | 实际取值仅 success/failure (代码注释中的 `skipped_backoff` 未使用) | 自愈成功率观测:<br>`sum by (aidb_raft_group_restart_outcome) (rate(aidb_raft_group_restart_total[1m]))` |
