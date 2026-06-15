# AiDb 可观测性

AiDb 侧 tracing 与 Prometheus 指标说明.

---

## 1. 架构总览

### 双轨策略

| 轨道 | 用途 | 技术 |
|------|------|------|
| **Tracing** | 请求路径追踪、延迟分析 | `tracing` crate + OpenTelemetry |
| **Metrics** | 运行时指标聚合、告警 | `prometheus` crate (feature: `monitoring`) |

### Span 层级树

```
request
├── kv_command (router)
│   ├── cmd_string / cmd_hash / cmd_list / cmd_zset / ...
│   └── storage.get / storage.set / storage.delete
├── db_open
│   ├── wal_recover
│   ├── sstable_read_meta
│   └── memtable_init
├── db_put
│   ├── wal_write
│   ├── memtable_insert
│   └── sstable_flush (触发时)
├── db_get
│   ├── memtable_lookup
│   ├── sstable_read (每层)
│   │   ├── bloom_check
│   │   ├── index_seek
│   │   └── block_read
│   └── cache_lookup
├── compaction
│   ├── compaction_pick
│   ├── compaction_merge
│   └── sstable_write
└── raft (cluster feature)
    ├── raft_propose
    ├── raft_append
    └── raft_apply
```

### OTel 导出拓扑

```
AiDb ──(OTLP/gRPC)──> OTel Collector ──> Jaeger / Tempo
AiDb ──(HTTP /metrics)──> Prometheus ──> Grafana
```

启动监控栈 (Prometheus / Grafana / OTel Collector) 按你的环境自行部署; 指标端点默认 `:9191/metrics`.

---

## 2. 模块可观测性矩阵

### WAL

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `wal_write` | INFO | `file_number`, `offset`, `size` | ✅ |
| `wal_sync` | DEBUG | `file_number`, `sync_ms` | ✅ |
| `wal_recover` | INFO | `file_count`, `entries_recovered` | ✅ |

### MemTable

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `memtable_insert` | TRACE | `key_len`, `value_len` | ✅ |
| `memtable_lookup` | DEBUG | `found` | ✅ |
| `memtable_flush` | INFO | `entries`, `size_bytes` | ✅ |

### SSTable

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `sstable_read` | DEBUG | `file_number`, `level` | ✅ |
| `sstable_write` | INFO | `file_number`, `level`, `size_bytes` | ✅ |
| `bloom_check` | TRACE | `hit`, `file_number` | ✅ |
| `index_seek` | TRACE | `file_number`, `block_offset` | ✅ |
| `block_read` | TRACE | `file_number`, `block_size` | ✅ |

### DB Engine

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `db_open` | INFO | `path`, `options` | ✅ |
| `db_put` | DEBUG | `key_len` | ✅ |
| `db_get` | DEBUG | `found` | ✅ |
| `db_delete` | DEBUG | `key_len` | ✅ |
| `db_flush` | INFO | `memtable_size` | ✅ |

### Compaction

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `compaction_pick` | INFO | `level`, `picked_files` | ✅ |
| `compaction_merge` | INFO | `input_files`, `output_files` | ✅ |
| `compaction_cleanup` | INFO | `files_removed` | ✅ |

### Bloom Filter

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `bloom_check` | TRACE | `hit`, `false_positive` | ✅ |
| `bloom_build` | DEBUG | `num_keys`, `num_bits` | ⚠️ 缺失 |

### Block Cache

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `cache_lookup` | TRACE | `hit`, `shard` | ✅ |
| `cache_insert` | TRACE | `size`, `shard` | ✅ |

### Snapshot

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `snapshot_create` | INFO | `sequence`, `id` | ✅ |
| `snapshot_release` | DEBUG | `id` | ✅ |

### Raft

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `raft_propose` | INFO | `group_id`, `entry_type` | ✅ |
| `raft_append` | DEBUG | `group_id`, `entries` | ✅ |
| `raft_apply` | INFO | `group_id`, `index` | ✅ |

### Backup

| Span | Level | 属性 | 状态 |
|------|-------|------|------|
| `backup_create` | INFO | `path`, `size_bytes` | ✅ |
| `backup_restore` | INFO | `path`, `entries` | ✅ |

### Metrics 清单

| Metric | 类型 | 标签 | Feature |
|--------|------|------|---------|
| `aidb_puts_total` | Counter | — | `monitoring` |
| `aidb_gets_total` | Counter | `hit` | `monitoring` |
| `aidb_deletes_total` | Counter | — | `monitoring` |
| `aidb_wal_bytes` | Counter | — | `monitoring` |
| `aidb_sstable_count` | Gauge | `level` | `monitoring` |
| `aidb_compaction_count` | Counter | `level` | `monitoring` |
| `aidb_bloom_false_positives` | Counter | — | `monitoring` |
| `aidb_cache_hit_rate` | Gauge | — | `monitoring` |
| `aidb_memtable_size` | Gauge | — | `monitoring` |
| `aidb_snapshot_count` | Gauge | — | `monitoring` |

### 已知缺口

| 模块 | 缺失内容 | 优先级 |
|------|---------|--------|
| Bloom | `bloom_build` span | P1 |
| Snapshot | compaction 下的 snapshot 保护 span | P2 |
| Raft | `failover` span | P2 |
| Cluster | `slot_migration` span | P2 |

---

## 3. 运维指南

### 关键 Dashboard 指标

| 面板 | 指标 | 告警阈值建议 |
|------|------|------------|
| 写入吞吐 | `rate(aidb_puts_total[1m])` | < 基线 50% → 告警 |
| 读取延迟 P99 | `histogram_quantile(0.99, aidb_get_duration)` | > 100ms → 告警 |
| WAL 写入延迟 | `aidb_wal_sync_duration` | > 10ms → 告警 |
| Compaction 积压 | `aidb_level0_sstable_count` | > 4 → 告警 |
| Bloom 假阳性率 | `rate(aidb_bloom_false_positives[5m])` | > 100/s → 关注 |
| 缓存命中率 | `aidb_cache_hit_rate` | < 80% → 告警 |

### 常见故障排查

**问题: 读取延迟突增**
1. 检查 `bloom_check` span — 假阳性率是否异常
2. 检查 `cache_lookup` hit rate — 是否缓存失效
3. 检查 `level0_sstable_count` — 是否 Compaction 积压导致多层查找

**问题: 写入吞吐下降**
1. 检查 `wal_sync` 延迟 — 磁盘是否瓶颈
2. 检查 `level0_sstable_count` — 是否触发 write stall (≥8)
3. 检查 Compaction 线程 CPU 占用

**问题: 崩溃恢复慢**
1. 检查 `wal_recover` span 中的 `entries_recovered`
2. 过大 WAL 考虑调小 `wal_size_mb` 参数
