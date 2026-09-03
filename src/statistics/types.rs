//! 统计模块强类型枚举与常数定义.

/// DB 基础读写操作枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DbOp {
    Put = 0,
    Delete = 1,
    WriteBatch = 2,
    WriteBatchNoWal = 3,
    Get = 4,
    Snapshot = 5,
    StallStop = 6,
    StallSlowdown = 7,
}
pub const NUM_DB_OPS: usize = 8;

impl DbOp {
    pub const ALL: [DbOp; NUM_DB_OPS] = [
        DbOp::Put,
        DbOp::Delete,
        DbOp::WriteBatch,
        DbOp::WriteBatchNoWal,
        DbOp::Get,
        DbOp::Snapshot,
        DbOp::StallStop,
        DbOp::StallSlowdown,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            DbOp::Put => "put",
            DbOp::Delete => "delete",
            DbOp::WriteBatch => "write_batch",
            DbOp::WriteBatchNoWal => "write_batch_no_wal",
            DbOp::Get => "get",
            DbOp::Snapshot => "snapshot",
            DbOp::StallStop => "stall_stop",
            DbOp::StallSlowdown => "stall_slowdown",
        }
    }
}

/// Compaction 阶段枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CompactionPhase {
    Pick = 0,
    Run = 1,
    Apply = 2,
}
pub const NUM_COMPACTION_PHASES: usize = 3;

impl CompactionPhase {
    pub const ALL: [CompactionPhase; NUM_COMPACTION_PHASES] = [
        CompactionPhase::Pick,
        CompactionPhase::Run,
        CompactionPhase::Apply,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CompactionPhase::Pick => "pick",
            CompactionPhase::Run => "run",
            CompactionPhase::Apply => "apply",
        }
    }
}

/// 备份与恢复操作枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BackupOp {
    Create = 0,
    Delete = 1,
    Restore = 2,
}
pub const NUM_BACKUP_OPS: usize = 3;

impl BackupOp {
    pub const ALL: [BackupOp; NUM_BACKUP_OPS] =
        [BackupOp::Create, BackupOp::Delete, BackupOp::Restore];

    pub fn as_str(&self) -> &'static str {
        match self {
            BackupOp::Create => "create",
            BackupOp::Delete => "delete",
            BackupOp::Restore => "restore",
        }
    }
}

/// Write stall 分类枚举 (Phase 2 预留)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WriteStallKind {
    MemTableSlowdown = 0,
    MemTableStop = 1,
    L0FilesSlowdown = 2,
    L0FilesStop = 3,
    LevelSizeSlowdown = 4,
    LevelSizeStop = 5,
}
pub const NUM_WRITE_STALL_KINDS: usize = 6;
