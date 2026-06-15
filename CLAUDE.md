# AiDb 项目指南

## 概述

基于 LSM-Tree 的 KV 存储引擎 (Rust lib crate). 核心引擎零可选依赖; cluster 和 monitoring 通过 feature 启用.

## 架构

- **引擎**: WAL → MemTable → SSTable → Compaction (Leveled)
- **集群**: MetaRaft (控制平面) + Multi-Raft (数据平面) + Cluster Ops (副本分配/成员变更/槽迁移), `cluster` feature
- **错误处理**: thiserror 枚举 (Io/Corruption/Busy/NotFound/InvalidArgument/InvalidState; `cluster` feature 下含 Cluster)
- **异步**: 引擎为同步代码, 通过 `spawn_blocking` 包装给异步调用方

## 命令

```bash
cargo build                     # 仅核心
cargo build --features cluster  # 含集群
cargo test                      # 运行测试
export RUSTFLAGS='-D warnings'  # 与 CI 相同
cargo clippy --all-targets      # 代码检查 (需 rust-toolchain.toml)
cargo fmt --check               # 格式检查
./install-hooks.sh              # 安装 pre-commit (推送前)
cargo test --test regression    # 回归测试 (已修 bug 复现)
cargo llvm-cov --html           # 覆盖率报告
```

## 验证工作流

每个模块实现后按以下步骤验证 (详见 CONTRIBUTING.md):

1. **功能测试** → `cargo test <模块名>` 或 `cargo test --test <模块名>`
2. **覆盖率** → `cargo llvm-cov --html --summary-only` (目标 ≥ 80%)
3. **回归检查** → `cargo test --test regression` (已有 bug 不复现)
4. **代码质量** → `RUSTFLAGS='-D warnings' cargo clippy --all-targets` + `cargo fmt --check`

示例:

```bash
cargo test --test wal
cargo test --test memtable -- --test-threads=1
cargo test --test sstable -- --test-threads=1
cargo test --test compaction -- --test-threads=1
cargo test --test pipeline -- --test-threads=1
cargo test --test engine -- --test-threads=1
PROPTEST_CASES=100 cargo test --test proptest -- --test-threads=1
cargo test --test regression -- --test-threads=1
```

详见 `tests/README.md`.

## 约束

1. 生产代码中禁止 `unwrap()`/`expect()` (仅在 `#[cfg(test)]` 中允许)
2. 每个 `unsafe` 块必须附带 `// SAFETY:` 注释
3. 公共 API 限制在 30 个函数以内
4. 单文件上限 800 行, 单函数上限 50 行, 嵌套上限 4 层
5. TDD: 先写测试 (RED → GREEN → IMPROVE)
6. **每个模块实现时必须同步添加 tracing 标注**: 按 spec 的 `## 可观测性` 章节添加 `#[instrument]` 和 `tracing::event!`; Phase 17 集中验证

## Known Limitations

### 集群 (v0.14.3)
- **OpenRaftNode 版本**: 当前 openraft 快照/日志 API 需随上游升级适配.
- **数据面端口**: 默认为 `rpc_port + 10000`, 可通过 AiKv `--cluster-data-port-offset` CLI 参数配置. 部署时需确保 `rpc_port ≤ 65535 - offset`. AiKv 启动时已做校验.

## 设计文档

- 模块规格: `/docs/aidb-inventory/`
- 可观测性: `docs/observability.md`
- 回归测试: `tests/regression/` (含 bloom 长期统计回归)
