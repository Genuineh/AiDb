# GitHub Actions 设置指南

本指南帮助你完成 AiDb 项目的 GitHub Actions CI/CD 配置。

## ✅ 已完成的配置

以下文件已创建并配置：

### 工作流文件
- ✅ `.github/workflows/ci.yml` - CI 流水线
- ✅ `.github/workflows/security.yml` - 安全扫描
- ✅ `.github/workflows/release.yml` - 自动发布
- ✅ `.github/dependabot.yml` - 依赖更新

### 配置文件
- ✅ `deny.toml` - cargo-deny 配置
- ✅ `CHANGELOG.md` - 版本更新日志

### 模板文件
- ✅ `.github/pull_request_template.md` - PR 模板
- ✅ `.github/ISSUE_TEMPLATE/bug_report.md` - Bug 报告模板
- ✅ `.github/ISSUE_TEMPLATE/feature_request.md` - 功能请求模板
- ✅ `.github/ISSUE_TEMPLATE/question.md` - 问题模板

### 文档
- ✅ `docs/CICD.md` - CI/CD 详细文档
- ✅ `.github/workflows/README.md` - 工作流说明
- ✅ 更新 `README.md` - 添加 CI 徽章
- ✅ 更新 `CONTRIBUTING.md` - 添加 CI 说明

## 🔧 需要手动配置的项目

### 1. GitHub Secrets 配置

在 GitHub 仓库设置中添加以下 Secrets：

#### CARGO_TOKEN (必需，用于发布到 crates.io)

1. 访问 https://crates.io/settings/tokens
2. 点击 "New Token"
3. 填写信息：
   - Token name: `github-actions-publish`
   - Scope: 选择 `publish-update`
4. 复制生成的 token
5. 在 GitHub 仓库中：
   - 进入 `Settings` → `Secrets and variables` → `Actions`
   - 点击 `New repository secret`
   - Name: `CARGO_TOKEN`
   - Secret: 粘贴你的 token
   - 点击 `Add secret`

#### CODECOV_TOKEN (推荐，用于代码覆盖率)

1. 访问 https://codecov.io
2. 使用 GitHub 账号登录
3. 点击 `Add new repository`
4. 选择 `aidb` 仓库
5. 复制显示的 Upload Token
6. 在 GitHub 仓库中：
   - 进入 `Settings` → `Secrets and variables` → `Actions`
   - 点击 `New repository secret`
   - Name: `CODECOV_TOKEN`
   - Secret: 粘贴你的 token
   - 点击 `Add secret`

### 2. 分支保护规则 (推荐)

保护 `main` 分支，确保代码质量：

1. 进入 `Settings` → `Branches`
2. 点击 `Add rule`
3. 配置规则：
   ```
   Branch name pattern: main
   
   ✅ Require a pull request before merging
      ✅ Require approvals: 1
   
   ✅ Require status checks to pass before merging
      ✅ Require branches to be up to date before merging
      添加必需的状态检查：
         - test (ubuntu-latest, stable)
         - clippy
         - fmt
   
   ✅ Require conversation resolution before merging
   
   ✅ Include administrators
   ```
4. 点击 `Create` 保存

### 3. 启用 GitHub Actions (通常自动启用)

1. 进入 `Actions` 标签
2. 如果看到 "Workflows aren't being run on this repository"
3. 点击 "I understand my workflows, go ahead and enable them"

### 4. 配置 Dependabot 警报 (推荐)

1. 进入 `Settings` → `Security & analysis`
2. 启用以下选项：
   - ✅ Dependency graph
   - ✅ Dependabot alerts
   - ✅ Dependabot security updates

### 5. 更新 README 中的链接

编辑 `README.md`，将 `yourusername` 替换为实际的 GitHub 用户名/组织名：

```markdown
[![CI](https://github.com/yourusername/aidb/workflows/CI/badge.svg)]...
                      ^^^^^^^^^^^^
```

