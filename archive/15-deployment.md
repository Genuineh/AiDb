# Cursor transcript

---

## 1. User

## 文档整理 — 新会话

工作目录: `<workspace>` (含 aidb/、aikv/、aifactory/)

请先阅读 (不要跳过):
1. `AiKv-Workflow/backup/design.md` — 总规程
2. `AiKv-Workflow/backup/PROGRESS.md` — 当前进度
3. `AiKv-Workflow/backup/INDEX.md` — 本章索引 (若已有行则更新)

---

### 本次任务

| 项 | 值 |
|----|-----|
| PROGRESS 总步 | 15 (阶段 2a 第 3 篇) |
| 目标文件 | `aidb/DEPLOYMENT.md` |
| 本次子步 | 从步 0 开始 |
| 对比深度 | **模块级** (汇总文档; 构建/feature/运行/嵌入, 不加深章逐段) |

### 必须遵守

- **确认门控**: 步 0→1→2→3→4, 每步先讨论, 我确认后再下一步
- **步 4**: 先出 **正文草稿**, 我确认后再写入 `aidb/`、更新 INDEX/PROGRESS
- **汇总文档**: **不必** Module Skill frontmatter; 提炼 **如何构建、启用 feature、作为依赖运行**, **不重复** module 实现细节 (链到 `docs/modules/*.md`)
- **过程只进 backup/**: INDEX; aidb 不写迁移历史
- **旧文档顺序**: `backup/aidb/DEPLOYMENT.md` (主) → `aidb-oldmain` 运维类 → `wiqun-factory/docs/{BUILD,MONITORING}.md` 查漏 → WiQunTools → `wiqun-db`
- **待核实**: 根文档至多简要提及; 详情仍引用 `aidb/ISSUES.md` (不展开排查过程)
- **步 4 落盘前**: 对照 design.md 验收项 — 链接有效、与当前 `Cargo.toml`/`examples`/CI 一致、与 ARCHITECTURE/DESIGN 不矛盾

### 上下文 (上一篇已完成)

- 步 13 ✅: [`aidb/ARCHITECTURE.md`](aidb/ARCHITECTURE.md) — 分层、数据流、feature 边界、AiKv 嵌入
- 步 14 ✅: [`aidb/DESIGN.md`](aidb/DESIGN.md) — 跨模块设计决策与 trade-off
- 阶段 1 modules 全部 ✅; `docs/development.md` **尚未**创建 (构建/测试矩阵可步 15 简要指向 AGENTS + `.github/README.md`, 详表留后续)
- 旧稿: [`AiKv-Workflow/backup/aidb/DEPLOYMENT.md`](AiKv-Workflow/backup/aidb/DEPLOYMENT.md) (已移出 aidb 仓库; **含过时内容**: 错误 API 示例、`--metrics-port`、OTLP/JSON log 环境变量、过时 Options 默认值)
- 当前 `aidb/` **尚无** 根目录 `DEPLOYMENT.md` (步 4 新建)
- 本会话归档 (可选参考): [`aidb/archive/14-design.md`](aidb/archive/14-design.md)

### 边界提示 (步 0 可细化)

- **本章**: 系统要求、**Cargo feature** 矩阵与构建命令、`protoc`、作为 **path/crates 依赖** 嵌入、数据目录与 `Options` **部署向**摘要、`examples/`、备份/恢复操作入口、集群 **指向 AiKv DEPLOYMENT**、metrics **`register_into` + 嵌入方 HTTP** (ISSUE-014)
- **不重复**: 架构/数据流 → [`ARCHITECTURE.md`](aidb/ARCHITECTURE.md); 设计 why → [`DESIGN.md`](aidb/DESIGN.md); WAL/compaction/Raft 细节 → modules; 完整 Options 字段 encyclopedia → 可链 `config.rs` / 日后 `docs/development.md`
- **宜含**: feature 组合表、单机 vs cluster 构建差异、生产配置 preset (`for_high_*`)、examples 运行命令、备份 snippet、可观测性嵌入说明 (tracing 始终 / `monitoring` feature)
- **不宜含**: 迁移对照、ISSUES 全文、项目特性概览/Milestone (步 16 README)、CONTRIBUTING/CI 全文 (步 17 可链 `.github/README.md`)、AiKv 完整集群运维 (链 `aikv/DEPLOYMENT.md`, 步 21 再写)

### 与相邻根文档分工

| 文档 | 回答 |
|------|------|
| ARCHITECTURE | 是什么、怎么分层、数据怎么走 |
| DESIGN | 为什么这样选、放弃了什么 |
| **DEPLOYMENT (本篇)** | **怎么构建、开哪些 feature、怎么跑/嵌、目录与运维要点** |
| README (步 16) | 项目入口与特性概览 |

### 本次不要

- 不要跳过未确认的步骤
- 不要一次写多篇 (README/CONTRIBUTING 等留步 16+)
- 不要修改 `design.md` / 流程, 除非我明确要求
- 不要 commit, 除非我明确要求

---

请从步 0 开始, 给出本步交付物, 等我确认。

---

## 2. Assistant

先阅读规程、进度与索引，并核对 `aidb/` 现状，以便给出步 0 交付物。

---

## 3. Assistant

核对 `aidb/` 源码与相邻根文档，以便准确界定 DEPLOYMENT 章范围。

---

## 4. Assistant

## 步 0 — 定范围 (PROGRESS 步 15: `aidb/DEPLOYMENT.md`)

已阅读 `design.md`、`PROGRESS.md`、`INDEX.md`, 并核对当前 `aidb/` 无根目录 `DEPLOYMENT.md` (旧稿在 `backup/aidb/DEPLOYMENT.md`). 以下为步 0 交付物, **尚未写入任何文件**.

---

### 本章定位

| 项 | 内容 |
|----|------|
| PROGRESS | 步 15 / 阶段 2a 第 3 篇 |
| 对比深度 | **模块级** (构建/feature/运行/嵌入汇总, 不逐段对照 oldmain) |
| 核心问题 | **怎么构建、开哪些 feature、怎么跑/嵌入、数据目录与运维要点** |
| 文档形态 | 根目录汇总文, **无** Module Skill frontmatter |

---

### 拟读文件列表

#### A. 当前实现 (步 1 主读, 以源码为准)

| 优先级 | 路径 | 步 1 关注 |
|--------|------|-----------|
| P0 | `Cargo.toml` | feature 矩阵、default、bench、version |
| P0 | `build.rs` | `cluster` + `protoc` / checked-in codegen |
| P0 | `rust-toolchain.toml` | stable 工具链 |
| P0 | `src/lib.rs` | 公共 re-export、feature gate |
| P0 | `src/config.rs` | `Options` / `ClusterConfig`、`for_testing` / `for_high_*`、`validate` |
| P0 | `examples/{basic,backup,cluster}.rs` + `examples/README.md` | 可运行入口 |
| P1 | `AGENTS.md` | 构建/测试命令摘要 |
| P1 | `.github/README.md` + `workflows/ci.yml` | CI feature 矩阵 (正文仅链, 不全文) |
| P1 | `proto/raft.proto` | cluster 构建前提 (仅一句) |

#### B. 已完成 module / 根文档 (步 1 提炼, 不重复正文)

| 路径 | DEPLOYMENT 取用 |
|------|-----------------|
| `docs/modules/engine.md` | 数据目录、`LOCK`、单进程约束 |
| `docs/modules/engine-storage.md` | `Checkpoint` 运维入口 |
| `docs/modules/backup.md` | 备份/恢复 snippet、feature default |
| `docs/modules/cluster.md` | cluster 构建、`ClusterConfig`、**不展开** Raft 运维 |
| `docs/modules/observability.md` | `monitoring` feature、`register_into`、嵌入方 HTTP |
| `ARCHITECTURE.md` | 定位边界、feature 分层 (链, 不重写) |
| `DESIGN.md` | YAGNI (无内置 HTTP metrics、无 aidb-admin) |
| `ISSUES.md` | **ISSUE-014** 一行引用 |

#### C. 嵌入方参照 (步 1 抽样)

| 路径 | 用途 |
|------|------|
| `aikv/Cargo.toml` | `path` 依赖 + `aidb/monitoring`、`aidb/cluster` 传递 feature |

#### D. 旧文档 (步 2 按序查, 步 0 仅列范围)

| 顺序 | 路径 | 预期用途 |
|------|------|----------|
| 1 | `backup/aidb/DEPLOYMENT.md` | **主参考** (已知多处过时, 步 2 分类) |
| 2 | `aidb-oldmain/docs/{FOOLPROOF_OPS_GUIDE,BACKUP_RECOVERY,PERFORMANCE_TUNING,USER_GUIDE,DEVELOPMENT}.md` | 运维/调优查漏 |
| 2 | `aidb-oldmain/docs/monitoring/*` | INDEX 已记「无有效内容跳过」, 步 2 复核 |
| 3 | `wiqun-factory/docs/{BUILD,MONITORING}.md` | Docker/factory 构建查漏 (likely 偏 aikv) |
| 4 | WiQunTools `wiqun-db-inventory` | 查漏, 不迁入 |
| 5 | `wiqun-db` | 与 backup 重叠时二选一 |

#### E. 可选会话参考

- `aidb/archive/14-design.md` (边界与 ISSUE 引用习惯)

---

### 本章边界

#### 宜含 (拟大纲骨架)

1. **定位** — lib crate、非网络服务; 链 `ARCHITECTURE.md`
2. **系统要求** — Rust stable、`protoc` (cluster)、OS/磁盘/内存原则
3. **Cargo feature 矩阵** — `default`/`backup`/`cluster`/`monitoring`/`compression`(占位); 组合表 + 典型构建命令
4. **构建与验证** — `cargo build/test/clippy` 分 feature; 测试 `--test-threads=1`; 详表链 `AGENTS.md` + `.github/README.md` (步 16 `docs/development.md` 未建)
5. **作为依赖嵌入** — crates.io / `path`; feature 传递; 最小 `DB::open` 示例 (对齐当前 API)
6. **数据目录与 Options (部署向)** — 目录布局摘要、`LOCK`、持久化相关字段; **preset** (`for_high_write/read_throughput`); 全字段 encyclopedia → 链 `src/config.rs` / 日后 `docs/development.md`
7. **examples/** — 三例运行命令 (与 `examples/README.md` 一致)
8. **备份/恢复** — 短 snippet + 链 `docs/modules/backup.md`
9. **集群** — 仅「需 `--features cluster` + protoc」+ **链 `aikv/DEPLOYMENT.md`** (步 21 再写完整集群运维)
10. **可观测性** — tracing 始终; `monitoring` + `register_into`; **无** `--metrics-port`/OTLP env; ISSUE-014 一行
11. **相关文档** — modules / ARCHITECTURE / DESIGN / ISSUES

#### 不宜含

| 排除项 | 归属 |
|--------|------|
| 架构分层、数据流 | `ARCHITECTURE.md` |
| 设计 why / trade-off | `DESIGN.md` |
| WAL/compaction/Raft 实现细节 | `docs/modules/*` |
| 迁移对照、整理过程 | `backup/` only |
| ISSUES 全文 / 排查过程 | `ISSUES.md` |
| 项目特性概览、Milestone | 步 16 `README.md` |
| CONTRIBUTING/CI 全文 | 步 17; 本篇仅链 |
| AiKv 完整集群部署 | `aikv/DEPLOYMENT.md` (步 21) |
| Module Skill frontmatter | modules 专用 |

#### 与相邻根文档分工 (确认)

| 文档 | 回答 |
|------|------|
| ARCHITECTURE | 是什么、怎么分层、数据怎么走 |
| DESIGN | 为什么这样选 |
| **DEPLOYMENT (本篇)** | **怎么构建、开 feature、怎么跑/嵌、目录与运维要点** |
| README (步 16) | 项目入口与特性概览 |

---

### 步 2 预判 (旧稿已知陷阱, 供范围确认)

`backup/aidb/DEPLOYMENT.md` 中以下应在步 2 标 **已过时**, 步 4 **不写回**:

- 版本 `0.13` → 当前 `0.14.10`
- `Options` 默认值 (如 memtable 4MB) → 当前 default 64MB
- `db.batch()` → 当前 `WriteBatch::new()` + `db.write(&batch)`
- `--metrics-port`、`AIDB_OTLP_ENDPOINT`、`AIDB_JSON_LOG` → 已移除; 嵌入方负责 HTTP/OTel

---

### INDEX 本章行 (草案, 确认步 0 后可写入)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `DEPLOYMENT.md` | 🔄 步 0 | `backup/aidb/DEPLOYMENT.md`; oldmain 运维类; wiqun-factory BUILD/MONITORING | 待写 | 模块级汇总; ISSUE-014 一行; 旧稿多处过时 |

---

### 步 0 自检

- [x] 对照 PROGRESS 步 15 与 design 目标结构
- [x] 汇总文档, 非 module ↔ `src/` 一一映射
- [x] 覆盖构建/feature/嵌入/目录/备份/可观测性/集群指向
- [x] 明确与 ARCHITECTURE/DESIGN/modules 不重复
- [x] 未写 `aidb/` 正文、未改 INDEX/PROGRESS

---

请确认步 0: **范围与拟读列表是否 OK?** 如需调整 (例如增加/删减章节、oldmain 某篇必查), 说明后我修订; 确认后进入 **步 1 (读新代码)**.

---

## 5. User

可以

---

## 6. Assistant

步 0 已确认，开始步 1：以当前源码为准梳理构建、feature、嵌入与运维要点。

---

## 7. Assistant

## 步 1 — 读新代码 (交付物)

以当前 `aidb/` 源码与已落盘 modules 为准, 梳理 DEPLOYMENT 所需要点. **未写正文、未改 INDEX.**

---

### 1. 职责 (DEPLOYMENT 视角)

AiDb 是 **嵌入式 lib crate** (v0.14.10), 无独立进程/HTTP listener. DEPLOYMENT 需说明:

| 维度 | 当前实现 |
|------|----------|
| 形态 | `DB::open(path, Options)` 同步 API; `Send + Sync`, 可 `Arc` 多线程共享 |
| 默认 feature | `default = ["backup"]` — engine 始终编译, backup 默认开 |
| 可选 feature | `cluster` / `monitoring` / `compression` (后两者独立, compression **未实现**) |
| 构建前提 | stable ( `rust-toolchain.toml` ); cluster 需 `protoc` (或 checked-in codegen) |
| 嵌入方 | AiKv: `aidb = { path = "../aidb" }`; monitoring/cluster 经 feature 传递 |
| 运维入口 | 库 API + `examples/`; 无 `aidb-admin`; 集群运维在 AiKv 层 |

---

### 2. 代码地图 (部署相关)

```shell
aidb/
├── Cargo.toml              # feature 矩阵; version 0.14.10
├── build.rs                # cluster: proto/raft.proto → src/cluster/ (protoc 可选)
├── rust-toolchain.toml     # stable + clippy/rustfmt
├── proto/raft.proto        # cluster gRPC (Vote/AppendEntries/InstallSnapshot)
├── src/
│   ├── lib.rs              # re-export; feature gate backup/cluster/metrics
│   ├── config.rs           # Options, ClusterConfig, presets, validate
│   ├── engine/             # 始终编译; DB::open, WAL, SST, compaction
│   ├── backup/             # feature backup (default)
│   ├── cluster/            # feature cluster
│   └── metrics.rs          # feature monitoring
├── examples/
│   ├── basic.rs            # CRUD / WriteBatch / scan / snapshot
│   ├── backup.rs           # BackupManager + RecoveryManager
│   └── cluster.rs          # key_to_slot / hash tag (无网络)
└── .github/workflows/ci.yml  # test-default + test-cluster (+ bench)
```

**数据目录** (单机 `DB` path):

```shell
{db_path}/
├── LOCK                 # fs2 独占锁; 多进程 → Error::Busy
├── CURRENT              # 指向活跃 MANIFEST
├── MANIFEST-*           # VersionSet 元数据
├── {nnnnnn}_L{level}.sst
└── wal_{n}.log          # WAL 文件
```

备份存储 (`LocalFileStorage`) 布局见 `docs/modules/backup.md` — DEPLOYMENT 仅 snippet + 链 module.

---

### 3. Cargo feature 矩阵 (步 4 核心表)

| Feature | 默认 | 启用内容 | 典型构建 |
|---------|------|----------|----------|
| `backup` | ✅ | `backup/*`, ring/hex/serde_json | `cargo build` |
| (none) | — | 仅 engine | `cargo build --no-default-features` |
| `cluster` | ❌ | `cluster/*`, tonic/prost/tokio/openraft | `cargo build --features cluster` |
| `monitoring` | ❌ | `metrics.rs`, Prometheus 系列 | `cargo build --features monitoring` |
| `compression` | ❌ | snap/lz4 依赖 | **占位; block 压缩未实现** |

**常见组合**:

| 场景 | features |
|------|----------|
| 单机 + 备份 (默认) | (default) |
| 单机 + 指标 | `monitoring` 或 `backup,monitoring` |
| 集群库开发 | `cluster` (+ 可选 `monitoring`) |
| 最小 engine | `--no-default-features` |
| AiKv 生产 | aikv 侧 `cluster` / `monitoring` → 传递 `aidb/cluster`, `aidb/monitoring` |

---

### 4. 构建与验证

**本地** (与 `AGENTS.md` / CI 一致):

```bash
export RUSTFLAGS='-D warnings'
cargo fmt --check
cargo clippy --all-targets
cargo clippy --all-targets --features cluster   # 需 protoc
cargo test -- --test-threads=1
cargo test --features cluster -- --test-threads=1
```

**CI** (`ci.yml`):

- `test-default`: fmt + clippy (无 feature) + 全量 test
- `test-cluster`: apt 装 `protobuf-compiler` → clippy/test `--features cluster`
- `bench`: write/read/backup bench (依赖 test-default)
- **无** 独立 `monitoring` / `compression` job — 步 4 可注明需本地 `--features monitoring` 验证

**cluster + protoc** (`build.rs`):

- `CARGO_FEATURE_CLUSTER` 时编译 proto
- `PROTOC` 或 PATH 有 `protoc` → `tonic_build::compile`
- 否则 warning + 使用 checked-in 生成代码 (CI 仍装 protoc 做 clippy)

---

### 5. 嵌入与运行

**依赖声明**:

```toml
# crates.io (版本以 Cargo.toml 为准)
aidb = { version = "0.14", features = ["monitoring"] }

# monorepo (AiKv 现状)
aidb = { path = "../aidb" }
# aikv: monitoring = [..., "aidb/monitoring"]; cluster = ["aidb/cluster"]
```

**最小用法** (对齐 `examples/basic.rs`):

```rust
let db = aidb::DB::open("/var/data/aidb", aidb::config::Options::default())?;
db.put(b"k", b"v")?;
let mut batch = aidb::WriteBatch::new();
batch.put(b"a", b"1");
db.write(&batch)?;
let iter = db.scan(Some(b"a"), Some(b"z"))?;
db.close()?;
```

要点:

- `Options::validate()` 在 `DB::open` 内调用
- `create_if_missing: true` (default) 自动建目录
- `scan` 为 `Option<&[u8]>` 半开区间, 非旧稿 `scan(b"a", b"z")`

---

### 6. Options / ClusterConfig (部署向摘要)

**生产 default** (`Options::default()` 关键值, 与旧稿不同):

| 字段 | 默认 | 部署含义 |
|------|------|----------|
| `memtable_size` | 64 MiB | flush 阈值 |
| `block_cache_size` | 64 MiB | 读缓存 |
| `sync_wal` | false | true = 每条写 fsync (强持久, 低吞吐) |
| `use_wal` | true | false 则 crash 不保证持久 |
| `background_compaction` | true | 测试可关 |
| `compaction_threads` | 1 (clamp 1–4) | 后台 compaction 线程 |

**Preset** (步 4 应用):

| Preset | 用途 | 注意 |
|--------|------|------|
| `for_testing()` | 单测/示例 | 小 memtable、无 bloom、关 background_compaction |
| `for_high_write_throughput()` | 写密集 | 256MB memtable; 设 `CompressionType::Snap` 但 **压缩未实现** |
| `for_high_read_throughput()` | 读密集 | 512MB cache, 低 bloom FP |

**ClusterConfig** (`feature cluster`):

- `for_production()`: group_count=256, RF=3 (注释写 16384 slot 在 Router 层, group 256)
- `for_testing()`: 4 group, RF=1
- 完整集群 **启动/组网/端口** 不在 aidb — 链 AiKv DEPLOYMENT

---

### 7. examples 运行命令

| 示例 | 命令 |
|------|------|
| basic | `cargo run --example basic` |
| backup | `cargo run --example backup` |
| cluster | `cargo run --features cluster --example cluster` |

`cluster.rs` 仅演示 `key_to_slot` / hash tag, **不含** MultiRaft 组网.

---

### 8. 备份 / 可观测性 (部署入口)

**备份** (`examples/backup.rs` 为准):

- `LocalFileStorage::new(path)` + `BackupManager::new(storage, RetentionPolicy::default())`
- `create_backup(&db)` → `list_backups()` → `RecoveryManager::verify_backup` / `restore`
- `RetentionPolicy` default: min_count=3, max_count=30, min_age=1d, max_age=30d
- 详情 → `docs/modules/backup.md`

**可观测性**:

- **Tracing**: 始终 (`tracing` 硬依赖); 嵌入方配 `tracing-subscriber`
- **Prometheus**: 仅 `monitoring`; `DB::open` → `metrics::init()`; 嵌入方 `aidb::metrics::register_into(&registry)?`
- **无** 内置 HTTP、`--metrics-port`、OTLP/JSON log 环境变量 → ISSUE-014 一行
- 详情 → `docs/modules/observability.md`; HTTP scrape → aikv observability

---

### 9. 与 ARCHITECTURE / DESIGN 边界 (步 4 不重复)

| 已在相邻文档 | DEPLOYMENT 做法 |
|--------------|-----------------|
| 分层、数据流、feature 边界 | 链 `ARCHITECTURE.md` |
| 为何 lib / sync / 无 HTTP metrics | 链 `DESIGN.md` |
| WAL/compaction/Raft 细节 | 链 modules |
| Options 全字段 encyclopedia | 链 `src/config.rs` / 日后 `docs/development.md` |

---

### 10. 步 4 拟写要点清单 (源自步 1, 供后续核对)

1. 系统要求 (Rust stable, SSD, protoc@cluster)
2. Feature 矩阵 + 组合表 + 构建命令
3. CI 摘要 + 链 `.github/README.md` / `AGENTS.md`
4. path/crates 依赖 + 最小 embed 示例
5. 数据目录 + LOCK + 持久化相关 Options 摘要 + preset 表
6. examples 三命令
7. 备份 snippet (对齐 backup.rs)
8. 集群: `--features cluster` + **链 aikv/DEPLOYMENT** (步 21)
9. 可观测性: tracing / monitoring / register_into + ISSUE-014
10. 相关文档链接

---

请确认步 1: **以上梳理是否准确、有无遗漏?** 确认后进入 **步 2 (查旧文档)**.

---

## 8. User

可以

---

## 9. Assistant

步 1 已确认，开始步 2：按既定顺序查阅旧文档并分类。

---

## 10. Assistant

## 步 2 — 查旧文档 (交付物)

按 design.md 顺序查阅完毕. **未写 `aidb/` 正文、未改 INDEX/PROGRESS.**

---

### 旧文档清单与分类

| 顺序 | 路径 | 分类 | 说明 |
|------|------|------|------|
| 1 | `backup/aidb/DEPLOYMENT.md` | **部分仍有效 + 大量已过时** | 骨架可沿用; API/默认值/可观测性整段作废 (见下表) |
| 2a | `aidb-oldmain/docs/FOOLPROOF_OPS_GUIDE.md` | **已过时 — 跳过** | 全文 `aidb-admin` CLI; 现码无 `admin-cli` / binary |
| 2b | `aidb-oldmain/docs/BACKUP_RECOVERY.md` | **部分仍有效 + 已过时** | 流程概念 OK; API 签名、S3 示例与现码不符 |
| 2c | `aidb-oldmain/docs/PERFORMANCE_TUNING.md` | **原则可借鉴 + 大量已过时** | WriteBatch/并发思路可用; 默认值、`wal_sync_interval`、Snappy、具体 QPS 数字不可用 |
| 2d | `aidb-oldmain/docs/USER_GUIDE.md` | **已过时 — 跳过** | v0.1、Windows、`db.batch()` 等; 与 `examples/basic.rs` 重复且错误 |
| 2e | `aidb-oldmain/docs/DEVELOPMENT.md` | **无有效内容 — 跳过** | 通用 `cargo build`; 归属步 17 CONTRIBUTING / 日后 `development.md` |
| 2f | `aidb-oldmain/docs/monitoring/*` | **已过时 — 跳过** | INDEX 已记 MetricsServer 移除; 步 2 复核一致 |
| 3a | `wiqun-factory/docs/BUILD.md` | **无 aidb 有效内容 — 跳过** | Docker stage wiqun-db/kv; **aikv/factory 域**, 不进 aidb DEPLOYMENT |
| 3b | `wiqun-factory/docs/MONITORING.md` | **参考边界 — 不迁入** | 环境变量为 **WIQUN_***/aikv CLI; 印证 metrics/OTLP 在 **嵌入方**, 非 aidb |
| 4 | WiQunTools `wiqun-db-inventory/` | **查漏 — 不迁入** | `13-backup-bench` 称 backup 模块不存在 (已实现); `07-block-cache` monitoring 概念已在 observability module |
| 5 | `wiqun-db/DEPLOYMENT.md` | **与 backup 重复 — 跳过** | 同 backup/aidb 旧稿, 仅 crate 名不同 |

