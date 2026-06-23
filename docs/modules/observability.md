---
name: aidb-observability
depends_on:
  - aidb-engine
description: AiDb observability — centralized Prometheus metrics (monitoring feature), tracing span index, metrics::init and register_into for embedders. Use when changing src/metrics.rs or cluster/metrics.rs, wiring aidb_* counters in aikv, or debugging Prometheus/tracing for engine and Raft paths.
---

# AiDb Observability (可观测性)

## 何时读本文

- 改 `src/metrics.rs`、`src/cluster/metrics.rs` 或排查 `aidb_*` Prometheus 指标
- 在 **嵌入方** (aikv) 注册 aidb 指标、理解 `register_into` 与 scrape 边界
- 查 tracing span / event 命名, 跨 module 定位埋点
- **不覆盖**: 各 module 内 span 实现细节 → [engine.md](engine.md) / [engine-storage.md](engine-storage.md) / [cluster.md](cluster.md) / [backup.md](backup.md)
- **不覆盖**: HTTP `/metrics`、OTel Collector、slowlog/INFO → aikv [observability.md](../../../aikv/docs/modules/observability.md)
- **监控栈部署**: AiFactory [`monitor/README.md`](../../../AiFactory/monitor/README.md) (115 中心 + worker Alloy)
- **构建**: `monitoring` feature 启用 `aidb::metrics`; 默认 **不** 启用

## 代码地图

| 路径 | 职责 | 入口 |
|------|------|------|
| `src/metrics.rs` | 引擎 Prometheus 系列 + `init` / `register_into` / `record_*` | `DB::open` → `init()` |
| `src/cluster/metrics.rs` | Raft RPC / log 计数 | `cluster/network.rs` |
| `src/lib.rs` | `#[cfg(monitoring)] pub mod metrics` | 无 monitoring 则无模块 |
| `tests/common/observability.rs` | `EventCatcher`、tracing 测试锁 | 跨模块 tracing 验收 |
| `tests/modules/metrics/prometheus.rs` | cache/bloom/DB histogram 接线 | `--test metrics` |
| `tests/modules/cluster/metrics.rs` | Raft 指标 register + gather | cluster 测试套件 |

**嵌入方**: `aikv/src/server/metrics.rs` 在 `Metrics::new()` 内调用 `aidb::metrics::register_into(&registry)?`, 与 `aikv_*` 共用 Registry 后由 HTTP 暴露.

## 架构: 双轨 + 嵌入

```mermaid
flowchart LR
  subgraph lib [aidb 库]
    T[tracing spans/events]
    M[metrics.rs LazyLock]
    R[register_into]
  end
  subgraph embed [嵌入方 aikv]
    REG[prometheus::Registry]
    HTTP[GET /metrics]
  end
  T --> T
  M --> R
  R --> REG
  REG --> HTTP
```

要点:

- **Tracing**: 始终编译 (`tracing` crate); 与 `monitoring` feature **无关**
- **Prometheus**: 仅 `monitoring` feature; `record_*` 在引擎热路径自动调用
- **aidb 无内置 HTTP scrape 端点**; `opentelemetry` / `tracing-opentelemetry` 在 `Cargo.toml` 列为 `monitoring` 依赖, 但 **aidb/src 无 OTel Layer 接线** (见 ISSUE-014)

## 生命周期

1. **`DB::open`** (`monitoring`): `metrics::init()` (幂等触摸所有 `LazyLock`) + `set_sequence`
2. **运行时**: put/get/flush/compaction/backup 等路径调 `record_*` 或直接 `Gauge::set`
3. **嵌入方启动**: `Registry::new()` → `aidb::metrics::register_into(&registry)?` → encode 暴露

`register_into` 在 `monitoring` + `cluster` 时链式注册 `cluster/metrics.rs`.

## Prometheus 指标 (`metrics.rs`)

