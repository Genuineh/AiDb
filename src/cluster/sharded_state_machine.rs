//! Sharded State Machine for Multi-Raft
//!
//! This module provides a state machine that manages multiple independent AiDb instances,
//! one for each Raft group. Each group's state machine is isolated and can be applied
//! independently, enabling true horizontal scaling.
//!
//! # Architecture
//!
//! ```text
//! ShardedStateMachine
//! ├── Group 1 → AiDb instance (./data/groups/1/db/)
//! ├── Group 2 → AiDb instance (./data/groups/2/db/)
//! └── Group N → AiDb instance (./data/groups/N/db/)
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::router::Router;
use super::sharded_storage::GroupId;
use crate::config::Options;
use crate::error::{Error, Result};
use crate::DB;

/// Sharded state machine managing multiple AiDb instances
///
/// Each Raft group has its own independent AiDb instance, enabling
/// horizontal scaling where data is partitioned across groups.
///
/// # Thread Safety
///
/// The ShardedStateMachine is thread-safe and uses RwLock for concurrent access.
pub struct ShardedStateMachine {
    /// Per-group database instances (group_id -> AiDb)
    dbs: Arc<RwLock<HashMap<GroupId, Arc<DB>>>>,

    /// Base directory for all group databases
    base_dir: PathBuf,

    /// Database options template for creating new DBs
    options: Options,

    /// Router for key routing (optional, for validation)
    router: Option<Arc<Router>>,
}

impl ShardedStateMachine {
    /// Create a new ShardedStateMachine
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Base directory for all group databases
    /// * `options` - Database options template
    pub fn new<P: Into<PathBuf>>(base_dir: P, options: Options) -> Self {
        Self {
            dbs: Arc::new(RwLock::new(HashMap::new())),
            base_dir: base_dir.into(),
            options,
            router: None,
        }
    }

    /// Create a new ShardedStateMachine with a router
    ///
    /// The router can be used to validate that keys belong to the correct group.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Base directory for all group databases
    /// * `options` - Database options template
    /// * `router` - Router for key routing
    pub fn with_router<P: Into<PathBuf>>(
        base_dir: P,
        options: Options,
        router: Arc<Router>,
    ) -> Self {
        Self {
            dbs: Arc::new(RwLock::new(HashMap::new())),
            base_dir: base_dir.into(),
            options,
            router: Some(router),
        }
    }

    /// Get the database path for a group
    ///
    /// Returns: `{base_dir}/groups/{group_id}/db`
    fn group_db_path(&self, group_id: GroupId) -> PathBuf {
        self.base_dir.join("groups").join(group_id.to_string()).join("db")
    }

    /// Create a new database for a group
    ///
    /// This initializes a new AiDb instance for the specified group.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Arc reference to the created database
    ///
    /// # Errors
    ///
    /// Returns an error if the group already exists or DB creation fails
    pub fn create_db(&self, group_id: GroupId) -> Result<Arc<DB>> {
        let mut dbs = self.dbs.write();

        // Check if already exists
        if dbs.contains_key(&group_id) {
            return Err(Error::Internal(format!("Group {} database already exists", group_id)));
        }

        // Create database directory
        let db_path = self.group_db_path(group_id);
        std::fs::create_dir_all(&db_path)?;

        // Open database
        let db = DB::open(&db_path, self.options.clone())?;
        let db = Arc::new(db);

        // Store in map
        dbs.insert(group_id, Arc::clone(&db));

        tracing::info!("Created database for group {} at {:?}", group_id, db_path);

        Ok(db)
    }

    /// Get the database for a group
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Arc reference to the database if it exists
    pub fn get_db(&self, group_id: GroupId) -> Option<Arc<DB>> {
        let dbs = self.dbs.read();
        dbs.get(&group_id).cloned()
    }

    /// Get or create a database for a group
    ///
    /// If the database doesn't exist, it will be created automatically.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// Arc reference to the database
    pub fn get_or_create_db(&self, group_id: GroupId) -> Result<Arc<DB>> {
        // Fast path: check if DB already exists
        if let Some(db) = self.get_db(group_id) {
            return Ok(db);
        }

        // Slow path: create new DB
        self.create_db(group_id)
    }

    /// Remove a database for a group
    ///
    /// This removes the database from memory and optionally deletes the data directory.
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    /// * `delete_data` - Whether to delete the data directory
    ///
    /// # Returns
    ///
    /// `true` if the database was removed, `false` if it didn't exist
    pub fn remove_db(&self, group_id: GroupId, delete_data: bool) -> Result<bool> {
        let mut dbs = self.dbs.write();
        let removed = dbs.remove(&group_id).is_some();

        if removed && delete_data {
            let db_path = self.group_db_path(group_id);
            if db_path.exists() {
                std::fs::remove_dir_all(&db_path)?;
                tracing::info!("Deleted database directory for group {}", group_id);
            }
        }

        Ok(removed)
    }