**顺带参考** (非步 2 主序, 已读):

| 路径 | 分类 | 说明 |
|------|------|------|
| `backup/aidb/README.md` | 部分有效 | 构建命令仍对; M5「内置 :9191 / OTel」已过时; v0.13 过时 |
| `backup/aidb/CONTRIBUTING.md` | 有效 (步 17) | cluster/monitoring 构建命令与现 CI 一致; DEPLOYMENT 可链, 不全文复制 |

---

### `backup/aidb/DEPLOYMENT.md` 逐段分类

| 段落 | 分类 | 步 4 处理 |
|------|------|-----------|
| 嵌入式 crate 定位 | ✅ 仍有效 | 保留, 一句链 ARCHITECTURE |
| 系统要求 (stable/SSD) | ✅ 仍有效 | 保留; **memtable 默认改 64 MiB** |
| `aidb = "0.13"` | ❌ 已过时 | → `0.14.x` / 以 `Cargo.toml` 为准 |
| 基本用法示例 | ❌ 已过时 | 对齐 `examples/basic.rs`: `WriteBatch::new`, `scan(Some, Some)` |
| 多线程 `Arc` | ✅ 仍有效 | 保留短示例 |
| Options 全表 (14 项) | ⚠️ 部分过时 | **不整表回迁**; 改「部署向摘要 + preset + 链 `config.rs`」 |
| 生产推荐 snippet | ⚠️ 部分有效 | 思路 OK; 与 `for_high_*` / default 对齐, 注明 `sync_wal` trade-off |
| 集群 → aikv DEPLOYMENT | ✅ 仍有效 | 保留; aikv 文档步 21 再写 |
| 备份 snippet | ⚠️ 部分有效 | 结构 OK; 对齐 `examples/backup.rs` (`Arc`, `RetentionPolicy::default`, 显式 path) |
| Prometheus `--metrics-port` | ❌ 已过时 | **删除**; 改 `register_into` + 链 observability / aikv |
| 指标列表 | ⚠️ 已过时 | 不全表; 链 `docs/modules/observability.md` (label/类型已变, ISSUE-018) |
| `AIDB_OTLP_ENDPOINT` | ❌ 已过时 | **删除**; 嵌入方 OTel (aikv) |
| `AIDB_JSON_LOG` | ❌ 已过时 | **删除**; tracing-subscriber 由嵌入方配置 |

