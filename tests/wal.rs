//! WAL 模块验收测试
//! @component aidb-engine
//!
//! 子模块:
//!   function — 功能测试 (编解码/写入/读取/恢复/清理)
//!   dataflow — 数据流向测试 (event 顺序)

mod common;
#[path = "modules/wal/dataflow.rs"]
pub mod dataflow;
#[path = "modules/wal/function.rs"]
pub mod function;
#[path = "modules/wal/write_batch_boundary.rs"]
pub mod write_batch_boundary;
