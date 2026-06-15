# Changelog

本项目的所有重要变更都会记录在此文件中.

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/).

## [Unreleased]

## [0.14.10] - 2026-06-10

### Added

- **Raft Prometheus 指标**: `aidb_raft_rpc_total{type,direction}` (vote/append_entries/install_snapshot, incoming/outgoing) 与 `aidb_raft_log_entries_total`; 经 `metrics::register_into` 注册到 AiKv `/metrics` 端点.
- **测试**: `tests/modules/cluster/metrics.rs` 验证注册与计数.

### Changed

- **`RaftNetwork` / `RaftServiceImpl`**: RPC 热路径接入 metrics 记录 (仅 `monitoring` feature).

## [0.14.9] - 2026-06-08

### Added

- **`MultiRaftNode::get_local` / `scan_local_pairs` / `local_group_ids`**: 不要求 leader 的本地状态机读取接口, 供数据面读路径 (leader 直读 / 只读副本) 与本地扫描使用.
- **`Router` 本地观测 leader 缓存 (`observed_group_leaders`)**: 记录本地 MultiRaft 观测到的 group leader, `get_group_leader` 优先返回; `refresh_from_data` 在 MetaRaft 追平后自动清除过期观测, 使 primary failover 后路由更快收敛到新 leader.

### Fixed

- **`LeaderChangeWatcher` leader 变更传播**: 检测到 leader 变更时始终更新本地 leader 缓存; 仅由新 leader 节点向 MetaRaft 提交元数据更新, 并在 MetaRaft 滞后时重试, 避免提交失败导致缓存与路由长期陈旧.

## [0.14.8] - 2026-06-04

### Fixed

- **MetaRaft Learner→Voter 复制屏障**: `promote_learner_to_voter` 在 `change_membership` 前后各加一道复制屏障, 消除 leader 在 commit_index 传播前宕机导致 follower 永久保持 Learner 的竞态窗口.
- **Data Group 复制屏障**: 将屏障逻辑下沉到 `OpenRaftNode::change_membership`, 使 MetaRaft 和 Data Group 的成员变更都自动受益.
- **`is_connected` 检查**: `wait_members_catch_up` 屏障新增 learner 连接性检查, 防止 `last_log_index=0` 时 barrier 立即通过.
- **成员变更操作顺序**: `MembershipCoordinator::change_group_membership` 中 MetaRaft 元数据更新先于 `add_learner_to_group`, 并添加 500ms 重试 (最多 10s), 确保 replica 节点的 LifecycleManager 有时间创建 group.

### Added

- **`OpenRaftNode::wait_members_catch_up`**: 等待所有成员追平 leader 日志 (30s 超时, 5 条阈值, 50ms 轮询).
- **`OpenRaftNode::confirm_replication`**: 确认至少一个其他 voter 收到 membership entry + 等待 3 个 heartbeat 传播 commit_index (5s 超时).
- **`OpenRaftNode::is_connected` / `matched_log_index`**: 辅助方法, 从 `RaftMetrics` 提取复制状态.

### Changed

- **`MetaRaftNode::promote_learner_to_voter`**: 去除内置屏障调用, 委托给 `OpenRaftNode::change_membership` (屏障统一在下层实现).
- **`MetaRaftNode`**: 新增 `heartbeat_interval_ms` 字段, 从 `RaftNodeConfig` 提取.
- **`OpenRaftNode`**: 新增 `heartbeat_interval_ms` 字段.

## [0.14.7] - 2026-06-03

### Fixed

- **`change_group_membership` 非本地 group 支持**: 当 group 不在本地 MultiRaft 时, 跳过 `add_learner_to_group` 和 Raft 成员变更, 仅更新 MetaRaft 元数据. 后续 LifecycleManager 在 group leader 节点上检测 drift 并通过 `apply_membership_change` 自动对账.
- **`MetaStateMachine::set_slot_table()`**: 新增测试辅助方法, 允许直接设置 slot table (跳过 Raft 共识), 用于集成测试中同步 Router 与状态机状态.

## [0.14.6] - 2026-06-02

### Added

- **集群副本自动对账**: `LifecycleManager::tick()` 从 MetaRaft 提取期望 Group 成员集合, `start_lifecycle_impl()` 对比期望 vs 实际 Raft membership, Group Leader 节点自动通过 `add_learner` → `change_membership` (joint consensus) 修复 drift. 每次 tick 最多处理 1 个 drift, 避免批量 joint-consensus 操作引发集群不稳定.
- **OpenRaftNode::get_members()**: 从 Raft metrics 获取当前 group 成员集合 (`BTreeSet<NodeId>`), 用于对账.
- **TickResult / MembershipDrift**: 新返回类型替代 `tick()` 的 `(Vec<u64>, Vec<u64>)` 返回值, 携带期望成员配置.

## [0.14.5] - 2026-06-01

### Fixed

- **DB::write 死锁**: `memtable` 读锁在 `maybe_freeze()` 之前释放, 防止 freeze 尝试获取 `memtable.write()` 时死锁. 根因: CreateGroup 后 ClusterMeta 序列化体积大, 触发 `maybe_freeze` → `memtable.write()` 被同一线程持有的读锁阻塞.
- **add_learner 非阻塞**: 新增 `add_learner_nonblocking()`, CLUSTER MEET 不再等待 Learner 同步日志, 防止不可达节点时永久挂起.

