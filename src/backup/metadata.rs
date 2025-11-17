//! Backup metadata management.
//!
//! This module defines structures for tracking backup information,
//! retention policies, and backup manifests.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Unique identifier for a backup.
pub type BackupId = String;

/// Information about a single backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// Unique identifier for this backup
    pub id: BackupId,

    /// Timestamp when the backup was created (Unix epoch seconds)
    pub created_at: u64,

    /// Sequence number at the time of backup
    pub sequence: u64,

    /// Size of the backup in bytes
    pub size: u64,

    /// Type of backup (Full or Incremental)
    pub backup_type: BackupType,

    /// For incremental backups, the ID of the base backup
    pub base_backup_id: Option<BackupId>,

    /// List of SSTable files included in this backup
    pub sstable_files: Vec<String>,

    /// List of WAL files included in this backup
    pub wal_files: Vec<String>,

    /// Optional description or tags
    pub description: Option<String>,
}

impl BackupInfo {
    /// Create a new backup info.
    pub fn new(id: BackupId, sequence: u64, backup_type: BackupType) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        Self {
            id,
            created_at,
            sequence,
            size: 0,
            backup_type,
            base_backup_id: None,
            sstable_files: Vec::new(),
            wal_files: Vec::new(),
            description: None,
        }
    }

    /// Get the age of this backup in seconds.
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
        now.saturating_sub(self.created_at)
    }

    /// Check if this backup should be retained based on the policy.
    pub fn should_retain(&self, policy: &RetentionPolicy) -> bool {
        let age = self.age_seconds();

        // Always retain recent backups
        if age < policy.min_age_seconds {
            return true;
        }

        // Check maximum age
        if let Some(max_age) = policy.max_age_seconds {
            if age > max_age {
                return false;
            }
        }

        // Check minimum count
        true // Let the caller handle count-based retention
    }
}

/// Type of backup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupType {
    /// Full backup containing all data
    Full,
    /// Incremental backup containing only changes since the base backup
    Incremental,
}

/// Retention policy for backups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Minimum number of backups to keep
    pub min_count: usize,

    /// Maximum number of backups to keep (None = unlimited)
    pub max_count: Option<usize>,

    /// Minimum age in seconds before a backup can be deleted
    pub min_age_seconds: u64,

    /// Maximum age in seconds, backups older than this will be deleted
    pub max_age_seconds: Option<u64>,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            min_count: 3,
            max_count: Some(30),
            min_age_seconds: 24 * 3600,      // 1 day
            max_age_seconds: Some(30 * 86400), // 30 days
        }
    }
}

/// Metadata for all backups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// List of all backups, sorted by creation time (oldest first)
    pub backups: Vec<BackupInfo>,

    /// Retention policy
    pub retention_policy: RetentionPolicy,
}

impl BackupMetadata {
    /// Create a new backup metadata with the given retention policy.
    pub fn new(retention_policy: RetentionPolicy) -> Self {
        Self { backups: Vec::new(), retention_policy }
    }

    /// Add a backup to the metadata.
    pub fn add_backup(&mut self, backup: BackupInfo) {
        self.backups.push(backup);
        self.backups.sort_by_key(|b| b.created_at);
    }

    /// Get a backup by ID.
    pub fn get_backup(&self, id: &BackupId) -> Option<&BackupInfo> {
        self.backups.iter().find(|b| &b.id == id)
    }

    /// Remove a backup by ID.
    pub fn remove_backup(&mut self, id: &BackupId) -> Option<BackupInfo> {
        if let Some(pos) = self.backups.iter().position(|b| &b.id == id) {
            Some(self.backups.remove(pos))
        } else {
            None
        }
    }

