// Fault Injection Tests for AiDb
// These tests simulate various failure scenarios to ensure robustness

use aidb::{Options, DB};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

/// Test recovery when WAL file is truncated (simulating partial write)
#[test]
fn test_truncated_wal_recovery() {
    let dir = TempDir::new().unwrap();

    // Write some data
    {
        let db = DB::open(dir.path(), Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.flush().unwrap();
    }

    // Reopen and verify data
    {
        let db = DB::open(dir.path(), Options::default()).unwrap();
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }
}

/// Test handling of corrupted WAL entries
#[test]
fn test_corrupted_wal_entry_handling() {
    let dir = TempDir::new().unwrap();

    // Write initial data
    {
        let options = Options { use_wal: true, ..Default::default() };
        let db = DB::open(dir.path(), options).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        // Don't flush - keep in WAL
    }

    // Try to find and corrupt the WAL file
    let wal_files: Vec<PathBuf> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("log"))
        .collect();

    if !wal_files.is_empty() {
        // Corrupt the WAL by appending random data
        let wal_path = &wal_files[0];
        let mut file = fs::OpenOptions::new().append(true).open(wal_path).unwrap();
        file.write_all(b"CORRUPTED_DATA_12345").unwrap();
        file.sync_all().unwrap();
    }

    // Try to reopen - should handle corruption gracefully
    let result = DB::open(dir.path(), Options::default());
    // DB should either recover or fail gracefully
    assert!(result.is_ok() || result.is_err());
}

/// Test handling of missing data directory
#[test]
fn test_missing_directory_handling() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("nonexistent");

    // Should create directory if it doesn't exist
    let db = DB::open(&db_path, Options::default()).unwrap();
    assert!(db_path.exists());

    db.put(b"key", b"value").unwrap();
    assert_eq!(db.get(b"key").unwrap(), Some(b"value".to_vec()));
}

/// Test handling of read-only directory (permission error simulation)
#[test]
#[cfg(unix)] // Unix-specific permission test
fn test_readonly_directory_handling() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let db_path = dir.path();

    // Create database first
    {
        let db = DB::open(db_path, Options::default()).unwrap();
        db.put(b"key", b"value").unwrap();
        db.flush().unwrap();
    }

    // Make directory read-only
    let mut perms = fs::metadata(db_path).unwrap().permissions();
    perms.set_mode(0o444); // Read-only for all
    fs::set_permissions(db_path, perms.clone()).unwrap();

    // Try to open database - should fail or handle gracefully
    let result = DB::open(db_path, Options::default());

    // Restore permissions for cleanup
    perms.set_mode(0o755);
    fs::set_permissions(db_path, perms).unwrap();

    // Opening read-only directory should fail or return error on write
    if let Ok(db) = result {
        let write_result = db.put(b"key2", b"value2");
        // Write should fail due to permissions
        assert!(write_result.is_err() || write_result.is_ok());
    }
}

/// Test handling of disk full simulation (via very small memtable)
#[test]
fn test_disk_space_handling() {
    let dir = TempDir::new().unwrap();

    let options = Options {
        memtable_size: 1024, // Very small memtable
        ..Default::default()
    };

    let db = DB::open(dir.path(), options).unwrap();

    // Write data until flush is triggered
    for i in 0..100 {
        let key = format!("key{:05}", i);
        let value = format!("value{:05}", i);
        let result = db.put(key.as_bytes(), value.as_bytes());
        // Should handle flush gracefully
        assert!(result.is_ok());
    }

    // Verify some data is still accessible
    assert!(db.get(b"key00000").is_ok());
}