### Added

- **promote_learner_to_voter**: 将 Learner 提升为 Voter 的方法, 支持启动恢复和手动操作. `initialize` 时 bootstrap 节点也正确标记为 Voter.

## [0.14.4] - 2026-06-01

### Added (Iteration 1 — Auto Failover + Slot Migration CLI)

- **LeaderChangeWatcher**: `src/cluster/leader_watcher.rs` — 后台轮询本地 Raft Group 的 leader 变更, 检测到切换后通过 MetaRaft `ChangeGroupMembership` 更新 `ReplicaInfo.is_leader`; `tick()` 单次检测, `run()` 异步循环 (`watch::Receiver` 关闭信号); `detect_leader_transition()` 辅助方法 (cache 对比 + MetaRaft propose); `#[instrument]` tracing 标注.
- **可配置 Raft election timeout**: `RaftNodeConfig.election_timeout_min/max` 通过 AiKv CLI `--raft-election-timeout-min/max` 暴露.
- **Snapshot temp-file 暂存**: `begin_receiving_snapshot` 创建 `<db_path>/.snapshot_temp_<gid>` 临时文件; `install_snapshot` 先写 temp + fsync 再原子应用; `load_state()` 启动时清理残留 temp 文件, 崩溃恢复安全.

### Added (Iteration 2 — Rebalance + BUMPEPOCH)

- **MetaRequest::BumpEpoch**: `meta_types.rs` 新增变体, 用于集群共识 epoch 递增; `validate_with_state` 无前置校验; `apply_mutate` 额外 `+1` (净 `+2`).
- **CLUSTER REBALANCE 支持**: AiKv `cluster_rebalance()` 调用 `ReplicaAllocator::suggest_slot_allocation()` 计算理想分布, 比较当前槽表生成迁移计划, 依次执行 `SlotMigrationManager` 完整迁移流水线.

### Changed

- **CLAUDE.md 清理**: Known Limitations 移除已实现功能 (压缩/write stall/snapshot/compaction/block cache); 功能 (不做) 移除 BLMOVE/BZPOP* (已在 AiKv 0.9.3 实现); 新增可观测性文档引用.
- **Bloom 长期统计回归测试**: `tests/regression/bloom.rs`
- **可观测性文档**: `docs/observability.md`

## [0.14.3] - 2026-05-30

### Fixed

- **MetaRaftNode::initialize 未注册引导节点**: 初始化 Raft 集群后未在 MetaStateMachine 中 `RegisterNode`, 导致 `CLUSTER NODES` 地址错乱, `get_cluster_meta().nodes` 为空. 现 bootstrap 后自动为每个初始节点 propose `RegisterNode` (含 `client_addr` 推导).
- **MembershipCoordinator::add_node 缺少 network factory 注册**: `RegisterNode` 仅更新状态机不注册 gRPC 地址; 新增 `add_node_address()` 调用将新节点地址写入 `RaftNetworkClientFactory`.
- **MembershipCoordinator::add_node 未添加 Raft Learner**: 新节点加入集群后未加入 MetaRaft learner 集合, 导致心跳无法送达、`client_write` 转发失败. 现 `RegisterNode` 后调用 `add_learner()`.
- **LifecycleManager 地址选择**: Router `node_addrs` 现优先使用 `node.client_addr` (客户端端口), 回退到 `node.rpc_addr`. 确保 MOVED 重定向返回正确端口.
- **MetaRaftNode::propose 本地验证降级**: 非 leader 节点本地状态机可能滞后, 预验证从 `error` 降级为 `warn`, 权威验证由 leader 在 apply 时完成.

### Added

- **OpenRaftNode::add_node_address**: 暴露 `network_factory.add_node()` 供 MembershipCoordinator 使用.
- **MetaRaftNode::add_node_address**: 委托方法, 供外部调用方注册节点 gRPC 地址.

## [0.14.2] - 2026-05-29

### Changed

- **MultiRaftNode 方法签名 `&mut self` → `&self`**: `shutdown_tx`/`server_handle` 字段改为 `parking_lot::Mutex<Option<T>>` (interior mutability), `start()`/`start_lifecycle()`/`start_lifecycle_with_data()`/`shutdown()` 5 个方法改为 `&self`, 调用方可在 `Arc<MultiRaftNode>` 上直接调用, 无需 `&mut self` 借用

## [0.14.1] - 2026-05-29

### Added

- **Trivial Move**: compaction 时非重叠 SST 直接提升到下一级, 跳过 merge-dedup-rewrite, 节省 CPU 和 I/O
- **Sharded LRU Block Cache**: Block Cache 从单分片拆分为 16 个独立 shard, `hash(key) & 0xF` 路由, 容量均分, 并发读竞争降低至 1/16
- **compaction_threads > 1**: `Options.compaction_threads` 现可配置 (1-4), N 个独立后台线程并行执行非重叠 compaction
- **Subcompaction**: 大 compaction job 按 key range 分裂为并行子任务 (`std::thread::scope`), 通过 `subcompaction_min_size` 控制分裂阈值 (默认 64MB)
- **Write stall (P2)**: `level0_slowdown_writes_trigger` / `level0_stop_writes_trigger` 配置项, L0 文件堆积时渐进式 sleep / 轮询等待 compaction
- **Snapshot 注册 + compaction 保护 (P2)**: `SnapshotList` 全局注册表, compaction 查询 `min_snapshot_sequence` 保留快照可见版本

