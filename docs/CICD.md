# CI/CD Pipeline 文档

本文档描述 AiDb 项目的持续集成和持续交付流程。

## 目录

- [概述](#概述)
- [CI Pipeline](#ci-pipeline)
- [Security Pipeline](#security-pipeline)
- [Release Pipeline](#release-pipeline)
- [配置说明](#配置说明)
- [徽章状态](#徽章状态)
- [常见问题](#常见问题)

---

## 概述

AiDb 使用 GitHub Actions 实现自动化的 CI/CD 流程，包括：

- ✅ **自动化测试** - 多平台、多 Rust 版本测试
- 🔍 **代码质量检查** - Clippy 静态分析
- ✨ **自动代码格式化** - 使用 rustfmt 自动格式化代码
- 🛡️ **安全扫描** - 依赖漏洞扫描、许可证检查
- 📦 **自动发布** - 多平台编译、自动创建 Release
- 📊 **代码覆盖率** - Codecov 集成

### Pipeline 架构

```
┌─────────────┐
│   Push/PR   │
└──────┬──────┘
       │
       ├──────────────┬──────────────┬───────────────┐
       │              │              │               │
       v              v              v               v
  ┌────────┐    ┌─────────┐   ┌───────────┐   ┌──────────┐
  │  Test  │    │ Clippy  │   │Auto-Format│   │ Security │
  └────────┘    └─────────┘   └───────────┘   └──────────┘
       │              │              │               │
       └──────────────┴──────────────┴───────────────┘
                      │
                      v
              ┌──────────────┐
              │   All Pass   │
              └──────┬───────┘
                     │
            ┌────────┴────────┐
            │                 │
            v                 v
       ┌─────────┐      ┌──────────┐
       │  Merge  │      │  Deploy  │
       └─────────┘      └──────────┘
```

---

## CI Pipeline

### 工作流: `.github/workflows/ci.yml`

#### 触发条件

- Push 到 `main` 分支
- Pull Request 到 `main` 分支 (仅在 ready for review 时运行)
  - 支持的事件类型: `opened`, `synchronize`, `reopened`, `ready_for_review`
  - Draft PR 不会触发 CI 流水线
  - 只有当 PR 标记为 "Ready for review" 时才会运行测试

#### 智能检测

CI 流水线包含智能文件变更检测：

- **只修改文档**: 如果PR只修改了文档文件（`*.md`, `docs/`, `LICENSE`, `CHANGELOG.md`等），将跳过所有代码测试，只运行文档检查
- **包含代码变更**: 如果包含任何代码文件的修改，将运行完整的测试套件
- **文档文件识别**:
  - Markdown 文件 (`**/*.md`)
  - 文档目录 (`docs/**`)
  - 许可证和变更日志 (`LICENSE`, `CHANGELOG.md`)
  - 工作流说明 (`.github/workflows/README.md`)

这样可以大大减少CI运行时间和资源消耗，同时保持代码质量。

#### 任务说明

##### 0. Detect Changes (变更检测)

**目的**: 智能检测文件变更类型，决定需要运行哪些测试

**工具**: [dorny/paths-filter](https://github.com/dorny/paths-filter)

**检测类型**:
- `code`: 代码文件变更（src/, tests/, Cargo.toml等）
- `docs_only`: 仅文档文件变更（*.md, docs/等）

**步骤**:
```yaml
- 检出代码
- 运行 paths-filter 检测文件变更
- 输出变更类型供后续jobs使用
```

**影响**:
- 如果检测到代码变更 → 运行所有代码测试
- 如果只有文档变更 → 跳过代码测试，只运行文档检查

##### 1. Test Suite (测试套件)

**目的**: 确保代码在多平台、多 Rust 版本下正常工作

**测试矩阵**:
- **操作系统**: Ubuntu, macOS, Windows
- **Rust 版本**: stable, beta, nightly

**步骤**:
```yaml
- 检出代码
- 安装 Rust 工具链
- 缓存依赖 (registry, git, target)
- 运行单元测试和集成测试
- 运行文档测试
```

**命令**:
```bash
cargo test --all-features --verbose
cargo test --doc --all-features
```

##### 2. Clippy (静态代码分析)

**目的**: 检查代码质量和潜在问题

**配置**: 将所有 Clippy 警告视为错误 (`-D warnings`)

**步骤**:
```yaml
- 检出代码
- 安装 Rust 工具链 (包含 clippy 组件)
- 运行 Clippy
```

**命令**:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

**常见 Clippy 检查**:
- 未使用的变量和导入
- 不符合习惯的代码
- 性能问题
- 可能的 bug 模式

##### 3. Code Coverage (代码覆盖率)

**目的**: 测量测试覆盖率

**工具**: [cargo-tarpaulin](https://github.com/xd009642/tarpaulin)

**步骤**:
```yaml
- 检出代码
- 安装 Rust 工具链
- 安装 tarpaulin
- 生成覆盖率报告
- 上传到 Codecov
```

**命令**:
```bash
cargo tarpaulin --all-features --workspace --timeout 300 --out xml
```

**查看报告**: 访问 [Codecov Dashboard](https://codecov.io)

##### 4. Build Check (构建检查 + 自动格式化)

**目的**: 自动格式化代码并验证代码可以成功编译

**特性**: 
- ✨ **自动格式化**: 使用 `rustfmt` 自动格式化代码
- 🔄 **自动提交**: 如果有格式变更，自动提交并推送
- 🚫 **Fork 保护**: 来自 fork 的 PR 会提示在本地运行格式化

**步骤**:
```yaml
- 检出代码 (with write permission)
- 安装 Rust 工具链 (包含 rustfmt 组件)
- 运行自动格式化
  - 执行 cargo fmt --all
  - 如果有变更，自动提交并推送 (仅限同仓库 PR)
  - 如果来自 fork，提示在本地运行格式化
- Debug 模式构建
- Release 模式构建
- 构建所有示例
```

**自动格式化命令**:
```bash
cargo fmt --all
```

**行为**:
- ✅ **同仓库 PR**: 自动格式化并推送，commit message 包含 `[skip ci]` 避免循环触发
- ⚠️ **Fork PR**: 无法自动推送，CI 会失败并提示在本地运行 `cargo fmt --all`
- 📝 **提交消息**: `style: auto-format code with rustfmt [skip ci]`

**优势**:
- 不再需要手动运行格式化命令
- 确保所有合并的代码都符合统一的格式规范
- 减少因格式问题导致的 PR 往返
- 开发者只需关注代码逻辑，格式由 CI 自动处理

**构建命令**:
```bash
cargo build --all-features
cargo build --release --all-features
cargo build --examples
```

##### 5. Benchmark Check (基准测试检查)

**目的**: 确保基准测试可以编译

**步骤**:
```yaml
- 检查基准测试编译 (不运行)
```

**命令**:
```bash
cargo bench --no-run --all-features
```

##### 6. Documentation Check (文档检查)

**目的**: 验证文档文件的完整性和结构

**触发条件**: 仅在只修改文档文件时运行

**步骤**:
```yaml
- 检出代码
- 检查所有 Markdown 文件
- 验证重要文档是否存在（README.md, LICENSE, CHANGELOG.md）
```

**优势**:
- 快速验证文档变更
- 不需要运行耗时的代码测试
- 保证文档的基本完整性

##### 7. CI Success (CI状态汇总)

**目的**: 提供统一的CI状态检查点，用于分支保护规则

**特性**:
- 总是运行（`if: always()`）
- 依赖所有其他jobs
- 根据实际运行的jobs判断成功/失败

**逻辑**:
```yaml
- 如果有代码变更 → 检查所有代码jobs是否成功
- 如果只有文档变更 → 只检查文档job是否成功
- 任何job失败 → 整体失败
```

**用途**:
- 在GitHub分支保护规则中，只需要检查这一个job
- 简化PR合并的状态检查
- 提供清晰的CI运行摘要

---

## Security Pipeline

### 工作流: `.github/workflows/security.yml`

#### 触发条件

- Push 到 `main` 分支
- Pull Request 到 `main` 分支
- 每日自动运行 (UTC 00:00)

#### 任务说明

##### 1. Cargo Audit (安全审计)

**目的**: 检查依赖项的已知安全漏洞

**工具**: [cargo-audit](https://github.com/RustSec/rustsec/tree/main/cargo-audit)

**数据源**: [RustSec Advisory Database](https://github.com/rustsec/advisory-db)

**命令**:
```bash
cargo audit
```

**示例输出**:
```
Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 450 security advisories (from /home/user/.cargo/advisory-db)
    Scanning Cargo.lock for vulnerabilities (123 crate dependencies)
```

##### 2. Cargo Deny (依赖策略检查)

**目的**: 检查依赖的许可证、重复版本、禁用的 crate

**工具**: [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)

**配置文件**: `deny.toml`

**检查项**:
- **Advisories**: 安全公告
- **Licenses**: 许可证合规性
- **Bans**: 禁止的依赖
- **Sources**: 可信的源

**命令**:
```bash
cargo deny check
```

**配置示例** (`deny.toml`):
```toml
[licenses]
unlicensed = "deny"
allow = ["MIT", "Apache-2.0", "BSD-3-Clause"]

[advisories]
vulnerability = "deny"
unmaintained = "warn"

[bans]
multiple-versions = "warn"
```

##### 3. Check Outdated Dependencies (过期依赖检查)

**目的**: 检查可更新的依赖

**工具**: [cargo-outdated](https://github.com/kbknapp/cargo-outdated)

**命令**:
```bash
cargo outdated
```

**示例输出**:
```
Name       Project  Compat  Latest  Kind    Platform
----       -------  ------  ------  ----    --------
anyhow     1.0.75   ---     1.0.80  Normal  ---
tokio      1.32.0   ---     1.35.1  Normal  ---
```

##### 4. CodeQL Security Scan (代码安全扫描)

**目的**: 深度代码安全分析

**工具**: [GitHub CodeQL](https://codeql.github.com/)

**分析内容**:
- SQL 注入
- 跨站脚本 (XSS)
- 路径遍历
- 命令注入
- 内存安全问题

**查看结果**: GitHub Security 选项卡

---

## Release Pipeline

### 工作流: `.github/workflows/release.yml`

#### 触发条件

推送版本标签时触发:
```bash
git tag v0.1.0
git push origin v0.1.0
```

**标签格式**:
- `v1.2.3` - 正式版本
- `v1.2.3-alpha.1` - Alpha 版本
- `v1.2.3-beta.1` - Beta 版本
- `v1.2.3-rc.1` - Release Candidate

#### 任务说明

##### 1. Create Release (创建 Release)

**步骤**:
1. 检出代码 (包含完整历史)
2. 从标签提取版本号
3. 生成 Changelog (自上个标签以来的提交)
4. 创建 GitHub Release

**Changelog 格式**:
```
- feat: implement WAL (abc123)
- fix: memory leak in compaction (def456)
- docs: update API documentation (ghi789)
```

**预发布判断**:
- 包含 `-alpha`、`-beta`、`-rc` 的标签标记为预发布

##### 2. Build Release (构建发布版本)

**目标平台**:

| 操作系统 | 架构 | Target |
|---------|------|--------|
| Linux   | x86_64 | x86_64-unknown-linux-gnu |
| Linux   | x86_64 (musl) | x86_64-unknown-linux-musl |
| Linux   | ARM64 | aarch64-unknown-linux-gnu |
| macOS   | x86_64 (Intel) | x86_64-apple-darwin |
| macOS   | ARM64 (M1/M2) | aarch64-apple-darwin |
| Windows | x86_64 | x86_64-pc-windows-msvc |
| Windows | ARM64 | aarch64-pc-windows-msvc |

**构建步骤**:
1. 安装目标平台工具链
2. 安装交叉编译工具 (如需)
3. 构建 Release 版本
4. Strip 二进制 (减小体积)
5. 创建归档文件 (tar.gz 或 zip)
6. 上传到 GitHub Release

**交叉编译配置**:
```yaml
# Linux musl (静态链接)
- 安装 musl-tools

# ARM64 Linux
- 安装 gcc-aarch64-linux-gnu
```

**归档文件命名**:
```
aidb-{version}-{target}.{ext}

示例:
aidb-0.1.0-x86_64-unknown-linux-gnu.tar.gz
aidb-0.1.0-x86_64-pc-windows-msvc.zip
```

##### 3. Publish to crates.io (发布到 crates.io)

**前提条件**:
- 需要配置 `CARGO_TOKEN` Secret

**步骤**:
1. 验证 Cargo.toml
2. 发布到 crates.io

**命令**:
```bash
cargo publish --token $CARGO_TOKEN
```

**注意事项**:
- 发布后无法删除
- 版本号不能重复
- 需要验证邮箱

---

## 配置说明

### GitHub Secrets

需要在仓库设置中配置以下 Secrets:

| Secret 名称 | 用途 | 必需 |
|------------|------|------|
| `GITHUB_TOKEN` | GitHub API 访问 | ✅ 自动提供 |
| `CARGO_TOKEN` | 发布到 crates.io | ⚠️ 发布时需要 |
| `CODECOV_TOKEN` | 上传代码覆盖率 | ⚠️ 推荐配置 |

#### 获取 CARGO_TOKEN

1. 访问 [crates.io/settings/tokens](https://crates.io/settings/tokens)
2. 创建新的 API Token
3. 在 GitHub 仓库设置中添加 Secret:
   - Name: `CARGO_TOKEN`
   - Value: 你的 token

#### 获取 CODECOV_TOKEN

1. 访问 [codecov.io](https://codecov.io)
2. 使用 GitHub 登录
3. 添加仓库
4. 复制 Upload Token
5. 在 GitHub 仓库设置中添加 Secret:
   - Name: `CODECOV_TOKEN`
   - Value: 你的 token

### Dependabot 配置

文件: `.github/dependabot.yml`

**功能**:
- 自动检查依赖更新
- 自动创建 PR
- 每周检查一次

**配置**:
```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
```

**使用建议**:
- 定期审查和合并 Dependabot PR
- 注意破坏性更新
- 运行测试后再合并

---

## 徽章状态

在 README.md 中添加状态徽章:

### CI 状态

```markdown
[![CI](https://github.com/yourusername/aidb/workflows/CI/badge.svg)](https://github.com/yourusername/aidb/actions/workflows/ci.yml)
```

### Security Audit

```markdown
[![Security Audit](https://github.com/yourusername/aidb/workflows/Security%20Audit/badge.svg)](https://github.com/yourusername/aidb/actions/workflows/security.yml)
```

### Code Coverage

```markdown
[![codecov](https://codecov.io/gh/yourusername/aidb/branch/main/graph/badge.svg)](https://codecov.io/gh/yourusername/aidb)
```

### Crates.io

```markdown
[![Crates.io](https://img.shields.io/crates/v/aidb.svg)](https://crates.io/crates/aidb)
[![Downloads](https://img.shields.io/crates/d/aidb.svg)](https://crates.io/crates/aidb)
```

### License

```markdown
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
```

---

## 常见问题

### Q1: CI 测试失败怎么办？

**检查步骤**:
1. 查看失败的测试日志
2. 在本地运行相同的测试
   ```bash
   cargo test
   cargo clippy --all-targets --all-features -- -D warnings
   ```
3. 修复问题后重新提交

**注意**: 不再需要手动运行 `cargo fmt --all`，CI 会自动格式化代码

### Q2: 在功能分支上 CI 不运行是正常的吗？

**是的！** 从最新配置开始，CI 流水线只在以下情况运行：
- Push 到 `main` 分支
- Pull Request 标记为 "Ready for review"

**工作流程**:
1. 在功能分支上开发时，push 不会触发 CI（节省资源）
2. 创建 PR 到 `main` 分支时:
   - 如果是 Draft PR，CI 不会运行
   - 当标记为 "Ready for review" 时，CI 才开始运行
3. PR 合并到 `main` 后，会再次运行完整的 CI

**如需在功能分支测试**:
```bash
# 本地运行所有检查
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
# 格式化会在 CI 中自动完成，但你也可以本地运行
cargo fmt --all
```

### Q2.2: 为什么只修改文档时，代码测试被跳过了？

**这是预期行为！** CI 包含智能文件变更检测：

**文档文件包括**:
- 所有 Markdown 文件 (`*.md`)
- `docs/` 目录
- `LICENSE`, `CHANGELOG.md`
- 工作流说明文件

**行为**:
- ✅ **只修改文档**: 跳过代码测试（test, clippy, build, bench, coverage），只运行文档检查
- ✅ **修改代码**: 运行完整的测试套件（包括自动格式化）
- ✅ **同时修改**: 运行完整的测试套件

**优势**:
- 大幅减少CI运行时间（文档PR通常只需几秒）
- 节省CI资源
- 鼓励更新文档

**查看运行的jobs**:
在GitHub Actions页面，你会看到：
- 文档PR: `changes` ✓, `docs-check` ✓, `ci-success` ✓
- 代码PR: `changes` ✓, `test` ✓, `clippy` ✓, `build` ✓ (含自动格式化), 等等...

### Q2.1: 如何跳过 CI？

在 commit message 中添加 `[skip ci]` 或 `[ci skip]`:
```bash
git commit -m "docs: update README [skip ci]"
```

**注意**: 
- 谨慎使用，可能违反分支保护规则
- 在当前配置下，功能分支的 push 已经不触发 CI

### Q3: 如何本地测试 Release 构建？

```bash
# 构建当前平台
cargo build --release --all-features

# 检查二进制大小
ls -lh target/release/aidb

# 使用 strip 减小体积
strip target/release/aidb
```

### Q4: Security Audit 发现漏洞怎么办？

1. **查看详情**:
   ```bash
   cargo audit
   ```

2. **更新依赖**:
   ```bash
   cargo update
   ```

3. **如果无法更新**:
   - 查看是否有补丁版本
   - 考虑替换依赖
   - 在 `deny.toml` 中临时忽略 (添加说明)

### Q5: 如何发布新版本？

**步骤**:

1. **更新版本号**:
   ```toml
   # Cargo.toml
   [package]
   version = "0.2.0"
   ```

2. **更新 CHANGELOG** (如有):
   ```markdown
   ## [0.2.0] - 2024-01-15
   ### Added
   - New feature X
   ### Fixed
   - Bug Y
   ```

3. **提交更改**:
   ```bash
   git add Cargo.toml CHANGELOG.md
   git commit -m "chore: bump version to 0.2.0"
   git push
   ```

4. **创建并推送标签**:
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

5. **等待 CI 完成**:
   - 构建多平台二进制
   - 创建 GitHub Release
   - 发布到 crates.io

6. **验证发布**:
   - 检查 GitHub Releases 页面
   - 检查 crates.io 页面
   - 测试安装: `cargo install aidb`

### Q6: 如何调试 GitHub Actions？

**方法 1: 启用调试日志**

在 workflow 中添加:
```yaml
env:
  ACTIONS_STEP_DEBUG: true
  ACTIONS_RUNNER_DEBUG: true
```

**方法 2: 添加调试步骤**

```yaml
- name: Debug info
  run: |
    echo "Current directory: $(pwd)"
    echo "Rust version: $(rustc --version)"
    echo "Cargo version: $(cargo --version)"
    ls -la
```

**方法 3: 使用 Act 本地运行**

安装 [Act](https://github.com/nektos/act):
```bash
# macOS
brew install act

# Linux
curl https://raw.githubusercontent.com/nektos/act/master/install.sh | sudo bash
```

运行工作流:
```bash
act push
act pull_request
```

### Q7: 缓存不工作？

**原因**:
- `Cargo.lock` 改变
- 依赖更新
- 缓存过期 (7天)

**解决方法**:
```yaml
- name: Clear cache
  run: |
    rm -rf ~/.cargo/registry
    rm -rf ~/.cargo/git
    rm -rf target
```

或在 GitHub Actions 页面手动清除缓存

### Q8: 如何测试特定平台？

**使用 cross**:

```bash
# 安装 cross
cargo install cross

# 构建 Linux ARM64
cross build --target aarch64-unknown-linux-gnu --release

# 构建 Windows
cross build --target x86_64-pc-windows-gnu --release
```

---

## 最佳实践

### 1. 分支保护规则

在 GitHub 仓库设置中配置:

```
Settings -> Branches -> Add rule

规则:
☑ Require status checks to pass before merging
  ☑ CI Success (推荐只检查这一个统一的状态)
  或者单独检查:
  ☑ test
  ☑ clippy
  ☑ build (包含自动格式化)
☑ Require branches to be up to date before merging
☑ Include administrators
```

### 2. Commit 规范

使用 [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

Types:
- feat: 新功能
- fix: Bug 修复
- docs: 文档
- style: 格式
- refactor: 重构
- test: 测试
- chore: 构建/工具

示例:
feat(wal): implement write-ahead log
fix(compaction): memory leak in level merger
docs: update API documentation
```

### 3. PR 模板

创建 `.github/pull_request_template.md`:

```markdown
## Description
<!-- 描述你的更改 -->

## Type of change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Checklist
- [ ] Tests pass locally (`cargo test`)
- [ ] Added tests for new code
- [ ] Updated documentation
- [ ] Ran `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Code will be auto-formatted by CI (no need to run `cargo fmt` manually)
```

### 4. Issue 模板

创建 `.github/ISSUE_TEMPLATE/bug_report.md`:

```markdown
---
name: Bug Report
about: Report a bug
---

## Bug Description
<!-- 清晰描述问题 -->

## Steps to Reproduce
1. 
2. 
3. 

## Expected Behavior
<!-- 预期行为 -->

## Actual Behavior
<!-- 实际行为 -->

## Environment
- OS: 
- Rust version: 
- AiDb version: 
```

---

## 监控和维护

### 定期检查

- [ ] 每周查看 Dependabot PR
- [ ] 每月审查 Security Audit 结果
- [ ] 每季度更新依赖
- [ ] 定期检查 CodeQL 建议

### 性能监控

考虑添加性能回归测试:
```yaml
- name: Run benchmarks
  run: cargo bench -- --save-baseline main

- name: Compare with baseline
  run: cargo bench -- --baseline main
```

---

## 参考资料

- [GitHub Actions 文档](https://docs.github.com/en/actions)
- [Rust CI 最佳实践](https://doc.rust-lang.org/cargo/guide/continuous-integration.html)
- [cargo-audit](https://github.com/RustSec/rustsec/tree/main/cargo-audit)
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
- [Codecov](https://docs.codecov.com/)
- [Conventional Commits](https://www.conventionalcommits.org/)

---

**维护者**: AiDb Team  
**最后更新**: 2024-01-15