---

### oldmain 仍可提炼的运维要点 (步 4 吸收方式)

| 来源 | 可提炼内容 | 写法 |
|------|------------|------|
| PERFORMANCE_TUNING | WriteBatch 批量写、`sync_wal` 可靠性 vs 吞吐 | preset 段 1–2 句 + 链 DESIGN |
| PERFORMANCE_TUNING | SSD/NVMe、compaction 线程 1–4 | 系统要求 / Options 摘要一句 |
| BACKUP_RECOVERY | 备份前 flush、restore 后验证 open | backup snippet 注释或链 backup module |
| BACKUP_RECOVERY | `BackupStorage` trait 自定义后端 | 一行「仅 `LocalFileStorage`; S3 未实现」→ ISSUE-012 已有 |
| wiqun-factory MONITORING | 嵌入方 env/端口模式 | **不写进 aidb**; 步 21 aikv DEPLOYMENT 参考 |

**明确不写进 aidb DEPLOYMENT**:

- FOOLPROOF_OPS / aidb-admin 全套命令
- PERFORMANCE_TUNING 具体 QPS/RocksDB 对比数字
- USER_GUIDE 安装/Windows
- factory Docker compose / `WIQUN_*` 变量表

---

### 拟写 ISSUES

**无新 ISSUE.** 旧稿偏离已被现有条目覆盖:

