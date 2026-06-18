# Cursor transcript

---

## 1. User

## 文档整理 — 新会话

工作目录: `/root/code/database`

请先阅读 (不要跳过):
1. `AiKv-Workflow/backup/design.md` — 总规程
2. `AiKv-Workflow/backup/PROGRESS.md` — 当前进度
3. `AiKv-Workflow/backup/INDEX.md` — 本章索引 (若已有行则更新)

---

### 本次任务

| 项 | 值 |
|----|-----|
| PROGRESS 总步 | 16 (阶段 2a 第 4 篇) |
| 目标文件 | `aidb/README.md` |
| 本次子步 | 从步 0 开始 |
| 对比深度 | **模块级** (汇总文档; 项目入口与特性概览, 不加深章逐段) |

### 必须遵守

- **确认门控**: 步 0→1→2→3→4, 每步先讨论, 我确认后再下一步
- **步 4**: 先出 **正文草稿**, 我确认后再写入 `aidb/`、更新 INDEX/PROGRESS
- **汇总文档**: **不必** Module Skill frontmatter; 作为 **仓库入口** — 一句话定位、特性概览、最小 quick start、文档导航; **不重复** 根文档与 module 正文
- **过程只进 backup/**: INDEX; aidb 不写迁移历史
- **旧文档顺序**: `backup/aidb/README.md` (主) → `aidb-oldmain` 根目录/README 类 → WiQunTools 查漏 → `wiqun-db`
- **待核实**: 根文档至多简要提及; 详情仍引用 `aidb/ISSUES.md` (不展开排查过程)
- **步 4 落盘前**: 对照 design.md 验收项 — 链接有效、与当前 `Cargo.toml`/examples 一致、与 ARCHITECTURE/DESIGN/DEPLOYMENT 不矛盾

### 上下文 (上一篇已完成)

- 步 13 ✅: [`aidb/ARCHITECTURE.md`](aidb/ARCHITECTURE.md) — 分层、数据流、feature 边界
- 步 14 ✅: [`aidb/DESIGN.md`](aidb/DESIGN.md) — 跨模块设计决策
- 步 15 ✅: [`aidb/DEPLOYMENT.md`](aidb/DEPLOYMENT.md) — 构建、feature、嵌入、目录与运维要点
- 阶段 1 modules 全部 ✅; `docs/development.md` **尚未**创建
- 旧稿: [`AiKv-Workflow/backup/aidb/README.md`](AiKv-Workflow/backup/aidb/README.md) (已移出 aidb; **含过时内容**: Milestone M5「内置 :9191/OTel」、v0.13、完整测试矩阵、架构 mermaid 与 ARCHITECTURE 重复)
- 当前 `aidb/` **尚无** 根目录 `README.md` (步 4 新建)
- 本会话归档 (可选参考): [`aidb/archive/15-deployment.md`](aidb/archive/15-deployment.md)

### 边界提示 (步 0 可细化)

- **本章**: 项目标题与一句话定位、**能力特性** bullet/短表 (单机 LSM / 可选 cluster·backup·monitoring)、与 **AiKv** 关系一句、**最小** quick start (依赖 + 5 行代码, 对齐 `examples/basic.rs`)、**文档导航** (ARCHITECTURE / DESIGN / DEPLOYMENT / AGENTS / docs/modules / ISSUES)、许可
- **不重复**: 架构图与数据流 → [`ARCHITECTURE.md`](aidb/ARCHITECTURE.md); 设计 why → [`DESIGN.md`](aidb/DESIGN.md); feature 矩阵与构建命令 → [`DEPLOYMENT.md`](aidb/DEPLOYMENT.md); 完整测试矩阵 → 链 DEPLOYMENT + 日后 CONTRIBUTING/`docs/development.md`
- **宜含**: 当前版本号 (Cargo.toml)、feature 一览 (链 DEPLOYMENT 详表)、examples 一行、bench 可链 DEPLOYMENT
- **不宜含**: Milestone 历史长表 (除非极简「现状」一句)、迁移对照、ISSUES 全文、CONTRIBUTING/CI 全文 (步 17)、AiKv 使用指南 (aikv README 步 22)

### 与相邻根文档分工

| 文档 | 回答 |
|------|------|
| **README (本篇)** | **项目入口**: 是什么、能做什么、怎么 30 秒跑起来、去哪读 |
| ARCHITECTURE | 分层与数据流 |
| DESIGN | 为什么这样设计 |
| DEPLOYMENT | 怎么构建、开 feature、嵌入与运维 |

### 本次不要

- 不要跳过未确认的步骤
- 不要一次写多篇 (CONTRIBUTING/CHANGELOG/docs/README 留步 17–18)
- 不要修改 `design.md` / 流程, 除非我明确要求
- 不要 commit, 除非我明确要求

---

请从步 0 开始, 给出本步交付物, 等我确认。

---

## 2. Assistant

先读规程、进度与索引，并核对 `aidb/` 现状，以便给出步 0 交付物。

---

## 3. Assistant

继续核对 `aidb/` 现状与相邻根文档边界，以便定步 0 范围。

---

## 4. Assistant

## 步 0 — 定范围 (PROGRESS 步 16 · `aidb/README.md`)

对照 `design.md`、PROGRESS 与已完成相邻根文档, 本章是 **阶段 2a 汇总入口**, 对比深度 **模块级**. 本章 **不** 映射单一 `src/` 域, 而是汇总全 crate 的 **对外叙事**; 实现细节仍以 modules + 步 13–15 根文档为准.

---

### 1. 本章定位 (一句话)

**仓库入口 README**: 回答「是什么、能做什么、30 秒怎么跑、下一步读哪」; **不** 承担架构图、设计 why、构建/feature 详表、测试矩阵.

---

### 2. 拟读文件列表 (步 1–4 依据)

| 优先级 | 路径 | 用途 |
|--------|------|------|
| **主** | [`aidb/Cargo.toml`](aidb/Cargo.toml) | 版本 `0.14.10`、feature、license |
| **主** | [`aidb/src/lib.rs`](aidb/src/lib.rs) | 公共 re-export、feature gate |
| **主** | [`aidb/examples/basic.rs`](aidb/examples/basic.rs) | quick start 对齐 |
| **主** | [`aidb/examples/README.md`](aidb/examples/README.md) | examples 一行导航 |
| **邻** | [`aidb/ARCHITECTURE.md`](aidb/ARCHITECTURE.md) | 避免重复分层/数据流/mermaid |
| **邻** | [`aidb/DESIGN.md`](aidb/DESIGN.md) | 避免重复设计 why |
| **邻** | [`aidb/DEPLOYMENT.md`](aidb/DEPLOYMENT.md) | feature 详表、构建、bench、嵌入 — 链出即可 |
| **邻** | [`aidb/AGENTS.md`](aidb/AGENTS.md) | 定位/AiKv 一句表述一致 |
| **邻** | [`aidb/docs/modules/*.md`](aidb/docs/modules/) (5 篇) | 文档导航 WHEN 一行 |
| **邻** | [`aidb/ISSUES.md`](aidb/ISSUES.md) | 至多根文档一行「待核实见 ISSUES」 |
| **步 2 主** | [`AiKv-Workflow/backup/aidb/README.md`](AiKv-Workflow/backup/aidb/README.md) | 旧稿结构参考; **多处过时** |
| **步 2 查漏** | [`aidb-oldmain/README.md`](aidb-oldmain/README.md) | P2P/Coordinator、aidb-admin、Snappy 等 — **不采纳** |
| **步 2 可选** | `aidb-oldmain/examples/README.md` | examples 命名查漏 |
| **步 2 跳过** | wiqun-db | 与 backup 重复 |
| **步 2 可选** | WiQunTools inventory | 仅查漏是否有 README 遗漏能力 |

**步 3 (模块级)**: 旧 README 叙事 vs 当前 `lib.rs` + 步 13–15 根文档 — 标记 **仍有效 / 已过时 / 宜链出**, 不做 oldmain 逐段对照.

---

### 3. 本章边界

#### ✅ 宜含

| 块 | 内容要点 | 深度 |
|----|----------|------|
| 标题 + 定位 | 嵌入式 LSM-Tree KV (Rust); lib crate; 核心 engine 零可选依赖; cluster/backup/monitoring 按需 feature | 1–2 句 |
| 能力特性 | 单机 LSM (WAL/MemTable/SSTable/compaction/MVCC/WriteBatch 等 **概览 bullet**); 可选 cluster (MetaRaft+MultiRaft+slot)、backup、monitoring (`register_into`, **非** 内置 HTTP) | 短 bullet 或简表 |
| 与 AiKv | AiDb = 存储+共识层; AiKv = RESP/Cluster/HTTP metrics; monorepo `path` 依赖 | **一句** |
| 版本 | `0.14.10` (与 Cargo.toml 一致) | 行内或副标题 |
| Feature 一览 | `backup`(默认) / `cluster` / `monitoring` / `compression`(占位) | **简表 + 链 DEPLOYMENT 详表** |
| Quick start | `[dependencies]` + **约 5 行** `DB::open` / `put` / `get` (对齐 `basic.rs`, 不比 DEPLOYMENT 长) | 最小可运行 |
| 运行示例 | `cargo run --example basic`; 另两例一行链 `examples/README.md` | 一行 |
| Bench | `cargo bench` 一句 + 链 DEPLOYMENT §基准 | 一行 |
| 文档导航 | ARCHITECTURE / DESIGN / DEPLOYMENT / AGENTS / docs/modules / ISSUES | 链接表 |
| 许可 | **MIT OR Apache-2.0** (与 Cargo.toml 一致, 旧稿仅 MIT 需修正) | 一行 |

#### ❌ 不含 (链相邻文档)

| 内容 | 归属 |
|------|------|
| 架构 mermaid / 数据流 | [`ARCHITECTURE.md`](aidb/ARCHITECTURE.md) |
| 设计取舍、与 RocksDB 对比 | [`DESIGN.md`](aidb/DESIGN.md) |
| feature 矩阵、clippy/CI、目录布局、嵌入详例、protoc | [`DEPLOYMENT.md`](aidb/DEPLOYMENT.md) |
| 完整测试矩阵、hooks | 步 17 CONTRIBUTING + 日后 `docs/development.md` |
| Milestone M1–M5 长表、v0.13「内置 :9191/OTel」 | **过时**; 至多一句「当前 0.14.x」 |
| Module Skill frontmatter | 汇总文档不需要 |
| ISSUES 全文、迁移对照、CONTRIBUTING/CHANGELOG 正文 | 步 17 / ISSUES |
| AiKv 使用指南 | aikv README (步 22) |

#### 与相邻根文档分工 (确认)

| 文档 | README 只… |
|------|------------|
| ARCHITECTURE | 链过去, 不写分层图 |
| DESIGN | 链过去, 不写 why |
| DEPLOYMENT | 特性名 + 「详见 DEPLOYMENT」 |
| AGENTS | AI 助手入口; README 可链, 不重复 CI 命令块 |

---

### 4. 拟章节骨架 (步 4 草稿方向, 步 0 仅定结构)

```markdown
# AiDb
<!-- 一句话定位 + 可选版本 -->

## 特性          <!-- bullet/简表: 单机 + 可选 cluster/backup/monitoring -->
## 与 AiKv       <!-- 一句 -->
## 快速开始      <!-- toml + ~5 行 rust -->
## 示例          <!-- 链 examples/README.md -->
## 文档          <!-- 导航表 -->
## 许可
```

预估篇幅: **~60–90 行** (远短于旧 backup README ~100 行, 且无 mermaid/测试矩阵).

---

### 5. 旧稿已知过时点 (步 2 预标记, 供后续核对)

[`backup/aidb/README.md`](AiKv-Workflow/backup/aidb/README.md) 中 **不宜回迁**:

- 版本 `0.13` → 当前 **`0.14.10`**
- M5「Prometheus `:9191/metrics`、OTel 导出」→ **monitoring 为库内注册, HTTP 在 AiKv**
- 架构 mermaid → 与 ARCHITECTURE 重复
- 完整测试矩阵 / `CONTRIBUTING` 构建节 → DEPLOYMENT + 步 17
- 许可仅 MIT → Cargo.toml 为 **MIT OR Apache-2.0**

**仍可借鉴**: 标题风格、特性 bullet 组织、quick start 形态 (需缩短并对齐 `basic.rs`).

---

### 6. 拟写入 INDEX 的本章行 (确认步 0 后落 backup/, 步 4 再 ✅)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `README.md` (入口) | 步 0 定范围 | `backup/aidb/README.md`; oldmain `README.md` (查漏); WiQunTools 查漏 | 待步 4 | 模块级; 不重复 ARCH/DESIGN/DEPLOY; 旧稿 M5/:9191/v0.13 过时 |

---

### 7. 步 0 验收自检 (对照 design.md)

- [x] 对照 PROGRESS 步 16, 明确为汇总入口 (非 module)
- [x] 拟读文件列表 + 旧文档顺序 (backup → oldmain → WiQunTools → wiqun-db)
- [x] 与步 13–15 边界清晰, 避免重复
- [x] quick start 锚点: `examples/basic.rs` + Cargo.toml 版本/feature
- [x] 步 4 前不写 `aidb/README.md`, 不更新 PROGRESS ✅

---

请确认步 0: **范围 / 边界 / 拟读列表 / 章节骨架** 是否 OK, 或需要调整 (例如: 是否要 CI badge、特性表是否改为纯 bullet、quick start 是否再缩短). 确认后进入 **步 1 (读新代码)**.

---

## 5. User

可以

---

## 6. Assistant

步 0 已确认，开始步 1：以当前源码为准梳理职责与对外面。

---

## 7. Assistant

## 步 1 — 读新代码 (交付物)

以当前 `aidb/` 源码与步 13–15 根文档为准, 梳理 README 应反映的 **对外事实**. 以下为讨论稿, **尚未写正文**.

---

### 1. 项目职责 (README 一句话素材)

- **形态**: Rust **lib crate** (`aidb`), 嵌入式 LSM-Tree KV; **无** 内置网络 listener / HTTP 服务.
- **默认能力**: `engine/` 始终编译 — WAL → MemTable → SSTable → Leveled Compaction; 同步 `DB` API.
- **按需扩展** (Cargo feature): `backup`(默认开) · `cluster` · `monitoring`; `compression` 为依赖占位、**块压缩未实现**.
- **生态**: [AiKv](../aikv/) 通过 `path = "../aidb"` 嵌入, 提供 RESP / Cluster / HTTP `/metrics`; AiDb 提供存储与 Raft 基础设施.

---

### 2. 代码地图 (入口级, 非 module 深度)

| 域 | `src/` | 公共面 | README 是否点名 |
|----|--------|--------|-----------------|
| 配置 | `config.rs` | `Options`, `CompressionType` | quick start 用 `Options::default()` |
| 引擎 | `engine/*` | `DB`, `WriteBatch`, `Snapshot`, `Checkpoint`, `BlockCache` | 特性 bullet 主语 |
| 错误 | `error.rs` | `Error`, `Result` | 一般不写 |
| 备份 | `backup/*` (`backup` feature) | `BackupManager`, `RecoveryManager`, … | 特性一行 |
| 集群 | `cluster/*` (`cluster` feature) | MetaRaft / MultiRaft / Router / slot 等 **pub mod** | 特性一行 |
| 指标 | `metrics.rs` (`monitoring`) | `aidb_*` Prometheus 系列, `register_into` | 强调 **嵌入方 scrape**, 非 `:9191` |
| Crate 根 | `lib.rs` | re-export + feature gate | 「公共 API 精简」可一句带过 |

**`lib.rs` 公共 re-export** (README quick start 只需 subset):

```16:19:aidb/src/lib.rs
pub use engine::cache::{BlockCache, CacheStats};
pub use engine::checkpoint::Checkpoint;
pub use engine::db::{DbIterGuard, Snapshot, WriteBatch, WriteOp, DB};
pub use error::{Error, Result};
```

---

### 3. 主流程要点 (README 特性 bullet 依据)

#### 单机 LSM (始终可用)

| 能力 | API / 行为 | 备注 |
|------|------------|------|
| 打开/关闭 | `DB::open(path, Options) -> Arc<DB>`, `close()` | 目录锁 `LOCK`; `Send + Sync` |
| CRUD | `put` / `get` / `delete` | |
| 批量写 | `WriteBatch` + `write()` | 原子 |
| 范围 | `scan(start, end)`, `iter()` | MVCC 可见性 |
| 快照读 | `snapshot()` → `Snapshot::get/scan` | sequence 隔离 |
| 区间删 | `delete_range` | |
| 刷盘 | `flush()`, 后台 compaction | 运维向, README 可不写 |
| 点快照 | `Checkpoint` | 备份底层; 默认 feature 下可用 |

写路径 (概览一句即可): stall 检查 → WAL append → MemTable → 满则 freeze/flush → L0+ → compaction.

#### 可选: backup (default)

- `BackupManager::create_backup` 等; 基于 `Checkpoint`; 见 `examples/backup.rs`.
- `--no-default-features` 可关掉 backup mod.

#### 可选: cluster

- MetaRaft (元数据) + MultiRaft (16384 slot, CRC16, per-Group DB); gRPC + OpenRaft.
- **协议语义** (MOVED/ASK) 在 AiKv; AiDb 提供 slot 路由与存储面.
- `examples/cluster.rs` 仅演示 slot/hash tag, 非完整集群部署.

#### 可选: monitoring

- Prometheus 指标注册; 嵌入方调用 `register_into` 后自行暴露 HTTP.
- **无** AiDb 内置 metrics 端口.

---

### 4. 版本 / feature / 示例 / bench (与 Cargo 一致)

| 项 | 当前值 | README 写法建议 |
|----|--------|-----------------|
| 版本 | **0.14.10** | 副标题或依赖示例 `0.14` |
| License | **MIT OR Apache-2.0** | 非旧稿单 MIT |
| default features | `["backup"]` | 简表一行 |
| examples | `basic`, `backup`, `cluster` | 表链 [`examples/README.md`](aidb/examples/README.md) |
| benches | `write_bench`, `read_bench`, `backup_bench` | 一句 + 链 DEPLOYMENT |

**Quick start 锚点** — 与 [`examples/basic.rs`](aidb/examples/basic.rs) / DEPLOYMENT 最小用法一致, README 可再缩短为 ~5 行:

```rust
use aidb::{DB, config::Options};

let db = DB::open("/tmp/aidb-data", Options::default())?;
db.put(b"hello", b"world")?;
assert_eq!(db.get(b"hello")?, Some(b"world".to_vec()));
db.close()?;
```

运行: `cargo run --example basic` (仓库根目录, 默认 feature 含 backup).

---

### 5. 文档导航 (README §文档 草稿结构)

| 读者意图 | 链接 |
|----------|------|
| 分层与数据流 | [ARCHITECTURE.md](aidb/ARCHITECTURE.md) |
| 设计 why | [DESIGN.md](aidb/DESIGN.md) |
| 构建 / feature / 嵌入 / 目录 / bench | [DEPLOYMENT.md](aidb/DEPLOYMENT.md) |
| AI 助手 / CI 入口 | [AGENTS.md](aidb/AGENTS.md) |
| 按域改代码 | [docs/modules/](aidb/docs/modules/) — engine, engine-storage, cluster, backup, observability |
| 待核实 | [ISSUES.md](aidb/ISSUES.md) (至多一句, 不展开) |

`docs/development.md`、`CONTRIBUTING.md` — **尚未存在**, README 可写「测试矩阵见 DEPLOYMENT; 贡献流程见步 17」或仅链 DEPLOYMENT + AGENTS.

---

### 6. 步 1 结论 → 步 4 正文倾向

1. **定位句**: 嵌入式 LSM KV · Rust · 核心 engine 无可选重依赖 · cluster/backup/monitoring 按需 · **不是** Redis 服务.
2. **特性**: 分 **单机** / **可选** 两组 bullet (或简表), 不写 Milestone 历史表.
3. **AiKv**: 一句 — 存储层 vs 协议层; 链 aikv (可选, monorepo 相对路径).
4. **Quick start**: `0.14` 依赖 + 5 行代码 + `cargo run --example basic`.
5. **Feature 简表**: 4 行 + 「详表见 DEPLOYMENT」.
6. **不写**: mermaid、测试矩阵全文、CI 命令块、ISSUES 条目.

与步 13–15 **无矛盾**: ARCHITECTURE 已写「非网络服务 / AiKv 嵌入」; DEPLOYMENT 已有更长嵌入示例 — README 只保留更短版.

---

### 7. 步 1 自检

- [x] 以 `lib.rs` / `Cargo.toml` / `examples/basic.rs` 为准
- [x] 版本 `0.14.10`、license 双许可、monitoring 无内置 HTTP
- [x] 边界与 ARCHITECTURE/DEPLOYMENT 分工一致
- [x] 未写 `aidb/README.md`

---

请确认步 1. 确认后进入 **步 2 (查旧文档)**: 按序读 `backup/aidb/README.md` → oldmain → WiQunTools 查漏, 产出旧文档分类与拟写要点.

---

## 8. User

可以

---

## 9. Assistant

步 1 已确认，开始步 2：按旧文档顺序查阅并分类。

---

## 10. Assistant

## 步 2 — 查旧文档 (交付物)

按 `backup/aidb/README.md` → `aidb-oldmain` → WiQunTools → `wiqun-db` 顺序查阅, 对照步 1 结论与步 13–15 根文档. **尚未写正文.**

---

### 1. 旧文档清单与分类

| 来源 | 路径 | 分类 | 说明 |
|------|------|------|------|
| **主** | [`backup/aidb/README.md`](AiKv-Workflow/backup/aidb/README.md) | **部分仍有效** | 定位句、quick start 形态、文档链、bench 简述可借鉴; 架构图/Milestone/测试矩阵/版本/许可/monitoring 表述 **过时** |
| 查漏 | [`aidb-oldmain/README.md`](aidb-oldmain/README.md) | **已过时** | P2P/Coordinator 双架构、一致性哈希、aidb-admin、Grafana、Snappy、云备份、`raft-cluster` feature、590 行长文 — **均非当前实现** |
| 查漏 | `aidb-oldmain/examples/README.md` | **跳过** | 与当前 [`aidb/examples/README.md`](aidb/examples/README.md) 同构 (3 示例), 无 README 增量 |
| WiQunTools | `docs/wiqun-db-inventory/01–13` | **无有效内容跳过** | 模块级 inventory, 无项目入口/README 专章; 能力点已在 modules 覆盖 |
| 重叠 | [`wiqun-db/README.md`](wiqun-db/README.md) | **重复跳过** | 与 `backup/aidb/README.md` 同版 (仅 crate 名 `wiqun-db` / `0.13`) |

---

### 2. `backup/aidb/README.md` 逐段处理

| 旧稿段落 | 判定 | 新 README 处理 |
|----------|------|----------------|
| 首段定位 (LSM / 嵌入式 / feature flags) | ✅ 仍有效 | **保留**, 可补「lib crate、非网络服务」 |
| §架构 mermaid | ❌ 过时/重复 | **删除**, 链 [ARCHITECTURE.md](aidb/ARCHITECTURE.md) |
| §特性 Milestone M1–M5 表 | ❌ 过时 | **不迁**; 改为 **能力 bullet/简表** (当前 0.14.x 现状) |
| M5「`:9191/metrics`、OTel、慢查询」 | ❌ 设计偏离 | monitoring 写 **`register_into` + 嵌入方 HTTP**; 链 ISSUE-014 一行 (与 ARCH/DEPLOY 一致) |
| Quick start `aidb = "0.13"` | ❌ 过时 | → **`0.14`** (与 Cargo.toml `0.14.10`) |
| Quick start Rust 片段 | ✅ 仍有效 | 保留 ~5 行; import 可与 DEPLOYMENT 对齐 `use aidb::{DB, config::Options}` |
| §构建与测试 / §集群 / §可观测性 三节 | ⚠️ 部分有效 | **不展开**; feature 名保留在简表, 命令链 DEPLOYMENT |
| §测试矩阵 | ❌ 重复 | **不迁**; 一句「见 DEPLOYMENT / 步 17 CONTRIBUTING」 |
| §性能基准 | ✅ 仍有效 | **压缩为 2 bullet + 链 DEPLOYMENT** (write/read/backup_bench, `AIDB_BENCH_PRELOAD`) |
| §设计文档 (3 链) | ✅ 仍有效 | **扩展**: + AGENTS、docs/modules、ISSUES |
| §许可 MIT | ❌ 过时 | → **MIT OR Apache-2.0** |

---

### 3. `aidb-oldmain/README.md` 查漏结论 (一律不采纳)

| 旧表述 | 当前事实 |
|--------|----------|
| 「分布式 KV 存储引擎」为主叙事 | 当前: **嵌入式 lib**; 分布式为可选 `cluster` |
| P2P / Coordinator / Primary-Replica | **MetaRaft + MultiRaft + 16384 slot** |
| 一致性哈希 | **CRC16 slot + hash tag** (Redis Cluster 模型) |
| Snappy 压缩 | `compression` feature **占位未实现** |
| aidb-admin CLI、Grafana 仪表盘 | **已移除**; 运维在 AiKv / 嵌入方 |
| `raft-cluster` feature | 现为 **`cluster`** |
| Badge / Security Audit workflow | 未核实 CI 是否仍存在; **步 0 已定不含 badge** — 步 4 不写 |
| 长 quick start (Arc、iter 循环) | 比 README 所需更长; **以 `examples/basic.rs` 为准** |

---

### 4. WiQunTools / wiqun-db

- Inventory **01–13**: 无 README 级遗漏 (WAL~backup~cluster 已在 module 文档).
- `wiqun-db/README.md`: 与 backup 旧稿重复, **不另读**.

---

### 5. ISSUES 结论

| 项 | 结论 |
|----|------|
| **新增 ISSUE** | **无** — README 不展开排查过程 |
| **根文档一行引用** | 若特性里提 monitoring, 沿用 ARCHITECTURE/DEPLOYMENT 同款: |
| | `见 ISSUES.md#ISSUE-014 — HTTP/OTel 在嵌入方 (AiKv), aidb 仅库内 Prometheus 注册` |
| **其它 ISSUE** | README **不列** ISSUE-001~013 等 (属 module 域) |

---

### 6. 步 4 正文素材汇总 (旧稿 → 新稿映射)

**从旧稿保留/改写:**

1. 标题 `# AiDb` + 定位段 (backup 首段微调).
2. **特性** — 由 Milestone 表改为:
   - **单机**: WAL, SkipMap MemTable, SSTable, Leveled compaction, Bloom, Block cache, MVCC snapshot, WriteBatch, scan, checkpoint.
   - **可选**: backup (默认), cluster (MetaRaft/MultiRaft/slot), monitoring (`aidb_*` + `register_into`).
3. **与 AiKv** — 新写一句 (旧稿无; oldmain 误导向「分布式产品」).
4. **Quick start** — backup 旧稿 + 版本更新.
5. **示例** — 一行 + [`examples/README.md`](aidb/examples/README.md).
6. **Feature 简表** — 4 行 + 链 DEPLOYMENT (旧稿无独立表).
7. **Bench** — backup §性能基准 压缩版.
8. **文档导航** — 扩展旧稿 §设计文档.
9. **许可** — 双许可.

**明确删除:** mermaid、Milestone 表、测试矩阵、构建三节详情、CONTRIBUTING 深链 (步 17)、CI badge.

---

### 7. 拟更新 INDEX 行 (步 4 落盘时写入 backup/)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `README.md` (入口) | 步 2 完成 | `backup/aidb/README.md` (主); oldmain `README.md` (查漏, 过时); WiQunTools/wiqun-db 跳过 | 待步 4 | 借鉴定位/quick start/bench/文档链; 弃 Milestone/:9191/v0.13/架构图/测试矩阵; ISSUE-014 一行 |

---

### 8. 步 2 自检

- [x] 旧文档顺序符合 design.md
- [x] 分类: 仍有效 / 已过时 / 设计偏离 / 跳过
- [x] 与步 13–15 无新增矛盾
- [x] 无新 ISSUE; ISSUE-014 引用方式已定
- [x] 未写 `aidb/README.md`

---

请确认步 2. 确认后进入 **步 3 (模块级旧代码对比)**: 主要对照 backup 旧 README 叙事 vs 当前 `lib.rs`/feature/examples, 产出差异表与表述建议 (不加深章逐段).

---

## 11. User

可以

---

## 12. Assistant

步 2 已确认，开始步 3：做模块级旧代码对比。

---

## 13. Assistant

## 步 3 — 模块级旧代码对比 (交付物)

对照 **backup 旧 README 叙事**、**oldmain 产品表述** 与 **当前 `aidb/` 实现** (抽样: `lib.rs`, `Cargo.toml`, `engine/db`, `cluster/`, `backup/`, `metrics.rs`, `examples/*`). 对比深度 **模块级**, 不做 oldmain 逐文件对照.

---

### 1. 差异表 (旧 → 新 → README 应如何写)

| 维度 | 旧文档 / oldmain 表述 | 当前实现 (模块级) | README 表述建议 |
|------|----------------------|-------------------|-----------------|
| **产品形态** | backup: 嵌入式; oldmain 标题偏「分布式 KV」 | **lib crate**, 同步 API, **无网络 listener** | 「嵌入式 LSM-Tree KV 库」; 补一句 **非独立服务**, 网络协议见 AiKv |
| **版本** | `aidb = "0.13"`, M5=0.13.0 | `Cargo.toml` **0.14.10** | 依赖示例 `0.14`; 不写 Milestone 版本表 |
| **默认 feature** | oldmain `default = ["snappy"]`; backup README 未写 default | **`default = ["backup"]`** | 简表注明 `backup` 默认开启; engine-only 链 DEPLOYMENT `--no-default-features` |
| **核心 engine** | M1 bullet (仍准确) | `engine/*` **始终编译**, `lib.rs` 仅 re-export 少量类型 | 单机 bullet 保留 (WAL/MemTable/SSTable/compaction/Bloom/cache/MVCC/WriteBatch/scan) |
| **压缩** | oldmain「Snappy 可选」 | `compression` feature **占位**, `Options.compression` 未实现 | **特性 bullet 不写 Snappy**; feature 简表可写 `compression` 占位 + 链 DEPLOYMENT |
| **集群架构** | oldmain: P2P / Coordinator / 一致性哈希 | **`cluster`**: MetaRaft + MultiRaft, **16384 slot**, CRC16, gRPC/OpenRaft | 一行: 「可选 MetaRaft/MultiRaft 集群」; **不写** P2P/Coordinator/一致性哈希 |
| **集群示例** | oldmain 长篇集群部署 | `examples/cluster.rs` 仅 **slot/hash tag 演示** | 示例表如实写「CRC16 槽位 / hash tag」, 不暗示 README 内可部署集群 |
| **备份** | M5 新增于 0.13 | `backup/*`, **默认 feature**; `Checkpoint` + `BackupManager` | 列为默认能力; 链 `examples/backup.rs` + `backup.md` |
| **可观测性** | M5: OTel、**:9191/metrics**、慢查询 | `metrics.rs`: **`register_into`**, `aidb_*` 系列; **无** HTTP server/OTel init | 「库内 Prometheus 注册, 嵌入方暴露 scrape」; **不写** 端口/OTel/slowlog; ISSUE-014 一行 |
| **oldmain monitoring** | `MetricsServer` + hyper | 模块 **已删除** (仅 oldmain `src/monitoring/`) | 不提及内置 metrics 服务 |
| **公共 API 面** | oldmain 长示例 (iter 循环, 手动 `Arc::new`) | `DB::open` → **`Arc<DB>`**; `put/get/delete/write/scan/snapshot/close` | quick start **≤5 行**, 无 `Arc::new` 包裹 |
| **架构图** | backup README 内 mermaid | 已在 `ARCHITECTURE.md` | **不重复**; 文档导航链 ARCHITECTURE |
| **测试 / CI** | backup README 完整测试矩阵 | DEPLOYMENT + AGENTS + `.github/` | 一句链 DEPLOYMENT; 不写矩阵 |
| **Bench** | write/read + `AIDB_BENCH_PRELOAD` | 同上 + **`backup_bench`** | 2–3 bullet + 链 DEPLOYMENT |
| **运维工具** | oldmain: aidb-admin, Grafana | **无** AiDb CLI; AiKv/嵌入方负责 | **不写** |
| **许可** | backup: MIT | **MIT OR Apache-2.0** | 双许可 |
| **与 AiKv** | 旧 README **无** | monorepo `path`; ARCHITECTURE 已分工 | **新增一句** 存储层 vs RESP/Cluster/HTTP |

---

### 2. 主流程等价性 (README 无需展开, 确认无冲突)

| 能力 | oldmain 模块路径 | 当前模块路径 | 对外 API 是否等价 (模块级) |
|------|-----------------|--------------|---------------------------|
| 单机 CRUD | `engine/db` | `engine/db/inner.rs` | ✅ `put/get/delete/write/scan/snapshot` 仍在 |
| LSM 栈 | wal/memtable/sstable/compaction | 同域, 目录略重组 | ✅ 概念一致 |
| 集群 | `cluster/` (已有 MetaRaft) | `cluster/*` 扩展 MultiRaft/slot | ✅ 模型升级, 非 oldmain README 的 P2P |
| 备份 | 后期加入 | `backup/*` (default) | ✅ |
| 指标 | `monitoring/` HTTP 服务 | `metrics.rs` 注册表 | ❌ **行为变更** — README 必须按新模型写 |

---

### 3. 步 4 正文表述原则 (步 3 结论)

1. **叙事锚点**: 当前 **0.14.x 嵌入式 lib**, 不是 oldmain「分布式产品」或 backup「Milestone 路线图」.
2. **特性组织**: **单机 bullet** + **可选能力简表** (`backup`/`cluster`/`monitoring`/`compression`), 替代 M1–M5 表.
3. **monitoring 防误导**: 凡写 monitoring, 必须带 **「嵌入方 scrape, 无内置 HTTP」**; 与 ARCHITECTURE/DEPLOYMENT/ISSUE-014 一致.
4. **cluster 防误导**: 只写 MetaRaft/MultiRaft/slot; 示例行说明 cluster 例为 **路由演示**.
5. **quick start**: 版本 `0.14` + 5 行 + `cargo run --example basic`; 不比 DEPLOYMENT 长.
6. **重复控制**: 架构/设计/构建/feature 详表/测试 → 只链, 不抄段落.

---

### 4. ISSUES

| 项 | 结论 |
|----|------|
| 步 3 新发现 | **无** — monitoring 边界已由 ISSUE-014 (doc-only) 覆盖 |
| 步 4 | 特性段若提 monitoring, 文末 **待核实** 一行链 ISSUE-014 (与步 13/15 同款) |

---

### 5. 拟更新 INDEX (步 4 落盘时)

| 新文档章节 | 状态 | 参考的旧文档 | 处理 | 备注 |
|------------|------|--------------|------|------|
| `README.md` (入口) | 步 3 完成 | 同上 | 待步 4 | 模块级: oldmain P2P/:9191/snappy/admin 已弃; 按 0.14 lib + default backup + register_into 写 |

---

### 6. 步 3 自检

- [x] 模块级对照 (非逐文件)
- [x] 差异表 + README 表述建议
- [x] 与步 1/2/13–15 一致
- [x] 无新 ISSUE
- [x] 未写 `aidb/README.md`

---

请确认步 3. 确认后进入 **步 4**: 先给出 **`aidb/README.md` 正文草稿** 供你审阅, 确认后再落盘并更新 INDEX/PROGRESS.

---

## 14. User

可以

---

## 15. Assistant

步 3 已确认。正在对照相邻根文档与 `Cargo.toml`，起草步 4 正文。

---

## 16. Assistant

## 步 4 — 正文草稿 (待确认, **尚未落盘**)

以下为拟写入 `aidb/README.md` 的全文. 已对照: `Cargo.toml` (`0.14.10`, features/license)、`examples/basic.rs`、步 13–15 根文档 (无架构图/测试矩阵重复). 确认后再写入 `aidb/` 并更新 INDEX/PROGRESS.

---

```markdown
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

使用 [criterion](https://github.com/bheisler/criterion.rs): `cargo bench`. 主要 bench: `write_bench`, `read_bench`, `backup_bench`; `read_bench` 可用环境变量 `AIDB_BENCH_PRELOAD` 调整预填充规模. 详见 [DEPLOYMENT.md §基准测试](DEPLOYMENT.md#构建与验证).

## 文档

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 分层、数据流、与 AiKv 边界 |
| [DESIGN.md](DESIGN.md) | 跨模块设计决策 |
| [DEPLOYMENT.md](DEPLOYMENT.md) | 构建、feature、嵌入、目录与运维 |
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
```

---

### 步 4 落盘前验收自检

| 项 | 状态 |
|----|------|
| 版本 / feature 与 `Cargo.toml` | ✅ `0.14.x`, default `backup`, 四 feature |
| quick start 与 `examples/basic.rs` | ✅ `DB::open` / put / get / close |
| 无架构 mermaid / Milestone 表 / 测试矩阵 | ✅ |
| monitoring 表述 | ✅ `register_into`, 无 `:9191`; ISSUE-014 一行 |
| 与 ARCHITECTURE/DESIGN/DEPLOYMENT 分工 | ✅ 只链, 不重复正文 |
| 链接目标存在 | ✅ (modules 5 篇、examples、根文档) |
| 无 Module Skill frontmatter | ✅ |
| 篇幅 | ~85 行, 在步 0 预估范围内 |

### 落盘时将同步

- 新建 `aidb/README.md`
- 更新 `AiKv-Workflow/backup/INDEX.md` 行 → ✅
- 更新 `AiKv-Workflow/backup/PROGRESS.md` 步 16 → ✅

---

请确认草稿: **可以落盘** / **需调整 …** (例如: 去掉「待核实」节、示例改回单行链、副标题是否写死 `0.14.10` 等).

---

## 17. User

可以

---

## 18. Assistant

草稿已确认，正在写入 `aidb/README.md` 并更新 INDEX 与 PROGRESS。

---

## 19. Assistant

**步 16 已完成.** `aidb/README.md` 已落盘, INDEX/PROGRESS 已更新.

### 落盘内容

- **新建** [`aidb/README.md`](aidb/README.md) — 项目入口 (~85 行): 定位、特性、AiKv 关系、quick start、示例、bench、文档导航、ISSUE-014、许可
- **更新** [`AiKv-Workflow/backup/INDEX.md`](AiKv-Workflow/backup/INDEX.md) — README 行 ✅
- **更新** [`AiKv-Workflow/backup/PROGRESS.md`](AiKv-Workflow/backup/PROGRESS.md) — 步 16 ✅ (2026-06-18)

### 旧文档处理

`backup/aidb/README.md` 已在 backup/ 作参考, aidb 侧无迁移历史; **未删除** 旧稿 (按 design.md 过程文档保留).

### 阶段 2a 进度

| 步 | 文档 | 状态 |
|----|------|------|
| 13–15 | ARCHITECTURE / DESIGN / DEPLOYMENT | ✅ |
| **16** | **README** | **✅** |
| 17 | CONTRIBUTING / CHANGELOG | ⬜ |
| 18 | `docs/README.md` | ⬜ |

下一步是 **步 17** (`CONTRIBUTING.md` / `CHANGELOG.md`). 需要的话开新会话或继续本会话, 从步 0 走 CONTRIBUTING/CHANGELOG.

---

## 20. User

使用 vibe-coding/scripts/archive-cursor-session.sh  帮我保存当前对话到 archive 里面 带 tool 和不带 tool 的都保存