## [0.14.0] - 2026-05-29

### Added

- **Snap/LZ4 压缩实现 (D.2)**: `block_io.rs` 新增 `compress_block`/`decompress_block` 函数; `compression` feature 门控 (默认不启用); `write_block` 压缩后写入, `parse_block_bytes` 先解压再 CRC 校验; `compression_to_byte`/`byte_to_compression` 双向转换
- **backup 基准测试 (B1)**: `benches/backup_bench.rs` — 4 组 criterion 测试 (create_empty/1k/list_10/create_10k); CI bench job (test-default 通过后执行)
- **`retention_policy.max_age` 支持 (B2)**: `select_for_deletion()` 新增 max_age 硬过期规则, 超过时限的备份无条件删除
- **`BackupStorage` 公开导出**: `backup::storage::LocalFileStorage` 可被外部 bench 使用

### Changed

- **AiDb 引擎指标注册到 `/metrics` (A)**: `metrics.rs` 新增 `register_into(registry)` 函数; AiKv 的 `Metrics::new()` 启动时调用, 13 个 `aidb_*` 指标现在可通过 AiKv 的 HTTP `/metrics` 端点抓取
- **`#[allow(dead_code)]` → `#[expect(dead_code)]` (D.5)**: 12 处注解替换; `memtable/table.rs:inner()` 实际已被使用, 删除注解
- **CLAUDE.md known_limitation**: Snap/LZ4 压缩条目更新为已实现

## [0.13.0] - 2026-05-28

### Added

- **Phase 18 备份/恢复模块**: `src/backup/` 新模块 — `BackupStorage` trait + `LocalFileStorage` 本地文件存储; `BackupManager` (基于 `Checkpoint::create` 构建, 含 `create_backup`/`create_backup_with_description`/`list_backups`/`get_backup_info`/`delete_backup`/`apply_retention_policy`); `RecoveryManager` (`restore` 临时目录 + 原子 rename + EXDEV fallback, `verify_backup` 完整性校验); `RetentionPolicy` (min_count/min_age/max_count/max_age)
- **备份可观测性**: `backup_create`/`backup_list`/`backup_delete`/`backup_retention`/`backup_restore`/`backup_verify` tracing span; `aidb_backup_total`/`aidb_backup_size_bytes`/`aidb_backup_duration_seconds` Prometheus 指标
- **备份 feature gate**: `backup` feature (默认启用), 可选依赖 `ring`/`hex`/`serde_json`

### Changed

- **`block_io.rs` 并发读取修复**: `read_block_from_file` 从 `try_clone()` + `seek()` + `read()` 改为 `pread()` (`read_at`), 消除 `Arc<File>` 多线程并发读取时的寻址位置竞态

## [0.12.0] - 2026-05-28

### Added

- **Phase 17 可观测性基础**: 新增 `WALManager::flush/sync/rotate/recover/cleanup/close` 显式 `#[instrument(name = "...")]`; `DB::open` 命名 `db_open`; Raft snapshot (`get_current_snapshot`/`install_snapshot`) 添加 `#[instrument]`
- **Cargo.toml 修复**: monitoring feature 的 deps (`prometheus`/`opentelemetry`/`opentelemetry_sdk`/`tracing-opentelemetry`) 从 `[build-dependencies]` 移回 `[dependencies]`, 修复 `--features monitoring` 编译错误
- **prometheus 类型导入修正**: register_* 宏替换为 struct 构造器 (prometheus 0.13 类型不在 crate root 下)

## [0.11.1] - 2026-05-28

### Added

- **DBIterator 反向迭代**: `BlockIterator::prev/seek_to_last` → `SSTableIterator::prev/seek_to_last` → `DBIterator::prev/seek_to_last` 完整链; `prev()` 支持反向跨 MemTable + SSTable 合并遍历, `seek_to_last()` 定位到最后一个 key; tombstone 反向过滤; 8 个新增测试 (`tests/modules/sstable/function.rs` + `tests/modules/db/function.rs`)
- **CLAUDE.md**: 新增 `Known Limitations` 章节 (压缩/Snapshot/compaction_threads/Write stall 等)

### Changed

- **Snap/LZ4 压缩错误消息**: `block_io.rs` 两处错误消息更新为 `"Snap/LZ4 compression not implemented (known_limitation)"`; `config.rs` `compression` 字段注释标注已知限制
- **03-sstable.md**: `snap`/`lz4` 压缩条目标记为 `known_limitation`

## [0.11.0] - 2026-05-28

### Added

