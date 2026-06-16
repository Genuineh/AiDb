# 贡献指南

## 仓库结构

```
src/
├── lib.rs           # 公共 API 入口 (< 30 个 pub fn)
├── error.rs         # 错误类型枚举 (thiserror)
├── config.rs        # Options 配置
├── engine/          # LSM-Tree 核心 (pub(crate))
│   ├── wal/         # Write-Ahead Log
│   ├── memtable/    # 内存索引
│   ├── sstable/     # 磁盘格式
│   ├── compaction/  # Leveled Compaction
│   ├── filter/      # Bloom Filter
│   └── cache/       # Block Cache
├── cluster/         # 分布式集群 (feature-gated)
├── backup/          # 备份与恢复
└── snapshot.rs      # MVCC 快照
```

## 工具链与 Git hooks

仓库根目录 [`rust-toolchain.toml`](rust-toolchain.toml) 固定 **stable** 并含 `clippy` / `rustfmt`, 与 GitHub Actions 一致. 首次进入目录执行 `rustup show` 确认已自动切换.

推送前安装 pre-commit (与 CI 同套检查: fmt + 默认/cluster clippy; 不含 test):

```bash
./install-hooks.sh   # 软链 hooks/pre-commit → .git/hooks/pre-commit
```

cluster clippy 需要本机已安装 `protoc` (与 CI `test-cluster` job 一致):

```bash
# Debian/Ubuntu
sudo apt-get install -y protobuf-compiler
```

## 构建与测试

```bash
cargo build
cargo test                          # 全部测试 (可重复执行)
export RUSTFLAGS='-D warnings'      # 与 CI 相同
cargo clippy --all-targets
cargo fmt --check
cargo test --test regression        # 回归测试 (已修 bug 不复现)
cargo llvm-cov --html               # 覆盖率报告 (目标 ≥ 80%)

# 集群 (cluster feature; 集成测单线程)
cargo build --features cluster
cargo test --features cluster --test raft -- --test-threads=1
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster -- -D warnings

# 可观测性 (monitoring feature)
cargo build --features monitoring

# 示例
cargo run --example basic
cargo run --example backup
```

## 开发与验证

1. **TDD**: 先写测试 (RED) → 实现 (GREEN) → 重构 (IMPROVE)
2. **覆盖率**: 保持 80%+
3. **提交格式**: `type: description` — feat, fix, refactor, test, docs, chore, perf
4. **PR**: CI 必须通过

每个模块实现后按以下步骤验证:

| 步骤 | 命令 | 验证标准 |
|:----:|------|---------|
| 1 功能测试 | `cargo test <模块名>` | 全部通过 |
| 2 覆盖率 | `cargo llvm-cov --html --summary-only` | ≥ 80% |
| 3 回归检查 | `cargo test --test regression` | 已修 bug 不复现 |
| 4 代码质量 | `RUSTFLAGS='-D warnings' cargo clippy --all-targets` + `cargo fmt --check` | 零警告 |

所有验证命令可重复运行.

## 共享测试基础设施

`tests/common/` 目录存放跨模块共享的测试工具, 各模块通过 `tests/{模块}.rs` 入口引用:

| 文件 | 用途 | 用法 |
|------|------|------|
| `dataflow.rs` | `capture_spans` / `EventCatcher` | 模块级可观测性因果链 (模式 A/C, 见 doc comment) |
| `observability.rs` | `EventCatcher` → 捕获 events | 配合 dataflow 做 event 时序断言 |

**测试目录** (分层编号见 [`tests/README.md`](tests/README.md)):

```
tests/
├── {wal,memtable,filter,cache,sstable,db,compaction}.rs
├── pipeline.rs + pipeline/
├── engine.rs + engine/
├── proptest.rs + proptest/
├── regression.rs + regression/
├── modules/{mod}/
└── common/
```

详见 [`tests/README.md`](tests/README.md).

## 回归测试规范

`tests/regression.rs` 入口 + `tests/regression/` 存放已修 bug 复现:

| 规则 | 说明 |
|------|------|
| 命名 | `test_issue_<编号>_<描述>` |
| 注释 | 写明 bug 现象和修复方式 |
| 每次修复 | 必须在同一 PR 中添加复现测试 |
| 运行 | `cargo test --test regression` (可重复执行) |
