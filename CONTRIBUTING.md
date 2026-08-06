# 贡献指南

本文说明 **如何本地验证、通过门禁、运行测试与提交 PR**. 项目概览见 [README.md](README.md); 构建与 feature 见 [DEPLOYMENT.md](DEPLOYMENT.md); CI 流程图与 job 详表见 [.github/README.md](.github/README.md).

## 仓库结构

```shell
src/
├── lib.rs           # 公共 API 入口 (< 30 个 pub fn)
├── config.rs        # Options
├── error.rs         # Error / Result
├── engine/          # LSM 核心 (始终编译)
│   ├── wal/
│   ├── memtable/
│   ├── sstable/
│   ├── compaction/
│   ├── filter/
│   ├── cache/
│   ├── checkpoint/
│   └── db/          # DB, WriteBatch, Snapshot (MVCC)
├── backup/          # backup feature (默认)
├── cluster/         # cluster feature
└── metrics.rs       # monitoring feature
```

实现细节见 [docs/modules/](docs/modules/); 分层架构见 [ARCHITECTURE.md](ARCHITECTURE.md).

## 工具链

[`rust-toolchain.toml`](rust-toolchain.toml) 固定 **stable**, 含 `clippy` / `rustfmt`, 与 GitHub Actions 一致. 进入仓库目录后 `rustup` 会自动切换; 可用 `rustup show` 确认.

## Git hooks

建议安装 Git 钩子 (`pre-commit` 代码与全 Feature Clippy 校验、`commit-msg` Conventional Commits 提交规范校验):

```bash
./install-hooks.sh   # 软链 hooks/* → .git/hooks/
```

[`hooks/pre-commit`](hooks/pre-commit) 依次执行:

1. `cargo fmt --check`
2. `cargo clippy --all-targets --all-features` (`RUSTFLAGS='-D warnings'`)

[`hooks/commit-msg`](hooks/commit-msg) 校验提交说明是否遵循 Conventional Commits 规范 (如 `feat:`, `fix:`, `chore:` 等).

**注意**: hook **不跑** `cargo test`; 测试在 CI (或 push 前手动) 执行.

## 本地验证 vs CI

| 层级 | 做什么 | 何时失败 |
|------|--------|----------|
| pre-commit | fmt + clippy (`--all-features`) | `git commit` |
| commit-msg | Conventional Commits 描述格式校验 | `git commit` |
| CI `test-default` | fmt → clippy → test (默认 feature) | push / PR |
| CI `test-cluster` | clippy + test (`--features cluster`, 装 protoc) | push / PR |
| CI `test-slow` | `cargo test -- --ignored` (slow + stress 集成测) | `test-default` 通过后 |
| CI `bench` | `write_bench` / `read_bench` / `backup_bench` (criterion) | `test-default` 通过后 |
| Security | `cargo audit` + `cargo deny check` | push / PR / 每日 cron |

Security ([`.github/workflows/security.yml`](.github/workflows/security.yml)) 与主 CI **并行、互不阻塞**. 同一分支新 push 会 cancel 未完成的旧 CI run.

触发分支: `main`, `new/main`, `new/wiqun` (见 [`.github/workflows/ci.yml`](.github/workflows/ci.yml)).

### 推送前推荐命令

```bash
export RUSTFLAGS='-D warnings'
cargo fmt --check
cargo clippy --all-targets
cargo clippy --all-targets --features cluster   # 需 protoc
cargo test -- --test-threads=1
cargo test --features cluster -- --test-threads=1
```

慢测与压测 (与 CI `test-slow` 一致):

```bash
cargo test -- --ignored --test-threads=1
```

与 [AGENTS.md](AGENTS.md) 速查块相同; job 细节见 [.github/README.md](.github/README.md).

## 完整测试矩阵

集成测与 Raft 相关用例 **必须** `--test-threads=1`. 分层说明见 [`tests/README.md`](tests/README.md).

### 按层级