| 指标 | 类型 | labels | 主要触发 |
|------|------|--------|----------|
| `aidb_wal_size_bytes` | Gauge | — | `wal/manager.rs` |
| `aidb_memtable_size_bytes` | IntGaugeVec | `state=active\|frozen` | `memtable/table.rs` |
| `aidb_sstable_count` | IntGaugeVec | `level` | `db/inner.rs` `update_sstable_metrics` |
| `aidb_sstable_size_bytes` | IntGaugeVec | `level` | 同上 |
| `aidb_operations_total` | CounterVec | `op` | `db/inner.rs` |
| `aidb_operation_duration_seconds` | HistogramVec | `op` | put/get/delete/write_batch |
| `aidb_flush_total` | Counter | — | flush 完成 |
| `aidb_flush_duration_seconds` | Histogram | — | flush 路径 |
| `aidb_block_cache_size_bytes` | Gauge | — | `block_cache.rs` |
| `aidb_block_cache_hits_total` | Counter | — | cache get hit |
| `aidb_block_cache_misses_total` | Counter | — | cache get miss |
| `aidb_bloom_false_positive_total` | Counter | — | `filter/bloom.rs` |
| `aidb_sequence` | IntGauge | — | open / allocate |
| `aidb_total_key_count` | IntGauge | — | put/delete 后 |
| `aidb_compaction_total` | CounterVec | **`type`** | pick/run/apply |
| `aidb_compaction_duration_seconds` | HistogramVec | **`phase`** | pick/run/apply |
| `aidb_backup_total` | CounterVec | `op=create\|delete\|restore` | `backup/*` |
| `aidb_backup_size_bytes` | IntGauge | — | create |
| `aidb_backup_duration_seconds` | Histogram | — | create |

**`aidb_operations_total` / `operation_duration` 的 `op`**: `put`, `get`, `delete`, `write_batch`, `snapshot`, `stall_stop`, `stall_slowdown`. **`scan` / `close` 无 counter** (见 ISSUE-018).

**命中率**: 无 `cache_hit_rate` gauge; 用 PromQL `rate(hits)/(rate(hits)+rate(misses))`.

### 集群指标 (`cluster/metrics.rs`, `monitoring` + `cluster`)

| 指标 | labels | 触发 |
|------|--------|------|
| `aidb_raft_rpc_total` | `type`=vote/append_entries/install_snapshot, `direction`=incoming/outgoing | `cluster/network.rs` |
| `aidb_raft_log_entries_total` | — | AppendEntries 入站 entry 数 |

## Tracing 索引 (按域)

> 完整字段见各 module; 此处只列 **instrument `name`** 与主要 **`target:` event**.

| 域 | instrument 名 | 主要 event (`target`) |
|----|---------------|----------------------|
| WAL | `wal_open`, `wal_write`, `wal_replay`, … | `wal`: `wal.write.*`, `wal.sync.*` |
| MemTable | `mem_put`, `mem_get`, `mem_freeze` | `mem`: `mem.put`, `mem.get.hit/miss` |
| SSTable | `sst_seek`, `sst_block_read`, `sst_build_add` | `sst`: `sst.seek.result`; `bloom_build` info_span |
| Cache | `cache_get`, `cache_insert` | — |
| DB | `db_open`, `db_put`, `db_get`, `db_scan`, `db_flush`, `db_close` | `db`: `db.put`, `db.get.result`, `db.flush.complete` |
| Compaction | `cmp_pick`, `cmp_run`, `cmp_merge`, `cmp_apply` | — |
| Checkpoint | `bgsave_checkpoint` | `db`: `checkpoint.create.complete` |
| Backup | `backup_create`, `backup_restore`, … | 见 [backup.md](backup.md) |
| Raft 存储 | `raft_append_log`, `raft_apply_sm`, … | — |
| Raft RPC | `raft_rpc_ae`, `raft_rpc_vote`, `raft_rpc_is` | — |
| Meta | `meta_propose`, `meta_apply`, `meta_slot_query` | — |

**不在 aidb**: `kv_command` / RESP 命令 span → aikv.

## 常见任务

### 启用引擎指标

```toml
# 嵌入方 Cargo.toml
aidb = { path = "../aidb", features = ["monitoring"] }
```

