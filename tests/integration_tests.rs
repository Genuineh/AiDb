// End-to-End Integration Tests for AiDb
// These tests verify complete CRUD flows, bulk operations, and various access patterns

use aidb::{Options, DB};
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

/// Test complete CRUD flow
#[test]
fn test_e2e_complete_crud() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Create
    db.put(b"user:1", b"Alice").unwrap();
    db.put(b"user:2", b"Bob").unwrap();
    db.put(b"user:3", b"Charlie").unwrap();

    // Read
    assert_eq!(db.get(b"user:1").unwrap(), Some(b"Alice".to_vec()));
    assert_eq!(db.get(b"user:2").unwrap(), Some(b"Bob".to_vec()));
    assert_eq!(db.get(b"user:3").unwrap(), Some(b"Charlie".to_vec()));

    // Update
    db.put(b"user:2", b"Bob_Updated").unwrap();
    assert_eq!(db.get(b"user:2").unwrap(), Some(b"Bob_Updated".to_vec()));

    // Delete
    db.delete(b"user:1").unwrap();
    assert_eq!(db.get(b"user:1").unwrap(), None);

    // Verify remaining data
    assert_eq!(db.get(b"user:2").unwrap(), Some(b"Bob_Updated".to_vec()));
    assert_eq!(db.get(b"user:3").unwrap(), Some(b"Charlie".to_vec()));
}

/// Test large data write (100k+ records)
#[test]
fn test_e2e_large_data_write() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    let record_count = 100_000;

    // Write 100k records
    for i in 0..record_count {
        let key = format!("key_{:08}", i);
        let value = format!("value_{:08}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Verify sample records
    for i in (0..record_count).step_by(10_000) {
        let key = format!("key_{:08}", i);
        let expected_value = format!("value_{:08}", i);
        assert_eq!(
            db.get(key.as_bytes()).unwrap(),
            Some(expected_value.as_bytes().to_vec())
        );
    }

    // Verify first and last
    assert_eq!(
        db.get(b"key_00000000").unwrap(),
        Some(b"value_00000000".to_vec())
    );
    assert_eq!(
        db.get(format!("key_{:08}", record_count - 1).as_bytes())
            .unwrap(),
        Some(format!("value_{:08}", record_count - 1).as_bytes().to_vec())
    );
}

/// Test sequential write + random read pattern
#[test]
fn test_e2e_sequential_write_random_read() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    let record_count = 10_000;

    // Sequential writes
    for i in 0..record_count {
        let key = format!("seq_key_{:06}", i);
        let value = format!("seq_value_{:06}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Random reads
    let random_indices = vec![100, 5000, 9999, 42, 7777, 1234, 8888, 0, 9500, 500];

    for idx in random_indices {
        let key = format!("seq_key_{:06}", idx);
        let expected_value = format!("seq_value_{:06}", idx);
        assert_eq!(
            db.get(key.as_bytes()).unwrap(),
            Some(expected_value.as_bytes().to_vec())
        );
    }
}

/// Test random write + random read pattern
#[test]
fn test_e2e_random_write_random_read() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Random writes with non-sequential keys
    let keys = vec![
        "zebra", "apple", "mango", "banana", "cherry", "date", "fig", "grape", "kiwi",
        "lemon",
    ];

    for (i, key) in keys.iter().enumerate() {
        let value = format!("value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Random reads
    assert_eq!(db.get(b"apple").unwrap(), Some(b"value_1".to_vec()));
    assert_eq!(db.get(b"zebra").unwrap(), Some(b"value_0".to_vec()));
    assert_eq!(db.get(b"kiwi").unwrap(), Some(b"value_8".to_vec()));
    assert_eq!(db.get(b"date").unwrap(), Some(b"value_5".to_vec()));

    // Non-existent key
    assert_eq!(db.get(b"orange").unwrap(), None);
}

/// Test overwrite scenarios
#[test]
fn test_e2e_overwrite_patterns() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    let key = b"counter";

    // Multiple overwrites
    for i in 0..100 {
        let value = format!("count_{}", i);
        db.put(key, value.as_bytes()).unwrap();
    }

    // Should have the latest value
    assert_eq!(db.get(key).unwrap(), Some(b"count_99".to_vec()));

    // Overwrite with delete
    db.delete(key).unwrap();
    assert_eq!(db.get(key).unwrap(), None);

    // Write after delete
    db.put(key, b"resurrected").unwrap();
    assert_eq!(db.get(key).unwrap(), Some(b"resurrected".to_vec()));
}

/// Test with large values (1MB+)
#[test]
fn test_e2e_large_values() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Create 1MB value
    let large_value: Vec<u8> = (0..1_048_576).map(|i| (i % 256) as u8).collect();

    db.put(b"large_key", &large_value).unwrap();

    let retrieved = db.get(b"large_key").unwrap().unwrap();
    assert_eq!(retrieved.len(), 1_048_576);
    assert_eq!(retrieved, large_value);
}

/// Test data persistence across reopens
#[test]
fn test_e2e_persistence() {
    let dir = TempDir::new().unwrap();

    // First session
    {
        let db = DB::open(dir.path(), Options::default()).unwrap();
        for i in 0..1000 {
            let key = format!("persist_key_{}", i);
            let value = format!("persist_value_{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        // db drops here and flushes
    }

    // Second session - verify data persists
    {
        let db = DB::open(dir.path(), Options::default()).unwrap();
        for i in 0..1000 {
            let key = format!("persist_key_{}", i);
            let expected_value = format!("persist_value_{}", i);
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                Some(expected_value.as_bytes().to_vec())
            );
        }
    }
}

/// Test mixed operations (put, get, delete)
#[test]
fn test_e2e_mixed_operations() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Interleaved operations
    db.put(b"key1", b"value1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

    db.put(b"key2", b"value2").unwrap();
    db.delete(b"key1").unwrap();

    assert_eq!(db.get(b"key1").unwrap(), None);
    assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));

    db.put(b"key3", b"value3").unwrap();
    db.put(b"key2", b"value2_updated").unwrap();

    assert_eq!(db.get(b"key2").unwrap(), Some(b"value2_updated".to_vec()));
    assert_eq!(db.get(b"key3").unwrap(), Some(b"value3".to_vec()));

    db.delete(b"key2").unwrap();
    db.delete(b"key3").unwrap();

    assert_eq!(db.get(b"key2").unwrap(), None);
    assert_eq!(db.get(b"key3").unwrap(), None);
}

