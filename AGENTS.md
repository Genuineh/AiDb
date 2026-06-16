# AiDb — AI 助手指南

LSM-Tree 嵌入式 KV 引擎 (Rust lib crate, v0.14.x). 核心路径零可选依赖; `cluster` / `monitoring` / `backup` 通过 feature 启用.

## 架构要点

- **引擎**: WAL → MemTable → SSTable → Leveled Compaction → Bloom / Block Cache
- **集群** (`cluster` feature): MetaRaft 控制平面 + Multi-Raft 数据平面; gRPC (`proto/raft.proto`, `build.rs` + checked-in `src/cluster/aidb.raft.rs`)
- **错误**: `thiserror` 枚举; 引擎同步 API, 异步调用方用 `spawn_blocking`
- **目录**: `src/engine/` 核心, `src/cluster/` 集群, `src/backup/` 备份

## 开发与 CI

详见 [`.github/README.md`](.github/README.md). 摘要:

```bash
./install-hooks.sh                 # pre-commit: fmt + clippy (默认 + cluster, 需 protoc)
cargo fmt && cargo fmt --check     # Rust 4 空格 (rustfmt.toml / .editorconfig)
export RUSTFLAGS='-D warnings'
cargo clippy --all-targets
cargo clippy --all-targets --features cluster   # 需 protobuf-compiler
cargo test -- --test-threads=1
cargo test --features cluster -- --test-threads=1
```

- 工具链: `rust-toolchain.toml` (stable + clippy/rustfmt)
- CI: `test-default` / `test-cluster` / `bench`; 安全: `security.yml` (audit + deny)
- pre-commit **不跑** test; 测试在 CI

## 模块验证 (改代码后)

1. 功能: `cargo test --test <name>` (见 [`tests/README.md`](tests/README.md))
2. 回归: `cargo test --test regression`
3. 质量: `RUSTFLAGS='-D warnings' cargo clippy --all-targets` + `cargo fmt --check`
4. 覆盖率 (可选): `cargo llvm-cov --html --summary-only` (目标 ≥ 80%)

常用:

```bash
cargo test --test wal -- --test-threads=1
cargo test --test engine -- --test-threads=1
cargo test --test raft --features cluster -- --test-threads=1
PROPTEST_CASES=100 cargo test --test proptest -- --test-threads=1
```

## 编码约束

1. 生产代码禁止 `unwrap()` / `expect()` (`#[cfg(test)]` 除外)
2. 每个 `unsafe` 块需 `// SAFETY:` 说明
3. 单文件 ≤ 800 行, 单函数 ≤ 50 行, 嵌套 ≤ 4 层
4. TDD: 先测后实现 (RED → GREEN → REFACTOR)

## 已知限制

- **OpenRaft**: 快照/日志 API 随上游演进, 升级需适配
- **数据面端口**: MultiRaft gRPC 默认 `rpc_port + 10000`; AiKv 侧用 `--cluster-data-port-offset` 配置, 需满足 `rpc_port ≤ 65535 - offset`

## 文档索引

| 文档 | 用途 |
|------|------|
| [README.md](README.md) | 产品概览与快速开始 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 贡献流程与测试矩阵 |
| [DESIGN.md](DESIGN.md) | 设计决策 |
| [docs/observability.md](docs/observability.md) | 可观测性约定 |
| [docs/superpowers/](docs/superpowers/) | 功能设计与计划 |
| [tests/README.md](tests/README.md) | 测试分层说明 |
| [.github/README.md](.github/README.md) | CI / hook 流程 |