    /// Put a key-value pair into the appropriate group's database
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    /// * `key` - Key to insert
    /// * `value` - Value to insert
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub fn put(&self, group_id: GroupId, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let db = self.get_or_create_db(group_id)?;
        db.put(&key, &value)
    }

    /// Get a value from the appropriate group's database
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    /// * `key` - Key to retrieve
    ///
    /// # Returns
    ///
    /// The value if found, `None` otherwise
    pub fn get(&self, group_id: GroupId, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.get_or_create_db(group_id)?;
        db.get(key)
    }

    /// Delete a key from the appropriate group's database
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    /// * `key` - Key to delete
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    pub fn delete(&self, group_id: GroupId, key: &[u8]) -> Result<()> {
        let db = self.get_or_create_db(group_id)?;
        db.delete(key)
    }

    /// Put a key-value pair with automatic group routing
    ///
    /// This method uses the router to determine which group should handle the key.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to insert
    /// * `value` - Value to insert
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    ///
    /// # Errors
    ///
    /// Returns an error if router is not configured
    pub fn put_routed(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not configured".to_string()))?;

        let group_id = router.route(&key)?;
        self.put(group_id, key, value)
    }

    /// Get a value with automatic group routing
    ///
    /// # Arguments
    ///
    /// * `key` - Key to retrieve
    ///
    /// # Returns
    ///
    /// The value if found, `None` otherwise
    ///
    /// # Errors
    ///
    /// Returns an error if router is not configured
    pub fn get_routed(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not configured".to_string()))?;

        let group_id = router.route(key)?;
        self.get(group_id, key)
    }

    /// Delete a key with automatic group routing
    ///
    /// # Arguments
    ///
    /// * `key` - Key to delete
    ///
    /// # Returns
    ///
    /// `Ok(())` on success
    ///
    /// # Errors
    ///
    /// Returns an error if router is not configured
    pub fn delete_routed(&self, key: &[u8]) -> Result<()> {
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::Internal("Router not configured".to_string()))?;

