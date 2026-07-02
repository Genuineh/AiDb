---
name: aidb-engine
description: AiDb write path — WAL, MemTable, DB API, WriteBatch, MVCC snapshot, crash recovery. Use when changing src/engine/{wal,memtable,db}, debugging put/get/write path, WAL replay, MemTable freeze, or Snapshot reads.
---

# AiDb Engine (写路径)

## 何时读本文

- 改 `engine/wal`, `engine/memtable`, `engine/db` 或 `DB::*` 公共 API
- 排查写路径、WAL 恢复、MemTable freeze、WriteBatch 原子性、Snapshot 读
- **不覆盖**: SSTable 布局 / compaction / Bloom / BlockCache / checkpoint → [engine-storage.md](engine-storage.md)

## 代码地图

| 路径 | 职责 | 入口 |
|------|------|------|
| `engine/wal/record.rs` | Record 物理格式; `WalEntry` 编解码; `OpType` | `WalEntry::encode`, `OpType` |
| `engine/wal/writer.rs` | 追加 Record; 32KB block padding; sync | `Writer::write_record` |
| `engine/wal/reader.rs` | 顺序读; 分片重组; strict/非 strict 损坏处理 | `Reader::read_record` |
| `engine/wal/manager.rs` | open/recover/append/rotate/cleanup; `LOCK` | `WALManager::*` |
| `engine/memtable/internal_key.rs` | InternalKey 编码; `ValueType`; sequence 上界 | `encode_internal_key`, `check_sequence` |
| `engine/memtable/table.rs` | `MemTable` / `ImmutableMemTable`; freeze | `MemTable::put`, `freeze` |
| `engine/memtable/iterator.rs` | MemTable 迭代 | `MemTableIterator` |
| `engine/db/inner.rs` | DB 总协调: open/写/读/flush/close/后台线程 | `DB::open`, `put`, `write`, `get` |
| `engine/db/write_batch.rs` | 批写容器 | `WriteBatch`, `WriteOp` |
| `engine/db/replay.rs` | WAL entry → MemTable | `replay_entries` |
| `engine/db/snapshot.rs` | MVCC 快照; `SnapshotList` | `Snapshot::get`, `SnapshotList` |
| `engine/db/iterator.rs` | 跨 MemTable + SSTable 归并迭代 | `DBIterator` |
| `engine/db/numbers.rs` | WAL 文件编号扫描 | `scan_next_wal_file_number` |

公共 re-export (`lib.rs`): `DB`, `WriteBatch`, `WriteOp`, `Snapshot`, `DbIterGuard`.

## 关键 invariant (勿破坏)

- **写顺序**: WAL append **先于** MemTable 写入 (`put`/`delete`/`write` 均如此).
- **Sequence**: 合法范围 `[1, 2^56)`; `alloc_sequence` 与 `check_sequence` 双重校验; overflow → `Error::InvalidState`.
- **User key**: 非空; 空 key → `Error::InvalidArgument`.
- **单进程**: 数据目录 `LOCK` 文件 + `fs2` 独占锁; 多进程打开 → `Error::Busy`.
- **Batch 崩溃原子性**: recover 时 `BatchStart` 标记不完整 batch 整批丢弃; 单条 put/delete 无 BatchStart.
- **MemTable freeze**: `freeze(self)` 消费可变表; `flush_seq` = 冻结时刻 sequence.
- **Snapshot 创建**: 在 `write_lock` 下读 sequence 并注册 `SnapshotList` (register 必须发生在释放锁之前); Drop 时 unregister. Compaction 读取 `min_snapshot_sequence()` 时同样短暂持有 `write_lock`, 与 snapshot 创建之间形成 happens-before, 避免读到"seq 已确定但尚未 register"的中间态.
- **iter/scan vs get**: `iter`/`scan` 使用 `K_MAX_SEQUENCE`; `get` 使用 `sequence.load()` — 行为 intentionally 不同.

## 数据流

### 写入 (put / write)

```mermaid
flowchart LR
  W[check_write_stall] --> L[write_lock]
  L --> S[alloc_sequence]
  S --> WAL[WAL append]
  WAL --> MT[MemTable put/delete]
  MT --> F{memtable_size?}
  F -->|yes| FR[freeze → immutable]
  FR --> BG[后台 flush 线程]
```

### 打开 (open + recover)

```mermaid
flowchart TD
  A[Options::validate] --> B[WALManager::recover]
  B --> C[replay_entries → MemTable]
  C --> D[VersionSet recover / bootstrap]
  D --> E[load SSTables]
  E --> F["sequence = max(WAL, MemTable, SST) + 1"]
  F --> G[WALManager::open + LOCK]
  G --> H[flush / compaction 后台线程]
```

SSTable / VersionSet 细节见 [engine-storage.md](engine-storage.md).

### 读取 (get / snapshot)

`get_at_sequence(key, max_seq)`: 构造 `seek_key = encode_internal_key(key, max_seq, TypePut)` → active MemTable → immutable (新→旧) → SSTable 层 (L0 新→旧, L1+ 二分).

## 关键类型与 API

### WalEntry / OpType

逻辑 WAL 记录 (非 InternalKey):

| OpType | 含义 | has_value |
|--------|------|-----------|
| `TypePut` | put | true |
| `TypeDelete` | delete | false |
| `BatchStart` | WriteBatch 边界; value = op count (u32 LE) | true |
| `FileHeader` | 文件元数据; key=`WAL` | true |