```bash
cargo build --features monitoring
cargo test --test metrics --features monitoring -- --test-threads=1
```

### 注册到自定义 Registry

```rust
let registry = prometheus::Registry::new();
aidb::metrics::register_into(&registry)?;
// prometheus::Encoder::gather → HTTP 或文件
```

aikv 已在 `Metrics::new()` 内完成上述步骤.

### 读取指标值 (测试)

```rust
aidb::metrics::init();
// 操作后:
assert!(aidb::metrics::OPERATIONS_TOTAL.with_label_values(&["put"]).get() > 0);
```

或 `tests/common/observability.rs` 的 `assert_gauge_eq` / `assert_counter_eq`.

### 验证 tracing event

```rust
use crate::common::observability::{capture_events_under_lock, EventCatcher};
let events = capture_events_under_lock(|| { /* 被测操作 */ });
// 或 EventCatcher + init_test_subscriber
```

含 tracing 的测试建议 `--test-threads=1` (避免 subscriber 竞争).

### 排查指标为 0

1. 确认编译启用了 `monitoring` feature
2. 确认 `DB::open` 已执行 (`init()` 在 open 内)
3. 确认嵌入方调用了 `register_into` 且 scrape 的 Registry 为同一实例
4. 对 gauge (如 `sstable_count`): 确认发生过 flush/compaction 触发 `update_sstable_metrics`

## 配置与 feature flags

| 项 | 位置 | 说明 |
|----|------|------|
| `monitoring` | `Cargo.toml` | `prometheus`, `opentelemetry*`, `tracing-opentelemetry`; 导出 `aidb::metrics` |
| `cluster` | 与 `monitoring` 叠加 | `register_into` 额外注册 `aidb_raft_*` |
| 无 `monitoring` | — | 无 `aidb::metrics` mod; `cluster::metrics::record_*` 为 no-op stub |

## 测试

```bash
cargo test --test metrics --features monitoring -- --test-threads=1
# Raft: tests/modules/cluster/metrics.rs (cluster 测试套件内)
```

| 测试 | 覆盖 |
|------|------|
| `test_block_cache_prometheus_counters_and_size` | hit/miss/size |
| `test_bloom_false_positive_prometheus_counter` | 与内部 atomic 一致 |
| `test_db_operation_and_flush_duration_histograms` | put/get/flush 有样本 |
| `test_raft_metrics_register_and_record` | gather 后 counter 值 |

## 已知限制

- **无内置 HTTP / OTel / JSON log 开关** — 嵌入方 (aikv) 负责 (ISSUE-014)
- **旧 observability 稿大量指标名/span 名已过时** — 以 `metrics.rs` 为准 (ISSUE-015)
- **未实现**: `wal_sync_duration`, `cache_hit_rate` gauge, `snapshot_count`, `cluster_nodes`, `errors_total`, `restore_duration` 等 (ISSUE-016)
- **compaction counter label `type` vs histogram label `phase`** — 同值不同名 (ISSUE-017)
- **`scan`/`close` 无 `operations_total`** (ISSUE-018)
- **无进程级 memory/disk 指标** — oldmain `monitoring` 模块已移除

## 待核实

- 见 [ISSUES.md](../../ISSUES.md#issue-014--httpoteljson-log-运行在嵌入方-aidb-仅库内指标) — HTTP/OTel 在嵌入方, aidb 仅库内指标
- 见 [ISSUES.md](../../ISSUES.md#issue-015--旧-observability-指标表与-span-名大量过时) — 旧稿指标表与 span 名过时
- 见 [ISSUES.md](../../ISSUES.md#issue-016--旧设计若干-prometheus-系列未实现) — 若干旧设计指标未实现
- 见 [ISSUES.md](../../ISSUES.md#issue-017--compaction-指标-counterhistogram-label-名不一致) — compaction label 名不一致
- 见 [ISSUES.md](../../ISSUES.md#issue-018--scanclose-未计入-aidb_operations_total) — scan/close 未计入 operations_total
