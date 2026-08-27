//! Snapshot 测试共享配置
//! @component aidb-snapshot

use std::sync::Arc;

use aidb::config::Options;
use aidb::DB;
use tempfile::TempDir;

/// 小 MemTable, 便于 flush; `for_testing()` 已关闭后台 compaction.
pub fn snapshot_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 256;
    o.sync_wal = true;
    o
}

/// L0 trigger=2 (for_testing 默认), 小 memtable 便于凑满 L0.
pub fn compaction_opts() -> Options {
    let mut o = snapshot_opts();
    o.level0_compaction_trigger = 2;
    o
}

pub fn open_db(dir: &std::path::Path) -> Arc<DB> {
    DB::open(dir, snapshot_opts()).unwrap()
}

pub fn open_db_compaction(dir: &std::path::Path) -> Arc<DB> {
    DB::open(dir, compaction_opts()).unwrap()
}

pub fn temp_db() -> (TempDir, Arc<DB>) {
    let dir = tempfile::tempdir().unwrap();
    let db = open_db(dir.path());
    (dir, db)
}

pub fn temp_db_compaction() -> (TempDir, Arc<DB>) {
    let dir = tempfile::tempdir().unwrap();
    let db = open_db_compaction(dir.path());
    (dir, db)
}
