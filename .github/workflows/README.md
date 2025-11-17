# GitHub Actions Workflows

本目录包含 AiDb 项目的所有 GitHub Actions 工作流配置。

## 📋 工作流列表

### 自动运行的工作流

#### 1. CI Pipeline (`ci.yml`)
**触发条件**: PR 到 main (仅在 ready for review 时)
**用途**: 持续集成，确保代码质量

**智能检测**: 
- 🚀 **文档变更**: 只修改文档时跳过代码测试，仅运行文档检查（快速通过）
- 🔧 **代码变更**: 包含代码修改时运行完整测试套件
- 📋 **混合变更**: 同时修改文档和代码时运行完整测试

**注意**: 
- 功能分支的 push 不会触发 CI
- Draft PR 不会触发 CI
- 只有当 PR 标记为 "Ready for review" 时才会运行

**Jobs数量**: 9个 (changes, test, clippy, fmt, coverage, build, bench, docs-check, ci-success)

包含的任务：
- ✅ 测试 (多平台、多版本)
- 🔍 Clippy 静态分析
- 📝 格式检查
- 📊 代码覆盖率
- 🔨 构建检查
- ⚡ 基准测试检查（仅检查编译，不运行）

#### 2. Security Audit (`security.yml`)
**触发条件**: Push/PR 到 main，每日定时运行
**用途**: 安全扫描和依赖检查

包含的任务：
- 🛡️ Cargo Audit (漏洞扫描)
- 📜 Cargo Deny (许可证检查)
- 📦 过期依赖检查
- 🔐 CodeQL 安全分析

#### 3. Release (`release.yml`)
**触发条件**: Push 版本标签 (v*.*.*)
**用途**: 自动发布和构建

包含的任务：
- 📦 创建 GitHub Release
- 🏗️ 多平台编译 (Linux, macOS, Windows)
- 📤 上传构建产物
- 🚀 发布到 crates.io

### 手动触发的工作流 (Manual Workflows)

### 4. Stress Tests (`stress-test.yml`) ⚡
**触发条件**: 手动触发 (workflow_dispatch)
**用途**: 运行长时间、高成本的压力测试

**为什么需要手动触发**：
- 这些测试需要较长时间（10分钟-2小时）
- 消耗大量系统资源
- 不适合在常规 CI 流程中运行
- 避免阻塞其他开发流程

**手动触发方法**：
1. 进入 [Actions 页面](../../actions)
2. 选择 "Stress Tests" 工作流
3. 点击 "Run workflow"
4. 选择测试配置：
   - `duration_minutes`: 测试时长（10/30/60/120分钟）
   - `test_type`: 测试类型（all/write-heavy/read-heavy/mixed/memory）
5. 点击 "Run workflow" 开始测试

**包含的测试**：
- 🔥 高频写入测试（100k ops/s 目标）
- 📖 高频读取测试
- 🔄 混合读写负载测试
- 💾 内存压力测试
- 📦 大值写入测试（1MB+）
- ⏳ 长时间运行测试（1小时+）
- 💿 磁盘空间压力测试

### 5. Performance Benchmarks (`benchmark.yml`) 📊
**触发条件**: 手动触发 (workflow_dispatch)
**用途**: 运行性能基准测试并生成报告

**为什么需要手动触发**：
- 基准测试需要稳定的环境以获得可重复的结果
- 运行时间较长（通常需要 10-30 分钟）
- 不适合在每次 PR 时运行
- 通常在重要性能优化后或发布前运行

**手动触发方法**：
1. 进入 [Actions 页面](../../actions)
2. 选择 "Performance Benchmarks" 工作流
3. 点击 "Run workflow"
4. 选择基准测试配置：
   - `benchmark_type`: 基准测试类型（all/write/read/mixed）
   - `compare_baseline`: 是否与主分支基线对比（true/false）
5. 点击 "Run workflow" 开始测试

**包含的基准测试**：
- ✍️ 写入性能基准测试（顺序写入、随机写入、批量写入）
- 📚 读取性能基准测试（顺序读取、随机读取、范围查询）
- 🔄 混合负载基准测试
- 📈 性能趋势对比（与主分支对比）