| 旧稿内容 | 已有 ISSUE | DEPLOYMENT 引用 |
|----------|------------|-----------------|
| 内置 HTTP/OTLP/JSON log | ISSUE-014 | 可观测性一节一行 |
| S3/增量备份 | ISSUE-012 | 备份一节可选一行 |
| `list_backups` vs `get_backup_info` | ISSUE-013 | 可不写 (运维细节在 backup module) |

---

### 步 4 文档表述建议 (源自步 2)

1. **以 `examples/*.rs` + 步 1 现码为 API 真源**, 旧 DEPLOYMENT 示例仅作结构参考.
2. **Options**: 三张表 — default 关键值、三个 preset、持久化/背压相关 5–6 项; 其余链 `src/config.rs`.
3. **可观测性**: 两 bullet — tracing 始终; `monitoring` + `register_into`; **禁止** 出现 `--metrics-port` / `AIDB_*` env.
4. **集群**: 2 句 + 链 `aikv/DEPLOYMENT.md` (占位说明步 21 再写).
5. **构建**: 沿用 backup README/CONTRIBUTING 命令 + CI 两 job 摘要; 测试矩阵详表留 `development.md` / 步 17.
6. **bench**: 可选一句 `cargo bench` + `AIDB_BENCH_PRELOAD` (现码 `benches/read_bench.rs` 有效).

