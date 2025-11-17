# Backup and Recovery Guide

This guide explains how to use AiDb's backup and recovery system to protect your data and recover from failures.

## Overview

AiDb provides a comprehensive backup and recovery system that allows you to:

- Create consistent point-in-time backups of your database
- Store backups locally or in cloud storage (S3, etc.)
- Restore databases from backups
- Manage backup retention policies
- Verify backup integrity

## Quick Start

### Creating a Backup

```rust
use aidb::{DB, Options};
use aidb::backup::{BackupManager, LocalFileStorage};

// Open your database
let db = DB::open("./my_db", Options::default())?;

// Create a backup manager with local storage
let storage = LocalFileStorage::new("./backups");
let backup_manager = BackupManager::new(storage);

// Create a backup
let backup_id = backup_manager.create_backup(&db)?;
println!("Backup created: {}", backup_id);
```

### Restoring from a Backup

```rust
use aidb::backup::{RecoveryManager, BackupManager, LocalFileStorage};

// Create backup manager
let storage = LocalFileStorage::new("./backups");
let backup_manager = BackupManager::new(storage);

// List available backups
let backups = backup_manager.list_backups();
for backup in &backups {
    println!("Backup: {} (created at: {})", backup.id, backup.created_at);
}

// Restore from the latest backup
if let Some(latest) = backups.last() {
    RecoveryManager::restore(&backup_manager, &latest.id, "./restored_db")?;
    println!("Database restored successfully");
}
```

## Storage Backends

### Local File Storage

The `LocalFileStorage` backend stores backups in a local directory:

```rust
use aidb::backup::LocalFileStorage;

let storage = LocalFileStorage::new("./backups");
```

### Custom Storage Backend

You can implement your own storage backend by implementing the `BackupStorage` trait:

```rust
use aidb::backup::BackupStorage;
use aidb::Result;
use std::path::Path;

struct MyCustomStorage {
    // Your storage implementation
}

impl BackupStorage for MyCustomStorage {
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        // Implement write logic
        Ok(())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>> {
        // Implement read logic
        Ok(vec![])
    }

    // Implement other required methods...
    fn exists(&self, path: &str) -> Result<bool> { Ok(false) }
    fn list(&self, prefix: &str) -> Result<Vec<String>> { Ok(vec![]) }
    fn delete(&self, path: &str) -> Result<()> { Ok(()) }
    fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> { Ok(()) }
    fn download_file(&self, remote_path: &str, local_path: &Path) -> Result<()> { Ok(()) }
}
```

## Retention Policies

Control how many backups are kept with retention policies:

```rust
use aidb::backup::{BackupManager, LocalFileStorage, RetentionPolicy};

let retention_policy = RetentionPolicy {
    min_count: 3,                    // Keep at least 3 backups
    max_count: Some(30),             // Keep at most 30 backups
    min_age_seconds: 24 * 3600,      // Don't delete backups less than 1 day old
    max_age_seconds: Some(30 * 86400), // Delete backups older than 30 days
};

let storage = LocalFileStorage::new("./backups");
let backup_manager = BackupManager::with_retention_policy(storage, retention_policy);

// The retention policy is automatically applied after each backup
let backup_id = backup_manager.create_backup(&db)?;

// Or manually apply it
backup_manager.apply_retention_policy()?;
```

## Backup Operations

### Creating Backups with Descriptions

Add descriptions to your backups for better organization:

```rust
let description = Some("Before schema migration".to_string());
let backup_id = backup_manager.create_backup_with_description(&db, description)?;
```

### Listing Backups

```rust
let backups = backup_manager.list_backups();
for backup in &backups {
    println!(
        "ID: {}, Created: {}, Size: {} bytes, Type: {:?}",
        backup.id, backup.created_at, backup.size, backup.backup_type
    );
    if let Some(desc) = &backup.description {
        println!("  Description: {}", desc);
    }
}
```

### Getting Backup Information

```rust
if let Some(info) = backup_manager.get_backup_info(&backup_id) {
    println!("Backup sequence: {}", info.sequence);
    println!("SSTable files: {:?}", info.sstable_files);
    println!("WAL files: {:?}", info.wal_files);
}
```

### Deleting Backups

```rust
backup_manager.delete_backup(&backup_id)?;
```

### Verifying Backup Integrity

```rust
use aidb::backup::RecoveryManager;

match RecoveryManager::verify_backup(&backup_manager, &backup_id) {
    Ok(_) => println!("Backup is valid"),
    Err(e) => eprintln!("Backup verification failed: {}", e),
}
```

## Recovery Operations

### Basic Recovery

```rust
use aidb::backup::RecoveryManager;

RecoveryManager::restore(&backup_manager, &backup_id, "./restored_db")?;
```

### Disaster Recovery Procedure

1. **Identify the latest valid backup:**
   ```rust
   let backups = backup_manager.list_backups();
   let latest = backups.last().expect("No backups found");
   ```

