# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.3] - 2026-04-29

### Added

- **RocksDB 风格合并迭代器**：`DBIterator::load_next_valid` 完全重写，收集所有持有相同 user_key 的层（MemTable + SSTable），统一选取最高序列号版本，跳过删除标记，**推进所有相关层**——与 RocksDB `MergingIterator` 语义对齐，彻底修复重复 key 问题。
- **幽灵 key 消除**：当某一层存在同一 key 的多个条目（如墓碑 + 旧数据），合并迭代器会连续跳过所有同 key 记录，确保已删除的 key 不会因层内残留旧数据而被误返回。
- **SSTable 墓碑检测**：`SSTableIterator::next_entry` 通过空值判断删除标记（`is_delete = value.is_empty()`），使 SSTable 中的墓碑也能被正确识别并过滤。
- **等序列号删除优先**：当来自不同 SSTable 的数据条目与墓碑序列号相同（均为 0）时，墓碑优先——删除代表的写入时间更晚。
- **MetaStateMachine 快照摘要**：为 `MetaStateMachine` 新增 `apply_summary` 支持，用于从快照状态恢复元数据版本，确保重启后集群元数据一致性。

### Changed

- **移除冗余 `get_at_sequence()` 调用**：合并迭代器直接使用各层迭代器返回的 `best_value`，不再为每个 key 执行额外 `DB::get_at_sequence()` 点查询；迭代性能提升约 2x，同时避免 MVCC 快照不一致。
- **移除 `DBIterator::db` 字段**：`db: Arc<DB>` 仅在构造期用于收集各层迭代器，存储期不再需要；移除后减少 Arc 引用计数开销。
- **`scan_groups_streaming` 组排序**：在多组扫描中对 `list_groups()` 返回值排序，确保游标（`group_idx:base64(last_key)`）在不同调用间稳定定位。
- **`next()` 简化为单次 `load_next_valid()` 调用**：此前在 `next()` 中手动推进最小层，与 `load_next_valid` 内部逻辑重复，现统一由 `load_next_valid` 处理推进与校验。
- **Raft 存储层增强**：`OpenRaftStorage` 与 `ShardedRaftStorage` 新增批量写入路由键、状态机值读取、slot 内键扫描等辅助方法，提升集群模式下的存储操作效率。
- **MetaRaft 节点管理**：`MetaRaftNode` 新增节点状态更新、成员地址解析等能力，为上层 AiKv 的 `CLUSTER METARAFT SETSTATUS` 与故障转移提供基础。

### Fixed

- **合并迭代器重复 key**（原行为）：只推进最小层，其他持有相同 key 的层可能残留，导致同一 key 被多次输出。
- **幽灵 key**：旧代码在 SSTable 条目序列号均为 0 时无法分辨数据与墓碑，且 MemTable 中墓碑后的旧数据条目会被误认为新 key。
- **`get_at_sequence()` 快照不一致**：点查询可能返回与迭代器当前序列号状态不一致的快照视图，改为直接使用层迭代器值后此问题消失。
- **Raft 元数据安全性**：`flush()` / `flush_db()` 不再触发 `clear_all_data()`，避免在 FLUSH 操作中意外清除 Raft 日志（`raft:log:N`）、投票状态（`raft:vote`）与成员配置（`raft:membership`），确保集群在 FLUSHDB/FLUSHALL 后仍能正常选主与提交。

## [0.7.2] - 2026-04-14

### Added

- `MetaRaftNode::get_member_address`：从 OpenRaft 成员配置中解析对等节点 gRPC 地址（与 `CLUSTER METARAFT ADDLEARNER` 登记的地址一致）。
- `MetaStateMachine::get_config_version`：轻量读取 `config_version`，供路由缓存与元数据新鲜度比对。
- `OpenRaftStorage::get_state_machine_value`：从状态机 key 空间（`sm:` 前缀）读取值，与 `apply_batch_internal` 写入路径一致。
- **`OpenRaftStorage::scan_state_machine_keys_in_slot`**：在单组 DB 上迭代 `sm:` 前缀键，按 `Router::key_to_slot`（含 hash tag）过滤 slot；与线上 Multi-Raft 每组合约一致，**不依赖**可选的 `MultiRaftNode::init_state_machine` / 顶层 `ShardedStateMachine`。
- **`MultiRaftNode::scan_local_group_slot_keys_sync`**：对本地已加载的数据组同步调用上述扫描，返回某 slot 下的逻辑 key 列表，供上层实现 Redis **`CLUSTER GETKEYSINSLOT`** / **`COUNTKEYSINSLOT`** 等与 `redis-cli --cluster reshard` 兼容的能力。
- `MetaRaftNode::update_node_status` 与 `MetaRequest::UpdateNodeStatus`：由 MetaRaft 提议更新节点 `Online` / `Offline` / `Joining` / `Leaving`，供上层（如 AiKv `CLUSTER METARAFT SETSTATUS`）与故障转移协同。
- **数据组选民对齐**：`MultiRaftNode::raft_voter_goal_for_group`（内部）——以 `GroupMeta.replicas` 为候选，排除 `ClusterMeta.nodes` 中标记为 `Offline` 的节点，得到目标 voter 集合。
- **可观测性（MetaRaft / Multi-Raft）**：新增诊断事件 `metaraft_sm_applied`、`sync_data_groups_from_meta_start`、`sync_data_groups_from_meta_done`、`multi_raft_create_group_from_meta`，用于对齐元数据变更、数据组加载与故障窗口。
- **`OpenRaftStorage::db`**：新增公开方法，暴露底层 `Arc<DB>` 引用，供上层（如 AiKv 备份路径）直接访问单组数据库实例。
- **`ShardedRaftStorage::backup_all_groups`**：新增方法，遍历所有已加载 Raft 分片的底层 DB，对每个调用 `BackupManager::create_backup`，返回 `(GroupId, BackupId)` 列表；用于支持 AiKv `SAVE`/`BGSAVE` 命令在集群模式下的文件级备份。