---

### INDEX 本章行 (步 2 草案, 确认后可写入 backup/INDEX.md)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `DEPLOYMENT.md` | 🔄 步 2 | `backup/aidb/DEPLOYMENT.md` (主, 多处过时); `backup/aidb/README.md` §构建; oldmain `BACKUP_RECOVERY`/`PERFORMANCE_TUNING` (查漏); wiqun-factory `MONITORING` (边界); oldmain FOOLPROOF/USER_GUIDE/monitoring 跳过; wiqun-db 重复跳过 | 待写 | 模块级; ISSUE-014 一行; 无新 ISSUE |

---

请确认步 2: **分类与步 4 写法是否 OK?** 确认后进入 **步 3 (对比 oldmain 代码, 模块级)**.

---

## 11. User

可以

---

## 12. Assistant

步 2 已确认，开始步 3：在 oldmain 中做模块级代码对照。

---

## 13. Assistant

## 步 3 — 对比旧代码 (模块级)

抽样对照 `aidb-oldmain` / `wiqun-db` 与当前 `aidb/` 的 **部署相关** 面 (构建、feature、嵌入、Options、备份、可观测性、examples). **未写正文.**

---

### 差异总表

| 域 | oldmain / 旧稿 | 当前 `aidb/` | 主流程是否等价 | DEPLOYMENT 表述 |
|----|----------------|--------------|----------------|-----------------|
| **产品形态** | lib + `aidb-admin` (`admin-cli` feature, `[[bin]]`) | **仅 lib crate**, 无 binary | ❌ 运维入口不同 | 明确「无 CLI」; 运维 = 库 API + `examples/`; 集群链 AiKv |
| **default feature** | `snappy` | `backup` | ❌ | 写 `default = ["backup"]`; `compression` 独立且未实现 |
| **cluster feature** | `cluster` + `raft-cluster` 两级 | 单一 `cluster` (含 OpenRaft) | ⚠️ 合并简化 | 仅 `--features cluster`; 不提 `raft-cluster` |
| **monitoring** | `MetricsServer` (hyper) + `examples/metrics_server` | `metrics::register_into`; 无 HTTP | ❌ | 嵌入方注册 + scrape; **ISSUE-014** 一行; 删 `--metrics-port` |
| **protoc / build** | `protobuf-src` 内置; `aidb.proto` + `raft.proto` | 系统 `protoc` 或 checked-in; 仅 `raft.proto` | ⚠️ 构建更简单 | 文档 protoc 安装 + CI `apt protobuf-compiler` |
| **Options 默认** | memtable 4MB, cache 8MB | memtable/cache **64MB**; 更多 stall/subcompaction 项 | ⚠️ 默认值变 | 不写旧表; 摘要 default + preset |
| **Options preset** | builder + 零散 high_* | `for_testing` / `for_high_write/read_throughput` + `validate()` | ✅ 意图等价 | 用 preset 替代旧「生产 snippet 手写字段」 |
| **不存在的选项** | 旧 PERFORMANCE_TUNING 有 `wal_sync_interval` | **无此字段** | ❌ | 不写; 仅 `sync_wal` bool |
| **DB::open / LOCK** | 单进程 `LOCK` (fs2) | 同 | ✅ | 保留数据目录 + LOCK 一句 |
| **scan API** | `scan(Option, Option)` (oldmain `iterator.rs`) | 同 | ✅ | 旧 DEPLOYMENT `scan(b"a", b"z")` **错误**, 用 `Some(...)` |
| **WriteBatch** | `WriteBatch::new()` + `db.write` | 同 | ✅ | 旧 `db.batch()` **错误** |
| **BackupManager** | `new(storage)` 或 `with_retention_policy` | `new(storage, RetentionPolicy)` | ✅ 语义等价 | 对齐 `examples/backup.rs` |
| **RecoveryManager** | `RecoveryManager::restore(&manager, id, path)` 静态 | `RecoveryManager::new(storage).restore(id, path)` | ✅ 流程等价 | 用实例方法 |
| **备份存储** | Local + 文档 S3 示例 | 仅 `LocalFileStorage` + trait | ⚠️ S3 未实现 | 一行链 backup module / **ISSUE-012** |
| **集群 examples** | 6 个 network demo (`node_runner` 等) | `cluster.rs` 仅 CRC16/hash tag | ❌ 演示范围缩小 | 说明 cluster example 不含组网; 完整部署 → aikv |
| **bench** | write + read | + `backup_bench`; `AIDB_BENCH_PRELOAD` | ✅ 扩展 | 可选一句 `cargo bench` |
| **wiqun-db** | feature 矩阵与现 aidb 一致 | = 当前 aidb 中间态 | ✅ | 步 3 不另查; 以现 `aidb/` 为准 |

