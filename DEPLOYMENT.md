# AiDb 部署与运行

本文说明 **如何构建、启用 Cargo feature、作为依赖嵌入、配置数据目录与运维入口**. **是什么、怎么分层** 见 [ARCHITECTURE.md](ARCHITECTURE.md); **设计取舍** 见 [DESIGN.md](DESIGN.md); WAL/compaction/Raft 等实现细节见 [docs/modules/](docs/modules/).

AiDb 是 **嵌入式 lib crate**, 无独立守护进程、无内置 HTTP listener. 需要 Redis 协议或集群运维时, 使用 [AiKv](../aikv/DEPLOYMENT.md) (该文档在 aikv 汇总阶段完善).

## 系统要求

| 项 | 要求 |
|----|------|
| Rust | **stable** (见 [rust-toolchain.toml](rust-toolchain.toml); 含 clippy、rustfmt) |
| 操作系统 | Linux / macOS (CI 为 `ubuntu-latest`) |
| 磁盘 | 推荐 SSD (WAL + SSTable 路径); 容量随数据量 |
| 内存 | 与 `Options` 中 MemTable、Block Cache 等相关 (默认各 64 MiB) |
| protoc | 仅 **`cluster` feature** 本地 clippy/测试需要; CI 通过 `apt install protobuf-compiler` 安装 |

## Cargo feature 矩阵

定义见 [Cargo.toml](Cargo.toml).

| Feature | 默认 | 启用内容 | 典型用途 |
|---------|------|----------|----------|
| `backup` | ✅ | `src/backup/*`, ring/hex/serde_json | 全量备份与恢复 |
| (engine only) | — | `src/engine/*` 始终编译 | 最小嵌入 |
| `cluster` | ❌ | `src/cluster/*`, tonic/prost/tokio/openraft | Multi-Raft / MetaRaft |
| `monitoring` | ❌ | `src/metrics.rs`, Prometheus 系列 | 嵌入方 scrape |
| `compression` | ❌ | snap/lz4 依赖 | SSTable Data Block Snap/Lz4 压缩 (`Options::default()` 默认 Snap); aikv 生产镜像 (aifactory Dockerfile) 已默认启用 |

**常见组合**:

| 场景 | 命令 / 依赖 |
|------|-------------|
| 单机 + 备份 (默认) | `cargo build` |
| 最小 engine | `cargo build --no-default-features` |
| 单机 + 指标 | `cargo build --features monitoring` |
| 集群库开发 | `cargo build --features cluster` |
| 集群 + 指标 | `cargo build --features cluster,monitoring` |

**嵌入方 (AiKv) 传递 feature** — 见 [aikv/Cargo.toml](../aikv/Cargo.toml): `monitoring` → `aidb/monitoring`, `cluster` → `aidb/cluster`.

## 构建与验证

本地与 CI 门禁见 [AGENTS.md](AGENTS.md)、[.github/README.md](.github/README.md).

```bash
export RUSTFLAGS='-D warnings'
cargo fmt --check
cargo clippy --all-targets
cargo clippy --all-targets --features cluster   # 需 protoc
cargo test -- --test-threads=1
cargo test --features cluster -- --test-threads=1
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) 分两 job: 默认 feature 与 `cluster` (安装 protoc). **`monitoring` 无独立 CI job** — 本地可 `cargo test --test metrics --features monitoring -- --test-threads=1`.

**cluster 与 protoc** ([build.rs](build.rs)): 启用 `cluster` 时编译 `proto/raft.proto`; 若系统无 `protoc` 且未设 `PROTOC`, 使用仓库内 checked-in 生成代码并打印 warning.

**基准测试** (可选):

```bash
cargo bench --bench write_bench
cargo bench --bench read_bench
cargo bench --bench backup_bench
# read_bench 预填充规模: AIDB_BENCH_PRELOAD=100000 cargo bench --bench read_bench
```

完整测试矩阵与 hook 说明留 [CONTRIBUTING.md](CONTRIBUTING.md) (汇总阶段) 与日后 `docs/development.md`.

## 作为依赖嵌入

### 声明依赖

```toml
[dependencies]
# 版本以 crates.io / Cargo.toml 为准 (当前 0.14.x)
aidb = { version = "0.14", features = ["monitoring"] }

