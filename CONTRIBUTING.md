# 贡献指南

本文是面向开源贡献者与开发者的 **本地开发、测试验证、Git 分支与 PR 提交流程指南**.

- 项目架构与分层设计见 [ARCHITECTURE.md](ARCHITECTURE.md);
- 部署与配置调优见 [docs/deployment.md](docs/deployment.md);
- 设计决策与技术权衡见 [docs/design.md](docs/design.md);
- 完整 CI 架构见 [.github/README.md](.github/README.md).

```mermaid
flowchart LR
    S1[1. 准备开发环境] --> S2[2. 配置 Git 钩子]
    S2 --> S3[3. Issue 与分支]
    S3 --> S4[4. 本地验证与测试]
    S4 --> S5[5. 规范 Commit]
    S5 --> S6[6. 文档同步]
    S6 --> S7[7. 提交 PR 并自检]
```

---

## 1. 准备开发环境

### Rust 工具链

本项目固定使用 **Rust stable** (含 `clippy` 与 `rustfmt`), 由 [`rust-toolchain.toml`](rust-toolchain.toml) 声明.
进入仓库目录后 `rustup` 会自动切换为对应版本, 可通过 `rustup show` 确认.

### 系统依赖

- **Linux / macOS**: 推荐开发环境 (CI 运行在 `ubuntu-latest`).
- **protoc (Protobuf 编译器)**: 编译 `cluster` 特性时生成 Raft gRPC 协议桩代码需要.

```bash
# Ubuntu / Debian
sudo apt-get install -y protobuf-compiler
# macOS
brew install protobuf
```

### 与 AiKv 协同开发 (本地 Patch)

