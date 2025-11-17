//! Database recovery from backups.

use crate::backup::manager::BackupManager;
use crate::backup::metadata::BackupId;
use crate::backup::storage::BackupStorage;
use crate::{Error, Result};
use std::path::Path;

/// Manager for restoring database from backups.
pub struct RecoveryManager;

impl RecoveryManager {
    /// Restore a database from a backup.
    ///
    /// This will:
    /// 1. Download all SSTable files from the backup
    /// 2. Download all WAL files from the backup
    /// 3. Place them in the target database directory
    /// 4. The database can then be opened normally and will replay the WAL
    ///
    /// # Arguments
    ///
    /// * `backup_manager` - The backup manager containing the backup
    /// * `backup_id` - ID of the backup to restore
    /// * `target_path` - Path where the database should be restored
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The backup does not exist
    /// - The target directory already exists and is not empty
    /// - Download fails
    /// - File operations fail
    pub fn restore<S: BackupStorage>(
        backup_manager: &BackupManager<S>,
        backup_id: &BackupId,
        target_path: &Path,
    ) -> Result<()> {
        // Get backup info
        let backup_info = backup_manager.get_backup_info(backup_id).ok_or_else(|| {
            Error::NotFound(format!("Backup not found: {}", backup_id))
        })?;

        // Create target directory
        if target_path.exists() {
            // Check if directory is empty
            if let Ok(mut entries) = std::fs::read_dir(target_path) {
                if entries.next().is_some() {
                    return Err(Error::AlreadyExists(format!(
                        "Target directory is not empty: {:?}",
                        target_path
                    )));
                }
            }
        } else {
            std::fs::create_dir_all(target_path)?;
        }

        log::info!("Restoring backup {} to {:?}", backup_id, target_path);

        // Restore SSTable files
        Self::restore_sstables(backup_manager, backup_id, &backup_info.sstable_files, target_path)?;

        // Restore WAL files
        Self::restore_wal_files(backup_manager, backup_id, &backup_info.wal_files, target_path)?;

        log::info!("Backup restored successfully");
        Ok(())
    }

    /// Verify the integrity of a backup.
    ///
    /// This checks that all files referenced in the backup metadata exist
    /// and can be read from storage.
    pub fn verify_backup<S: BackupStorage>(
        backup_manager: &BackupManager<S>,
        backup_id: &BackupId,
    ) -> Result<()> {
        let backup_info = backup_manager.get_backup_info(backup_id).ok_or_else(|| {
            Error::NotFound(format!("Backup not found: {}", backup_id))
        })?;

        let storage = backup_manager.storage();

        // Verify SSTable files
        for sstable_file in &backup_info.sstable_files {
            let path = format!("{}/sstables/{}", backup_id, sstable_file);
            if !storage.exists(&path)? {
                return Err(Error::Corruption(format!("Missing SSTable file: {}", path)));
            }
        }

        // Verify WAL files
        for wal_file in &backup_info.wal_files {
            let path = format!("{}/wal/{}", backup_id, wal_file);
            if !storage.exists(&path)? {
                return Err(Error::Corruption(format!("Missing WAL file: {}", path)));
            }
        }

        Ok(())
    }

    /// Restore SSTable files from backup.
    fn restore_sstables<S: BackupStorage>(
        backup_manager: &BackupManager<S>,
        backup_id: &BackupId,
        sstable_files: &[String],
        target_path: &Path,
    ) -> Result<()> {
        let storage = backup_manager.storage();

        for sstable_file in sstable_files {
            let remote_path = format!("{}/sstables/{}", backup_id, sstable_file);
            let local_path = target_path.join(sstable_file);

            log::debug!("Restoring SSTable: {}", sstable_file);
            storage.download_file(&remote_path, &local_path)?;
        }

        Ok(())
    }

    /// Restore WAL files from backup.
    fn restore_wal_files<S: BackupStorage>(
        backup_manager: &BackupManager<S>,
        backup_id: &BackupId,
        wal_files: &[String],
        target_path: &Path,
    ) -> Result<()> {
        let storage = backup_manager.storage();

        for wal_file in wal_files {
            let remote_path = format!("{}/wal/{}", backup_id, wal_file);
            let local_path = target_path.join(wal_file);

            log::debug!("Restoring WAL: {}", wal_file);
            storage.download_file(&remote_path, &local_path)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::storage::LocalFileStorage;
    use crate::{DB, Options};
    use tempfile::TempDir;

    #[test]
    fn test_restore_backup() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let restore_dir = TempDir::new().unwrap();

        // Create and populate a database
        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.flush().unwrap();

        // Create backup
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);
        let backup_id = manager.create_backup(&db).unwrap();

        // Restore to new location
        RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();

        // Open restored database
        let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();

        // Verify data
        assert_eq!(restored_db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(restored_db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_restore_to_existing_nonempty_directory_fails() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let restore_dir = TempDir::new().unwrap();

        // Create a file in restore directory
        std::fs::write(restore_dir.path().join("existing.txt"), b"data").unwrap();

        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);
        let backup_id = manager.create_backup(&db).unwrap();

        // Restore should fail
        let result = RecoveryManager::restore(&manager, &backup_id, restore_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_backup() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();

        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.flush().unwrap();

        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);
        let backup_id = manager.create_backup(&db).unwrap();

        // Verify should succeed
        let result = RecoveryManager::verify_backup(&manager, &backup_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_nonexistent_backup_fails() {
        let backup_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);

        let result = RecoveryManager::verify_backup(&manager, &"nonexistent".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_with_wal_replay() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let restore_dir = TempDir::new().unwrap();

        // Create database with data in WAL (not yet flushed)
        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        // Don't flush - keep data in WAL

        // Create backup
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);
        let backup_id = manager.create_backup(&db).unwrap();

        // Close original DB to ensure WAL is synced
        drop(db);

        // Restore
        RecoveryManager::restore(&manager, &backup_id, restore_dir.path()).unwrap();

        // Open restored database - should replay WAL
        let restored_db = DB::open(restore_dir.path(), Options::default()).unwrap();

        // Verify data was recovered from WAL
        assert_eq!(restored_db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(restored_db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }
}
