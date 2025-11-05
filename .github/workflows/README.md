# GitHub Actions Workflows

本目录包含 AiDb 项目的所有 GitHub Actions 工作流配置。

## 📋 工作流列表

### 1. CI Pipeline (`ci.yml`)
**触发条件**: Push 到 main 分支，PR 到 main (仅在 ready for review 时)
**用途**: 持续集成，确保代码质量

**注意**: 
- 功能分支的 push 不会触发 CI
- Draft PR 不会触发 CI
- 只有当 PR 标记为 "Ready for review" 时才会运行

包含的任务：
- ✅ 测试 (多平台、多版本)
- 🔍 Clippy 静态分析
- 📝 格式检查
- 📊 代码覆盖率
- 🔨 构建检查
- ⚡ 基准测试检查

### 2. Security Audit (`security.yml`)
**触发条件**: Push/PR 到 main，每日定时运行
**用途**: 安全扫描和依赖检查

包含的任务：
- 🛡️ Cargo Audit (漏洞扫描)
- 📜 Cargo Deny (许可证检查)
- 📦 过期依赖检查
- 🔐 CodeQL 安全分析

### 3. Release (`release.yml`)
**触发条件**: Push 版本标签 (v*.*.*)
**用途**: 自动发布和构建

包含的任务：
- 📦 创建 GitHub Release
- 🏗️ 多平台编译 (Linux, macOS, Windows)
- 📤 上传构建产物
- 🚀 发布到 crates.io

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