**查看结果**：
- 测试完成后，下载 artifacts 中的 `benchmark-results`
- 打开 `target/criterion/report/index.html` 查看详细报告
- 查看 `benchmark-report.txt` 获取快速摘要

## 🚀 快速开始

### 本地测试 CI 检查

在提交 PR 前，本地运行这些命令：

```bash
# 运行测试
cargo test --all-features --verbose

# 运行 Clippy
cargo clippy --all-targets --all-features -- -D warnings

# 检查格式
cargo fmt --all -- --check

# 构建
cargo build --all-features
```

### 本地运行压力测试

压力测试默认被标记为 `#[ignore]`，不会在常规测试中运行：

```bash
# 运行所有压力测试（可能需要数小时）
cargo test --release -- --ignored --nocapture

# 运行特定的压力测试
cargo test --release stress_high_frequency_writes -- --ignored --nocapture
cargo test --release stress_mixed_workload -- --ignored --nocapture

# 仅列出可用的压力测试
cargo test --test stress_tests -- --list
```

**注意**：
- 压力测试应该使用 `--release` 模式以获得更准确的性能数据
- 使用 `--nocapture` 查看测试输出和性能统计
- 某些测试（如 `stress_long_running`）可能运行超过 1 小时

### 本地运行基准测试

```bash
# 运行所有基准测试
cargo bench --all-features

# 运行特定基准测试
cargo bench --bench write_bench
cargo bench --bench read_bench

# 保存基准测试结果作为基线
cargo bench --all-features -- --save-baseline my-baseline

# 与基线对比
cargo bench --all-features -- --baseline my-baseline
```

**查看基准测试结果**：
- 结果保存在 `target/criterion/` 目录
- 打开 `target/criterion/report/index.html` 查看可视化报告

### 创建新版本发布

```bash
# 1. 更新版本号
# 编辑 Cargo.toml 中的 version 字段

# 2. 更新 CHANGELOG
# 编辑 CHANGELOG.md

# 3. 提交更改
git add Cargo.toml CHANGELOG.md
git commit -m "chore: bump version to 0.2.0"
git push

# 4. 创建并推送标签
git tag v0.2.0
git push origin v0.2.0

# 5. GitHub Actions 会自动：
#    - 运行所有测试
#    - 构建多平台二进制
#    - 创建 GitHub Release
#    - 发布到 crates.io
```

## 📊 查看工作流状态

访问以下页面查看工作流运行状态：
- [Actions 页面](../../actions)
- [CI 工作流](../../actions/workflows/ci.yml)
- [Security 工作流](../../actions/workflows/security.yml)
- [Release 工作流](../../actions/workflows/release.yml)
- [Stress Tests 工作流](../../actions/workflows/stress-test.yml)
- [Performance Benchmarks 工作流](../../actions/workflows/benchmark.yml)

## 🔧 配置

### 必需的 Secrets

在仓库设置中配置：

| Secret | 用途 | 状态 |
|--------|------|------|
| `GITHUB_TOKEN` | GitHub API | ✅ 自动提供 |
| `CARGO_TOKEN` | crates.io 发布 | ⚠️ 需要配置 |
| `CODECOV_TOKEN` | 代码覆盖率 | ⚠️ 推荐配置 |

### 获取 Token

**CARGO_TOKEN**:
1. 访问 https://crates.io/settings/tokens
2. 创建新 token
3. 在 GitHub 仓库设置中添加 Secret

**CODECOV_TOKEN**:
1. 访问 https://codecov.io
2. 使用 GitHub 登录并添加仓库
3. 复制 token
4. 在 GitHub 仓库设置中添加 Secret

## 📚 文档

详细的 CI/CD 文档：[docs/CICD.md](../../docs/CICD.md)

## 🤝 贡献

如需修改工作流配置：
1. 在功能分支中进行修改
2. 测试修改（可使用 [act](https://github.com/nektos/act) 本地测试）
3. 创建 PR
4. 等待审查和合并