### Changed

- `RaftNetworkClient` / `RaftNetworkClientFactory`：为每条 Multi-Raft 组维护 `group_id`；新增 `with_group_id` 以共享节点地址表、按组构造网络工厂；所有 Vote / AppendEntries / InstallSnapshot protobuf 均携带正确 `group_id`（`multi_raft_network` 同步调整）。
- `MultiRaftNode::create_raft_group`：Raft `initialize` 仅包含当前节点（单 voter）；副本通过 reconcile 流程逐步加入，避免 `ADDSLOTSRANGE` 早于 `REPLICATE` 时形成「永远无法加入副本」的静态成员集。
- `MultiRaftNode`：`ensure_data_groups_for_current_meta` 与 `ensure_data_groups_async` / `sync_data_groups_from_meta` 等行为调整，避免在异步上下文中嵌套 `block_on`；加载已有组时使用按组的网络工厂。
- `Router::key_to_slot`：按 Redis Cluster 规则解析 **hash tag**（`{...}` 子串参与 CRC16），与 `CLUSTER KEYSLOT` / MOVED 一致。
- `ShardedRaftStorage`（加载持久化 Raft 组失败）与 `DB::drop`（flush / WAL sync 失败）：`eprintln!` 改为 `log::warn!` / `log::error!`，便于与集群与容器化部署下的统一日志采集一致。
- **`MultiRaftNode::reconcile_data_group_membership`（重要）**：在本节点为数据组 Raft Leader 时，先对 `ClusterMeta` 中缺失的副本执行 `add_learner`；当所有**存活**（非 Offline）副本均已出现在 Raft 成员中后，调用 **`change_membership(desired_voters, retain=true)`**，将目标副本集提升为 **voters**。`retain=true` 使旧配置中的节点可保留为 learner，便于平滑 joint 过渡。单次调用超时由环境变量 **`AIKV_DATA_GROUP_MEMBERSHIP_TIMEOUT_MS`** 控制（默认 **5000** ms，由 AiKv 等进程传入同一环境即可）。
- `OpenRaftStorage::apply_to_state_machine`：新增批量应用摘要日志 `raft_storage_apply_entries_summary`，区分 MetaRaft 与数据 Raft（按 op 数/载荷大小判定 heavy），降低常规流量日志噪声并保留异常窗口证据。

### Fixed

- **集群 AiDb 指标聚合来源修复**：新增 `ShardedRaftStorage::aggregate_aidb_storage_stats`，并通过 `MultiRaftNode::aggregate_aidb_storage_stats` 聚合本节点所有已加载数据组的 `DB`（MemTable/WAL/BlockCache）统计；避免误依赖可选的 `ShardedStateMachine` 路径导致 `INFO memory` 中 `aidb_*` 在集群模式长期为 `0`。
- **Multi-Raft gRPC 路由**：此前 RPC 中 `group_id` 恒为 `0`，数据组请求在对端被路由到 MetaRaft，导致数据 Raft 无法正确选主或提交（如 `ForwardToLeader`，`leader_id: None`）。
- **Failover 后读空**：副本侧数据 Raft 若从未被加入成员，则状态机未应用写入；单节点初始化 + Leader 侧 `add_learner` 修复复制与提升后可读性。
- **数据 Raft 组「谁来做 initialize」**：`load_groups_from_meta` 改为经 `create_raft_group_with_leader` 与 `maybe_initialize_raft`，以 `GroupMeta.leader`（元数据指定 Leader）决定唯一执行 `raft.initialize` 的节点；不再仅用 `replicas` 中最小 `node_id`。避免副本的哈希 `node_id` 小于主节点时，副本误创建单 voter 组并自任 Leader，进而与 slot 路由、`ForwardToLeader` / 客户端 `MOVED` 行为矛盾。
- **「Redis 一主多从」但数据 Raft 仍不可写**：仅 `add_learner` 时副本仅为 learner，不参与选主；单主宕机后可能出现无 leader、双 voter 缺一无法多数派、或 TRYAGAIN。通过 **`change_membership` 将各存活副本提升为 voter**，使例如 **三副本分片在挂 1 台时仍满足 2/3 多数派**，与 Redis Cluster 故障转移语义一致。
- **`build_snapshot` 元数据与状态机不一致**：旧实现从 `raft:snapshot_meta` 反序列化快照元数据，当该键不存在或过时时返回 `last_log_id: None` 的空默认值，导致 OpenRaft 无法感知已应用进度、日志无法正确截断。改为从 `raft:last_applied`（状态机最后应用的 LogId）和 `raft:membership`（当前成员配置）直接构造 `SnapshotMeta`，并持久化回 `raft:snapshot_meta`；读取路径使用优雅降级（键缺失时回退 `None` / `Default`），确保快照元数据始终与状态机实际状态一致。

