# GitHub Actions 配置检查清单

使用此清单确保 CI/CD 流水线配置完整。

## ✅ 文件创建检查

- [x] `.github/workflows/ci.yml` - CI Pipeline
- [x] `.github/workflows/security.yml` - Security Audit
- [x] `.github/workflows/release.yml` - Auto Release
- [x] `.github/dependabot.yml` - Dependency Updates
- [x] `.github/pull_request_template.md` - PR Template
- [x] `.github/ISSUE_TEMPLATE/bug_report.md` - Bug Report
- [x] `.github/ISSUE_TEMPLATE/feature_request.md` - Feature Request
- [x] `.github/ISSUE_TEMPLATE/question.md` - Question
- [x] `.github/workflows/README.md` - Workflows Documentation
- [x] `.github/SETUP.md` - Setup Guide
- [x] `.github/QUICKSTART.md` - Quick Start
- [x] `docs/CICD.md` - Complete CI/CD Documentation
- [x] `CHANGELOG.md` - Changelog
- [x] `deny.toml` - Cargo Deny Configuration
- [x] `CICD_SETUP_SUMMARY.md` - Setup Summary
- [x] 更新 `README.md` - CI Badges
- [x] 更新 `CONTRIBUTING.md` - CI Instructions

## 🔧 GitHub 配置检查

### Secrets (在 Settings → Secrets and variables → Actions)

- [ ] `CARGO_TOKEN` - crates.io 发布 (必需)
- [ ] `CODECOV_TOKEN` - 代码覆盖率 (推荐)

### 分支保护 (Settings → Branches → main)

- [ ] Require a pull request before merging
- [ ] Require status checks to pass:
  - [ ] test (ubuntu-latest, stable)
  - [ ] clippy
  - [ ] fmt
- [ ] Require branches to be up to date

### 安全设置 (Settings → Security & analysis)

- [ ] Dependency graph (启用)
- [ ] Dependabot alerts (启用)
- [ ] Dependabot security updates (启用)

## 📝 代码更新检查

- [ ] 替换所有 "yourusername" 为实际用户名
  - [ ] README.md
  - [ ] .github/workflows/*.yml
  - [ ] docs/CICD.md
  - [ ] 其他引用位置

## 🧪 测试验证

- [ ] 本地测试通过: `cargo test`
- [ ] 代码检查通过: `cargo clippy`
- [ ] 格式检查通过: `cargo fmt --check`
- [ ] 创建测试 PR 验证 CI
- [ ] 所有 CI 检查通过

## 📦 发布准备

- [ ] Cargo.toml 版本号正确
- [ ] CHANGELOG.md 已更新
- [ ] crates.io 账号已验证邮箱
- [ ] 包名可用 (未被占用)

## 📚 文档检查

- [ ] 所有文档链接正确
- [ ] 徽章 URL 正确
- [ ] 示例代码可运行
- [ ] 截图/图表清晰

## ✅ 最终验证

完成以上所有检查后:

1. [ ] 推送所有更改到 GitHub
2. [ ] 创建测试 PR 并验证
3. [ ] 合并 PR
4. [ ] 观察 main 分支的 CI 运行
5. [ ] 检查 Actions 页面无错误
6. [ ] (可选) 创建测试标签验证 Release 流程

## 🎉 完成

当所有项目都打勾后，你的 CI/CD 流水线就完全配置好了！

---

**下一步**: 查看 [快速开始指南](.github/QUICKSTART.md) 开始使用
