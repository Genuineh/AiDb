//! Integration tests for backup and recovery functionality.
//!
//! These tests validate end-to-end backup and recovery scenarios,
//! including large datasets, fault injection, and disaster recovery drills.

use aidb::backup::{BackupManager, LocalFileStorage, RecoveryManager, RetentionPolicy};
use aidb::{Options, DB};
use rand::Rng;
use std::sync::Arc;
use tempfile::TempDir;

/// Helper to generate random data
fn random_data(size: usize) -> Vec<u8> {
    let mut rng = rand::rng();
    (0..size).map(|_| rng.random()).collect()
}

#[test]
fn test_end_to_end_backup_recovery() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();
    let restore_dir = TempDir::new().unwrap();

    // Create database and write some data
    {
        let db = DB::open(db_dir.path(), Options::default()).unwrap();

        for i in 0..1000 {
            let key = format!("key-{:05}", i);
            let value = format!("value-{:05}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        db.flush().unwrap();
    }

    // Create backup
    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&db).unwrap();

    // Verify backup was created
    let backups = manager.list_backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].id, backup_id);

    // Restore to new location
    RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();

    // Verify all data was restored
    let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();

    for i in 0..1000 {
        let key = format!("key-{:05}", i);
        let expected_value = format!("value-{:05}", i);
        let actual_value = restored_db.get(key.as_bytes()).unwrap();
        assert_eq!(actual_value, Some(expected_value.into_bytes()));
    }
}

#[test]
fn test_large_dataset_backup_recovery() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();
    let restore_dir = TempDir::new().unwrap();

    // Create database with 10K records
    {
        let db = DB::open(db_dir.path(), Options::default()).unwrap();

        for i in 0..10_000 {
            let key = format!("key-{:08}", i);
            let value = random_data(256); // 256 bytes per value
            db.put(key.as_bytes(), &value).unwrap();

            // Flush periodically
            if i % 1000 == 0 {
                db.flush().unwrap();
            }
        }

        db.flush().unwrap();
    }

    // Create backup
    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&db).unwrap();

    // Verify backup info
    let backup_info = manager.get_backup_info(&backup_id).unwrap();
    assert!(backup_info.size > 0);
    assert!(!backup_info.sstable_files.is_empty());

    // Restore and verify a sample of keys
    RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();
    let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();

    // Verify first, middle, and last keys
    for i in [0, 5000, 9999] {
        let key = format!("key-{:08}", i);
        let value = restored_db.get(key.as_bytes()).unwrap();
        assert!(value.is_some());
        assert_eq!(value.unwrap().len(), 256);
    }
}

#[test]
fn test_multiple_backups_and_retention_policy() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();

    let db = DB::open(db_dir.path(), Options::default()).unwrap();

    // Create backup manager with custom retention policy
    let retention_policy = RetentionPolicy {
        min_count: 2,
        max_count: Some(3),
        min_age_seconds: 0,
        max_age_seconds: Some(3600),
    };

    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::with_retention_policy(storage, retention_policy);

    // Create multiple backups
    let mut backup_ids = Vec::new();
    for i in 0..5 {
        db.put(format!("key-{}", i).as_bytes(), b"value").unwrap();
        db.flush().unwrap();

        let id = manager.create_backup(&db).unwrap();
        backup_ids.push(id);

        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Apply retention policy
    manager.apply_retention_policy().unwrap();

    // Should keep max_count (3) backups
    let remaining_backups = manager.list_backups();
    assert!(remaining_backups.len() <= 3);
    assert!(remaining_backups.len() >= 2); // min_count
}

#[test]
fn test_backup_with_concurrent_writes() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();

    let db = Arc::new(DB::open(db_dir.path(), Options::default()).unwrap());

    // Write initial data
    for i in 0..100 {
        db.put(format!("key-{}", i).as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    // Create backup manager
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = Arc::new(BackupManager::new(storage));

    let db_clone = Arc::clone(&db);
    let manager_clone = Arc::clone(&manager);
    let backup_thread = std::thread::spawn(move || manager_clone.create_backup(&*db_clone));

    // Write more data concurrently
    for i in 100..200 {
        db.put(format!("key-{}", i).as_bytes(), b"value").unwrap();
    }

    // Wait for backup to complete
    let backup_id = backup_thread.join().unwrap().unwrap();

    // Backup should be consistent (contains at least the first 100 keys)
    let restore_dir = TempDir::new().unwrap();
    RecoveryManager::restore(&*manager, &backup_id, restore_dir.path()).unwrap();

    let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();
    for i in 0..100 {
        let key = format!("key-{}", i);
        assert!(restored_db.get(key.as_bytes()).unwrap().is_some());
    }
}

#[test]
fn test_backup_verification() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();

    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    db.put(b"key", b"value").unwrap();
    db.flush().unwrap();

    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&db).unwrap();

    // Verify backup integrity
    let result = RecoveryManager::verify_backup(&manager, &backup_id);
    assert!(result.is_ok());
}

