//! Test to reproduce the key exists issue from AiKv
//!
//! This test simulates how AiKv might use AiDb for key existence checks

use aidb::{Options, DB};
use tempfile::TempDir;

#[test]
fn test_key_exists_after_delete() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Set a key
    db.put(b"testkey", b"testvalue").unwrap();

    // Verify it exists
    assert!(db.get(b"testkey").unwrap().is_some(), "Key should exist after put");

    // Delete the key
    db.delete(b"testkey").unwrap();

    // Verify it does NOT exist (this is what AiKv's TestKeyIsExistsAsync is checking)
    let result = db.get(b"testkey").unwrap();
    assert!(result.is_none(), "Key should NOT exist after delete, but got: {:?}", result);
}

#[test]
fn test_key_exists_after_delete_with_flush() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { use_wal: false, memtable_size: 1024, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Set a key and flush
    db.put(b"testkey", b"testvalue").unwrap();
    db.flush().unwrap();

    // Verify it exists in SSTable
    assert!(db.get(b"testkey").unwrap().is_some(), "Key should exist after flush");

    // Delete the key
    db.delete(b"testkey").unwrap();

    // Immediately check - should not exist even before flush
    let result = db.get(b"testkey").unwrap();
    assert!(
        result.is_none(),
        "Key should NOT exist after delete (before flush), but got: {:?}",
        result
    );

    // Flush the delete
    db.flush().unwrap();

    // Check again after flush - should still not exist
    let result = db.get(b"testkey").unwrap();
    assert!(
        result.is_none(),
        "Key should NOT exist after delete and flush, but got: {:?}",
        result
    );
}

#[test]
fn test_key_exists_with_empty_value() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Try to set an empty value (which is treated as tombstone)
    db.put(b"empty_key", b"").unwrap();

    // Empty value is treated as deletion, so key should not exist
    let result = db.get(b"empty_key").unwrap();
    assert!(
        result.is_none(),
        "Empty value should be treated as tombstone, but got: {:?}",
        result
    );
}

#[test]
fn test_exists_check_pattern() {
    // This simulates a common pattern in Redis-like systems:
    // exists(key) -> delete(key) -> exists(key) should return false

    let temp_dir = TempDir::new().unwrap();
    let db = DB::open(temp_dir.path(), Options::default()).unwrap();

    let key = b"mykey";
    let value = b"myvalue";

    // Set
    db.put(key, value).unwrap();

    // Exists check (returns Some means exists)
    assert!(db.get(key).unwrap().is_some());

    // Delete
    db.delete(key).unwrap();

    // Exists check should return None (does not exist)
    assert!(db.get(key).unwrap().is_none());
}

#[test]
fn test_reopen_db_after_delete() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // First session: create and delete
    {
        let db = DB::open(db_path, Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.delete(b"key1").unwrap();
        db.close().unwrap();
    }

    // Reopen database
    {
        let db = DB::open(db_path, Options::default()).unwrap();

        // Key should still not exist after reopening
        let result = db.get(b"key1").unwrap();
        assert!(
            result.is_none(),
            "Deleted key should not exist after reopen, but got: {:?}",
            result
        );
    }
}

#[test]
fn test_multiple_deletes() {
    let temp_dir = TempDir::new().unwrap();
    let db = DB::open(temp_dir.path(), Options::default()).unwrap();

    // Set a key
    db.put(b"key", b"value").unwrap();
    assert!(db.get(b"key").unwrap().is_some());

    // Delete it multiple times (should be idempotent)
    db.delete(b"key").unwrap();
    assert!(db.get(b"key").unwrap().is_none());

    db.delete(b"key").unwrap();
    assert!(db.get(b"key").unwrap().is_none());

    db.delete(b"key").unwrap();
    assert!(db.get(b"key").unwrap().is_none());
}