# monorepo (AiKv)
aidb = { path = "../aidb" }
```

按需启用 feature: `features = ["backup"]` (默认已含)、`["cluster"]`、`["monitoring"]` 或组合.

### 最小用法

与 [examples/basic.rs](examples/basic.rs) 一致:

```rust
use aidb::{DB, WriteBatch, config::Options};

let db = DB::open("/var/data/aidb", Options::default())?;

db.put(b"hello", b"world")?;
assert_eq!(db.get(b"hello")?, Some(b"world".to_vec()));

let mut batch = WriteBatch::new();
batch.put(b"a", b"1");
db.write(&batch)?;

let iter = db.scan(Some(b"a"), Some(b"z"))?;
for entry in iter {
    let (k, v) = entry?;
    let _ = (k, v);
}

db.close()?;
```

### 多线程

`DB` 为 `Send + Sync`, 可 `Arc` 在线程间共享 (见 `examples/basic.rs` 模式).

### 与 AiKv 的分工

| 能力 | AiDb | AiKv |
|------|------|------|
| LSM 存储 | `DB::put/get/...` | `AiDbEngine` + `spawn_blocking` |
| 集群 Raft / slot | `cluster` API | `ClusterDataAdapter`, MOVED/ASK |
| HTTP `/metrics` | `register_into` 仅注册 | HTTP 暴露与 OTel |

详见 [ARCHITECTURE.md §与 AiKv 的嵌入关系](ARCHITECTURE.md#与-aikv-的嵌入关系).

## 数据目录

单机 `DB::open(path, opts)` 的 `path` 即数据根目录:

```shell
{db_path}/
├── LOCK                 # 单进程独占; 多进程打开 → Error::Busy
├── CURRENT              # 指向活跃 MANIFEST
├── MANIFEST-*           # VersionSet 元数据
├── {nnnnnn}_L{level}.sst
└── wal_{n}.log
```

磁盘格式与早期 `aidb-oldmain` **不兼容** — 勿跨版本直接打开旧目录. 布局细节见 [docs/modules/engine.md](docs/modules/01-engine.md)、[engine-storage.md](docs/modules/02-engine-storage.md).

## 配置 (部署向摘要)

完整字段见 [src/config.rs](src/config.rs). `DB::open` 前会调用 `Options::validate()`.

### 默认值 (生产起点)

| 字段 | 默认 | 部署含义 |
|------|------|----------|
| `create_if_missing` | true | 目录不存在则创建 |
| `memtable_size` | 64 MiB | 触发 flush 阈值 |
| `block_cache_size` | 64 MiB | 读缓存 (0 = 禁用) |
| `use_wal` | true | false 则 crash 不保证持久 |
| `sync_wal` | false | true = 每条写 fsync (强持久, 低吞吐) |
| `bloom_false_positive_rate` | 0.01 | 0.0 = 禁用 Bloom |
| `background_compaction` | true | 测试可关 |
| `compaction_threads` | 1 | 后台线程数 (内部 clamp 1–4) |

### Preset

| 方法 | 用途 | 说明 |
|------|------|------|
| `Options::for_testing()` | 单测 / 示例 | 小 memtable、无 bloom、关 background_compaction |
| `Options::for_high_write_throughput()` | 写密集 | 大 memtable; 启用 `compression` feature 时 `CompressionType::Snap`, 否则 `None` |
| `Options::for_high_read_throughput()` | 读密集 | 大 block cache、低 bloom FP、`CompressionType::None` |

强持久场景可在 default 或 preset 基础上设 `sync_wal: true`, 并酌情调大 `memtable_size` / `block_cache_size`.

### ClusterConfig (`feature cluster`)

| 方法 | group_count | replication_factor | 典型场景 |
|------|-------------|-------------------|----------|
| `ClusterConfig::for_production()` | 256 | 3 | 生产 (slot 路由见 cluster module) |
| `ClusterConfig::for_testing()` | 4 | 1 | 单测 |

`RaftNodeConfig` 等运行时参数见 [docs/modules/cluster.md](docs/modules/03-cluster.md). **集群进程启动、端口、compose** 不在 aidb — 见 [AiKv 部署](../aikv/DEPLOYMENT.md).

## 示例

| 示例 | 命令 | 说明 |
|------|------|------|
| 基本 CRUD | `cargo run --example basic` | open / put / batch / scan / snapshot |
| 备份恢复 | `cargo run --example backup` | BackupManager + RecoveryManager |
| 槽位路由 | `cargo run --features cluster --example cluster` | CRC16 / hash tag (**无网络、无组网**) |

更多说明见 [examples/README.md](examples/README.md).

## 备份与恢复

默认 feature 已含 `backup`. 流程: `Checkpoint` → manifest + SHA256 → 可选保留策略.

```rust
use std::sync::Arc;
use aidb::{DB, config::Options};
use aidb::backup::{BackupManager, LocalFileStorage, RecoveryManager, RetentionPolicy};

