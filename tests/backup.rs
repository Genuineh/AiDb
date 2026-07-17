//! @component aidb-engine
//! 备份模块端到端集成测试。
//! 覆盖跨模块场景：空数据库备份、完整 roundtrip、并发写入一致性。

#[path = "engine/backup.rs"]
mod backup;