    /// Get the list of backups that should be deleted according to the retention policy.
    pub fn get_backups_to_delete(&self) -> Vec<BackupId> {
        let mut to_delete = Vec::new();

        // First, collect all backups that are too old
        for backup in &self.backups {
            if !backup.should_retain(&self.retention_policy) {
                to_delete.push(backup.id.clone());
            }
        }

        // Then, check if we have too many backups
        if let Some(max_count) = self.retention_policy.max_count {
            if self.backups.len() > max_count {
                let delete_count = self.backups.len() - max_count;
                for backup in self.backups.iter().take(delete_count) {
                    if !to_delete.contains(&backup.id) {
                        to_delete.push(backup.id.clone());
                    }
                }
            }
        }

        // Make sure we always keep at least min_count backups
        let keep_count = self.backups.len().saturating_sub(to_delete.len());
        if keep_count < self.retention_policy.min_count {
            let needed = self.retention_policy.min_count - keep_count;
            // Remove the newest items from to_delete list
            to_delete.reverse();
            to_delete.truncate(to_delete.len().saturating_sub(needed));
            to_delete.reverse();
        }

        to_delete
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| Error::Serialization(format!("Failed to serialize metadata: {}", e)))
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| Error::Serialization(format!("Failed to deserialize metadata: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_info_creation() {
        let info = BackupInfo::new("backup-001".to_string(), 100, BackupType::Full);
        assert_eq!(info.id, "backup-001");
        assert_eq!(info.sequence, 100);
        assert_eq!(info.backup_type, BackupType::Full);
        assert!(info.created_at > 0);
    }

    #[test]
    fn test_backup_metadata_add_and_get() {
        let mut metadata = BackupMetadata::new(RetentionPolicy::default());

        let backup1 = BackupInfo::new("backup-001".to_string(), 100, BackupType::Full);
        let backup2 = BackupInfo::new("backup-002".to_string(), 200, BackupType::Incremental);

        metadata.add_backup(backup1.clone());
        metadata.add_backup(backup2.clone());

        assert_eq!(metadata.backups.len(), 2);
        assert!(metadata.get_backup(&"backup-001".to_string()).is_some());
        assert!(metadata.get_backup(&"backup-002".to_string()).is_some());
        assert!(metadata.get_backup(&"backup-003".to_string()).is_none());
    }

    #[test]
    fn test_backup_metadata_remove() {
        let mut metadata = BackupMetadata::new(RetentionPolicy::default());

        let backup = BackupInfo::new("backup-001".to_string(), 100, BackupType::Full);
        metadata.add_backup(backup);

        assert_eq!(metadata.backups.len(), 1);

        let removed = metadata.remove_backup(&"backup-001".to_string());
        assert!(removed.is_some());
        assert_eq!(metadata.backups.len(), 0);
    }

    #[test]
    fn test_retention_policy_default() {
        let policy = RetentionPolicy::default();
        assert_eq!(policy.min_count, 3);
        assert_eq!(policy.max_count, Some(30));
        assert_eq!(policy.min_age_seconds, 24 * 3600);
        assert_eq!(policy.max_age_seconds, Some(30 * 86400));
    }

    #[test]
    fn test_metadata_serialization() {
        let mut metadata = BackupMetadata::new(RetentionPolicy::default());
        let backup = BackupInfo::new("backup-001".to_string(), 100, BackupType::Full);
        metadata.add_backup(backup);

        let json = metadata.to_json().unwrap();
        let deserialized = BackupMetadata::from_json(&json).unwrap();

        assert_eq!(deserialized.backups.len(), 1);
        assert_eq!(deserialized.backups[0].id, "backup-001");
    }

    #[test]
    fn test_get_backups_to_delete_respects_min_count() {
        let mut metadata = BackupMetadata::new(RetentionPolicy {
            min_count: 2,
            max_count: Some(3),
            min_age_seconds: 0,
            max_age_seconds: Some(1), // Very short max age
        });

        // Add 3 backups
        for i in 1..=3 {
            let mut backup =
                BackupInfo::new(format!("backup-{:03}", i), i as u64 * 100, BackupType::Full);
            backup.created_at = i; // Old timestamps
            metadata.add_backup(backup);
        }

        let to_delete = metadata.get_backups_to_delete();
        // Should only delete 1 backup to keep min_count of 2
        assert!(to_delete.len() <= 1);
    }
}