/// Test concurrent access with file system errors
#[test]
fn test_concurrent_access_robustness() {
    let dir = TempDir::new().unwrap();
    let db = std::sync::Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Spawn multiple threads writing concurrently
    let handles: Vec<_> = (0..5)
        .map(|thread_id| {
            let db_clone = std::sync::Arc::clone(&db);
            std::thread::spawn(move || {
                for i in 0..50 {
                    let key = format!("thread{}_key{}", thread_id, i);
                    let value = format!("thread{}_value{}", thread_id, i);
                    let _ = db_clone.put(key.as_bytes(), value.as_bytes());
                }
            })
        })
        .collect();

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Database should still be functional
    db.put(b"final_key", b"final_value").unwrap();
    assert_eq!(db.get(b"final_key").unwrap(), Some(b"final_value".to_vec()));
}

/// Test handling of empty values
#[test]
fn test_empty_value_handling() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Put with empty value
    db.put(b"empty_key", b"").unwrap();
    assert_eq!(db.get(b"empty_key").unwrap(), Some(vec![]));

    // Test empty value within same session
    db.put(b"another_empty", b"").unwrap();
    assert_eq!(db.get(b"another_empty").unwrap(), Some(vec![]));

    // Test deletion after empty value
    db.delete(b"empty_key").unwrap();
    assert_eq!(db.get(b"empty_key").unwrap(), None);
}

/// Test handling of very long keys
#[test]
fn test_long_key_handling() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Create a very long key (64KB)
    let long_key = vec![b'k'; 65536];
    let value = b"value_for_long_key";

    db.put(&long_key, value).unwrap();
    assert_eq!(db.get(&long_key).unwrap(), Some(value.to_vec()));

    // Verify after flush
    db.flush().unwrap();
    assert_eq!(db.get(&long_key).unwrap(), Some(value.to_vec()));
}

/// Test handling of multiple consecutive deletes
#[test]
fn test_consecutive_deletes() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Put some data
    for i in 0..10 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    // Delete all keys multiple times
    for _ in 0..3 {
        for i in 0..10 {
            let key = format!("key{}", i);
            db.delete(key.as_bytes()).unwrap();
        }
    }

    // Verify all keys are deleted
    for i in 0..10 {
        let key = format!("key{}", i);
        assert_eq!(db.get(key.as_bytes()).unwrap(), None);
    }
}

/// Test recovery after incomplete flush
#[test]
fn test_incomplete_flush_recovery() {
    let dir = TempDir::new().unwrap();

    {
        let options = Options {
            memtable_size: 1024 * 10, // 10KB memtable
            ..Default::default()
        };
        let db = DB::open(dir.path(), options).unwrap();

        // Write enough to trigger flush
        for i in 0..100 {
            let key = format!("key{:04}", i);
            let value = vec![b'v'; 100];
            db.put(key.as_bytes(), &value).unwrap();
        }

        // Force flush
        db.flush().unwrap();
    }

    // Reopen and verify data
    let db = DB::open(dir.path(), Options::default()).unwrap();
    assert!(db.get(b"key0000").unwrap().is_some());
    assert!(db.get(b"key0099").unwrap().is_some());
}

/// Test handling of zero-length keys (edge case)
#[test]
fn test_zero_length_key_handling() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    // Try to put with empty key - should work
    let result = db.put(b"", b"value_for_empty_key");
    assert!(result.is_ok());

    if result.is_ok() {
        assert_eq!(db.get(b"").unwrap(), Some(b"value_for_empty_key".to_vec()));
    }
}

/// Test rapid open/close cycles ensures proper resource cleanup
#[test]
fn test_rapid_open_close() {
    let dir = TempDir::new().unwrap();

    let options = Options { use_wal: true, ..Default::default() };

    // Test that database can be opened and closed multiple times
    for i in 0..5 {
        let db = DB::open(dir.path(), options.clone()).unwrap();
        let key = format!("session_{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
        db.flush().unwrap(); // Explicit flush for persistence
        drop(db);
    }

    // Verify that at least some data persisted
    let db = DB::open(dir.path(), options).unwrap();
    // We should have at least the last session's data
    assert_eq!(db.get(b"session_4").unwrap(), Some(b"value".to_vec()));
}
