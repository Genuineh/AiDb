# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- MemTable: Added `keys_at_sequence(max_seq)` and fixed `DBIterator` to use snapshot-aware key collection to avoid listing tombstoned keys in iterators and snapshot views.

## [0.6.1] - 2024-12-30

## [0.6.1] - 2024-12-30

### ✨ 新功能

#### MetaRaftNode 网络地址管理
- **add_node_address() 方法**: 在 `MetaRaftNode` 中添加预注册节点地址功能
  - 使 `MetaRaftNode` 与 `OpenRaftNode` 和 `MultiRaftNode` 行为一致
  - 支持 Multi-Raft 集群场景下上层应用（如 AiKv）预填充节点地址
  - 解决 `add_learner` 前 network factory 为空导致无法连接 peer 的问题
- **remove_node_address() 方法**: 移除已注册的节点地址
- **node_addresses() 方法**: 获取所有已知节点地址列表
- **network_factory() 方法**: 提供 network factory 的直接访问，支持高级操作
- **自动地址注册**: `initialize()` 和 `add_learner()` 现在会自动注册节点地址

### 📚 文档更新
- 更新 `MULTI_RAFT_API_REFERENCE.md`: 添加新 API 使用说明
- 更新 `MULTI_RAFT_ARCHITECTURE.md`: 更新 MetaRaftNode API 列表

### 🧪 测试增强
- 添加 `test_add_node_address`: 测试添加节点地址功能
- 添加 `test_remove_node_address`: 测试移除节点地址功能
- 添加 `test_network_factory_access`: 测试 network factory 访问
- 更新 `test_initialize_cluster`: 验证 initialize 自动注册地址

### 🔧 技术改进
- `MetaRaftNode` 现在保存 `Arc<RwLock<RaftNetworkClientFactory>>` 引用
- 网络工厂与 Raft 实例共享底层节点地址 HashMap

## [0.6.0] - 2024-12-30

### 🚀 重大更新

#### Multi-Raft 架构优化与稳定性提升
- **架构精简**: 移除 Primary-Replica、Peer-to-Peer、Coordinator 等过时的集群模式，专注于 Multi-Raft 架构
- **代码清理**: 删除 9 个已废弃的源文件、5 个示例文件、6 个测试文件
- **文档归档**: 将历史文档移至 `docs/archive/`，保持文档结构清晰

### 🐛 Bug修复

#### Raft 集群核心修复
- **LogIndex(0) 错误修复**: 修复节点重启时的 'try to get log at index 0 but got None' 错误
  - 实现了日志清除（log purge）恢复逻辑
  - 在 `load_state()` 中扫描实际存在的日志条目
  - 自动设置虚拟的 `last_purged_log_id` 以告知 OpenRaft 日志起始位置
- **节点地址传播**: 修复 BasicNode 地址在 `initialize()` 和 `add_learner()` 中的传播问题
- **日志条目读取**: 修复 `get_log_entries()` 对 index 0 的正确处理
- **状态机数据查询**: 修复 GET 命令使用 `sm:` 前缀进行状态机数据查找
- **元数据同步**: 修复 Multi-Raft 元数据同步问题

### ✨ 新功能

#### 集群部署与运维
- **Docker 部署**: 添加 OpenRaft 集群的 Docker Compose 配置
  - `docker-compose.cluster.yml`: 标准 3 节点集群
  - `docker-compose.debug.*.yml`: 调试配置
- **运维脚本**: 完整的集群管理脚本套件
  - `init_cluster.sh`: 集群初始化
  - `verify_cluster.sh`: 集群验证
  - `membership_check.sh`: 成员状态检查
  - `replication_check.sh`: 复制验证
  - `admin_check.py`: Python 管理工具

#### 测试增强
- **Chaos 测试**: 添加 19 个混沌测试用例，验证故障恢复能力
  - 随机节点故障和恢复
  - 交错崩溃和恢复
  - 快速重启循环
  - 延迟操作和高延迟场景
  - 内存压力模拟
- **边界测试**: 添加 Raft 边界条件测试
- **多节点测试**: 完整的多节点集成测试套件

### 📚 文档更新

#### 架构文档
- 更新 `ARCHITECTURE.md`: 反映 Multi-Raft 唯一架构
- 更新 `MULTI_RAFT_ARCHITECTURE.md`: 完整实现细节
- 更新 `MULTI_RAFT_API_REFERENCE.md`: API 参考文档
- 更新 `MULTI_RAFT_QUICKSTART.md`: 快速入门指南
- 更新 `examples/cluster/README.md`: 集群示例说明