/// Test empty database operations
#[test]
fn test_e2e_empty_database() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Read from empty database
    assert_eq!(db.get(b"nonexistent").unwrap(), None);

    // Delete from empty database (should not error)
    db.delete(b"nonexistent").unwrap();

    // Add data, then remove it
    db.put(b"temp", b"data").unwrap();
    db.delete(b"temp").unwrap();
    assert_eq!(db.get(b"temp").unwrap(), None);
}

/// Test database with many deletes
#[test]
fn test_e2e_many_deletes() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Write 1000 records
    for i in 0..1000 {
        let key = format!("del_key_{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    // Delete every other record
    for i in (0..1000).step_by(2) {
        let key = format!("del_key_{}", i);
        db.delete(key.as_bytes()).unwrap();
    }

    // Verify deletions
    for i in 0..1000 {
        let key = format!("del_key_{}", i);
        let result = db.get(key.as_bytes()).unwrap();

        if i % 2 == 0 {
            assert_eq!(result, None, "Key should be deleted: {}", i);
        } else {
            assert_eq!(result, Some(b"value".to_vec()), "Key should exist: {}", i);
        }
    }
}

/// Test automatic flush behavior with large dataset
#[test]
fn test_e2e_auto_flush_behavior() {
    let dir = TempDir::new().unwrap();
    
    // Configure smaller memtable to trigger flushes
    let mut options = Options::default();
    options.memtable_size = 1024 * 256; // 256KB memtable

    let db = DB::open(dir.path(), options).unwrap();

    // Write enough data to trigger multiple flushes
    for i in 0..10_000 {
        let key = format!("flush_key_{:06}", i);
        let value = vec![b'x'; 100]; // 100 byte values
        db.put(key.as_bytes(), &value).unwrap();
    }

    // Verify all data is still accessible
    for i in (0..10_000).step_by(500) {
        let key = format!("flush_key_{:06}", i);
        let result = db.get(key.as_bytes()).unwrap();
        assert!(result.is_some(), "Key should exist: {}", key);
        assert_eq!(result.unwrap().len(), 100);
    }
}

/// Test concurrent reads (should be safe)
#[test]
fn test_e2e_concurrent_reads() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Prepare data
    for i in 0..1000 {
        let key = format!("concurrent_key_{}", i);
        let value = format!("concurrent_value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Spawn multiple reader threads
    let mut handles = vec![];
    for thread_id in 0..10 {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in 0..1000 {
                let key = format!("concurrent_key_{}", i);
                let expected = format!("concurrent_value_{}", i);
                let result = db_clone.get(key.as_bytes()).unwrap();
                assert_eq!(
                    result,
                    Some(expected.as_bytes().to_vec()),
                    "Thread {} failed to read key {}",
                    thread_id,
                    key
                );
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test key edge cases
#[test]
fn test_e2e_key_edge_cases() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Empty key (if allowed by implementation)
    // db.put(b"", b"empty_key_value").unwrap();
    // assert_eq!(db.get(b"").unwrap(), Some(b"empty_key_value".to_vec()));

    // Very long key
    let long_key = vec![b'k'; 10_000];
    db.put(&long_key, b"long_key_value").unwrap();
    assert_eq!(db.get(&long_key).unwrap(), Some(b"long_key_value".to_vec()));

    // Binary data in key
    let binary_key = vec![0u8, 1, 2, 255, 254, 253];
    db.put(&binary_key, b"binary_key_value").unwrap();
    assert_eq!(
        db.get(&binary_key).unwrap(),
        Some(b"binary_key_value".to_vec())
    );

    // Special characters in key
    let null_key = vec![b'k', b'e', b'y', 0u8, b'w', b'i', b't', b'h', 0u8, b'n', b'u', b'l', b'l', b's'];
    db.put(&null_key, b"null_key_value").unwrap();
    assert_eq!(
        db.get(&null_key).unwrap(),
        Some(b"null_key_value".to_vec())
    );
}

/// Test value edge cases
#[test]
fn test_e2e_value_edge_cases() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Empty value
    db.put(b"empty_value", b"").unwrap();
    assert_eq!(db.get(b"empty_value").unwrap(), Some(vec![]));

    // Binary value
    let binary_value = vec![0u8, 1, 2, 255, 254, 253];
    db.put(b"binary_value", &binary_value).unwrap();
    assert_eq!(db.get(b"binary_value").unwrap(), Some(binary_value));

    // Value with nulls
    let null_value = vec![b'v', b'a', b'l', b'u', b'e', 0u8, b'w', b'i', b't', b'h', 0u8, b'n', b'u', b'l', b'l', b's'];
    db.put(b"null_value", &null_value).unwrap();
    assert_eq!(db.get(b"null_value").unwrap(), Some(null_value));
}