可以使用以下命令批量替换：
```bash
# macOS
sed -i '' 's/yourusername/your-actual-username/g' README.md

# Linux
sed -i 's/yourusername/your-actual-username/g' README.md
```

## 🚀 测试 CI/CD 设置

### 测试 CI Pipeline

创建一个测试分支并推送：

```bash
git checkout -b test/ci-setup
git add .
git commit -m "ci: setup GitHub Actions pipeline"
git push origin test/ci-setup
```

然后：
1. 在 GitHub 上创建 PR
2. 观察 Actions 标签中的工作流运行
3. 确保所有检查通过（绿色 ✓）

### 测试 Security Pipeline

Security 工作流会在 PR 创建时自动运行，你可以在 Actions 标签查看。

### 测试 Release Pipeline

**注意**：仅在准备好正式发布时测试！

```bash
# 1. 确保在 main 分支
git checkout main
git pull

# 2. 更新版本（如果还未更新）
# 编辑 Cargo.toml: version = "0.1.0"

# 3. 创建标签
git tag v0.1.0

# 4. 推送标签
git push origin v0.1.0

# 5. 观察 Actions 标签中的 Release 工作流
```

发布完成后，检查：
- ✅ GitHub Releases 页面有新的 release
- ✅ Release 包含多平台的二进制文件
- ✅ crates.io 上可以看到新版本

## 📊 监控和维护

### 定期检查

**每周**：
- 查看 Dependabot PR 并合并
- 检查 Security Audit 结果

**每月**：
- 审查所有依赖更新
- 检查是否有新的安全建议

**每季度**：
- 更新 Rust 工具链版本
- 审查和更新 GitHub Actions

### 查看 CI/CD 状态

- **Actions 页面**: https://github.com/yourusername/aidb/actions
- **Security 页面**: https://github.com/yourusername/aidb/security
- **Insights → Dependency graph**: 查看依赖关系

## 🐛 故障排查

### CI 测试失败

1. 点击失败的检查查看日志
2. 本地复现：
   ```bash
   cargo test
   cargo clippy --all-targets --all-features -- -D warnings
   cargo fmt --all -- --check
   ```
3. 修复问题后重新推送

### Release 构建失败

检查：
- Cargo.toml 版本号是否正确
- 是否所有平台都能编译
- CARGO_TOKEN 是否正确配置

本地测试不同平台：
```bash
# 安装 cross
cargo install cross

# 测试 Linux
cross build --target x86_64-unknown-linux-gnu --release

# 测试 Windows
cross build --target x86_64-pc-windows-gnu --release
```

### 无法发布到 crates.io

检查：
- CARGO_TOKEN 是否正确
- crates.io 账号是否验证邮箱
- 包名是否已被占用
- Cargo.toml 是否包含所有必需字段

## 📚 更多资源

- [CI/CD 完整文档](../docs/CICD.md)
- [开发指南](../docs/DEVELOPMENT.md)
- [贡献指南](../CONTRIBUTING.md)
- [GitHub Actions 文档](https://docs.github.com/en/actions)
- [Cargo 发布指南](https://doc.rust-lang.org/cargo/reference/publishing.html)

## ✅ 设置完成检查清单

完成以下检查，确保所有配置正确：

- [ ] CARGO_TOKEN 已配置
- [ ] CODECOV_TOKEN 已配置（可选）
- [ ] 分支保护规则已设置
- [ ] GitHub Actions 已启用
- [ ] Dependabot 已配置
- [ ] README 链接已更新（替换 yourusername）
- [ ] 创建测试 PR 验证 CI
- [ ] 所有 CI 检查通过
- [ ] Security 扫描无问题

---

**配置完成后**，你的 AiDb 项目将拥有：
- ✅ 自动化测试和质量检查
- ✅ 安全漏洞扫描
- ✅ 自动化发布流程
- ✅ 依赖自动更新
- ✅ 专业的协作工作流

如有问题，请查阅 [CI/CD 文档](../docs/CICD.md) 或提 Issue。