磁盘 Record: `CRC32(4) + Length(2 LE) + Type(1) + Data`. Data 承载编码后的 `WalEntry`. 超大 entry 分片为 First/Middle/Last; 32KB block 边界 padding.

### InternalKey

`user_key + 7B (sequence<<8 高 56 位, 每位取反) + 1B ValueType`. `TypePut=0`, `TypeDelete=1`. MemTable 与 SSTable 共用编码.

### DB 公共 API (摘录)

| API | 说明 |
|-----|------|
| `DB::open(path, Options)` | 恢复 + 启动后台 flush/compaction |
| `put` / `delete` | 单条写; 连续 sequence |
| `write(&WriteBatch)` | 原子 batch; BatchStart + 连续 seq; 一次 `sync_wal` |
| `get` | 最新可见版本 |
| `delete_range(start, end)` | `[start,end)` scan + WriteBatch (非 RangeTombstone) |
| `snapshot()` | MVCC 点快照 |
| `iter` / `scan` | 全表或范围迭代 (见 invariant) |
| `flush` / `close` | 手动 flush; 优雅关闭 (停线程 → flush → WAL sync) |

### Snapshot

- `Snapshot::get/iter/scan` 固定 `sequence` 边界, 仅见 `seq <= snapshot_seq`.
- `SnapshotList::min_snapshot_sequence()` 供 compaction 保留旧版本 (见 engine-storage).

## 常见任务

### 排查写路径未持久化

1. 确认 `Options.use_wal` 与 `sync_wal` (false 时进程 crash 可能丢末批写).
2. 查 WAL 文件 `wal_{n}.log` 是否存在最新 entry.
3. `cargo test --test wal function::test_crash_recovery -- --test-threads=1`

### 排查 open 恢复丢数据

1. 读 `WALManager::recover` 日志 / `strict_wal_recovery` 配置.
2. 检查是否有未完成 BatchStart batch (recover 会整批 rollback).
3. `cargo test --test engine crash_recovery -- --test-threads=1`

### 改 MemTable freeze 行为

1. 入口: `DB::maybe_freeze`, `wait_for_memtable_slot` (`inner.rs`).
2. 阈值: `Options.memtable_size`, `max_write_buffer_number`.
3. freeze 后 immutable 由后台 `flush_pending` 或 `DB::flush` 写出 SST.

### 使用 WriteBatch

```rust
let mut batch = WriteBatch::new();
batch.put(b"k1", b"v1");
batch.delete(b"k2");
db.write(&batch)?;
```

空 batch 为 no-op (不写 WAL, 不分配 sequence).

### Snapshot 点读

```rust
let snap = db.snapshot()?;
db.put(b"k", b"new")?;
assert_eq!(snap.get(b"k")?, Some(b"old".to_vec()));
```

### 跑 engine 相关测试

见下方「测试」节.

## 配置与 feature flags

引擎写路径相关 `Options` 字段 (`config.rs`):

| 项 | 默认 (生产) | 说明 |
|----|-------------|------|
| `memtable_size` | 64 MiB | freeze 阈值 |
| `max_write_buffer_number` | 2 | immutable 上限; 超出背压 |
| `min_write_buffer_number_to_merge` | 1 | flush 合并控制 |
| `use_wal` | true | 禁用则 crash 不保证持久 |
| `sync_wal` | false | true = 每条写后 fsync |
| `strict_wal_recovery` | false | true = CRC 损坏报 `Corruption` |
| `max_wal_size` | 64 MiB | WAL 自动轮转 (0=禁用) |
| `flush_poll_ms` | 500 | 后台 flush 轮询 |
| `write_stall_poll_ms` | 10 | L0 过多时写 stall |
| `memtable_wait_iters` / `memtable_wait_interval_ms` | 10000 / 1 | immutable 满时等待 flush |
| `background_compaction` | true | false 时无写 stall (测试用) |

SSTable / compaction / cache 字段见 [engine-storage.md](engine-storage.md). `Options::for_testing()` 缩小 memtable/WAL 便于单测.

Feature: `monitoring` 启用 WAL/MemTable/DB span 与 OTel 指标 → [observability.md](observability.md).

## 测试

```bash
cargo test --test wal --test memtable --test db --test pipeline -- --test-threads=1
cargo test --test engine --test snapshot -- --test-threads=1
```

| 测试集 | 覆盖 |
|--------|------|
| `tests/wal` | Record 格式, recover, BatchStart rollback, LOCK, cleanup, **WriteBatch/WAL rotate 边界** |
| `tests/memtable` | InternalKey, put/delete/get, freeze |
| `tests/pipeline/wal_memtable` | recover → replay 管线 |
| `tests/db` | DB 模块 (wal_corruption, bootstrap 等) |
| `tests/engine` | 黑盒场景 + crash_recovery |
| `tests/snapshot` | MVCC + compaction 并发保版本 |

## 已知限制

- 并发 `get` 无锁读 MemTable: WriteBatch 逐条写入 MemTable 期间, 其他线程可能看到 batch **部分** 效果 (与 LevelDB 一致). Snapshot 创建持 `write_lock`, 无此问题.
- `iter`/`scan` 不过滤到当前 sequence, 使用 `K_MAX_SEQUENCE` 见全部已写入版本.
- `total_key_count` 在 `write_lock` 内通过 active MemTable 快速检查 key 存在性, 仅作近似统计 / metrics (AtomicUsize Relaxed 序, 不持久化).
- 数据目录格式与旧版 `aidb-oldmain` **不兼容** (文本 WAL → 二进制 WalEntry).

## 待核实

- 无.