## [0.7.1] - 2026-03-26

### Added

- 添加 `total_key_count` 全局计数器，实现 O(1) 的 DBSIZE 操作
- 添加 `key_exists()` 方法，检查 key 是否存在于 MemTable 或 SSTable
- 添加 `key_exists_in_memtables()` 快速辅助方法，只检查 MemTable 层
- 添加 `dbsize()` 方法返回准确 key 计数
- 添加 `reset_key_count()` 方法用于 FLUSHDB 时重置计数器
- 添加 `clear_all_data()` 方法，用于 FLUSHDB 时清除所有 SSTable 和 WAL 文件
- MemTable: 添加 `key_map` 和 `unique_key_count` 来追踪唯一 key

### Changed

- `put()` 和 `delete()` 操作现在会更新全局 key 计数器
- `key_exists()` 会跳过 `__exp__:` 前缀的内部过期元数据 key
- WAL 轮转现在在 `flush_pending()` 中执行，MemTable flush 到 SSTable 后自动回收磁盘空间
- MemTable 的 `key_map` 现在存储 `(sequence, ValueType)` 元组，正确区分 value 和 tombstone

### Fixed

- 修复 MemTable flush 时重复计数问题：使用 HashMap 追踪每个 user_key 的最新版本
- WAL: 修复后台 flush 后 WAL 未轮转问题，导致 WAL 文件无限增长
- Performance: 新增 `estimated_dbsize()` 和 `estimated_memtable_entries()` 方法，实现 O(1) 复杂度 key 数量估算
- Performance: 移除热路径（put/delete）上的所有 `log::info!()` 调用，避免日志 I/O 拖慢性能
- Bug Fix: 修复 MemTable 的 `put()` 和 `delete()` 在 key_map 计数时的竞态条件，正确处理 put-after-delete 和重复 delete 场景
- 修复 `put()` 中 `total_key_count` 增量逻辑，使用 `fetch_update` + `saturating_add` 替代简单 `fetch_add`，确保原子性并防止溢出
- 修复 `delete()` 中 `total_key_count` 使用 `saturating_sub` 替代 `fetch_sub`，防止计数器变成负数（DBSIZE 显示负值）

## [0.7.0] - 2026-03-01

### Added

- 初始集成 AiKv 存储引擎
- 后台 flush 线程，MemTable 自动刷写到 SSTable
- Block cache 和内存指标导出

### Fixed

- MemTable: 新增 `keys_at_sequence(max_seq)`，修复 `DBIterator` 使用快照感知 key 收集

- MemTable: Added `keys_at_sequence(max_seq)` and fixed `DBIterator` to use snapshot-aware key collection to avoid listing tombstoned keys in iterators and snapshot views.
- Tombstone / Delete semantics: Ensure MemTable and SSTable preserve tombstone markers (empty `Vec`) and `DB::get()` maps tombstones to `None` for the public API; fix search order to check newest SSTables first to avoid returning stale values. Added comprehensive concurrent delete+flush tests.

## [0.6.3] - 2026-01-09

### Fixed

- Corrected tombstone handling so deleted keys cannot be read from SSTables; `MemTable::get()` and `SSTableReader::get()` return `Some(Vec::new())` for tombstones, and `DB::get()` converts that to `None` for callers.
- Fixed a race condition between delete and concurrent flush that could cause old values to be visible after a deletion; added `tombstone_concurrent_tests` and related unit/integration tests to validate behavior.



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

[Unreleased]: https://github.com/Genuineh/aidb/compare/v0.7.3...HEAD
[0.7.3]: https://github.com/Genuineh/aidb/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/Genuineh/aidb/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/Genuineh/aidb/compare/v0.7.0...v0.7.1
[0.6.1]: https://github.com/Genuineh/aidb/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/Genuineh/aidb/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Genuineh/aidb/compare/v0.3.0...v0.5.0
[0.3.0]: https://github.com/Genuineh/aidb/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Genuineh/aidb/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Genuineh/aidb/releases/tag/v0.1.0