| 层级 | 命令 | 说明 |
|------|------|------|
| **L0** | `cargo test --lib` | `src/**` 单元测试 |
| **L1** | `cargo test --test wal -- --test-threads=1` | 单模块 (wal, memtable, filter, cache, sstable, db, compaction, snapshot) |
| **L2** | `cargo test --test pipeline -- --test-threads=1` | 子系统直连 (不经 `DB::open`) |
| **L2** | `cargo test --test engine -- --test-threads=1` | `DB` 公共 API 黑盒 (崩溃恢复, compaction 集成, dataflow) |
| **L3** | `PROPTEST_CASES=100 cargo test --test proptest -- --test-threads=1` | 随机操作 + 引擎不变式 |
| **L4** | `cargo test --test regression -- --test-threads=1` | 已修 bug 固化 |

L4 示例: WAL WriteBatch 边界 → `cargo test --test wal write_batch_boundary -- --test-threads=1` (见 `tests/modules/wal/write_batch_boundary.rs`).

### L1 模块入口

```bash
cargo test --test wal -- --test-threads=1
cargo test --test memtable -- --test-threads=1
cargo test --test filter -- --test-threads=1
cargo test --test cache -- --test-threads=1
cargo test --test sstable -- --test-threads=1
cargo test --test db -- --test-threads=1
cargo test --test compaction -- --test-threads=1
cargo test --test snapshot -- --test-threads=1
cargo test --test span_contract -- --test-threads=1  # 热路径 span 级别契约 (源码扫描)
```

可观测性 dataflow 子集示例:

```bash
cargo test --test db dataflow -- --test-threads=1
cargo test --test engine dataflow -- --test-threads=1
```

### Feature 专项

| Feature | 命令 | CI |
|---------|------|-----|
| `backup` (默认) | `cargo test --test backup -- --test-threads=1` | `test-default` 内含 |
| `monitoring` | `cargo test --test metrics --features monitoring -- --test-threads=1` | **无独立 job** (本地) |
| `cluster` | 见下表 | `test-cluster` 全量 `--features cluster` |

### Cluster 入口 (`--features cluster`)

```bash
cargo test --features cluster --test raft -- --test-threads=1
cargo test --features cluster --test meta -- --test-threads=1
cargo test --features cluster --test multi_raft -- --test-threads=1
cargo test --features cluster --test cluster_ops -- --test-threads=1
cargo test --features cluster --test cluster_replica_reconcile -- --test-threads=1
```

CI 等价于:

```bash
cargo test --features cluster -- --test-threads=1
```

### CI 全量 (与 push 门禁一致)

```bash
cargo test -- --test-threads=1                    # 默认 feature, ~377 项
cargo test --features cluster -- --test-threads=1 # 含 cluster, ~621 项
```

### 基准测试 (可选)

```bash
cargo bench --bench write_bench
cargo bench --bench read_bench
cargo bench --bench backup_bench
# read_bench 预填充: AIDB_BENCH_PRELOAD=100000 cargo bench --bench read_bench
```

