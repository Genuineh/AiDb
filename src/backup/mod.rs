//! 备份/恢复模块: BackupStorage trait, LocalFileStorage, BackupManager, RecoveryManager.
//!
//! Phase 18 交付, 基于 Checkpoint::create 构建。
//! 设计规格见 WiQunTools/docs/aidb-inventory/13-backup-bench.md

mod manager;
mod recovery;
mod storage;
mod util;

pub use manager::*;
pub use recovery::*;
pub use storage::*;
