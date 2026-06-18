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

推送前建议安装 pre-commit (fmt + clippy, **不含 test**):

```bash
./install-hooks.sh   # 软链 hooks/* → .git/hooks/
```

[`hooks/pre-commit`](hooks/pre-commit) 依次执行:

1. `cargo fmt --check`
2. `cargo clippy --all-targets` (`RUSTFLAGS='-D warnings'`)
3. `cargo clippy --all-targets --features cluster` (需本机 `protoc`)

cluster clippy 与 CI `test-cluster` job 一致:

```bash
# Debian/Ubuntu
sudo apt-get install -y protobuf-compiler
```

**注意**: hook **不跑** `cargo test`; 测试在 CI (或 push 前手动) 执行.

## 本地验证 vs CI

| 层级 | 做什么 | 何时失败 |
|------|--------|----------|
| pre-commit | fmt + clippy (默认 + cluster) | `git commit` |
| CI `test-default` | fmt → clippy → test (默认 feature) | push / PR |
| CI `test-cluster` | clippy + test (`--features cluster`, 装 protoc) | push / PR |
| CI `bench` | `write_bench` / `read_bench` / `backup_bench` | `test-default` 通过后 |
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
cargo test -- --test-threads=1                    # 默认 feature, ~375 项
cargo test --features cluster -- --test-threads=1 # 含 cluster, ~551 项
```

### 基准测试 (可选)

```bash
cargo bench --bench write_bench
cargo bench --bench read_bench
cargo bench --bench backup_bench
# read_bench 预填充: AIDB_BENCH_PRELOAD=100000 cargo bench --bench read_bench
```

CI 在 `test-default` 通过后运行上述 bench. 详见 [DEPLOYMENT.md §构建与验证](DEPLOYMENT.md#构建与验证).

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
3. **修 bug**: 同一 PR 在 `tests/regression/` 添加复现测试 (见下节).
4. **用户面向变更**: 更新 [CHANGELOG.md](CHANGELOG.md) 对应版本或 `[Unreleased]`.
5. **PR**: CI + Security 须绿; 相关文档一并更新.

### PR 检查清单

- [ ] `cargo fmt --check` 通过 (或已跑 `./install-hooks.sh`)
- [ ] clippy 默认 + cluster 无警告 (`RUSTFLAGS='-D warnings'`)
- [ ] `cargo test -- --test-threads=1` 通过
- [ ] 若改 cluster: `cargo test --features cluster -- --test-threads=1` 通过
- [ ] 若修 bug: `cargo test --test regression -- --test-threads=1` 含新用例
- [ ] 用户面向 API/行为变更已写 CHANGELOG
- [ ] 模块文档或根文档已更新 (若适用)

## 共享测试基础设施

`tests/common/` 供跨模块测试引用:

| 文件 | 用途 |
|------|------|
| `dataflow.rs` | Span 树、调用顺序 (模式 A/B) |
| `observability.rs` | `EventCatcher`, event 时序 (模式 C) |

用法与模式速查见 [`tests/common/mod.rs`](tests/common/mod.rs) 模块注释.

## 回归测试规范

入口: [`tests/regression.rs`](tests/regression.rs) → `tests/regression/`.

| 规则 | 说明 |
|------|------|
| 命名 | 描述性 `test_*` (如 `test_bloom_fpr_*`); 注释写明 bug 现象与修复 |
| 每次修复 | 同一 PR 添加复现测试 |
| 运行 | `cargo test --test regression -- --test-threads=1` |

现有场景: `empty_value_compaction`, `bloom` (长期 FPR 统计).

## 相关文档

| 文档 | 内容 |
|------|------|
| [DEPLOYMENT.md](DEPLOYMENT.md) | 构建、feature、嵌入 |
| [.github/README.md](.github/README.md) | CI / Security 详表 |
| [tests/README.md](tests/README.md) | 测试分层与新增约定 |
| [CHANGELOG.md](CHANGELOG.md) | 版本变更记录 |
| [ISSUES.md](ISSUES.md) | 待核实项 |
