//! Reproduce production-scale memtable flush (64KB × ~1100 keys).

use aidb::config::Options;
use aidb::DB;
use tempfile::tempdir;

#[test]
fn test_large_memtable_flush_completes() {
    let dir = tempdir().unwrap();
    let mut opts = Options::default();
    opts.memtable_size = 64 * 1024 * 1024;
    opts.max_write_buffer_number = 2;
    opts.sync_wal = false;
    let db = DB::open(dir.path(), opts).unwrap();
    let value = vec![b'x'; 64 * 1024];
    for i in 0..1100u32 {
        let key = format!("0:{{stalltest}}r8:k:{i}");
        db.put(key.as_bytes(), &value).unwrap();
    }
    assert!(
        db.immutable_memtable_count() >= 1 || db.level0_sstable_count() >= 1,
        "expected freeze or flush"
    );
    db.flush().unwrap();
    assert_eq!(db.immutable_memtable_count(), 0);
    assert!(db.level0_sstable_count() >= 1, "L0 SST should exist after flush");
    db.close().unwrap();
}
