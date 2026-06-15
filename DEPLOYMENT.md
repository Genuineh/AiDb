# AiDb 部署指南

AiDb 是嵌入式存储引擎, 以 Rust crate 形式供应用引入.

## 系统要求

- Rust 工具链: stable 分支 (见 `rust-toolchain.toml`)
- 操作系统: Linux / macOS
- 磁盘: 取决于数据量, 推荐 SSD (特别是 WAL + SSTable 路径)
- 内存: MemTable 大小可配置 (默认 4MB), 取决于工作负载

## 作为依赖使用

```toml
[dependencies]
aidb = "0.13"
```

### 基本用法

```rust
use aidb::{DB, config::Options};

let opts = Options {
    create_if_missing: true,
    ..Options::default()
};
let db = DB::open("/var/data/aidb", opts)?;

// 读写
db.put(b"key", b"value")?;
let val = db.get(b"key")?;

// WriteBatch 原子写入
let mut batch = db.batch();
batch.put(b"a", b"1");
batch.put(b"b", b"2");
db.write(batch)?;

// 范围扫描
let iter = db.scan(b"a", b"z")?;
for entry in iter {
    let (key, value) = entry?;
}
```

### 多线程使用

`DB` 是 `Send + Sync`, 可在多线程间共享:

```rust
let db = Arc::new(DB::open("/path", opts)?);

// 线程 1
let db1 = db.clone();
thread::spawn(move || db1.put(b"k1", b"v1"));

// 线程 2
let db2 = db.clone();
thread::spawn(move || db2.get(b"k1"));
```

## 配置参考

`Options` 支持以下配置项:

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `create_if_missing` | true | 目录不存在时创建 |
| `error_if_exists` | false | 目录已存在时报错 |
| `memtable_size` | 4MB | MemTable 大小上限 |
| `max_write_buffer_number` | 2 | 最大 immutable MemTable 数 (超出触发背压) |
| `block_size` | 4KB | SSTable Block 大小 |
| `block_cache_size` | 8MB | Block Cache 容量 (0=禁用) |
| `bloom_false_positive_rate` | 1% | Bloom Filter 假阳性率 (0=禁用) |
| `compression` | None | 压缩算法 (Snap/LZ4 预留, 未实现) |
| `use_wal` | true | 启用 WAL |
| `sync_wal` | false | 每次写入 fsync (true=强持久) |
| `max_wal_size` | 64MB | WAL 轮转阈值 |
| `max_levels` | 7 | LSM-Tree 层级数 |
| `strict_wal_recovery` | false | WAL 损坏时严格模式 (非严格: 截断到最后一个完整 entry) |
| `background_compaction` | true | 启用后台 compaction |

### 生产推荐配置

```rust
Options {
    create_if_missing: true,
    memtable_size: 64 * 1024 * 1024,     // 64MB
    block_cache_size: 256 * 1024 * 1024, // 256MB
    max_write_buffer_number: 4,
    sync_wal: true,                        // 强持久
    bloom_false_positive_rate: 0.01,       // 1%
    ..Options::default()
}
```

## 集群部署

集群模式需要 `--features cluster` 构建.

详见 [aikv/DEPLOYMENT.md](../aikv/DEPLOYMENT.md) 了解完整集群部署方案.

## 备份

```rust
use aidb::backup::*;
use std::sync::Arc;

let storage = Arc::new(LocalFileStorage::new("/var/backups/".into()));
let policy = RetentionPolicy {
    min_count: 3,
    max_count: 30,
    min_age: Duration::from_secs(86400),     // 1 天
    max_age: Duration::from_secs(86400 * 30), // 30 天
};
let manager = BackupManager::new(storage, policy);

// 创建备份
let id = manager.create_backup(&db)?;

// 列举备份
let backups = manager.list_backups()?;

// 从备份恢复
let recovery = RecoveryManager::new(storage);
recovery.restore(id, "/var/data/aidb-restored")?;
```

## 可观测性

### Prometheus 指标

启动 `--features monitoring` 后, 通过 `--metrics-port` (默认 9191) 暴露:

```
curl localhost:9191/metrics
```

关键指标:

| 指标 | 类型 | 说明 |
|------|------|------|
| `aidb_operations_total` | Counter | 操作数 (put/get/delete/scan/close) |
| `aidb_flush_total` | Counter | MemTable flush 次数 |
| `aidb_compaction_total` | Counter | Compaction 次数 |
| `aidb_compaction_duration_seconds` | Histogram | Compaction 耗时 |
| `aidb_sequence` | Gauge | 当前 sequence |
| `aidb_total_key_count` | Gauge | 总 key 数 (估算) |
| `aidb_sstable_count` | Gauge | SSTable 文件数 |
| `aidb_sstable_size_bytes` | Gauge | SSTable 总大小 |
| `aidb_memtable_size_bytes` | Gauge | MemTable 大小 |
| `aidb_wal_size_bytes` | Gauge | WAL 大小 |
| `aidb_bloom_false_positive_total` | Counter | Bloom Filter 误判总数 |
| `aidb_backup_total` | Counter | 备份操作数 (create/delete/restore) |
| `aidb_backup_size_bytes` | Gauge | 备份大小 |
| `aidb_backup_duration_seconds` | Histogram | 备份耗时 |

### OpenTelemetry

设置环境变量 `AIDB_OTLP_ENDPOINT` 启用 OTLP 导出:

```bash
export AIDB_OTLP_ENDPOINT=http://collector:4317
```

### JSON 日志

默认输出 JSON 格式, 可切换为人类可读:

```bash
export AIDB_JSON_LOG=false
```