#### 开发者指引
- 更新 `.github/copilot-instructions.md`: AI 辅助开发指引
- 明确项目只保留 Multi-Raft 架构的决策
- 添加 Raft 测试指南 `RAFT_TESTING_GUIDE.md`

### 🔧 技术改进

#### 代码质量
- ✅ **Clippy 通过**: 所有代码通过 `cargo clippy --all-targets --all-features -- -D warnings`
- ✅ **格式化**: 代码自动格式化 (cargo fmt)
- ✅ **测试覆盖**: 574+ 测试全部通过（包括 19 个新的 chaos 测试）

#### 性能与可靠性
- **持久化一致性**: 改进日志条目的持久化逻辑
- **故障恢复**: 增强节点崩溃后的恢复能力
- **状态一致性**: 修复多种状态不一致问题

### 📊 测试统计

- **总测试数**: 574+ 测试用例
- **新增测试**: 19 个 chaos 测试 + 边界测试 + 多节点测试
- **测试通过率**: 100%
- **覆盖场景**: 
  - 节点故障和恢复
  - 日志压缩和清除
  - 网络延迟和超时
  - 内存压力
  - 并发操作

### 🎯 版本亮点

1. **架构清晰**: 专注 Multi-Raft，移除所有过时代码
2. **稳定性提升**: 修复关键的日志恢复 bug
3. **运维完善**: 完整的 Docker 部署和管理脚本
4. **测试增强**: 大幅提升混沌测试覆盖
5. **文档完善**: 更新所有文档以反映当前架构

### ⚠️ Breaking Changes

- 移除了 `PrimaryNode`、`ReplicaNode`、`CoordinatorNode` 等旧 API
- 移除了 `PeerToPeerCluster` 相关功能
- 移除了 `ShardGroup` 和 `AutoScaler` (将在未来版本基于 Multi-Raft 重新实现)

### 🔄 迁移指南

如果你使用的是 0.5.x 版本的旧集群模式，请：
1. 查看 `examples/cluster/` 中的 Multi-Raft 示例
2. 参考 `docs/MULTI_RAFT_QUICKSTART.md` 进行迁移
3. 使用 `OpenRaftNode` 替代旧的节点类型

## [0.5.0] - 2024-12-10

### 🚀 重大更新

#### 版本升级
- **版本号**: 从 0.3.0 升级到 0.5.0
- **稳定性**: 进一步提升生产环境稳定性
- **文档**: 更新所有相关文档以反映新版本

### 📚 文档更新
- 更新 Cargo.toml 版本号
- 更新 README.md 中的版本引用
- 更新 PROJECT_STATUS.md 项目状态
- 更新 TODO.md 版本信息
- 统一所有文档中的版本标识

### 📊 当前状态
- **版本**: 0.5.0
- **核心功能**: 生产就绪 ✅
- **集群功能**: 完整实现 ✅
- **监控运维**: 完善 ✅
- **代码质量**: 高标准 ✅

## [0.3.0] - 2025-11-20

### 🧹 代码清理和重构

#### OpenRaft 集成完成
- **从 tikv/raft-rs 迁移到 openraft 0.9**
  - ✅ 实现 OpenRaftStorage (RaftStorage trait)
  - ✅ 实现 RaftNetwork 和 RaftNetworkFactory
  - ✅ 实现 OpenRaftNode (替代旧的 RaftNode)
  - ✅ 完整的 protobuf RPC 定义
  - ✅ 使用 Rust native async traits (RPITIT)

#### 移除旧代码
- **删除旧的 Raft 实现文件**
  - 移除 `raft_node_old.rs` (基于 tikv/raft-rs 的旧实现)
  - 移除 `raft_storage_old.rs` (旧存储实现)
  - 移除 `raft_storage_old_backup.rs` (备份副本)
  - 移除 `raft_peer.rs` (旧 peer 实现)
  - 移除 `raft_transport.rs` (旧传输层)

- **删除旧的示例文件**
  - 移除 `raft_cluster_demo_old.rs` (旧演示)
  - 移除 `raft_integration_test.rs` (旧集成测试)
  - 移除 `raft_peer_cluster.rs` (旧 peer 集群)

#### 文档更新
- 更新 `examples/cluster/README.md`
  - 更新 API 示例使用 OpenRaft
  - 移除对旧示例的引用
  - 添加 `openraft_demo.rs` 作为推荐示例
- 更新 `Cargo.toml`
  - 移除关于旧 API 的注释
- 清理代码库，提高可维护性

### 📊 当前状态
- **版本**: 0.3.0
- **OpenRaft 集成**: Phase 2-5 完成 ✅
- **核心功能**: 生产就绪 ✅
- **代码清洁度**: 移除所有旧代码 ✅

