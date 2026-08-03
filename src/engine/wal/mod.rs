//! WAL (Write-Ahead Log) 模块: 在数据落入 MemTable / SSTable 之前先持久化写操作,
//! 保证进程 crash 后可重放恢复.
//!
//! - `record` — Record 物理格式: `WalEntry` 编解码, `OpType` 语义
//! - `writer` — 追加 Record: 分片 / 32KB block padding / CRC32 / sync
//! - `reader` — 顺序读 Record: 分片重组 / 损坏容忍
//! - `manager` — WAL 生命周期: open / recover / append / rotate / cleanup (`LOCK` 单进程)
//!
//! 写顺序约束见 `docs/modules/01-engine.md`: WAL append 必须先于 MemTable 写入.

pub mod manager;
pub mod reader;
pub mod record;
pub mod writer;