CI 在 `test-default` 通过后运行上述 bench. 详见 [DEPLOYMENT.md §构建与验证](DEPLOYMENT.md#构建与验证).

### `#[ignore]` 慢测与压测

默认 `cargo test` **跳过** 带 `#[ignore]` 的用例; CI `test-slow` job 通过 `--ignored` 运行全部. 新增慢/压测须使用统一 reason 前缀:

| 前缀 | 含义 | 示例 |
|------|------|------|
| `slow:` | 真实等待或长时间 hold (秒~分钟) | snapshot 长 hold + 大量写入 |
| `stress:` | 大数据集、高吞吐 | 10K compaction、1M bloom 采样 |

写法: `#[ignore = "slow: …"]` 或 `#[ignore = "stress: …"]`. **禁止** 裸 `#[ignore]`.

| 测试 | 标签 | test target | CI job |
|------|------|-------------|--------|
| `test_snapshot_long_hold_heavy_write` | slow | `snapshot` | `test-slow` |
| `test_large_dataset_compaction_stress_10000` | stress | `engine` | `test-slow` |
| `test_bloom_stress` | stress | `regression` | `test-slow` |
| `test_concurrent_write_and_compaction` | stress | `engine` | `test-slow` |
| `test_concurrent_write_with_filter` | stress | `engine` | `test-slow` |

> **与 bench 区分**: `cargo bench` (criterion) 在 `bench` job; 上表为 integration test 的 `#[ignore]` 用例.

### 示例

| 示例 | 命令 |
|------|------|
| basic | `cargo run --example basic` |
| backup | `cargo run --example backup` |
| cluster | `cargo run --features cluster --example cluster` |

见 [examples/README.md](examples/README.md).

## 开发与 PR 规范

1. **TDD (建议)**: 先写测试 → 实现 → 重构.
2. **提交格式**: `type: 中文描述` — `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
3. **修 bug (必带回归测)**: 见下节; `docs:` / doc-only 关闭 ISSUE 可豁免.
4. **用户面向变更**: 更新 [CHANGELOG.md](CHANGELOG.md) 对应版本或 `[Unreleased]`.
5. **PR**: CI + Security 须绿; 相关文档一并更新.

### PR 检查清单

- [ ] `cargo fmt --check` 通过 (或已跑 `./install-hooks.sh`)
- [ ] clippy 默认 + cluster 无警告 (`RUSTFLAGS='-D warnings'`)
- [ ] `cargo test -- --test-threads=1` 通过
- [ ] 若改 cluster: `cargo test --features cluster -- --test-threads=1` 通过
- [ ] 若修 bug: 回归测已添加且 `cargo test --test regression -- --test-threads=1` 含新用例 (或见下节放置决策)
- [ ] 若新增/改动测试: 落点符合 [tests/README.md §测试写法与范围 (硬性)](tests/README.md#测试写法与范围-硬性)
- [ ] 若改动 `tests/`: 文件有 `//! @component` + 中文摘要; 每个新增/改动的 `#[test]` 有中文 `///`
- [ ] 若改 slow/stress 用例: `cargo test -- --ignored --test-threads=1` 通过
- [ ] 用户面向 API/行为变更已写 CHANGELOG
- [ ] 模块文档或根文档已更新 (若适用)

## 共享测试基础设施

`tests/common/` 供跨模块测试引用:

| 文件 | 用途 |
|------|------|
| `dataflow.rs` | Span 树、调用顺序 (模式 A/B) |
| `observability.rs` | `EventCatcher`, event 时序 (模式 C) |

用法与模式速查见 [`tests/common/mod.rs`](tests/common/mod.rs) 模块注释.

## 回归测试 (必带)

所有 **bugfix PR** (`fix:`、修 ISSUE、行为修正) **必须** 在同一 PR 内附带可复现回归测. **豁免**: 纯文档变更 (`docs:`) 或 doc-only 关闭 ISSUE.

入口: [`tests/regression.rs`](tests/regression.rs) → `tests/regression/` (L4). 跨模块引擎级 bug 也可放在 L2 `tests/engine/` (见放置决策).

| 规则 | 说明 |
|------|------|
| 同一 PR | 测试与修复同 PR; 建议先红后绿 |
| 命名 / 注释 | 描述性 `test_*`; **`///`** 写明 bug 现象、期望与 ISSUE (若有) |
| `@component` | entry 文件加 `//! @component aidb-{domain}` (与 test-ui B2-v1 一致) |
| 运行 | `cargo test --test regression -- --test-threads=1` |

### 放置决策

| 场景 | 落点 |
|------|------|
| 单模块 WAL/MemTable 等 | L1 `tests/modules/{mod}/` |
| `DB::open` 崩溃恢复/compaction | L2 `tests/engine/` |
| 已修 bug 长期固化 | **优先** L4 `tests/regression/` 新文件 + `regression.rs` 挂载 |
| 随机不变式 | L3 `tests/proptest/` |

示例 (B1.2): ISSUE-001/002 WAL WriteBatch → [`tests/modules/wal/write_batch_boundary.rs`](tests/modules/wal/write_batch_boundary.rs), [`tests/engine/wal_write_batch_boundary.rs`](tests/engine/wal_write_batch_boundary.rs).

现有 L4 场景: `empty_value_compaction`, `bloom` (长期 FPR 统计).

## 相关文档

| 文档 | 内容 |
|------|------|
| [DEPLOYMENT.md](DEPLOYMENT.md) | 构建、feature、嵌入 |
| [.github/README.md](.github/README.md) | CI / Security 详表 |
| [tests/README.md](tests/README.md) | 测试分层与新增约定 |
| [CHANGELOG.md](CHANGELOG.md) | 版本变更记录 |
| [ISSUES.md](ISSUES.md) | 待核实项 |