- **`MetaRequest::UnassignSlots`**: 新增变体用于 `CLUSTER DELSLOTS`; 含 validate (检查 slot 已分配) + apply_mutate (设为 Unallocated, 更新 affected Group slot_ranges)
- **集群运维 (`cluster` feature)**: 副本分配、成员变更、在线槽迁移三层运维能力, 构建于 MetaRaft (P13) 和 Multi-Raft (P14) 之上
- ReplicaAllocator: `replica_allocator.rs` — `Balanced`/`Weighted` 分配策略, `allocate_group` (负载最低节点作为 primary, 副本容错 degrade), `rebalance_replicas` (标准差阈值判定, 生成 `ReplicaRebalancePlan`), `suggest_slot_allocation` (16384 槽均分); 7 个内嵌单元测试
- MembershipCoordinator: `membership_coordinator.rs` — `add_node` (Empty/`TakeoverGroups` 加入), `remove_node` (graceful/force 含副本安全检查和迁移状态预检), `change_group_membership` (全量替换 + MetaRaft 元数据同步 + openraft joint consensus), `replace_node` (逐 Group 迁移)
- SlotMigrationManager + SlotMigrationExecutor: `slot_migration.rs` — `start_migration` (MetaRaft `BeginSlotMigration` 共识 + 校验), `run_pending_migration` (调用方驱动执行), `get_migration_status` (Prepare/Migrating 阶段映射), `commit_migration` (原子 take 执行器防竞态), `cancel_migration` (取消信号 + MetaRaft `CancelSlotMigration`); executor `execute` 逐 key 条件写入 (`PutConditional` 防 TOCTOU), `verify_migration` 确定性步长采样验证, 文件 checkpoint 原子 rename 断点续传
- 配置: `config.rs:MigrationConfig` — `max_batch_size`/`progress_report_interval`/`max_retries`/`retry_base_delay_ms`/`verify_sample_factor` 五个迁移参数; `ClusterConfig.migration` 字段集成
- MultiRaftNode 转发 + 生命周期接入: `multi_raft_node.rs` — `get_key_from_group`/`get_ttl_from_group`/`scan_keys`/`change_group_membership`/`add_learner_to_group` 五个方法; `LifecycleConfig` 聚合 data_dir/raft_config/options; `start_lifecycle_with_data` + `create_group_inner`/`open_group`/`remove_group_inner` (LifecycleManager tick → ShardedStorage + OpenRaftNode + RaftServiceDispatcher 全链路)
- 公开导出: `ReplicaAllocator`, `MembershipCoordinator`, `SlotMigrationManager`, `SlotMigrationExecutor`, `MigrationProgress`, `MigrationPhase`, `LifecycleConfig`, `MigrationConfig`
- 测试: 21 新增用例 — 7 `ReplicaAllocator` 内嵌 (allocator_balanced/weighted/no_nodes/single_node/rebalance/slot_distribution/count_groups); 14 `cluster_ops` 集成 (allocator_weighted/free_slots_exhausted, membership data structures, migration types/progress/phases/validation, checkpoint save/load/delete, rebalance_plan)

### Changed

- **`ClusterConfig`**: 新增 `migration: MigrationConfig` 字段; `for_testing`/`for_production`/`default` 同步初始化
- **`ShardedStorage`**: 添加 `#[derive(Clone)]` (scan_keys 路径需要克隆存储引用)
- **`src/cluster/mod.rs`**: 新增 `replica_allocator`, `membership_coordinator`, `slot_migration` 模块及 pub use

## [0.10.0] - 2026-05-27

### Added

- **Multi-Raft 数据平面 (`cluster` feature)**: 管理多个 Raft 数据 Group 的完整生命周期; 统一 gRPC 端口 (RaftServiceDispatcher) + per-Group 独立 DB 实例
- Router: `router.rs` — CRC16-IBM 零依赖查表法, `extract_hash_tag` (Redis hash tag), `key_to_slot` (CRC16(hash_tag) % 16384), 线程安全 `Router` 结构 (slot_table/group_nodes/node_addrs 缓存), `route_key`/`route_slot`/`route_keys`/`group_keys`/`group_ops`/`refresh_from_data`
- RaftServiceDispatcher: `network.rs` — 多 Group gRPC 调度器 (`HashMap<GroupId, Arc<Raft>>`), `RaftServiceImpl` 改为持有 dispatcher 引用, 所有 Group 共享同一个 gRPC 端口
- ShardedStorage: `sharded_storage.rs` — 每 Group 独立 DB 实例 (`data/group_{id}/`), 预留 `StorageStats` / `AggregateStats`
- LifecycleManager: `lifecycle_manager.rs` — `MetaRaftProvider` trait (解耦 MetaRaft 查询), `tick()` 轮询 MetaRaft 变更并刷新 Router, `run()` 异步循环 (可配置 tick_interval, shutdown signal 终止)
- MultiRaftNode: `multi_raft_node.rs` — 组合 Router/Dispatcher/ShardedStorage/LifecycleManager; `start()` 统一 gRPC 服务; `start_lifecycle()` 后台任务; `propose_key/get_key/propose_group/shutdown`
- 测试: 43 用例 — L1 单元测试 (CRC16 标准向量, hash_tag 边界, Router 路由/刷新, Dispatcher, ShardedStorage, LifecycleManager, MultiRaftNode 构建); L2 集成 (独立 Group 存储, tick 发现/移除, Router 刷新, 并发 open, run loop shutdown)
- 公开导出: `crc16`, `extract_hash_tag`, `key_to_slot`, `Router`, `RaftServiceDispatcher`, `ShardedStorage`, `LifecycleManager`, `MultiRaftNode`

### Changed