## [0.2.0] - 2025-11-18

### 🚀 分布式集群功能全面完成 (Week 21-48)

#### 阶段1: RPC 网络层 (Week 21-24) ✅
- **Primary 节点**: 完整的 gRPC 服务实现，支持所有 DB 操作
- **Replica 节点**: LRU 缓存实现，智能转发机制
- **协议定义**: 8个 RPC 方法，包括流式扫描
- **连接管理**: 连接池和自动重连
- **测试**: 7个集成测试全部通过

#### 阶段2: Coordinator (Week 25-28) ✅
- **一致性哈希**: 虚拟节点实现，负载均衡
- **路由管理**: Shard 注册和键路由
- **健康检查**: 自动故障检测和状态管理
- **请求转发**: GET/PUT/DELETE 操作转发
- **测试**: 37个测试全部通过

#### 阶段3: Shard Group (Week 29-34) ✅
- **ShardGroupManager**: 完整的 Shard Group 生命周期管理
- **多Shard协同**: 数据分片和分布式路由
- **状态管理**: 节点状态跟踪和故障处理
- **集成测试**: 14个基础测试 + 15个集成测试
- **性能优化**: 热点代码优化，减少锁竞争

#### 阶段4: 备份恢复 (Week 35-40) ✅
- **BackupManager**: 快照创建、WAL归档、保留策略
- **RecoveryManager**: 快照恢复、WAL Replay
- **存储适配**: 本地文件存储（S3/OSS 预留接口）
- **测试**: 22个单元测试 + 11个集成测试
- **文档**: [BACKUP_RECOVERY.md](docs/BACKUP_RECOVERY.md) 用户指南

#### 阶段5: 弹性伸缩 (Week 41-44) ✅
- **ScalingManager**: 手动添加/移除节点
- **AutoScaler**: 自动伸缩策略和触发机制
- **指标收集**: CPU、内存、QPS、存储使用监控
- **测试**: 29个单元测试 + 31个集成测试
- **安全性**: 节点健康检查、数据完整性验证

#### 阶段6: 监控运维 (Week 45-48) ✅
- **Prometheus监控**: 14种指标类型，完整的监控体系
- **HTTP Metrics服务**: `/metrics` 端点，Prometheus格式
- **Grafana仪表盘**: 10个面板，系统全方位监控
- **告警规则**: 15条规则（critical/warning/info）
- **aidb-admin CLI**: 集群管理、备份恢复、健康检查工具
- **测试**: 12个监控测试
- **文档**: 25KB+ 完整文档

### 📊 里程碑达成
- ✅ M4: RPC通信完成 (Week 24)
- ✅ M5: 集群路由完成 (Week 28)
- ✅ M6: 多Shard运行 (Week 34)
- ✅ M7: 备份恢复完成 (Week 40)
- ✅ M8: 弹性伸缩完成 (Week 44)
- ✅ M9: 生产就绪 (Week 48)

### 📈 测试覆盖
- **总测试数**: 522+ 测试用例
- **单机版测试**: 216个单元测试
- **集群功能测试**: 306个测试
  - RPC集成测试: 7个
  - Coordinator测试: 37个
  - ShardGroup测试: 43个
  - 备份恢复测试: 33个
  - 弹性伸缩测试: 60个
  - 监控测试: 12个
  - 其他集成测试: 114个
- **测试通过率**: 100%

### 🎯 性能指标
- **单机性能**: 达到设计目标的70%（相对RocksDB）
- **集群扩展**: 支持多Shard线性扩展
- **监控延迟**: < 100ms 指标收集延迟

### 📚 文档完善
- [用户指南](docs/USER_GUIDE.md)
- [最佳实践](docs/BEST_PRACTICES.md)
- [性能调优指南](docs/PERFORMANCE_TUNING.md)
- [备份恢复指南](docs/BACKUP_RECOVERY.md)
- [监控配置指南](docs/monitoring/)
- [完成总结文档](docs/completions/) - 所有阶段完成总结

### 🔧 运维工具
- **aidb-admin**: 命令行运维工具
  - 集群状态查询
  - 节点管理（添加/删除）
  - 备份和恢复
  - 健康检查
  - 指标查询

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

[Unreleased]: https://github.com/Genuineh/aidb/compare/v0.6.1...HEAD
[0.6.1]: https://github.com/Genuineh/aidb/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/Genuineh/aidb/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Genuineh/aidb/compare/v0.3.0...v0.5.0
[0.3.0]: https://github.com/Genuineh/aidb/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Genuineh/aidb/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Genuineh/aidb/releases/tag/v0.1.0