---

### 分域说明 (模块级)

#### 1. 构建与 feature

```text
oldmain:  default=snappy | cluster | raft-cluster | monitoring(+hyper) | admin-cli
current:  default=backup  | cluster | monitoring | compression(占位)
```

- oldmain **monitoring** 拉入 hyper/tokio 为内置 HTTP; 现 **monitoring** 仅 Prometheus 库 + `register_into`, 无 listener.
- oldmain **admin-cli** 与 release workflow 打 `aidb-admin` 包; 现 **无 `[[bin]]`**, CI 也不构建 admin.
- `wiqun-db/Cargo.toml` feature 与现 aidb 对齐 → 确认新仓库即部署真源.

#### 2. 嵌入与运行

| 项 | oldmain | current |
|----|---------|---------|
| 公开 API 面 | `DB`, `Options`, `monitoring::MetricsServer` | `DB`, `Options`, `metrics::register_into` |
| 网络服务 | aidb 可自建 `:9090/metrics` | **无**; AiKv `path = "../aidb"` + feature 传递 |
| 同步 API | sync DB | 同 (AiKv `spawn_blocking`) |

#### 3. Options / 数据目录

- oldmain `Options::default()` memtable **4MB** → 现 **64MB**; 旧 DEPLOYMENT/USER_GUIDE 数值均不可沿用.
- 现增: `level0_*_trigger`, `subcompaction_*`, `*_poll_ms`, `max_manifest_size` 等 — DEPLOYMENT **不全表**, 链 `config.rs`.
- `for_high_write_throughput()` 设 `CompressionType::Snap` — 与 DESIGN 一致, 文档须注 **compression 未实现**.

