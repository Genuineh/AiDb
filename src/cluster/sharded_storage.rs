//! Sharded Raft Storage for Multi-Raft implementation
//!
//! This module provides a storage layer that manages multiple independent Raft groups,
//! each with its own storage instance. This is the foundation for horizontal scaling
//! in a Multi-Raft architecture.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::raft_storage::{NodeId, OpenRaftStorage};
use crate::config::Options;
use crate::error::{Error, Result};
use crate::DB;

/// Group identifier type
pub type GroupId = u64;

/// Sharded Raft Storage managing multiple Raft groups
///
/// Each Raft group has its own independent storage instance and data directory.
/// This enables horizontal scaling where each group manages a subset of the data.
///
/// # Architecture
///
/// ```text
/// ShardedRaftStorage
/// ├── Group 1 → OpenRaftStorage + DB (./data/groups/1/)
/// ├── Group 2 → OpenRaftStorage + DB (./data/groups/2/)
/// └── Group N → OpenRaftStorage + DB (./data/groups/N/)
/// ```
pub struct ShardedRaftStorage {
    /// Map of group ID to storage instance
    groups: Arc<RwLock<HashMap<GroupId, Arc<OpenRaftStorage>>>>,

    /// Base directory for all groups
    base_dir: PathBuf,

    /// Node ID for this storage instance
    node_id: NodeId,
}

impl ShardedRaftStorage {
    /// Create a new ShardedRaftStorage
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Base directory for storing all group data
    /// * `node_id` - Node ID for this storage instance
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use aidb::cluster::sharded_storage::ShardedRaftStorage;
    ///
    /// let storage = ShardedRaftStorage::new("./data/groups", 1).unwrap();
    /// ```
    pub fn new<P: Into<PathBuf>>(base_dir: P, node_id: NodeId) -> Result<Self> {
        let base_dir = base_dir.into();
        std::fs::create_dir_all(&base_dir)?;

        Ok(Self { groups: Arc::new(RwLock::new(HashMap::new())), base_dir, node_id })
    }

    /// Create or open storage for a specific group
    ///
    /// This creates a new directory for the group and initializes its storage.
    /// If the group already exists, returns the existing storage instance.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Unique identifier for the group
    ///
    /// # Returns
    ///
    /// Arc reference to the group's storage instance
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use aidb::cluster::sharded_storage::ShardedRaftStorage;
    /// # let storage = ShardedRaftStorage::new("./data/groups", 1).unwrap();
    /// let group_storage = storage.create_group(1).unwrap();
    /// ```
    pub fn create_group(&self, group_id: GroupId) -> Result<Arc<OpenRaftStorage>> {
        let mut groups = self.groups.write();

        // Return existing storage if already created
        if let Some(storage) = groups.get(&group_id) {
            return Ok(Arc::clone(storage));
        }

        // Create directory for this group
        let group_dir = self.group_path(group_id);
        std::fs::create_dir_all(&group_dir)?;

        // Create DB instance for this group with default options
        let db_dir = group_dir.join("db");
        let options = Options::default();
        let db = Arc::new(DB::open(&db_dir, options)?);

        // Create OpenRaftStorage instance
        let storage = Arc::new(OpenRaftStorage::new(db)?);

        // Store in map
        groups.insert(group_id, Arc::clone(&storage));

        Ok(storage)
    }

    /// Get storage for an existing group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Some(storage) if the group exists, None otherwise
    pub fn get_group(&self, group_id: GroupId) -> Option<Arc<OpenRaftStorage>> {
        let groups = self.groups.read();
        groups.get(&group_id).map(Arc::clone)
    }

