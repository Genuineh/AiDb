//! key_exists: 完整存在性判定 (不物化 Value)
//! @component aidb-engine

#[test]
fn key_exists_sees_key_after_flush_to_sst() {
    let dir = tempfile::tempdir().unwrap();
    let db = aidb::DB::open(dir.path(), aidb::config::Options::for_testing()).unwrap();
    let big = vec![0u8; 256 * 1024];
    db.put(b"big", &big).unwrap(); // Task 1 时 put 仍为 Result<()>; Task 2 起变为 Result<bool> 后忽略 bool 即可
    db.flush().unwrap();
    assert!(db.key_exists(b"big").unwrap());
    assert!(!db.key_exists(b"missing").unwrap());
    assert_eq!(db.get(b"big").unwrap().as_deref(), Some(big.as_slice()));
}

#[test]
fn key_exists_false_after_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = aidb::DB::open(dir.path(), aidb::config::Options::for_testing()).unwrap();
    db.put(b"k", b"v").unwrap();
    db.delete(b"k").unwrap();
    assert!(!db.key_exists(b"k").unwrap());
}