#[test]
fn test_backup_with_deletes() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();
    let restore_dir = TempDir::new().unwrap();

    // Create database with some data, then delete some keys
    {
        let db = DB::open(db_dir.path(), Options::default()).unwrap();

        for i in 0..100 {
            db.put(format!("key-{}", i).as_bytes(), b"value").unwrap();
        }

        // Delete every other key
        for i in (0..100).step_by(2) {
            db.delete(format!("key-{}", i).as_bytes()).unwrap();
        }

        db.flush().unwrap();
    }

    // Create backup
    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&db).unwrap();

    // Restore and verify
    RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();
    let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();

    for i in 0..100 {
        let key = format!("key-{}", i);
        let value = restored_db.get(key.as_bytes()).unwrap();

        if i % 2 == 0 {
            // Deleted keys should not exist
            assert!(value.is_none());
        } else {
            // Non-deleted keys should exist
            assert!(value.is_some());
        }
    }
}

#[test]
fn test_backup_empty_database() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();
    let restore_dir = TempDir::new().unwrap();

    // Create empty database
    let db = DB::open(db_dir.path(), Options::default()).unwrap();

    // Backup empty database
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&db).unwrap();

    // Restore
    RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();

    // Verify restored database is also empty
    let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();
    assert!(restored_db.get(b"any-key").unwrap().is_none());
}

#[test]
fn test_backup_with_overwrites() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();
    let restore_dir = TempDir::new().unwrap();

    // Create database with overwritten keys
    {
        let db = DB::open(db_dir.path(), Options::default()).unwrap();

        // Write initial values
        for i in 0..50 {
            db.put(format!("key-{}", i).as_bytes(), b"value1").unwrap();
        }

        // Overwrite with new values
        for i in 0..50 {
            db.put(format!("key-{}", i).as_bytes(), b"value2").unwrap();
        }

        db.flush().unwrap();
    }

    // Backup and restore
    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&db).unwrap();

    RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();
    let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();

    // Verify all keys have the latest value
    for i in 0..50 {
        let key = format!("key-{}", i);
        let value = restored_db.get(key.as_bytes()).unwrap();
        assert_eq!(value, Some(b"value2".to_vec()));
    }
}

#[test]
fn test_disaster_recovery_drill() {
    let original_db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();
    let recovery_db_dir = TempDir::new().unwrap();

    // Simulate production database
    let original_db = DB::open(original_db_dir.path(), Options::default()).unwrap();

    for i in 0..500 {
        original_db
            .put(format!("key-{}", i).as_bytes(), format!("value-{}", i).as_bytes())
            .unwrap();
    }
    original_db.flush().unwrap();

    // Create backup
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backup_id = manager.create_backup(&original_db).unwrap();

    // Simulate disaster: drop original database
    drop(original_db);
    std::fs::remove_dir_all(&original_db_dir).unwrap();

    // Disaster recovery: restore from backup
    RecoveryManager::restore(&manager, &backup_id, recovery_db_dir.path()).unwrap();

    // Verify recovered database
    let recovered_db = DB::open(recovery_db_dir.path(), Options::default()).unwrap();

    for i in 0..500 {
        let key = format!("key-{}", i);
        let expected_value = format!("value-{}", i);
        let actual_value = recovered_db.get(key.as_bytes()).unwrap();
        assert_eq!(actual_value, Some(expected_value.into_bytes()));
    }
}

#[test]
fn test_backup_metadata_persistence() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();

    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    db.put(b"key", b"value").unwrap();

    // Create backup with first manager
    {
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);
        manager.create_backup(&db).unwrap();
    }

    // Create new manager and verify it can read existing metadata
    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);
    let backups = manager.list_backups();
    assert_eq!(backups.len(), 1);
}

#[test]
fn test_backup_with_description() {
    let db_dir = TempDir::new().unwrap();
    let backup_dir = TempDir::new().unwrap();

    let db = DB::open(db_dir.path(), Options::default()).unwrap();
    db.put(b"key", b"value").unwrap();

    let storage = LocalFileStorage::new(backup_dir.path());
    let manager = BackupManager::new(storage);

    let description = Some("Pre-migration backup".to_string());
    let backup_id = manager.create_backup_with_description(&db, description.clone()).unwrap();

    let backup_info = manager.get_backup_info(&backup_id).unwrap();
    assert_eq!(backup_info.description, description);
}
