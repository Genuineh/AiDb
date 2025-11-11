# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-11-11

AiDb 的首个功能完整版本！这个版本包含了一个完整的、生产就绪的单机 LSM-Tree 存储引擎。

### 🎉 核心功能

#### 基础组件
- **WAL (Write-Ahead Log)**: 完整的预写日志实现，确保数据持久化
- **MemTable**: 基于 SkipList 的内存索引
- **SSTable**: 分层持久化存储

#### DB 引擎
- **完整的 CRUD 操作**: Put, Get, Delete
- **Flush 机制**: 自动和手动 MemTable 刷新
- **崩溃恢复**: 基于 WAL 的可靠恢复
- **线程安全**: Arc + RwLock 实现并发访问

### 🚀 性能优化

- **Compaction**: Leveled Compaction 策略
- **Bloom Filter**: 减少 90%+ 的无效磁盘读取
- **Block Cache**: LRU Cache 缓存管理
- **压缩支持**: Snappy 和 LZ4 压缩算法

### ✨ 高级功能

- **Snapshot**: 点时间一致性读取
- **Iterator**: 完整遍历和范围查询
- **WriteBatch**: 原子批量写入

### 📊 测试覆盖

- **315+ 测试用例**: 全面的测试覆盖
- **代码覆盖率**: > 80%
- **CI/CD**: 自动化测试和检查

### 📚 文档完善

#### 用户文档
- **[用户指南](docs/USER_GUIDE.md)**: 完整的使用说明
- **[最佳实践](docs/BEST_PRACTICES.md)**: 生产环境指南
- **[性能调优指南](docs/PERFORMANCE_TUNING.md)**: 深度性能优化

#### 技术文档
- **[架构设计](docs/ARCHITECTURE.md)**: 系统架构说明
- **[实施计划](docs/IMPLEMENTATION.md)**: 开发路线图
- **[设计决策](docs/DESIGN_DECISIONS.md)**: 技术选型说明

#### 示例代码
- **[examples/README.md](examples/README.md)**: 9 个完整示例

### 🎯 性能指标

单机性能（NVMe SSD）：
- 顺序写入: ~140K ops/s
- 随机写入: ~70K ops/s  
- 随机读取: ~140K ops/s

### 🏗️ 项目组织

- 文档整理至 `docs/completions/`
- 清晰的目录结构
- 完整的索引文档

### 🐛 Bug 修复

- 修复 WAL 恢复逻辑
- 修复空 SSTable 处理
- 修复 SSTable 管理
- 修复数据恢复问题

### 🔒 安全性

- CRC32 校验
- 线程安全
- 崩溃恢复
- 安全扫描

---

[Unreleased]: https://github.com/Genuineh/aidb/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Genuineh/aidb/releases/tag/v0.1.0