#### 4. 备份 (`src/backup/`)

- 创建/恢复主路径等价: Checkpoint → manifest → SHA256 → restore tmp → `DB::open` 冒烟 → rename.
- API 表面变化: `RetentionPolicy` 必填入 `new`; `RecoveryManager` 实例化 — **仅 snippet 差异**.
- oldmain `BackupType::Incremental` 等 — 现无; 不写入 DEPLOYMENT.

#### 5. 可观测性 (`monitoring/` → `metrics.rs`)

```text
oldmain: src/monitoring/{server,metrics}.rs — MetricsServer :9090/metrics
current: src/metrics.rs — LazyLock + record_* + register_into(registry)
```

- old `examples/metrics_server.rs` **无对应** — 勿写「运行 metrics_server example」.
- OTel/JSON log 从未在现 `aidb/src` 接线 — 旧 `AIDB_*` env 属 oldmain 文档臆测或已删代码.

#### 6. 集群 (`cluster/`)

- oldmain: `raft-cluster` + 多 example 起节点; proto 双文件.
- current: 统一 `cluster`, `ClusterConfig::for_production/for_testing`, gRPC 在 `cluster/network.rs`.
- **组网/端口/compose** 均在 AiKv — aidb DEPLOYMENT 只写 feature + `ClusterConfig` 摘要 + 链 aikv.

---

### 待核实

| 点 | 结论 | 处理 |
|----|------|------|
| 内置 HTTP metrics 是否回归 | 现码无 | **ISSUE-014** (doc-only, 已有) |
| S3 / 增量备份 | 现码无 | **ISSUE-012** (可选一行) |
| `for_high_write_throughput` 设 Snap | 压缩未实现 | DESIGN 已述; DEPLOYMENT preset 表加脚注 |
| oldmain QPS/延迟数字 | 未验证 | **不写入** DEPLOYMENT |

**无新 ISSUE.**

---

### 步 4 文档表述建议 (步 3 结论)

1. **真源优先级**: `Cargo.toml` / `examples/*.rs` / `config.rs` > old DEPLOYMENT > oldmain 文档.
2. **删整节**: aidb-admin、MetricsServer、`metrics_server` example、`AIDB_*` env、`--metrics-port`.
3. **重写节**: feature 矩阵 (含 default=backup)、protoc、Options default+preset、备份 snippet、可观测性嵌入.
4. **保留/缩短**: 系统要求、Arc 多线程、数据目录+LOCK、集群 2 句 + 链 aikv.
5. **examples 说明**: 三例 + 注明 `cluster.rs` 仅路由工具函数, 非集群部署.
6. **旧 main 等价性**: 备份/恢复/Open+LOCK **流程仍成立**; 运维从「CLI + 内置 metrics」变为「库 + 嵌入方 (AiKv)」.

---

### INDEX 本章行 (步 3 更新草案)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `DEPLOYMENT.md` | 🔄 步 3 | 同上 + oldmain `Cargo.toml`/`monitoring/`/`bin/aidb-admin`/`backup/` 模块级对照 | 待写 | admin/MetricsServer 移除; API/preset 以现码为准; ISSUE-014 |

---

请确认步 3: **差异表与步 4 写法是否 OK?** 确认后进入 **步 4 (正文草稿)**.

---

## 14. User

可以

---