    /// Remove storage for a group
    ///
    /// This stops the group's storage and removes it from the active groups map.
    /// The data directory is not deleted to allow for recovery.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Ok(true) if group was removed, Ok(false) if group didn't exist
    pub fn remove_group(&self, group_id: GroupId) -> Result<bool> {
        let mut groups = self.groups.write();

        if groups.remove(&group_id).is_some() {
            // Note: We don't delete the data directory here to allow for recovery.
            // If needed, the directory can be manually deleted later.
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete storage data for a group
    ///
    /// This permanently deletes the group's data directory.
    /// The group must be removed first using `remove_group()`.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Safety
    ///
    /// This operation is irreversible. Make sure the group is no longer active.
    pub fn delete_group_data(&self, group_id: GroupId) -> Result<()> {
        let groups = self.groups.read();

        // Ensure group is not active
        if groups.contains_key(&group_id) {
            return Err(Error::Internal(format!(
                "Group {} is still active, remove it first",
                group_id
            )));
        }

        let group_dir = self.group_path(group_id);
        if group_dir.exists() {
            std::fs::remove_dir_all(group_dir)?;
        }

        Ok(())
    }

    /// List all active group IDs
    ///
    /// # Returns
    ///
    /// Vector of all group IDs that have been created
    pub fn list_groups(&self) -> Vec<GroupId> {
        let groups = self.groups.read();
        groups.keys().copied().collect()
    }

    /// Get the number of active groups
    pub fn group_count(&self) -> usize {
        let groups = self.groups.read();
        groups.len()
    }

    /// Check if a group exists
    pub fn has_group(&self, group_id: GroupId) -> bool {
        let groups = self.groups.read();
        groups.contains_key(&group_id)
    }

    /// Get the data directory path for a specific group
    fn group_path(&self, group_id: GroupId) -> PathBuf {
        self.base_dir.join(format!("{}", group_id))
    }

    /// Load all existing groups from disk
    ///
    /// This scans the base directory and loads any existing group storage.
    /// Useful for recovery after restart.
    pub fn load_existing_groups(&self) -> Result<usize> {
        let mut loaded_count = 0;

        // Scan base directory for group directories
        if !self.base_dir.exists() {
            return Ok(0);
        }

        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Try to parse directory name as group ID
                if let Some(dir_name) = path.file_name() {
                    if let Some(dir_str) = dir_name.to_str() {
                        if let Ok(group_id) = dir_str.parse::<GroupId>() {
                            // Try to load this group
                            match self.create_group(group_id) {
                                Ok(_) => {
                                    loaded_count += 1;
                                }
                                Err(e) => {
                                    eprintln!("Warning: Failed to load group {}: {}", group_id, e);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(loaded_count)
    }

    /// Get base directory path
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Create independent snapshot for a specific group
    ///
    /// This creates a snapshot of the group's state machine data without affecting
    /// other groups. Snapshots are stored in the group's data directory.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Ok with snapshot metadata if successful
    pub async fn create_group_snapshot(&self, group_id: GroupId) -> Result<()> {
        let storage = self
            .get_group(group_id)
            .ok_or_else(|| Error::Internal(format!("Group {} not found", group_id)))?;

        // Create snapshot using the storage's snapshot builder
        #[cfg(feature = "raft-cluster")]
        {
            use openraft::{RaftSnapshotBuilder, RaftStorage};
            // Note: get_snapshot_builder requires &mut self
            // We'll use Arc::make_mut to get mutable access
            let storage_clone = (*storage).clone();
            let mut storage_mut = storage_clone;
            let mut builder = storage_mut.get_snapshot_builder().await;

            builder
                .build_snapshot()
                .await
                .map_err(|e| Error::Internal(format!("Failed to build snapshot: {:?}", e)))?;
        }

        #[cfg(not(feature = "raft-cluster"))]
        {
            let _ = storage; // Use the variable
            return Err(Error::Internal("raft-cluster feature not enabled".to_string()));
        }

        Ok(())
    }

    /// Get snapshot path for a group
    pub fn group_snapshot_path(&self, group_id: GroupId) -> PathBuf {
        self.group_path(group_id).join("snapshots")
    }

    /// Get node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_sharded_storage() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        assert_eq!(storage.node_id(), 1);
        assert_eq!(storage.group_count(), 0);
    }

    #[test]
    fn test_create_group() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        // Create a group
        let group_storage = storage.create_group(1).unwrap();
        assert!(Arc::ptr_eq(&group_storage, &storage.get_group(1).unwrap()));

        assert_eq!(storage.group_count(), 1);
        assert!(storage.has_group(1));
        assert!(!storage.has_group(2));
    }

    #[test]
    fn test_create_multiple_groups() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        // Create multiple groups
        for i in 1..=10 {
            storage.create_group(i).unwrap();
        }

        assert_eq!(storage.group_count(), 10);

        // Verify all groups exist
        for i in 1..=10 {
            assert!(storage.has_group(i));
            assert!(storage.get_group(i).is_some());
        }
    }

    #[test]
    fn test_create_group_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        // Create same group twice
        let storage1 = storage.create_group(1).unwrap();
        let storage2 = storage.create_group(1).unwrap();

        // Should return the same instance
        assert!(Arc::ptr_eq(&storage1, &storage2));
        assert_eq!(storage.group_count(), 1);
    }

    #[test]
    fn test_remove_group() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        // Create and remove a group
        storage.create_group(1).unwrap();
        assert!(storage.has_group(1));

        let removed = storage.remove_group(1).unwrap();
        assert!(removed);
        assert!(!storage.has_group(1));
        assert_eq!(storage.group_count(), 0);

        // Try to remove again
        let removed = storage.remove_group(1).unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_list_groups() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        // Create groups
        storage.create_group(1).unwrap();
        storage.create_group(5).unwrap();
        storage.create_group(3).unwrap();

        let mut groups = storage.list_groups();
        groups.sort();

        assert_eq!(groups, vec![1, 3, 5]);
    }

    #[test]
    fn test_load_existing_groups() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().to_path_buf();

        // Create some groups
        {
            let storage = ShardedRaftStorage::new(&storage_path, 1).unwrap();
            storage.create_group(1).unwrap();
            storage.create_group(2).unwrap();
            storage.create_group(3).unwrap();
        }

        // Create new storage instance and load existing groups
        let storage = ShardedRaftStorage::new(&storage_path, 1).unwrap();
        let loaded = storage.load_existing_groups().unwrap();

        assert_eq!(loaded, 3);
        assert_eq!(storage.group_count(), 3);
        assert!(storage.has_group(1));
        assert!(storage.has_group(2));
        assert!(storage.has_group(3));
    }

    #[test]
    fn test_delete_group_data() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ShardedRaftStorage::new(temp_dir.path(), 1).unwrap();

        // Create a group
        storage.create_group(1).unwrap();
        let group_path = storage.group_path(1);
        assert!(group_path.exists());

        // Cannot delete active group
        assert!(storage.delete_group_data(1).is_err());

        // Remove group first
        storage.remove_group(1).unwrap();

        // Now delete should work
        storage.delete_group_data(1).unwrap();
        assert!(!group_path.exists());
    }

    #[test]
    fn test_concurrent_group_creation() {
        use std::thread;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(ShardedRaftStorage::new(temp_dir.path(), 1).unwrap());

        // Create groups concurrently
        let mut handles = vec![];
        for i in 1..=20 {
            let storage = Arc::clone(&storage);
            let handle = thread::spawn(move || {
                storage.create_group(i).unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(storage.group_count(), 20);
    }
}