let db = DB::open("/var/data/aidb", Options::default())?;
let storage = Arc::new(LocalFileStorage::new("/var/backups"));
let manager = BackupManager::new(storage.clone(), RetentionPolicy::default());

let id = manager.create_backup(&db)?;
let backups = manager.list_backups()?;

let recovery = RecoveryManager::new(storage);
assert!(recovery.verify_backup(id)?);
recovery.restore(id, "/var/data/aidb-restored")?;
```

- 仅 **`LocalFileStorage`** 内置; S3 / 增量备份未实现 — 见 [ISSUES.md#ISSUE-012](ISSUES.md#issue-012-无-backup_id-碰撞重试与压缩增量s3).
- AiKv `BGSAVE` 直调 `Checkpoint`, 不经 `BackupManager` — 见 aikv [commands-extended.md](../aikv/docs/modules/05-commands-extended.md).
- 详情见 [docs/modules/backup.md](docs/modules/04-backup.md).

## 可观测性

| 能力 | 编译 | 说明 |
|------|------|------|
| **Tracing** | 始终 | `tracing` crate; 嵌入方配置 `tracing-subscriber` |
| **Prometheus** | `monitoring` feature | `DB::open` 时 `metrics::init()`; 热路径 `record_*` |

AiDb **无内置 HTTP `/metrics`、无 OTLP/JSON log 环境变量**. 嵌入方创建 `prometheus::Registry` 后:

```rust
aidb::metrics::register_into(&registry)?;
// 再由 HTTP handler encode gather()
```

AiKv 在 `Metrics::new()` 内完成注册并暴露 `/metrics`. 指标列表与 PromQL 见 [docs/modules/observability.md](docs/modules/05-observability.md).

## 集群 (库侧)

构建: `cargo build --features cluster` (需 protoc, 见上).

aidb 提供 MetaRaft / Multi-Raft、slot 路由与 gRPC; **MOVED/ASK、CLUSTER 命令、节点部署** 由 AiKv 实现. 完整集群运维见 [AiKv 部署](../aikv/DEPLOYMENT.md) (aikv 文档整理步 21).

库侧深入: [docs/modules/cluster.md](docs/modules/03-cluster.md).

## 相关文档

- [ARCHITECTURE.md](ARCHITECTURE.md) — 分层与数据流
- [DESIGN.md](DESIGN.md) — 设计决策
- [AGENTS.md](AGENTS.md) — AI 助手与 CI 速查
- [docs/modules/](docs/modules/) — 域级实现文档
- [ISSUES.md](ISSUES.md) — 待核实项

## 待核实

- HTTP `/metrics` 与 OTel 运行在嵌入方 (AiKv), 非 aidb 库内 — 见 [ISSUES.md#ISSUE-014](ISSUES.md#issue-014-httpoteljson-log-运行在嵌入方-aidb-仅库内指标).