## 15. Assistant

步 3 已确认。正在核对相邻根文档的链接与引用格式，随后给出步 4 正文草稿。

---

## 16. Assistant

## 步 4 — 正文草稿 (`aidb/DEPLOYMENT.md`)

以下为拟写入内容, **尚未落盘**. 请确认或提出修改意见; 确认后再写入 `aidb/`、更新 `backup/INDEX.md` 与 `PROGRESS.md`.

---

```markdown
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
| `compression` | ❌ | snap/lz4 依赖 | **占位; block 压缩未实现** |

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

磁盘格式与早期 `aidb-oldmain` **不兼容** — 勿跨版本直接打开旧目录. 布局细节见 [docs/modules/engine.md](docs/modules/engine.md)、[engine-storage.md](docs/modules/engine-storage.md).

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
| `Options::for_high_write_throughput()` | 写密集 | 大 memtable; 设 `CompressionType::Snap` 但 **压缩未实现** |
| `Options::for_high_read_throughput()` | 读密集 | 大 block cache、低 bloom FP |

强持久场景可在 default 或 preset 基础上设 `sync_wal: true`, 并酌情调大 `memtable_size` / `block_cache_size`.

### ClusterConfig (`feature cluster`)

| 方法 | group_count | replication_factor | 典型场景 |
|------|-------------|-------------------|----------|
| `ClusterConfig::for_production()` | 256 | 3 | 生产 (slot 路由见 cluster module) |
| `ClusterConfig::for_testing()` | 4 | 1 | 单测 |

`RaftNodeConfig` 等运行时参数见 [docs/modules/cluster.md](docs/modules/cluster.md). **集群进程启动、端口、compose** 不在 aidb — 见 [AiKv 部署](../aikv/DEPLOYMENT.md).

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
- AiKv `BGSAVE` 直调 `Checkpoint`, 不经 `BackupManager` — 见 aikv [commands-extended.md](../aikv/docs/modules/commands-extended.md).
- 详情见 [docs/modules/backup.md](docs/modules/backup.md).

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

AiKv 在 `Metrics::new()` 内完成注册并暴露 `/metrics`. 指标列表与 PromQL 见 [docs/modules/observability.md](docs/modules/observability.md).

## 集群 (库侧)

构建: `cargo build --features cluster` (需 protoc, 见上).

aidb 提供 MetaRaft / Multi-Raft、slot 路由与 gRPC; **MOVED/ASK、CLUSTER 命令、节点部署** 由 AiKv 实现. 完整集群运维见 [AiKv 部署](../aikv/DEPLOYMENT.md) (aikv 文档整理步 21).

库侧深入: [docs/modules/cluster.md](docs/modules/cluster.md).

## 相关文档

- [ARCHITECTURE.md](ARCHITECTURE.md) — 分层与数据流
- [DESIGN.md](DESIGN.md) — 设计决策
- [AGENTS.md](AGENTS.md) — AI 助手与 CI 速查
- [docs/modules/](docs/modules/) — 域级实现文档
- [ISSUES.md](ISSUES.md) — 待核实项

## 待核实

- HTTP `/metrics` 与 OTel 运行在嵌入方 (AiKv), 非 aidb 库内 — 见 [ISSUES.md#ISSUE-014](ISSUES.md#issue-014-httpoteljson-log-运行在嵌入方-aidb-仅库内指标).
```

---

### 草稿自检 (design.md 验收项)

| 项 | 状态 |
|----|------|
| 与 `Cargo.toml` features / version 一致 | ✅ |
| 与 `examples/*.rs` API 一致 | ✅ |
| 与 CI (`ci.yml`) 一致 | ✅ |
| 与 ARCHITECTURE / DESIGN 不矛盾 | ✅ (链出, 不重复架构/why) |
| 无 `--metrics-port` / `AIDB_*` / `aidb-admin` | ✅ |
| 无迁移历史 | ✅ |
| ISSUE-014 一行 | ✅ |
| 集群链 aikv DEPLOYMENT (占位) | ✅ |

### 落盘时将同步

- 新建 `aidb/DEPLOYMENT.md`
- `backup/INDEX.md` — DEPLOYMENT 行 → ✅
- `backup/PROGRESS.md` — 步 15 → ✅ 完成

---

请确认步 4 草稿: **可以落盘 / 需要调整 (请指出段落)**.

---

## 17. User

可以

---

## 18. Assistant

草稿已确认，正在写入 `aidb/DEPLOYMENT.md` 并更新 INDEX 与 PROGRESS。

---

## 19. Assistant

## 步 15 已完成

已落盘并更新进度:

| 文件 | 动作 |
|------|------|
| [`aidb/DEPLOYMENT.md`](aidb/DEPLOYMENT.md) | 新建 |
| [`AiKv-Workflow/backup/INDEX.md`](AiKv-Workflow/backup/INDEX.md) | DEPLOYMENT 行 → ✅ |
| [`AiKv-Workflow/backup/PROGRESS.md`](AiKv-Workflow/backup/PROGRESS.md) | 步 15 → ✅ 2026-06-18 |

**本篇覆盖**: 系统要求、feature 矩阵、构建/CI、path 嵌入、数据目录、Options 部署摘要与 preset、examples、备份 snippet、可观测性 (`register_into` + ISSUE-014)、集群库侧 + 链 aikv DEPLOYMENT.

**未做**: git commit (未要求); 旧稿 `backup/aidb/DEPLOYMENT.md` 保留作参考 (已在 aidb 仓库外).

---

**阶段 2a 进度**: 步 13–15 ✅ · 下一步 **步 16 `README.md`**. 新开对话时可复制 `SESSION-PROMPT.md` 模板, 填 PROGRESS 步 16.

---

## 20. User

使用 vibe-coding/scripts/archive-cursor-session.sh  帮我保存当前对话到 archive 里面 带 tool 和不带 tool 的都保存
