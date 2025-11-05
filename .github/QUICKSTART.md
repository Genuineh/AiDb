# GitHub Actions 快速开始指南

> 5分钟快速配置 AiDb CI/CD 流水线

## 🚀 立即开始

### 第一步: 推送代码

```bash
# 添加所有新文件
git add .github/ docs/CICD.md CHANGELOG.md deny.toml CICD_SETUP_SUMMARY.md

# 提交
git commit -m "ci: add GitHub Actions CI/CD pipeline

- Add CI workflow (test, clippy, fmt, coverage)
- Add security scanning (audit, deny, CodeQL)
- Add automated release workflow
- Add Dependabot configuration
- Add PR and Issue templates
- Add comprehensive CI/CD documentation"

# 推送
git push
```

### 第二步: 配置 Secrets (必需)

#### CARGO_TOKEN (必需 - 用于发布)

1. 访问: https://crates.io/settings/tokens
2. 创建 Token (name: `github-actions`, scope: `publish-update`)
3. 复制 token
4. 在 GitHub 仓库:
   - `Settings` → `Secrets and variables` → `Actions`
   - `New repository secret`
   - Name: `CARGO_TOKEN`
   - Value: 粘贴 token

#### CODECOV_TOKEN (可选 - 用于覆盖率)

1. 访问: https://codecov.io (用 GitHub 登录)
2. 添加仓库
3. 复制 Upload Token
4. 在 GitHub 仓库添加 Secret: `CODECOV_TOKEN`

### 第三步: 更新用户名

```bash
# 替换 README 中的用户名
sed -i 's/yourusername/YOUR_GITHUB_USERNAME/g' README.md

# 或手动编辑 README.md，将所有 "yourusername" 改为你的用户名
```

### 第四步: 测试 CI

```bash
# 创建测试分支
git checkout -b test/ci-pipeline

# 做一个小改动
echo "# CI Test" >> README.md

# 提交并推送
git add README.md
git commit -m "test: verify CI pipeline"
git push origin test/ci-pipeline

# 在 GitHub 创建 PR，观察 Actions 运行
```

## ✅ 验证清单

完成配置后，检查以下项目:

- [ ] ✅ 推送代码到 GitHub
- [ ] ✅ 配置 CARGO_TOKEN
- [ ] ✅ 配置 CODECOV_TOKEN (可选)
- [ ] ✅ 更新用户名引用
- [ ] ✅ 创建测试 PR
- [ ] ✅ CI 检查全部通过 ✓
- [ ] ✅ 合并 PR

## 📦 发布第一个版本

```bash
# 1. 更新 Cargo.toml
# version = "0.1.0"

# 2. 更新 CHANGELOG.md
# ## [0.1.0] - 2024-XX-XX
# ### Added
# - Initial release

# 3. 提交
git add Cargo.toml CHANGELOG.md
git commit -m "chore: prepare v0.1.0 release"
git push

# 4. 创建并推送标签
git tag v0.1.0
git push origin v0.1.0

# 5. 等待自动构建和发布！
```

## 📊 查看结果

- **Actions**: https://github.com/YOUR_USERNAME/aidb/actions
- **Releases**: https://github.com/YOUR_USERNAME/aidb/releases
- **Coverage**: https://codecov.io/gh/YOUR_USERNAME/aidb
- **crates.io**: https://crates.io/crates/aidb

## 🔧 常用命令

### 本地验证

```bash
# 运行测试
cargo test

# 代码检查
cargo clippy --all-targets --all-features -- -D warnings

# 格式化
cargo fmt

# 构建
cargo build --release
```

### 安全检查

```bash
# 安装工具
cargo install cargo-audit cargo-deny

# 运行检查
cargo audit
cargo deny check
```

## 📚 更多信息

- 📖 [完整 CI/CD 文档](../docs/CICD.md) - 详细配置说明
- 🔧 [设置指南](SETUP.md) - 分步配置
- 📊 [总结报告](../CICD_SETUP_SUMMARY.md) - 功能概览
- 🚀 [工作流说明](workflows/README.md) - 快速参考

## ❓ 遇到问题？

### CI 失败？

1. 检查错误日志
2. 本地运行相同命令
3. 查看 [故障排查](../docs/CICD.md#常见问题)

### 无法发布？

1. 确认 CARGO_TOKEN 已配置
2. 确认 crates.io 邮箱已验证
3. 确认包名未被占用

### 需要帮助？

- 查看 [CI/CD 文档](../docs/CICD.md)
- 提交 [Issue](https://github.com/YOUR_USERNAME/aidb/issues)
- 查看 [Discussions](https://github.com/YOUR_USERNAME/aidb/discussions)

---

**配置只需 5 分钟，收益整个项目生命周期！** 🎉
