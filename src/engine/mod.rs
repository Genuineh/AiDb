//! AiDb Engine — 存储引擎核心: 写路径 (WAL → MemTable) 与读路径 (MemTable → SSTable) 的编排,
//! 以及后台 compaction / checkpoint.
//!
//! # 子模块
//!
//! - `wal`: WAL 追加与回放, 崩溃恢复; 不依赖其它子模块.
//! - `memtable`: 内存写缓冲 (SkipMap) + InternalKey 编码; 不依赖其它子模块.
//! - `filter`: Bloom 过滤器; 不依赖其它子模块.
//! - `cache`: Data Block LRU 缓存; 供 `sstable` / `db` 使用.
//! - `sstable`: SSTable 读写 (Data Block / Index / Footer / Bloom); 依赖 `memtable`
//!   (InternalKey)、`filter`、`cache`.
//! - `compaction`: 层级归并 + MANIFEST / VersionSet; 依赖 `sstable`、`memtable`.
//! - `db`: 总协调 (open / 写 / 读 / flush / 后台线程); 依赖 `wal`、`memtable`、
//!   `sstable`、`compaction`、`cache`.
//! - `checkpoint`: 目录一致性快照; 依赖 `db`.
//!
//! # 关键不变式
//!
//! - 写顺序: WAL append 先于 MemTable 写入 (`put` / `delete` / `write` 均如此).
//! - 读路径以 `encode_internal_key(key, max_seq, TypePut)` 构造 seek key, 只返回
//!   `seq <= max_seq` 的版本.
//!
//! 详见 `docs/modules/01-engine.md` (写路径) 与 `docs/modules/02-engine-storage.md` (SSTable).
pub mod cache;
pub mod checkpoint;
pub mod compaction;
pub mod db;
pub mod filter;
pub mod memtable;
pub mod sstable;
pub mod wal;