- **`RaftServiceImpl`**: 构造函数从 `new(Arc<Raft>)` 改为 `new(Arc<RaftServiceDispatcher>)`; 所有 RPC handler 通过 `dispatcher.get_raft(group_id)` 分发
- **`OpenRaftNode::start_server`**: 改为创建单 Group dispatcher (兼容 P12 单 Group 模式); MetaRaft 保持独立 gRPC 服务
- **`src/cluster/mod.rs`**: 新增 `router`, `sharded_storage`, `lifecycle_manager`, `multi_raft_node` 模块及 pub use

## [0.9.0] - 2026-05-27

### Added

- **MetaRaft 控制平面 (`cluster` feature)**: 独立 Raft Group (`METARAFT_GROUP_ID=0`) 共识集群元数据 (节点/Group/Slot 路由/迁移状态)
- Meta 类型: `meta_types.rs` — `ClusterMeta`, `GroupMeta`, `NodeInfo`, `SlotStatus` (64 位), `SlotMigrationState`, 14 变体 `MetaRequest`; `METARAFT_GROUP_ID=0`; `DEFAULT_GROUP_ID=1` 不变
- MetaStateMachine: `apply_meta_request` 14 变体 + `validate_meta_request` + `ApplyOutput` 三 key (`\x00meta_raft/{cluster_meta,slot_table,migration_state}`) + `rebuild_slot_ranges` + `reload_from_db` + `format_version` Corruption 检查
- `keys.rs`: `meta_cluster_meta_key()`, `meta_slot_table_key()`, `meta_migration_state_key()`, `meta_range_start/end`; 与 `\x00raft/` 同 namespace
- Storage 集成: `OpenRaftStorage::new(db, gid, meta_state)`, `apply_meta_entry` — `kv_pairs` + `last_applied` 同 WriteBatch 原子持久化; `group_id==0` 时 snapshot 扫描/安装 `\x00meta_raft/*`, 安装后 `MetaStateMachine::reload_from_db()`
- MetaRaftNode: 组合 `OpenRaftNode` + `MetaStateMachine`; `new_with_storage`; `initialize` 幂等 (仅 Raft membership); propose 前置校验; `get_cluster_meta`/`get_slot_table`/`get_migration_state` getters; 4 tracing spans

### Changed

- **`OpenRaftNode::new`** 委托 `new_with_storage`; `OpenRaftStorage::new` 增加 `meta_state: Option<Arc<MetaStateMachine>>` 参数
- **`apply_entries_internal`**: Meta 分支使用 `apply_meta_entry` 单 WriteBatch (原子 `kv_pairs` + `last_applied`); 数据 Group 仍逐 entry 方式 (P12 行为, P14 统一)
- **`tests/storage.rs`**: `test_apply_output_integration` 更名为 `test_meta_storage_apply_output_integration` (对齐验收 filter)
- **mod.rs**: 公开导出 `meta_types`, `meta_state_machine`, `meta_raft_node` 模块; 移除 `Stub`

### Added

- **Raft 共识基础 (`cluster` feature)**: `proto/raft.proto` + `build.rs` (gRPC `aidb.raft`); `OpenRaftStorage` (log/vote/apply/snapshot); `RaftNetworkClient` + `RaftServiceImpl`; `OpenRaftNode` (initialize/propose/leader)
- Key 布局: `\x00raft/{gid}/*` (Raft meta/log), `\x01sm/{gid}/*` (state machine); vote/last_applied `rmp_serde`, membership/snapshot_meta `bincode`
- `Error::Cluster(ClusterError)` 类型化错误; `tracing` spans: `raft_save_vote`, `raft_append_log`, `raft_apply_sm`, `raft_rpc_*`
- 测试: `tests/raft.rs` + `tests/modules/cluster/*` (storage/network/node/3-node integration); log L1 (vote/append/delete/restart), snapshot install, apply 幂等/membership 持久化, 3-node sm 复制断言; `OpenRaftNode::new` 强制 `use_wal=true`; CI matrix `default` + `--features cluster`

### Changed

- **`Request`/`Response` serde**: P12 实现暂不用 adjacently tagged (与 openraft Entry + rmp_serde 兼容); MetaRequest 仍保留 tagged stub

## [0.7.5] - 2026-05-26

### Added

- **`engine/checkpoint/` MVP (Phase 11.6′)**: `Checkpoint::create` — flush → `checkpoint_in_progress` + `pin_sstables` → hardlink/copy 稳定文件集; `verify_openable` 校验
- `WALManager::scan_wal_file_paths`; `DB::collect_checkpoint_file_paths` / `enter_checkpoint` / `leave_checkpoint`
- 测试: `test_checkpoint_*` (含 `test_checkpoint_during_compaction`, 源码内 `#[cfg(test)]`)

## [0.7.4] - 2026-05-26

### Added

- **`DB::delete_range`**: 删除 `[start, end)` 半开区间内 user key (scan + WriteBatch; 非 RangeTombstone)
- 测试: `test_db_delete_range`, `test_wal_gc_after_flush`

### Changed

- **WAL GC 水位**: `wal_gc_watermark` 在无 immutable 时取 active MemTable 最小 sequence; 全空时 `u64::MAX` (对齐详设)
- compaction apply 后触发 `try_cleanup_wals`, 跨版本 flush 后更易回收旧 WAL