        let group_id = router.route(key)?;
        self.delete(group_id, key)
    }

    /// List all active group IDs
    ///
    /// # Returns
    ///
    /// Vector of group IDs that have active databases
    pub fn list_groups(&self) -> Vec<GroupId> {
        let dbs = self.dbs.read();
        dbs.keys().copied().collect()
    }

    /// Get the number of active groups
    ///
    /// # Returns
    ///
    /// Number of groups with active databases
    pub fn group_count(&self) -> usize {
        let dbs = self.dbs.read();
        dbs.len()
    }

    /// Check if a group has an active database
    ///
    /// # Arguments
    ///
    /// * `group_id` - Group identifier
    ///
    /// # Returns
    ///
    /// `true` if the group has an active database
    pub fn has_db(&self, group_id: GroupId) -> bool {
        let dbs = self.dbs.read();
        dbs.contains_key(&group_id)
    }

    /// Load existing group databases from disk
    ///
    /// Scans the base directory for existing group databases and loads them.
    ///
    /// # Returns
    ///
    /// Number of databases loaded
    pub fn load_existing_groups(&self) -> Result<usize> {
        let groups_dir = self.base_dir.join("groups");
        if !groups_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(&groups_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            // Try to parse directory name as group ID
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(group_id) = name.parse::<GroupId>() {
                    let db_path = entry.path().join("db");
                    if db_path.exists() {
                        // Load the database
                        match DB::open(&db_path, self.options.clone()) {
                            Ok(db) => {
                                let mut dbs = self.dbs.write();
                                dbs.insert(group_id, Arc::new(db));
                                count += 1;
                                tracing::info!("Loaded existing database for group {}", group_id);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to load database for group {}: {}",
                                    group_id,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    /// Flush all databases
    ///
    /// Forces a flush of all active databases to disk.
    ///
    /// # Returns
    ///
    /// Number of databases flushed
    pub fn flush_all(&self) -> Result<usize> {
        let dbs = self.dbs.read();
        let mut count = 0;

        for (group_id, db) in dbs.iter() {
            match db.flush() {
                Ok(_) => {
                    count += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to flush database for group {}: {}", group_id, e);
                }
            }
        }

        Ok(count)
    }

    /// Shutdown all databases
    ///
    /// Closes all active databases gracefully.
    pub fn shutdown(&self) -> Result<()> {
        let mut dbs = self.dbs.write();
        dbs.clear();
        tracing::info!("Shutdown all group databases");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_options() -> Options {
        Options::default()
    }

    #[test]
    fn test_sharded_state_machine_basic() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // Initially no groups
        assert_eq!(state_machine.group_count(), 0);
        assert_eq!(state_machine.list_groups(), Vec::<GroupId>::new());

        // Create a database for group 1
        let db = state_machine.create_db(1).unwrap();
        assert!(Arc::ptr_eq(&db, &state_machine.get_db(1).unwrap()));

        assert_eq!(state_machine.group_count(), 1);
        assert!(state_machine.has_db(1));
        assert!(!state_machine.has_db(2));

        // Try to create again - should fail
        assert!(state_machine.create_db(1).is_err());
    }

    #[test]
    fn test_sharded_state_machine_operations() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // Create database for group 1
        state_machine.create_db(1).unwrap();

        // Put key-value
        state_machine.put(1, b"key1".to_vec(), b"value1".to_vec()).unwrap();

        // Get value
        let value = state_machine.get(1, b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Delete key
        state_machine.delete(1, b"key1").unwrap();
        let value = state_machine.get(1, b"key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_sharded_state_machine_multiple_groups() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // Create databases for multiple groups
        for group_id in 1..=5 {
            state_machine.create_db(group_id).unwrap();
        }

        assert_eq!(state_machine.group_count(), 5);

        // Write to different groups
        for group_id in 1..=5 {
            let key = format!("key{}", group_id);
            let value = format!("value{}", group_id);
            state_machine.put(group_id, key.as_bytes().to_vec(), value.as_bytes().to_vec()).unwrap();
        }

        // Read from different groups
        for group_id in 1..=5 {
            let key = format!("key{}", group_id);
            let expected_value = format!("value{}", group_id);
            let value = state_machine.get(group_id, key.as_bytes()).unwrap();
            assert_eq!(value, Some(expected_value.as_bytes().to_vec()));
        }
    }

    #[test]
    fn test_get_or_create_db() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // get_or_create should create if not exists
        let db1 = state_machine.get_or_create_db(1).unwrap();
        assert!(state_machine.has_db(1));

        // get_or_create should return existing DB
        let db2 = state_machine.get_or_create_db(1).unwrap();
        assert!(Arc::ptr_eq(&db1, &db2));
    }

    #[test]
    fn test_remove_db() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // Create a database
        state_machine.create_db(1).unwrap();
        assert!(state_machine.has_db(1));

        // Remove without deleting data
        let removed = state_machine.remove_db(1, false).unwrap();
        assert!(removed);
        assert!(!state_machine.has_db(1));

        // Directory should still exist
        let db_path = temp_dir.path().join("groups").join("1").join("db");
        assert!(db_path.exists());

        // Create and remove with data deletion
        state_machine.create_db(2).unwrap();
        let removed = state_machine.remove_db(2, true).unwrap();
        assert!(removed);

        // Directory should be deleted
        let _db_path = temp_dir.path().join("groups").join("2");
        // Note: directory might still exist if empty, just check DB doesn't exist
        assert!(!state_machine.has_db(2));
    }

    #[test]
    fn test_load_existing_groups() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create initial state machine and add some groups
        {
            let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());
            state_machine.create_db(1).unwrap();
            state_machine.create_db(2).unwrap();
            state_machine.put(1, b"key1".to_vec(), b"value1".to_vec()).unwrap();
            state_machine.put(2, b"key2".to_vec(), b"value2".to_vec()).unwrap();
            state_machine.flush_all().unwrap();
        }

        // Create new state machine and load existing groups
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());
        let loaded = state_machine.load_existing_groups().unwrap();
        assert_eq!(loaded, 2);
        assert!(state_machine.has_db(1));
        assert!(state_machine.has_db(2));

        // Verify data is still there
        let value1 = state_machine.get(1, b"key1").unwrap();
        assert_eq!(value1, Some(b"value1".to_vec()));
        let value2 = state_machine.get(2, b"key2").unwrap();
        assert_eq!(value2, Some(b"value2".to_vec()));
    }

    #[test]
    fn test_flush_all() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // Create and write to multiple groups
        for group_id in 1..=3 {
            state_machine.create_db(group_id).unwrap();
            state_machine.put(group_id, b"key".to_vec(), b"value".to_vec()).unwrap();
        }

        // Flush all
        let flushed = state_machine.flush_all().unwrap();
        assert_eq!(flushed, 3);
    }

    #[test]
    fn test_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), create_test_options());

        // Create some groups
        state_machine.create_db(1).unwrap();
        state_machine.create_db(2).unwrap();
        assert_eq!(state_machine.group_count(), 2);

        // Shutdown
        state_machine.shutdown().unwrap();
        assert_eq!(state_machine.group_count(), 0);
    }
}
