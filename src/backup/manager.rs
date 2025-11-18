//! Backup manager for creating and managing database backups.

use crate::backup::metadata::{BackupId, BackupInfo, BackupMetadata, BackupType, RetentionPolicy};
use crate::backup::storage::BackupStorage;
use crate::{Result, DB};
use std::sync::Arc;

const METADATA_FILE: &str = "metadata.json";

/// Manager for creating and managing database backups.
pub struct BackupManager<S: BackupStorage> {
    /// Storage backend
    storage: Arc<S>,

    /// Backup metadata
    metadata: parking_lot::RwLock<BackupMetadata>,
}

impl<S: BackupStorage> BackupManager<S> {
    /// Create a new backup manager with the given storage backend.
    pub fn new(storage: S) -> Self {
        Self::with_retention_policy(storage, RetentionPolicy::default())
    }

    /// Create a new backup manager with a custom retention policy.
    pub fn with_retention_policy(storage: S, retention_policy: RetentionPolicy) -> Self {
        let storage = Arc::new(storage);

        // Try to load existing metadata
        let metadata = match storage.read(METADATA_FILE) {
            Ok(data) => match BackupMetadata::from_json(&String::from_utf8_lossy(&data)) {
                Ok(mut meta) => {
                    meta.retention_policy = retention_policy;
                    meta
                }
                Err(_) => BackupMetadata::new(retention_policy),
            },
            Err(_) => BackupMetadata::new(retention_policy),
        };

        Self { storage, metadata: parking_lot::RwLock::new(metadata) }
    }

    /// Create a full backup of the database.
    ///
    /// This creates a consistent snapshot of all SSTables and the current WAL.
    pub fn create_backup(&self, db: &DB) -> Result<BackupId> {
        self.create_backup_with_description(db, None)
    }

    /// Create a full backup with a description.
    pub fn create_backup_with_description(
        &self,
        db: &DB,
        description: Option<String>,
    ) -> Result<BackupId> {
        // Generate unique backup ID
        let backup_id = self.generate_backup_id();

        // Flush memtable to ensure all data is on disk
        db.flush()?;

        // Get current sequence number
        let sequence = db.get_sequence();

        // Create backup info
        let mut backup_info = BackupInfo::new(backup_id.clone(), sequence, BackupType::Full);
        backup_info.description = description;

        // Backup SSTables
        let sstable_files = self.backup_sstables(db, &backup_id)?;
        backup_info.sstable_files = sstable_files;

        // Backup WAL files
        let wal_files = self.backup_wal_files(db, &backup_id)?;
        backup_info.wal_files = wal_files;

        // Calculate total size
        backup_info.size = self.calculate_backup_size(&backup_id)?;

        // Add to metadata
        {
            let mut metadata = self.metadata.write();
            metadata.add_backup(backup_info);
            self.save_metadata(&metadata)?;
        }

        // Apply retention policy
        self.apply_retention_policy()?;

        Ok(backup_id)
    }

    /// List all available backups.
    pub fn list_backups(&self) -> Vec<BackupInfo> {
        let metadata = self.metadata.read();
        metadata.backups.clone()
    }

    /// Get information about a specific backup.
    pub fn get_backup_info(&self, backup_id: &BackupId) -> Option<BackupInfo> {
        let metadata = self.metadata.read();
        metadata.get_backup(backup_id).cloned()
    }

    /// Delete a specific backup.
    pub fn delete_backup(&self, backup_id: &BackupId) -> Result<()> {
        // Remove backup files
        let backup_dir = format!("{}/", backup_id);
        self.storage.delete(&backup_dir)?;

        // Remove from metadata
        {
            let mut metadata = self.metadata.write();
            metadata.remove_backup(backup_id);
            self.save_metadata(&metadata)?;
        }

        Ok(())
    }

    /// Apply retention policy and delete old backups.
    pub fn apply_retention_policy(&self) -> Result<()> {
        let to_delete = {
            let metadata = self.metadata.read();
            metadata.get_backups_to_delete()
        };

        for backup_id in to_delete {
            log::info!("Deleting old backup: {}", backup_id);
            if let Err(e) = self.delete_backup(&backup_id) {
                log::warn!("Failed to delete backup {}: {}", backup_id, e);
            }
        }

        Ok(())
    }