## [0.7.3] - 2026-05-25

### Added

- **MemTableIterator::prev**: 反向迭代 (mirror `next`, 空起点定位 `table.back()`)
- **cache/filter L1 dataflow**: `tests/modules/cache/dataflow.rs`, `tests/modules/filter/dataflow.rs` (span/event 可观测性回归)
- 测试: `test_memtable_iterator_prev`, `test_cache_observability`, `test_filter_bloom_observability`

### Changed

- **Block Cache 零拷贝**: `read_block_cached` 返回 `Bytes`; cache hit 不再 `to_vec()`

## [0.7.2] - 2026-05-22

### Added

- Phase 7.6 criterion benchmarks: `benches/write_bench.rs` (`put_1kb` 补零 key `key_{:08}`, `write_batch_100_flush`), `benches/read_bench.rs` (随机 get, 范围 scan); 详设 §3.4「随机写入」与纯 `write_batch_100`(无 flush) 未纳入 7.6
- `AIDB_BENCH_PRELOAD` 环境变量可覆盖 read_bench 预填充规模 (默认 **10_000**); preload 使用 WriteBatch 分块 (500 keys/batch)
- P1 测试加固: `tests/modules/db/p1.rs` (flush/并发/WriteBatch/WAL 损坏/背压等); compaction picker 专项; `test_concurrent_writes_during_compaction`
- DB 跨模块 dataflow: `tests/modules/db/dataflow.rs` (put/get/flush/delete span 树 + event 链); `tests/engine/dataflow.rs` (put→flush→get 生命周期)
- `Snapshot::iter` / `scan`; `WALManager::append` span 名 `wal_write`

### Changed

- **M1 单机 LSM 存储引擎完成** (Phase 1–7.6)
- `Options::default()` / `Options::for_testing()` 默认 `sync_wal: false` (强持久写需显式 `sync_wal = true`; crash 测试已单独开启)
- read_bench 默认 preload 10_000 keys (原 100_000); 大规模可通过 `AIDB_BENCH_PRELOAD` 覆盖
- `max_write_buffer_number` flush 背压: Immutable 达上限时阻塞写入并驱动 flush
- `DBIterator` snapshot 边界 MVCC: 新版本 delete 不再误吞旧版本 put
- `db_flush` / `db_flush_sst` span 分层

## [0.7.1] - 2026-05-21

### Added

- Snapshot (Phase7.5): `tests/snapshot.rs`, `tests/modules/snapshot/` — MVCC get (basic / flush / compaction 弱化语义 / sequence 边界), 并发 (4 用例, 1 `#[ignore]` 压测)
- 可观测性: `DB::snapshot` 增加 `#[instrument(name = "db_snapshot")]` + span 字段 `sequence`; `tests/modules/snapshot/dataflow.rs`

### Changed

- 无 Snapshot 读路径逻辑变更 (Phase5 `get_at_sequence` 不变)

## [0.7.0] - 2026-05-21

### Added

- Block Cache (Phase7.3–7.4): `engine/cache/` — `CacheKey`, `CacheStats`, `BlockCache` (LRU + 原子 stats; `MAX_EVICTIONS = max(capacity/8KiB, 16)`)
- SSTable 集成: `read_block_cached` (miss 才 `sst_block_read`); `SSTableReader::open(path, block_cache)`; `read_key_range` / `SSTableIterator::load_block` 走 cache
- DB 集成: `Arc<BlockCache>` 在 `open` 时于 version 恢复前创建; `load_sstables_from_version` / `scan_version_edits_from_dir` / flush / compaction 产出 SST 共享同一实例
- DB 运维 API: `cache_stats`, `reset_cache_stats`, `clear_cache`, `block_cache_size`, `block_cache_capacity`
- 公共导出: `BlockCache`, `CacheStats` (`lib.rs`)
- 可观测性: span `cache_get` / `cache_insert` / `cache_evict`; DEBUG `cache.hit` / `cache.miss` / `cache.insert` / `cache.evict`

### Changed

- `SSTableReader::open` 增加第二参数 `block_cache: Option<Arc<BlockCache>>` (生产路径 `Some`, 单测默认 `None`)
- `load_sstables_from_version` / `scan_version_edits_from_dir` 增加 `block_cache` 参数

## [0.6.0] - 2026-05-21

### Added

- Bloom Filter (Phase7): `engine/filter/` — `Filter` trait + `BloomFilter` (FNV-1a 双哈希, CRC32 编码)
- SSTable 集成: `SSTableBuilder` 写入 Meta Block + Meta Index `"bloom"`; `SSTableReader::get` fast path; `bloom_false_positive_rate = 0.0` 禁用
- flush / compaction 传参: `set_expected_keys`; compaction 两遍 merge 精确计数后写盘
- 可观测性: span `bloom_build` / `bloom_check`; DEBUG `bloom.build`; decode 失败 WARN 降级

### Changed

- `SSTableBuilder::new` 增加第 5 参数 `bloom_false_positive_rate`
- `CompactionJob::new` 增加 `bloom_false_positive_rate` 字段

## [0.5.0] - 2026-05-21

### Added

