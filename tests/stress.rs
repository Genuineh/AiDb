//! 并发写入 + compaction 压力测试.
//! @component aidb-engine
//!
//! 验证高频写入与 compaction 并发执行时数据完整性.
//! 这些测试默认 `#[ignore]` (耗时), 在 CI `test-slow` job 中用 `--ignored` 运行.

#[path = "engine/stress.rs"]
mod stress;