2. **Verify the backup:**
   ```rust
   RecoveryManager::verify_backup(&backup_manager, &latest.id)?;
   ```

3. **Restore to a new location:**
   ```rust
   RecoveryManager::restore(&backup_manager, &latest.id, "./recovered_db")?;
   ```

4. **Open and verify the restored database:**
   ```rust
   let recovered_db = DB::open("./recovered_db", Options::default())?;
   // Verify your data...
   ```

## Best Practices

### Regular Backups

Create backups on a regular schedule:

```rust
use std::time::Duration;
use std::thread;

loop {
    // Create backup
    match backup_manager.create_backup(&db) {
        Ok(id) => println!("Backup created: {}", id),
        Err(e) => eprintln!("Backup failed: {}", e),
    }

    // Wait for next backup interval (e.g., 1 hour)
    thread::sleep(Duration::from_secs(3600));
}
```

### Before Critical Operations

Always create a backup before:
- Schema changes
- Major data migrations
- Software upgrades
- Bulk data modifications

```rust
// Before migration
let backup_id = backup_manager
    .create_backup_with_description(&db, Some("Pre-migration".to_string()))?;

// Perform migration
perform_migration(&db)?;

// Verify migration was successful
if migration_successful() {
    // Keep the backup
    println!("Migration successful, backup {} retained", backup_id);
} else {
    // Restore from backup
    RecoveryManager::restore(&backup_manager, &backup_id, "./db")?;
}
```

### Off-site Backups

For production systems, store backups off-site:

```rust
// Implement S3Storage or use cloud storage
struct S3Storage {
    // AWS S3 configuration
}

impl BackupStorage for S3Storage {
    // Implement methods to upload/download from S3
}

let s3_storage = S3Storage::new(/* config */);
let backup_manager = BackupManager::new(s3_storage);
```

### Testing Recovery

Regularly test your recovery process:

```rust
#[test]
fn test_backup_recovery_drill() {
    // Create test database
    let db = create_test_db()?;
    
    // Backup
    let backup_id = backup_manager.create_backup(&db)?;
    
    // Restore to different location
    RecoveryManager::restore(&backup_manager, &backup_id, "./test_restore")?;
    
    // Verify data
    let restored_db = DB::open("./test_restore", Options::default())?;
    verify_data(&restored_db)?;
}
```

## Backup Architecture

### What Gets Backed Up

A full backup includes:

1. **SSTable Files**: All immutable sorted string table files
2. **WAL Files**: Write-ahead log files containing recent writes
3. **Metadata**: Backup information including sequence numbers and file lists

### Consistency Guarantees

- Backups are consistent point-in-time snapshots
- The database is automatically flushed before backup to ensure all data is on disk
- The backup sequence number ensures proper ordering of operations

### Storage Layout

Backups are organized as follows:

```
backups/
├── metadata.json                 # Global backup metadata
├── backup-1234567890/           # Individual backup
│   ├── sstables/                # SSTable files
│   │   ├── 000001.sst
│   │   └── 000002.sst
│   └── wal/                     # WAL files
│       └── wal-000001
└── backup-1234567891/           # Another backup
    └── ...
```

## Performance Considerations

### Backup Performance

- Backup speed depends on database size and storage backend
- For large databases (>100GB), consider:
  - Using high-performance storage (SSD, fast network)
  - Scheduling backups during off-peak hours
  - Monitoring backup duration and adjusting retention policies

### Recovery Performance

- Recovery time is proportional to backup size
- WAL replay can take additional time for backups with large WAL files
- For faster recovery, flush more frequently before backups

## Troubleshooting

### Backup Fails

If backup creation fails:

1. Check disk space on both database and backup locations
2. Verify write permissions
3. Check database logs for errors
4. Ensure database is not corrupted

### Recovery Fails

If recovery fails:

1. Verify backup integrity: `RecoveryManager::verify_backup()`
2. Check target directory is empty
3. Verify storage backend is accessible
4. Check logs for specific error messages

### Slow Backups

If backups are slow:

1. Monitor disk I/O during backup
2. Consider using compression (future enhancement)
3. Use faster storage for backups
4. Reduce backup frequency if acceptable

## Future Enhancements

Planned improvements to the backup system:

- **Incremental Backups**: Only backup changes since last backup
- **Compression**: Compress backup data to save space
- **Encryption**: Encrypt backups for security
- **S3 Backend**: Native AWS S3 storage backend
- **Backup Scheduling**: Built-in scheduler for automatic backups
- **Parallel Restore**: Faster recovery using parallel downloads

## API Reference

For detailed API documentation, see:
- `BackupManager` - Main interface for creating and managing backups
- `RecoveryManager` - Interface for restoring from backups
- `BackupStorage` - Trait for implementing custom storage backends
- `RetentionPolicy` - Configuration for backup retention
- `BackupInfo` - Metadata about individual backups
