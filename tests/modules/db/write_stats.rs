//! put / write / write_without_wal 返回 insert 摘要, 与 total_key_count 同源
//! @component aidb-engine

#[test]
fn put_returns_false_when_key_only_in_sst() {
    let dir = tempfile::tempdir().unwrap();
    let db = aidb::DB::open(dir.path(), aidb::config::Options::for_testing()).unwrap();
    assert!(db.put(b"k", b"v1").unwrap());
    db.flush().unwrap();
    assert!(!db.put(b"k", b"v2").unwrap());
}

#[test]
fn write_batch_overlay_put_del_put_net_stats() {
    let dir = tempfile::tempdir().unwrap();
    let db = aidb::DB::open(dir.path(), aidb::config::Options::for_testing()).unwrap();
    let mut wb = aidb::WriteBatch::new();
    wb.put(b"k", b"a");
    wb.delete(b"k");
    wb.put(b"k", b"b");
    let stats = db.write(&wb).unwrap();
    // 空库 Put→Del→Put: insert, delete existed, insert again → inserted=2, deleted=1
    // (与 apply effects [true,true,true] / aikv overlay 计数一致; net delta = +1)
    assert_eq!(stats.inserted, 2);
    assert_eq!(stats.deleted, 1);
    assert!(db.key_exists(b"k").unwrap());
}

#[test]
fn write_without_wal_overlay_cover_sst_key_inserted_zero() {
    let dir = tempfile::tempdir().unwrap();
    let db = aidb::DB::open(dir.path(), aidb::config::Options::for_testing()).unwrap();
    assert!(db.put(b"k", b"v1").unwrap());
    db.flush().unwrap();
    let mut wb = aidb::WriteBatch::new();
    wb.put(b"k", b"v2");
    let stats = db.write_without_wal(&wb).unwrap();
    // stats 与 total_key_count 同源: 覆盖 SST 已有 key 不得算 insert
    assert_eq!(stats.inserted, 0);
    assert_eq!(stats.deleted, 0);
    assert!(db.key_exists(b"k").unwrap());
}
