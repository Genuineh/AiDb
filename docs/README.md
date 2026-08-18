# AiDb 开发文档

`docs/` 域的开发文档导航. 项目介绍与快速开始见 [README.md](../README.md).

## 阅读路径

- **首次了解** — [README.md](../README.md) → [ARCHITECTURE.md](../ARCHITECTURE.md) → 按需打开下方 modules
- **改某域代码** — 查 [按域阅读](#按域阅读-modules) WHEN → 对应 module; 跨域边界见 module 内「不覆盖」
- **构建 / 测试 / PR** — [deployment.md](deployment.md) + [CONTRIBUTING.md](../CONTRIBUTING.md); AI 助手速览见 [AGENTS.md](../AGENTS.md)

## 汇总文档

| 文档                                    | 内容                     |
| ------------------------------------- | ---------------------- |
| [ARCHITECTURE.md](../ARCHITECTURE.md) | 分层、数据流、与 AiKv 边界       |
| [design.md](design.md)                | 跨模块设计决策与权衡 (why)       |
| [deployment.md](deployment.md)        | 依赖嵌入、数据目录、配置调优与运维      |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | hooks、CI、测试矩阵、提交/PR 规范 |
| [CHANGELOG.md](../CHANGELOG.md)       | 版本变更记录                 |
| [AGENTS.md](../AGENTS.md)             | AI 助手与 CI 入口           |

## 按域阅读 (modules)

| Module                                            | 何时读                                                                     |
| ------------------------------------------------- | ----------------------------------------------------------------------- |
| [01-engine.md](modules/01-engine.md)                 | 改 `engine/{wal,memtable,db}`; 写路径、WAL 恢复、MemTable flush、Snapshot        |
| [02-engine-storage.md](modules/02-engine-storage.md) | 改 SSTable / compaction / Bloom / cache / checkpoint; flush 或读放大         |
| [03-cluster.md](modules/03-cluster.md)               | 改 `cluster/*`; MetaRaft / MultiRaft / slot 路由 / 迁移 (`cluster` feature)  |
| [04-backup.md](modules/04-backup.md)                 | 改 `backup/*`; BackupManager、恢复、保留策略                                     |
| [05-observability.md](modules/05-observability.md)   | 改 `metrics.rs` / cluster metrics; 嵌入方注册 `aidb_*` (`monitoring` feature) |

依赖顺序: engine → engine-storage; cluster / backup / observability 相对独立.

## 构建与测试

构建、Cargo feature 与完整测试矩阵见 [deployment.md](deployment.md) 与 [CONTRIBUTING.md](../CONTRIBUTING.md).
