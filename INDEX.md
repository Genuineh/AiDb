# AiDb 文档索引

欢迎！这是AiDb项目的完整文档导航。

## 🚀 快速开始

| 文档 | 内容 | 适合人群 |
|------|------|---------|
| [README.md](README.md) | 项目介绍、快速开始 | 所有人 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构设计 | 开发者、架构师 |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | 开发指南 | 贡献者 |
| [TODO.md](TODO.md) | 当前任务 | 贡献者 |

## 📚 核心文档

### 架构和设计

- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** 
  - 单机版架构
  - 集群版架构
  - 数据模型
  - 关键设计决策

- **[docs/DESIGN_DECISIONS.md](docs/DESIGN_DECISIONS.md)** 
  - 从RocksDB借鉴什么
  - 避免RocksDB什么问题
  - 详细的技术对比

### 实现文档

- **[docs/WAL_IMPLEMENTATION.md](docs/WAL_IMPLEMENTATION.md)** ✅
  - WAL架构设计
  - Record格式详解
  - Writer和Reader实现
  - 使用示例和最佳实践

### 实施和开发

- **[docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md)** 
  - 48周完整实施计划
  - 每个阶段的详细任务
  - 里程碑和交付物

- **[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)** 
  - 环境准备
  - 代码结构
  - 开发流程
  - 编码规范
  - 测试指南

- **[TODO.md](TODO.md)** 
  - 当前Sprint任务
  - 待办事项
  - 进度跟踪

### 贡献

- **[CONTRIBUTING.md](CONTRIBUTING.md)** 
  - 如何贡献
  - PR流程
  - 代码规范

## 📂 项目结构

```
aidb/
├── README.md              # 项目介绍 ⭐ 从这里开始
├── INDEX.md               # 本文档
├── TODO.md                # 任务清单
├── CONTRIBUTING.md        # 贡献指南
├── LICENSE                # 许可证
├── Cargo.toml             # Rust项目配置
│
├── src/                   # 源代码
│   ├── lib.rs            # 库入口
│   ├── error.rs          # 错误定义
│   ├── config.rs         # 配置
│   ├── wal/              # WAL模块 ✅
│   ├── memtable/         # MemTable模块（待实现）
│   └── sstable/          # SSTable模块（待实现）
│
├── tests/                 # 集成测试
├── benches/               # 性能测试
├── examples/              # 示例代码
│
└── docs/                  # 文档
    ├── ARCHITECTURE.md    # 架构设计 ⭐ 理解设计
    ├── IMPLEMENTATION.md  # 实施计划 ⭐ 开发路线
    ├── DESIGN_DECISIONS.md # 设计决策
    ├── DEVELOPMENT.md     # 开发指南 ⭐ 参与开发
    ├── WAL_IMPLEMENTATION.md # WAL实现 ✅
    │
    └── archive/           # 历史文档
        ├── README.md
        └── ... (演进过程中的文档)
```

## 🎯 按角色导航

### 我是项目使用者

1. 阅读 [README.md](README.md) 了解项目
2. 查看示例代码（待补充）
3. 参考API文档（待生成）

### 我想了解技术架构

1. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - 完整架构设计
2. [docs/DESIGN_DECISIONS.md](docs/DESIGN_DECISIONS.md) - 设计理念
3. [docs/archive/](docs/archive/) - 演进过程

### 我想参与开发

1. [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) - 开发环境和流程
2. [TODO.md](TODO.md) - 认领任务
3. [CONTRIBUTING.md](CONTRIBUTING.md) - 贡献指南
4. [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) - 了解长期规划

### 我是项目管理者

1. [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) - 整体计划
2. [TODO.md](TODO.md) - 当前进度
3. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - 技术选型

## 📖 学习路径

### 第1天：快速了解

- [ ] 阅读 README.md（10分钟）
- [ ] 浏览 docs/ARCHITECTURE.md 的"架构概览"（15分钟）
- [ ] 查看项目结构（5分钟）

### 第2-3天：深入理解

- [ ] 完整阅读 docs/ARCHITECTURE.md（1小时）
- [ ] 阅读 docs/DESIGN_DECISIONS.md（1小时）
- [ ] 理解单机版和集群版的差异（30分钟）

### 第4-5天：准备开发

- [ ] 搭建开发环境 docs/DEVELOPMENT.md（30分钟）
- [ ] 阅读开发流程（30分钟）
- [ ] 浏览现有代码（1小时）
- [ ] 运行测试（15分钟）

### 第6天+：开始贡献

- [ ] 从 TODO.md 认领简单任务
- [ ] 提交第一个PR
- [ ] 参与代码审查

## 🔍 常见问题

### Q: AiDb和RocksDB有什么区别？

查看 [docs/DESIGN_DECISIONS.md](docs/DESIGN_DECISIONS.md)

### Q: 项目的开发计划是什么？

查看 [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md)

### Q: 如何开始开发？

查看 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)

### Q: 当前的开发进度？

查看 [TODO.md](TODO.md)

### Q: 如何贡献代码？

查看 [CONTRIBUTING.md](CONTRIBUTING.md)

### Q: 历史文档在哪里？

查看 [docs/archive/](docs/archive/)

## 📞 获取帮助

- 📖 查看文档（优先）
- 💬 [GitHub Discussions](https://github.com/yourusername/aidb/discussions)
- 🐛 [GitHub Issues](https://github.com/yourusername/aidb/issues)

## 🔄 文档更新

文档持续更新中，如有问题或建议，欢迎提Issue。

| 文档 | 最后更新 |
|------|---------|
| README.md | 2025-11-05 |
| docs/ARCHITECTURE.md | 2025-11-04 |
| docs/IMPLEMENTATION.md | 2025-11-05 |
| docs/DESIGN_DECISIONS.md | 2025-11-04 |
| docs/DEVELOPMENT.md | 2025-11-04 |
| docs/WAL_IMPLEMENTATION.md | 2025-11-05 ✅ |
| TODO.md | 2025-11-05 |

---

**开始探索AiDb吧！** 🚀
