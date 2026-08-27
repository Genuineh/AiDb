//! compaction 后空 value 不应被误删.
//! @component aidb-engine

use aidb::config::Options;
use aidb::DB;
use std::sync::Arc;
use tempfile::tempdir;

/// 验证 Compaction 压实后 0 字节空 Value 的记录不被误剔除
#[test]
fn regression_empty_value_not_removed_by_compaction() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.memtable_size = 256;
    opts.level0_compaction_trigger = 2;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());
    db.put(b"empty", b"").unwrap();
    db.flush().unwrap();
    db.put(b"pad", b"x").unwrap();
    db.flush().unwrap();
    db.drain_compactions().unwrap();
    assert_eq!(db.get(b"empty").unwrap(), Some(vec![]));
    db.close().unwrap();
}