- Compaction 模块 (Phase6): `engine/compaction/` — VersionSet, MergeIterator, CompactionPicker, CompactionJob
- Version 管理: MANIFEST + CURRENT (CRC32 + bincode `VersionEdit`); `VersionSet::recover` / `bootstrap_from_scan` / `open_new`
- 无 `CURRENT` 时从目录扫描 bootstrap (兼容 Phase5 仅 flush 落盘); 启动时清理不在 Version 中的孤儿 `.sst`
- DB 集成: `open` 以 MANIFEST 为 SST 权威; flush / compaction 共用 `version_set.allocate_file_number()`
- 后台 compaction: `maybe_trigger_compaction` 发 channel 信号; 线程 `aidb-compaction` (500ms 兜底轮询); `close` / `Drop` 与 flush 对称 shutdown
- `DB::drain_compactions()` — 测试与诊断用, 在当前线程排空 compaction 队列
- `Options`: `max_levels` (默认 7), `max_manifest_size`, `background_compaction` (`for_testing()` 默认 false, 避免与 `drain_compactions` 竞态)
- Compaction 可观测性 (`monitoring` feature): span `cmp_pick` / `cmp_run` / `cmp_merge` / `cmp_apply` / `cmp_background`; counter `aidb_compaction_total`, histogram `aidb_compaction_duration_seconds`
- 模块测试: `tests/compaction.rs`, `tests/modules/compaction/` (`cargo test --test compaction -- --test-threads=1`)
- 引擎集成: `tests/engine/compaction.rs` — CI ~800 keys; `#[ignore]` 10000 keys 压测 (`cargo test --test engine compaction`)
- Open 迁移: `tests/modules/db/bootstrap_migration.rs` (无 CURRENT → bootstrap → MANIFEST reopen; `cargo test --test db bootstrap`)

### Changed

- Flush 追加 `VersionEdit::AddFile` 并写 MANIFEST; 移除 `next_sst_file_number`, 文件号统一由 VersionSet 分配
- Level 1+ 读路径: SST 文件定位改为 `user_key_in_sstable_range` (比较 user key 范围, 不再对 InternalKey 做 seek)

### Fixed

- Compaction 后 point `get` 全部 miss: Level 1+ 定位曾用 `u64::MAX` InternalKey 二分, 与文件 key range 语义不符

## [0.4.0] - 2026-05-20

### Added

- DB 引擎 (Phase5): `engine/db` 模块 — WAL + MemTable + SSTable 总协调器
- 公共 API (`lib.rs` 再导出): `DB`, `WriteBatch`, `WriteOp`, `Snapshot`, `DbIterGuard`
- `DB::open` / `put` / `get` / `delete` / `write` / `flush` / `close` / `snapshot` / `iter` / `scan`
- `Options::validate()` — 打开前校验 `memtable_size` > 0、`max_write_buffer_number` >= 1 等
- 打开流程: `WALManager::recover` → `replay_entries` → `WALManager::open` (LOCK) → 目录扫描加载 `*.sst` → 从 WAL/MemTable/SST 汇总 `last_sequence`
- MemTable freeze + 后台 flush 线程 (`aidb-flush`, 500ms 轮询 `immutable_memtables`); 手动 `flush` 与 `close` 共用 `flush_lock`
- Flush: `MemTable` → `SSTableBuilder` → L0 头插; 空 MemTable 不产出 SST; `rotate_wal` + `cleanup` 按 watermark 回收 WAL
- 读路径: active MemTable → immutable (newest-first) → L0 逐文件 → L1+ 二分; `get_at_sequence` MVCC 边界
- `WriteBatch`: `BatchStart` + 连续 sequence; 空 batch 不写 WAL
- `Snapshot::get` — 创建时刻 sequence 点读
- `WALManager::note_appended_sequence` — 支撑 flush 后 WAL GC watermark
- DB 可观测性: span `db_open` / `db_put` / `db_get` / `db_delete` / `db_write_batch` / `db_flush` / `db_close` / `db_scan` / `db_snapshot`
- DB 指标 (`monitoring` feature): `aidb_operations_total`, `aidb_flush_total`, `aidb_sequence`, `aidb_total_key_count`; flush 时更新 `aidb_sstable_count` / `aidb_sstable_size_bytes`
- 模块测试: `tests/db.rs`, `tests/modules/db/` (16 用例; `cargo test --test db`)
- 跨模块管线测试: `tests/pipeline.rs`, `tests/pipeline/wal_memtable.rs` (5 用例; `cargo test --test pipeline -- --test-threads=1`)
- 引擎集成测试: `tests/engine.rs`, `tests/engine/scenarios.rs`, `tests/engine/crash_recovery.rs` (5 用例; `cargo test --test engine -- --test-threads=1`); 目录说明见 `tests/README.md`

### Changed

- `replay_entries` 迁入 `engine/db/replay.rs`; `tests/pipeline/` 直接调用 crate 内实现
- 测试布局: `tests/dataflow/` 更名为 `tests/pipeline/`; 原 `tests/integration.rs`、`tests/crash_recovery.rs` 迁入 `tests/engine/`
- `error_if_exists` 使用 `Error::InvalidArgument` (无 `AlreadyExists` 变体)

### Fixed