当与上层服务 **[AiKv](https://github.com/wiqun/AiKv)** 联合调试本地未发布的 `aidb` 改动时, 可以在 `AiKv` 仓库或其工作区配置 `[patch]` 覆盖 Git 依赖, 具体参考 [AiKv CONTRIBUTING.md](https://github.com/wiqun/AiKv/blob/main/CONTRIBUTING.md).

---

## 2. 配置 Git 钩子

建议在克隆仓库后立即安装本地 Git 钩子, 将质量与规范门禁前置到本地提交阶段:

```bash
./install-hooks.sh   # 软链 hooks/* → .git/hooks/
```

### 钩子职责说明

- [`hooks/pre-commit`](hooks/pre-commit): 在 `git commit` 时依次执行:
  1. 分支保护 ([`hooks/check-branch.sh`](hooks/check-branch.sh)): 禁止直接在 `main` 分支提交
  2. 文档链接检查 ([`hooks/check-docs-links.sh`](hooks/check-docs-links.sh)): 检查暂存区 `.md` 文件的相对链接有效性
  3. 代码格式检查: `cargo fmt --check`
  4. 静态分析检查: `RUSTFLAGS='-D warnings' cargo clippy --all-targets --all-features`
  5. 安全检查 ([`hooks/check-security.sh`](hooks/check-security.sh)): `cargo audit` 与 `cargo deny check`
- [`hooks/commit-msg`](hooks/commit-msg): 校验提交说明是否遵循 Conventional Commits 规范 (如 `feat:`, `fix:`, `chore:` 等).

> **说明**: Git hook 默认 **不执行** `cargo test`, 测试由开发者本地手动或 CI 运行.

### Security 检查

`pre-commit` 中的 `cargo audit` 与 `cargo deny check` 基于 `Cargo.lock` 全量扫描依赖. `cargo audit` 读取 `.cargo/audit.toml` (与 `deny.toml` 独立).

若本地未安装对应工具, 钩子会自动跳过并提示安装命令 (`cargo install cargo-audit --locked` / `cargo install cargo-deny --locked`).

---

## 3. Issue 驱动与分支规范

### GitHub Issues 驱动

所有新功能、bug 修复、重构或文档改动均须与 GitHub Issue 关联:

1. 先创建 GitHub Issue (按场景选用模板: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`).
2. 明确 Issue 的需求、复现步骤或验收条件后再开始编码.

### 分支命名规范

所有工作分支基于主开发分支 (`main`) 创建:

- **分支命名格式**: `{type}/{NN}-{slug}`
- **示例**:
  - `feat/108-multi-raft-migration`
  - `fix/42-wal-record-crc-mismatch`
  - `docs/55-contributing-guide-reorganize`

---

## 4. 本地编码与测试矩阵

在本地提交或推送前, 请确保代码通过以下校验与测试.
集成测试与 Raft 相关用例 **必须** 使用 `--test-threads=1`.

### 核心快速门禁 (推荐推送前必跑)

```bash
export RUSTFLAGS='-D warnings'
cargo fmt --check
cargo clippy --all-targets
cargo test -- --test-threads=1
```

### Feature 专项测试

| Feature / 专项 | 校验与测试命令 | 说明 |
| --- | --- | --- |
| `cluster` | `cargo clippy --all-targets --features cluster`<br>`cargo test --features cluster -- --test-threads=1` | MultiRaft 与集群路由测试 (需本地 protoc) |
| `compression` | `cargo test --features compression --test sstable -- multi_block_read_with -- --test-threads=1` | Snap / LZ4 SSTable 块压缩 roundtrip 测试 |
| `monitoring` | `cargo test --test metrics --features monitoring -- --test-threads=1` | OpenTelemetry 指标与 Tracing 上下文测试 |
| **故障隔离 (脑裂)** | `cargo test --features cluster,cluster-test-util --test raft -- judge_leader_quorum_rules network_blackhole_drops_rpc lease_read_fails_after_leader_isolated -- --test-threads=1` | 脑裂与故障注入测试 (精确命中 3 个用例) |

### 回归测试 (Bugfix 必带)

所有 **bugfix PR** (`fix:`, 修 Issue, 行为修正) **必须** 附带可复现回归测试:

- **落点**: [`tests/regression.rs`](tests/regression.rs) (L4 长期固化) 或对应模块的 L1/L2 测试.
- **注释规范**: 测试函数正上方必须包含中文 `///` 注释, 清晰说明 bug 现象、期望行为以及关联的 Issue 编号.
- **纯文档变更豁免**: 纯 `docs:` 变更或纯文档 Issue 可豁免回归测试.

### 慢测与压测 (`#[ignore]`)

默认 `cargo test` 会跳过标记为 `#[ignore]` 的耗时用例. 新增慢测或压力测试时:

- **规范**: 必须使用 `#[ignore = "slow: <原因>"]` 或 `#[ignore = "stress: <原因>"]`, **禁止裸** `#[ignore]`.
- **运行慢测**:
```bash
cargo test -- --ignored --test-threads=1
```

> 详细的 L0~L4 测试分层架构、跨仓测试边界与测试代码编写约定详见 [`tests/README.md`](tests/README.md).

---

## 5. 提交规范

提交说明遵循 Conventional Commits 格式, 统一使用中文描述变更目的:

```md
<type>: <中文简短描述>

[可选的详细说明段落]

Fixes #<Issue编号>
```

### 支持的 Type 列表

- `feat`: 新增功能特性
- `fix`: 修补缺陷与 bug
- `refactor`: 代码重构 (不改变外部行为与 API)
- `test`: 新增或修改测试用例
- `docs`: 文档增补与修改
- `chore`: 构建配置、依赖更新、辅助工具等杂项
- `perf`: 性能优化

### 提交示例

```
fix(wal): 修复写入批次跨分片时的 CRC 校验计算

当单个 WriteBatch 跨越 WAL block 边界时, CRC 校验和未正确重置导致崩溃重放失败.

Fixes #42
```

---

## 6. 文档同步要求

代码与文档保持一致是工程质量的重要一环. 修改涉及公共 API、核心行为、架构或模块边界时, 必须同步更新相关文档:

1. **修改公共 API 或对外能力** → 同步更新 [`README.md`](README.md) 与 [`docs/deployment.md`](docs/deployment.md)
2. **修改架构、核心机制或设计决策** → 同步更新 [`ARCHITECTURE.md`](ARCHITECTURE.md) 与 [`docs/design.md`](docs/design.md)
3. **修改具体模块实现细节** → 同步更新 [`docs/modules/`](docs/modules/) 对应模块文档
4. **面向用户的重大变更** → 在 [`CHANGELOG.md`](CHANGELOG.md) 的 `[Unreleased]` 小节登记

> 若本次改动纯属内部微调且对文档无任何影响, 请在 PR 描述中显式注明「文档无需变更」.

---

## 7. PR 提交流程与自检清单

### PR 提交步骤

1. 将本地分支推送到远程, 并创建 Pull Request.
2. PR 标题对齐 Commit 规范 (`type: 中文描述`).
3. PR 描述首行附带 `Closes #<Issue编号>`, 以便 PR 合并后 GitHub 自动关闭关联 Issue.

### PR 提交前自检清单

- [ ] 代码已通过 `cargo fmt --check` (或已运行 `./install-hooks.sh`)
- [ ] Clippy 检查无任何 warning (`RUSTFLAGS='-D warnings' cargo clippy --all-targets`)
- [ ] 核心测试通过 (`cargo test -- --test-threads=1`)
- [ ] 若改动涉及 `cluster` / `compression` / `monitoring`, 对应 feature 专项测试已通过
- [ ] 若为 bug 修复: 已在同一 PR 内附带回归测试, 且用例附有清晰的中文 `///` 注释
- [ ] 若新建或改动测试文件: 测试文件顶部包含 `//! @component` 标注与中文说明
- [ ] 若修改了 slow/stress 用例: `cargo test -- --ignored --test-threads=1` 验证通过
- [ ] 相关文档已同步更新, 或在 PR 描述中已注明「文档无需变更」

### CI 门禁

PR 提交后将自动触发 GitHub Actions CI 检查. 所有流水线 (Lint、Test、Security、Docs Link Check) 全绿后方可进入代码审查与合并. 详细的 CI Job 编排与说明见 [`.github/README.md`](.github/README.md).
