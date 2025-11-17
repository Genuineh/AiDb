//! Simple example demonstrating backup and recovery functionality.

use aidb::backup::{BackupManager, LocalFileStorage, RecoveryManager};
use aidb::{Options, DB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== AiDb Backup and Recovery Example ===\n");

    // Create a database and write some data
    println!("1. Creating database and writing data...");
    let db = DB::open("./example_db", Options::default())?;

    for i in 0..100 {
        let key = format!("key-{:03}", i);
        let value = format!("value-{:03}", i);
        db.put(key.as_bytes(), value.as_bytes())?;
    }
    db.flush()?;
    println!("   Written 100 key-value pairs\n");

    // Create a backup
    println!("2. Creating backup...");
    let storage = LocalFileStorage::new("./example_backups");
    let backup_manager = BackupManager::new(storage);

    let backup_id = backup_manager
        .create_backup_with_description(&db, Some("Example backup".to_string()))?;
    println!("   Backup created: {}\n", backup_id);

    // List backups
    println!("3. Listing available backups:");
    let backups = backup_manager.list_backups();
    for backup in &backups {
        println!("   - ID: {}", backup.id);
        println!("     Created: {}", backup.created_at);
        println!("     Size: {} bytes", backup.size);
        println!("     Type: {:?}", backup.backup_type);
        if let Some(desc) = &backup.description {
            println!("     Description: {}", desc);
        }
        println!();
    }

    // Simulate disaster: close the database
    drop(db);
    println!("4. Database closed (simulating disaster)\n");

    // Restore from backup
    println!("5. Restoring from backup...");
    RecoveryManager::restore(&backup_manager, &backup_id, std::path::Path::new("./example_restored"))?;
    println!("   Database restored to ./example_restored\n");

    // Open restored database and verify data
    println!("6. Verifying restored data...");
    let restored_db = DB::open("./example_restored", Options::default())?;

    let mut verified_count = 0;
    for i in 0..100 {
        let key = format!("key-{:03}", i);
        let expected_value = format!("value-{:03}", i);

        if let Some(value) = restored_db.get(key.as_bytes())? {
            if value == expected_value.as_bytes() {
                verified_count += 1;
            }
        }
    }

    println!("   Verified {}/100 key-value pairs", verified_count);
    println!("\n=== Example completed successfully! ===");

    // Cleanup
    drop(restored_db);
    std::fs::remove_dir_all("./example_db").ok();
    std::fs::remove_dir_all("./example_backups").ok();
    std::fs::remove_dir_all("./example_restored").ok();

    Ok(())
}