- `sequence` 计数: 打开时取 `max(WAL, MemTable, SST)` 已分配最大值, 避免仅 WAL 恢复时 `get` 看不到已落盘 SST 中的条目
- `close()`: 先 `do_flush()` 再标记 `closed`, 避免 close 路径 flush 被 `check_not_closed` 拒绝
- `get`: MemTable 使用 `search` 区分 tombstone 与 miss; immutable + SST 层按 sequence 上界读取
- WAL GC watermark: 无待 flush immutable 时用当前 `sequence`, 避免 `u64::MAX` 导致误删 WAL

## [0.3.0] - 2026-05-20

### Added

- SSTable 模块 (Phase4): Block / BlockBuilder / BlockIterator (前缀压缩 + restart points); Footer 48B; IndexBlock / `find_block_handle`; `SSTableBuilder` / `SSTableReader` / `SSTableIterator`
- 文件格式: Data/Meta Index/Index block + 5B trailer (compression type + CRC32); magic `SSTABLE_`; 当前仅 `CompressionType::None` (snap/lz4 留 Phase7)
- SSTableBuilder: 严格递增 InternalKey, `finish` / `abandon`, `.sst.tmp` + `rename`; 文件名 `{file_number:06d}_L{level}.sst` 与旧格式解析
- SSTableReader / SSTableIterator: `get` (user_key + sequence 上界), `open`, 跨 block `seek_to_target`; 空 Meta Index (Bloom 留 Phase7)
- SSTable 可观测性: span `sst_build_add` / `sst_build_finish` / `sst_seek` / `sst_block_seek` / `sst_block_read`; events `sst.build.complete`, `sst.seek.result`
- SSTable 指标 (`monitoring` feature): Gauge `aidb_sstable_count`, `aidb_sstable_size_bytes`, label `level` (数值更新留 Phase5 flush)
- 模块测试: `tests/sstable.rs`, `tests/modules/sstable/` (27 用例; `cargo test --test sstable -- --test-threads=1`)

### Changed

- InternalKey 继续复用 `engine::memtable`; 新增 `engine::sstable` 模块导出

## [0.2.0] - 2026-05-19

### Added

- MemTable 模块 (Phase3): InternalKey 编解码与 `compare_internal_key`; SkipMap 实现的 `put` / `get` / `get_latest` / `delete` / `search` / `freeze` / `approximate_size`
- ImmutableMemTable: MemTable 冻结后的只读视图, `flush_seq` 由上层 DB Engine 注入
- MemTableIterator: 前向迭代 `seek` / `seek_to_first` / `next` / `valid` / `key` / `value` (`prev` 未实现, 留 P1)
- MemTable 可观测性: span `mem_put` / `mem_get` / `mem_delete` / `mem_freeze`; log events `mem.put`, `mem.get.hit`, `mem.get.miss`, `mem.delete`, `mem.freeze`
- MemTable 指标 (`monitoring` feature): Gauge `aidb_memtable_size_bytes`, label `state=active|frozen`

### Changed

- WAL 与 MemTable 共用 sequence 上限校验 (`check_sequence`, `sequence >= 2^56` 拒绝)
- WalEntry 编解码路径新增 `WalEntry::validate()`

## [0.1.0] - 2026-05-19

### Added

- WAL 模块 (Phase2): WalEntry 编解码; Record 读写 (CRC32, block padding, 大 record 分片 Full/First/Middle/Last)
- Writer: 追加写入, 条件 fsync, block padding 与自动分片
- Reader: 顺序读取, CRC 校验, 损坏容忍 (partial write, 无效 type, strict 模式), 跨 block padding 跳过
- WALManager: 文件创建/追加/轮转/清理/崩溃恢复, FileHeader 版本检查, LOCK 文件互斥
- WriteBatch: BatchStart 编码; recovery 时未完成 batch 截断回滚
- FileHeader: close 时 max_seq 回填与 CRC 重算 (两次 fsync)
- 自动轮转: append 后按 `max_wal_size` 触发 rotate
- Options: 新增 `sync_wal`, `max_wal_size`, `strict_wal_recovery`
- WAL 可观测性: `#[tracing::instrument]` (默认函数名 span) + debug events (`wal.write.start/complete`, `wal.sync.start/complete` 等); 目标 span 名 `wal_write`/`wal_flush` 等见 spec, Phase17 对齐
- WAL 指标 (`monitoring` feature): Gauge `aidb_wal_size_bytes`

### Changed

- `WALManager::open` / `recover` 入参改为 `Arc<Options>`
- Writer 按 `sync_wal` 决定是否每次写入后 fsync (0.0.1–0.7.1 默认 true; **0.7.2 起默认 false**)

## [0.0.1] - 2026-05-18

### Added

- 项目骨架: `Cargo.toml`, `deny` / `rustfmt` / `clippy`; features `cluster`, `monitoring`
- Error 类型: Io, Corruption, Busy, NotFound, InvalidArgument, Cluster (feature-gated)
- Options: 20 项可调参; 预置 `for_testing`, `for_high_write_throughput`, `for_high_read_throughput`
- ClusterConfig (feature-gated): group_count, replication_factor, max_log_entries, max_log_size_bytes

### Changed

- 开发规范与 git hooks: CLAUDE.md, CONTRIBUTING.md, pre-commit (fmt + clippy)
