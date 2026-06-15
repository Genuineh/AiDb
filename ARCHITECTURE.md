# AiDb 架构

## 目录结构

```
src/
├── lib.rs                 # 公共 API 入口 (< 30 个 pub fn)
├── error.rs               # 错误类型 (thiserror 枚举)
├── config.rs              # Options 配置 (20 项)
├── engine/                # LSM-Tree 核心
│   ├── mod.rs
│   ├── wal/               # WAL 写前日志
│   │   ├── mod.rs
│   │   ├── record.rs      # Record 格式 (CRC32 + 分片)
│   │   ├── writer.rs      # 追加写入 + fsync
│   │   ├── reader.rs      # 顺序读取 + CRC 校验
│   │   └── manager.rs     # 文件管理 (轮转/清理/恢复)
│   ├── memtable/          # 内存索引
│   │   ├── mod.rs
│   │   ├── key.rs         # InternalKey 编解码
│   │   └── table.rs       # SkipList 并发读写
│   ├── sstable/           # 磁盘存储
│   │   ├── mod.rs
│   │   ├── block.rs       # Block 格式 (restart points)
│   │   ├── builder.rs     # SSTableBuilder
│   │   ├── reader.rs      # SSTableReader
│   │   └── iterator.rs    # SSTableIterator
│   ├── compaction/        # Leveled Compaction
│   │   ├── mod.rs
│   │   ├── version.rs     # Version/VersionSet/VersionEdit
│   │   ├── merge.rs       # MergeIterator (多路归并)
│   │   ├── picker.rs      # CompactionPicker
│   │   ├── job.rs         # CompactionJob
│   │   └── background.rs  # 后台 compaction 线程
│   ├── filter/            # Bloom Filter
│   │   └── bloom.rs
│   ├── cache/             # LRU Block Cache
│   │   ├── mod.rs
│   │   └── block_cache.rs
│   ├── checkpoint/        # Checkpoint 快照
│   │   └── mod.rs
│   └── db/                # DB 引擎总协调
│       ├── mod.rs         # DB 结构体 + open/close
│       ├── iterator.rs    # DBIterator
│       ├── snapshot.rs    # MVCC Snapshot
│       ├── write_batch.rs # 原子批量写入
│       └── replay.rs      # WAL replay
├── cluster/               # 分布式集群 (feature-gated)
│   ├── mod.rs
│   ├── router.rs          # CRC16 Slot 路由
│   ├── raft_storage.rs    # OpenRaftStorage
│   ├── raft_network.rs    # gRPC 网络层
│   ├── raft_node.rs       # OpenRaftNode
│   ├── meta_types.rs      # 元数据结构
│   ├── meta_state_machine.rs  # 控制平面状态机
│   ├── meta_raft_node.rs  # MetaRaft 封装
│   ├── multi_raft_network.rs # 多 Group gRPC 分发
│   ├── multi_raft_node.rs # 多 Group 管理
│   ├── sharded_storage.rs # per-Group DB 实例
│   ├── lifecycle_manager.rs   # 生命周期管理
│   ├── replica_allocator.rs   # 副本分配
│   ├── membership_coordinator.rs  # 成员变更
│   └── slot_migration.rs  # 在线槽迁移
├── backup/                # 备份与恢复
│   ├── mod.rs
│   ├── storage.rs         # BackupStorage trait + LocalFileStorage
│   ├── manager.rs         # BackupManager + RetentionPolicy
│   ├── recovery.rs        # RecoveryManager
│   └── util.rs            # SHA256 工具
├── metrics.rs             # Prometheus 指标定义
└── snapshot.rs            # MVCC 快照 (引擎层)
```

## 数据流

### 写入路径

```
put(key, value)
  → 序列化 InternalKey (user_key + sequence + type)
  → WALManager::append (追加日志, 可选 fsync)
  → MemTable::put (跳表插入)
  → (如果 MemTable 满) freeze → 触发 Flush 线程
    → ImmutableMemTable → SSTableBuilder
      → 写入 Data Block → Index Block → Meta Block (Bloom) → Footer
      → .sst.tmp → rename → .sst
      → VersionEdit::AddFile → MANIFEST append
      → WAL cleanup (按 watermark)
```

### 读取路径

```
get(key)
  → 计算 InternalKey (user_key + sequence 上界)
  → 搜索 active MemTable
  → 搜索 immutable MemTables (newest → oldest)
  → 搜索 L0 SSTable (逐文件, 带 Bloom Filter 预检)
  → 搜索 L1+ SSTable (user_key 范围二分, 带 Block Cache)
  → 返回 Value 或 NotFound
```

### Compaction 路径

```
后台线程循环 (单线程或多线程, 通过 compaction_threads 配置):
  → CompactionPicker::pick_compaction (L0 文件数/LN 大小)
  → try_claim_files (HashSet 防重叠)
  → 如果无 key 范围重叠: Trivial Move (直接 rename, 不归并)
  → 如果输入大且配置允许: Subcompaction (按 key range 分裂为 N 个子任务)
    → CompactionJob::run (MergeIterator 多路归并, 并行)
      → count_dedup_entries (第一遍计数, 快照保护)
      → (可选) count_dedup_with_splits (记录分割点)
      → MergeIterator::with_range (子范围)
      → 输出 N 个 SSTable
  → VersionSet::apply (VersionEdit, MANIFEST)
  → 清理旧 SSTable
```

### 集群路径

```
CLUSTER MEET node
  → MetaRaftStateMachine::apply(MetaRequest::RegisterNode) (共识)
  → LifecycleManager::tick 发现新拓扑变化
  → CLUSTER ADDSLOTS slot [...] (本节点调用)
    → 自动创建 Group (node_id 作为 group_id, 本节点作为 leader)
    → MetaRequest::CreateGroup + AssignSlots
  → MultiRaftNode.start_lifecycle_with_data 后台创建 ShardedStorage + OpenRaftNode
  → MultiRaftNode.start 启动统一 gRPC server (所有数据 Group 共享端口)
  → Router::refresh_metadata 更新路由表
  → 客户端命令: Router::route_key → MultiRaftNode::propose/get
```

## 关键设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| Compaction 策略 | Leveled | 读放大优于 Tiered, 适合点查场景 |
| MemTable 实现 | Crossbeam SkipMap | 无锁并发, 读写不阻塞 |
| WAL 格式 | Record 分片 (Full/First/Middle/Last) | 兼容大记录 (>32KB) |
| Block Cache 替换策略 | LRU | 实现简单, 工业验证成熟 |
| 集群共识 | OpenRaft | 成熟的 Raft 库, 支持 joint consensus |
| 备份方式 | Checkpoint + 文件复制 | 与 LSM-Tree flush 自然对齐 |

## 设计原则

详见 [DESIGN.md](DESIGN.md).
