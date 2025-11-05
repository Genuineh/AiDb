# 贡献指南

感谢你对AiDb项目感兴趣！

## 如何贡献

### 报告Bug

如果发现bug，请创建Issue并包含：
- 清晰的问题描述
- 复现步骤
- 预期行为 vs 实际行为
- 环境信息（OS、Rust版本等）
- 相关日志和错误信息

### 提出新功能

创建Issue说明：
- 功能的用途和价值
- 预期的API设计
- 可能的实现方案

### 提交代码

1. **Fork项目**
   ```bash
   # 在GitHub上Fork
   git clone https://github.com/your-username/aidb.git
   cd aidb
   ```

2. **创建分支**
   ```bash
   git checkout -b feature/your-feature-name
   ```

3. **开发**
   - 遵循[开发指南](docs/DEVELOPMENT.md)
   - 编写测试
   - 更新文档

4. **测试**
   ```bash
   # 运行所有测试
   cargo test
   
   # 代码检查
   cargo clippy --all-targets --all-features -- -D warnings
   
   # 格式化
   cargo fmt
   
   # 确保能编译
   cargo build --all-features
   ```

5. **提交**
   ```bash
   git add .
   git commit -m "feat: add your feature"
   ```

6. **推送并创建PR**
   ```bash
   git push origin feature/your-feature-name
   # 在GitHub上创建Pull Request
   ```

7. **CI/CD 检查**
   - PR创建后，GitHub Actions会自动运行CI测试
   - 确保所有检查通过（测试、lint、格式化）
   - 查看[CI/CD文档](docs/CICD.md)了解详情

## 代码规范

### Commit Message

遵循[Conventional Commits](https://www.conventionalcommits.org/)：

```
<type>(<scope>): <subject>

<body>

<footer>
```

类型：
- `feat`: 新功能
- `fix`: Bug修复
- `docs`: 文档更新
- `style`: 代码格式
- `refactor`: 重构
- `test`: 测试
- `chore`: 构建/工具

示例：
```
feat(wal): implement write-ahead log

- Add WAL writer
- Add WAL reader  
- Add recovery mechanism

Closes #123
```

### 代码风格

- 运行 `cargo fmt` 格式化
- 运行 `cargo clippy` 检查
- 为公共API添加文档注释
- 添加测试

### PR要求

- [ ] 所有测试通过 (`cargo test`)
- [ ] Clippy无警告 (`cargo clippy -- -D warnings`)
- [ ] 代码已格式化 (`cargo fmt`)
- [ ] 文档已更新
- [ ] 添加了测试
- [ ] CI检查全部通过
- [ ] 更新了 CHANGELOG.md (如适用)

## 开发流程

1. 查看[TODO.md](TODO.md)选择任务
2. 阅读[DEVELOPMENT.md](docs/DEVELOPMENT.md)
3. 实现功能并测试
4. 提交PR
5. 代码审查
6. 合并

## 获取帮助

- 查看[文档](docs/)
- 创建[Discussion](https://github.com/yourusername/aidb/discussions)
- 提[Issue](https://github.com/yourusername/aidb/issues)

## 行为准则

- 尊重他人
- 建设性反馈
- 专注技术讨论

---

再次感谢你的贡献！