    /// Generate a unique backup ID.
    fn generate_backup_id(&self) -> BackupId {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs();

        format!("backup-{}", timestamp)
    }

    /// Backup all SSTable files.
    fn backup_sstables(&self, db: &DB, backup_id: &BackupId) -> Result<Vec<String>> {
        let db_path = db.get_path();
        let mut backed_up = Vec::new();

        // Get list of SSTable files from database
        let sstables = db.list_sstable_files()?;

        for sstable_file in sstables {
            let source_path = db_path.join(&sstable_file);
            let dest_path = format!("{}/sstables/{}", backup_id, sstable_file);

            self.storage.upload_file(&source_path, &dest_path)?;
            backed_up.push(sstable_file);
        }

        Ok(backed_up)
    }

    /// Backup WAL files.
    fn backup_wal_files(&self, db: &DB, backup_id: &BackupId) -> Result<Vec<String>> {
        let db_path = db.get_path();
        let mut backed_up = Vec::new();

        // Get list of WAL files
        let wal_files = db.list_wal_files()?;

        for wal_file in wal_files {
            let source_path = db_path.join(&wal_file);
            let dest_path = format!("{}/wal/{}", backup_id, wal_file);

            self.storage.upload_file(&source_path, &dest_path)?;
            backed_up.push(wal_file);
        }

        Ok(backed_up)
    }

    /// Calculate the total size of a backup.
    fn calculate_backup_size(&self, backup_id: &BackupId) -> Result<u64> {
        let prefix = format!("{}/", backup_id);
        let files = self.storage.list(&prefix)?;

        let mut total_size = 0u64;
        for file in files {
            if let Ok(data) = self.storage.read(&file) {
                total_size += data.len() as u64;
            }
        }

        Ok(total_size)
    }

    /// Save metadata to storage.
    fn save_metadata(&self, metadata: &BackupMetadata) -> Result<()> {
        let json = metadata.to_json()?;
        self.storage.write(METADATA_FILE, json.as_bytes())
    }

    /// Get the storage backend.
    pub fn storage(&self) -> Arc<S> {
        Arc::clone(&self.storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::storage::LocalFileStorage;
    use crate::Options;
    use tempfile::TempDir;

    #[test]
    fn test_backup_manager_creation() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(tmp_dir.path());
        let manager = BackupManager::new(storage);

        let backups = manager.list_backups();
        assert_eq!(backups.len(), 0);
    }

    #[test]
    fn test_create_backup() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();

        // Create and populate a database
        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.flush().unwrap();

        // Create backup
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);
        let backup_id = manager.create_backup(&db).unwrap();

        // Verify backup was created
        let backups = manager.list_backups();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].id, backup_id);
        assert_eq!(backups[0].backup_type, BackupType::Full);
    }

    #[test]
    fn test_list_backups() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();

        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        db.put(b"key1", b"value1").unwrap();

        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);

        // Create multiple backups
        let id1 = manager.create_backup(&db).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let id2 = manager.create_backup(&db).unwrap();

        let backups = manager.list_backups();
        assert_eq!(backups.len(), 2);
        assert!(backups.iter().any(|b| b.id == id1));
        assert!(backups.iter().any(|b| b.id == id2));
    }

    #[test]
    fn test_get_backup_info() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();

        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);

        let backup_id = manager.create_backup(&db).unwrap();
        let info = manager.get_backup_info(&backup_id);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.id, backup_id);
        assert_eq!(info.backup_type, BackupType::Full);
    }

    #[test]
    fn test_delete_backup() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();

        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);

        let backup_id = manager.create_backup(&db).unwrap();
        assert_eq!(manager.list_backups().len(), 1);

        manager.delete_backup(&backup_id).unwrap();
        assert_eq!(manager.list_backups().len(), 0);
    }

    #[test]
    fn test_backup_with_description() {
        let db_dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();

        let db = DB::open(db_dir.path(), Options::default()).unwrap();
        let storage = LocalFileStorage::new(backup_dir.path());
        let manager = BackupManager::new(storage);

        let desc = Some("Test backup".to_string());
        let backup_id = manager.create_backup_with_description(&db, desc.clone()).unwrap();

        let info = manager.get_backup_info(&backup_id).unwrap();
        assert_eq!(info.description, desc);
    }
}
