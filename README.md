# AiDb

基于 LSM-Tree 的嵌入式 KV 存储引擎 (Rust lib crate, 当前 **0.14.x**). 核心 `engine` 始终编译; 备份、集群、指标通过 Cargo feature 按需启用. **AiDb 不是网络服务** — 无内置 HTTP listener.

## 特性

**单机引擎** (默认):

- WAL 持久化, SkipMap MemTable, SSTable 分层存储
- Leveled Compaction, Bloom Filter, Block Cache
- `put` / `get` / `delete` / `scan`, WriteBatch 原子写, MVCC Snapshot
- Checkpoint 目录快照 (备份底层)

**可选能力** (feature):

| 能力 | Feature | 说明 |
|------|---------|------|
| 全量备份与恢复 | `backup` (默认开启) | `BackupManager`, `RecoveryManager` |
| 分布式存储 | `cluster` | MetaRaft 控制面 + Multi-Raft 数据面, 16384 slot (CRC16) |
| Prometheus 指标 | `monitoring` | `aidb_*` 系列, `register_into` 供嵌入方 scrape |
| 块压缩 | `compression` | 占位, 尚未实现 |

Feature 组合与构建命令见 [DEPLOYMENT.md](DEPLOYMENT.md).

## 与 AiKv

[AiKv](../aikv/) 在本库之上实现 Redis RESP、Cluster 重定向 (MOVED/ASK) 与 HTTP `/metrics`. AiDb 负责 LSM 存储与 Raft/slot 基础设施; monorepo 内 AiKv 通过 `path = "../aidb"` 依赖.

## 快速开始

```toml
[dependencies]
aidb = "0.14"
```

```rust
use aidb::{DB, config::Options};

let db = DB::open("/tmp/aidb-data", Options::default())?;
db.put(b"hello", b"world")?;
assert_eq!(db.get(b"hello")?, Some(b"world".to_vec()));
db.close()?;
```

仓库内运行完整示例:

```bash
cargo run --example basic
```

## 示例

| 示例 | 说明 | 运行 |
|------|------|------|
| `basic` | CRUD、批量写、扫描、快照 | `cargo run --example basic` |
| `backup` | 备份创建与恢复 | `cargo run --example backup` |
| `cluster` | CRC16 槽位 / hash tag 演示 | `cargo run --features cluster --example cluster` |

详见 [examples/README.md](examples/README.md).

## 基准测试

使用 [criterion](https://github.com/bheisler/criterion.rs): `cargo bench`. 主要 bench: `write_bench`, `read_bench`, `backup_bench`; `read_bench` 可用环境变量 `AIDB_BENCH_PRELOAD` 调整预填充规模. 详见 [DEPLOYMENT.md §构建与验证](DEPLOYMENT.md#构建与验证).

## 文档

开发文档 hub: [docs/README.md](docs/README.md) (汇总文档 + modules WHEN 路由).

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 分层、数据流、与 AiKv 边界 |
| [DESIGN.md](DESIGN.md) | 跨模块设计决策 |
| [DEPLOYMENT.md](DEPLOYMENT.md) | 构建、feature、嵌入、目录与运维 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | hooks、CI、测试矩阵、提交/PR 规范 |
| [CHANGELOG.md](CHANGELOG.md) | 版本变更记录 |
| [AGENTS.md](AGENTS.md) | AI 助手与 CI 入口 |
| [docs/modules/engine.md](docs/modules/engine.md) | WAL, MemTable, 写路径, `DB` API |
| [docs/modules/engine-storage.md](docs/modules/engine-storage.md) | SSTable, compaction, Bloom, cache |
| [docs/modules/cluster.md](docs/modules/cluster.md) | MetaRaft, Multi-Raft, slot 迁移 |
| [docs/modules/backup.md](docs/modules/backup.md) | BackupManager, 恢复流程 |
| [docs/modules/observability.md](docs/modules/observability.md) | 指标与 tracing |
| [ISSUES.md](ISSUES.md) | 待核实项 |

## 待核实

- HTTP `/metrics` 与 OTel 运行在嵌入方 (AiKv), 非 aidb 库内 — 见 [ISSUES.md#ISSUE-014](ISSUES.md#issue-014-httpoteljson-log-运行在嵌入方-aidb-仅库内指标).

## 许可

[MIT OR Apache-2.0](LICENSE) (见 [Cargo.toml](Cargo.toml)).
