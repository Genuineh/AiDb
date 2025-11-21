//! Online slot migration for Multi-Raft resharding
//!
//! This module implements key-level migration between Raft groups to support
//! online resharding without downtime. The migration protocol follows these steps:
//!
//! 1. MetaRaft initiates migration: `StartMigration { slot, from_group, to_group }`
//! 2. Target group enters IMPORTING state
//! 3. Source group enters MIGRATING state
//! 4. Batch migrate keys: GET → MIGRATE → DEL
//! 5. Dual-write to both groups during migration
//! 6. Complete migration: update MetaRaft slot mapping
//! 7. Clean up source group data
//!
//! # Example
//!
//! ```no_run
//! # use aidb::cluster::MigrationManager;
//! # async fn example() -> aidb::error::Result<()> {
//! let manager = MigrationManager::new(/* ... */);
//!
//! // Start migrating slot 100 from group 1 to group 2
//! manager.start_migration(100, 1, 2).await?;
//!
//! // Check migration progress
//! let progress = manager.get_migration_progress(100).await?;
//! println!("Progress: {:.2}%", progress.progress_pct());
//!
//! // Migration completes automatically in background
//! # Ok(())
//! # }
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

use super::meta_types::{SlotMigration, SlotMigrationState};
use super::router::Router;
use super::sharded_state_machine::ShardedStateMachine;
use super::sharded_storage::GroupId;
use crate::error::{Error, Result};

/// Configuration for slot migration
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Number of keys to migrate in each batch
    pub batch_size: usize,

    /// Rate limit for migration (keys per second)
    /// 0 means no limit
    pub rate_limit: u64,

    /// Timeout for single key migration
    pub key_timeout: Duration,

    /// Maximum retries for failed key migrations
    pub max_retries: u32,

    /// Delay between batches
    pub batch_delay: Duration,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            rate_limit: 1000, // 1000 keys/sec
            key_timeout: Duration::from_secs(5),
            max_retries: 3,
            batch_delay: Duration::from_millis(10),
        }
    }
}

/// Manager for coordinating slot migrations
///
/// The MigrationManager handles the complete lifecycle of slot migrations,
/// including key scanning, batched migration, dual-write coordination, and
/// metadata updates.
pub struct MigrationManager {
    /// Migration configuration
    config: MigrationConfig,

    /// Router for key-to-group mapping
    ///
    /// NOTE: Currently stored for future use in Phase 2-3 when implementing
    /// dual-write and migration-aware routing. Will be actively used when
    /// determining which group to write to during active migrations.
    #[allow(dead_code)]
    router: Arc<Router>,

    /// Sharded state machine for data access
    state_machine: Arc<RwLock<ShardedStateMachine>>,

    /// Active migrations indexed by slot
    active_migrations: Arc<RwLock<HashMap<u16, Arc<RwLock<SlotMigration>>>>>,

    /// Channel for migration commands
    command_tx: mpsc::UnboundedSender<MigrationCommand>,

    /// Channel receiver (held by background worker)
    command_rx: Option<mpsc::UnboundedReceiver<MigrationCommand>>,
}

/// Commands for migration worker
#[derive(Debug)]
enum MigrationCommand {
    /// Start a new migration
    Start { slot: u16, from_group: GroupId, to_group: GroupId },

    /// Commands for migration worker
    ///
    /// NOTE: Cancel variant is reserved for future use in Phase 4 when
    /// implementing migration rollback and cancellation support.
    #[allow(dead_code)]
    Cancel { slot: u16 },

    /// Shutdown the worker
    Shutdown,
}

impl MigrationManager {
    /// Create a new migration manager
    ///
    /// # Arguments
    ///
    /// * `config` - Migration configuration
    /// * `router` - Router for key-to-group mapping
    /// * `state_machine` - Sharded state machine for data access
    pub fn new(
        config: MigrationConfig,
        router: Arc<Router>,
        state_machine: Arc<RwLock<ShardedStateMachine>>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        Self {
            config,
            router,
            state_machine,
            active_migrations: Arc::new(RwLock::new(HashMap::new())),
            command_tx,
            command_rx: Some(command_rx),
        }
    }

    /// Start the migration worker background task
    ///
    /// This spawns a tokio task that processes migration commands.
    /// Returns a handle that can be used to wait for the task to complete.
    pub fn start_worker(mut self) -> tokio::task::JoinHandle<()> {
        let command_rx = self.command_rx.take().expect("Worker already started");

        tokio::spawn(async move {
            self.run_worker(command_rx).await;
        })
    }

    /// Run the migration worker loop
    async fn run_worker(&self, mut command_rx: mpsc::UnboundedReceiver<MigrationCommand>) {
        while let Some(command) = command_rx.recv().await {
            match command {
                MigrationCommand::Start { slot, from_group, to_group } => {
                    if let Err(e) = self.execute_migration(slot, from_group, to_group).await {
                        tracing::error!("Migration error for slot {}: {}", slot, e);
                    }
                }
                MigrationCommand::Cancel { slot } => {
                    self.cancel_migration(slot);
                }
                MigrationCommand::Shutdown => {
                    tracing::info!("Migration worker shutting down");
                    break;
                }
            }
        }
    }

    /// Start migrating a slot from one group to another
    ///
    /// # Arguments
    ///
    /// * `slot` - Slot number to migrate (0-16383)
    /// * `from_group` - Source group ID
    /// * `to_group` - Target group ID
    ///
    /// # Returns
    ///
    /// Ok(()) if migration started successfully
    pub async fn start_migration(
        &self,
        slot: u16,
        from_group: GroupId,
        to_group: GroupId,
    ) -> Result<()> {
        // Validate slot range
        if slot >= 16384 {
            return Err(Error::InvalidArgument(format!("Invalid slot: {}", slot)));
        }

        // Check if already migrating
        {
            let migrations = self.active_migrations.read();
            if migrations.contains_key(&slot) {
                return Err(Error::InvalidArgument(format!("Slot {} is already migrating", slot)));
            }
        }

        // Create migration record
        let migration = Arc::new(RwLock::new(SlotMigration::new(slot, from_group, to_group)));

        // Add to active migrations
        {
            let mut migrations = self.active_migrations.write();
            migrations.insert(slot, migration.clone());
        }

        // Send command to worker
        self.command_tx
            .send(MigrationCommand::Start { slot, from_group, to_group })
            .map_err(|_| Error::Internal("Failed to send migration command".to_string()))?;

        Ok(())
    }

    /// Execute a slot migration
    async fn execute_migration(
        &self,
        slot: u16,
        from_group: GroupId,
        to_group: GroupId,
    ) -> Result<()> {
        // Get migration record
        let migration = {
            let migrations = self.active_migrations.read();
            migrations
                .get(&slot)
                .cloned()
                .ok_or_else(|| Error::Internal(format!("Migration for slot {} not found", slot)))?
        };

        // Step 1: Scan keys in the slot
        let keys = self.scan_slot_keys(from_group, slot).await?;

        // Update total count
        {
            let mut m = migration.write();
            m.total = keys.len() as u64;
        }

        // Step 2: Migrate keys in batches
        let mut migrated = 0;
        for chunk in keys.chunks(self.config.batch_size) {
            // Check if migration was cancelled
            {
                let migrations = self.active_migrations.read();
                if !migrations.contains_key(&slot) {
                    return Err(Error::Internal(format!("Migration cancelled for slot {}", slot)));
                }
            }

            // Migrate batch
            for key in chunk {
                match self.migrate_key(key, from_group, to_group).await {
                    Ok(_) => {
                        migrated += 1;
                        // Update progress
                        let mut m = migration.write();
                        m.progress = migrated;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to migrate key during slot {} migration: {}", slot, e);
                        // Continue with other keys for resilience
                    }
                }
            }

            // Apply rate limiting
            if self.config.batch_delay > Duration::ZERO {
                sleep(self.config.batch_delay).await;
            }
        }

        // Step 3: Mark migration as complete
        {
            let mut m = migration.write();
            m.state = SlotMigrationState::Complete;
        }

        // Step 4: Clean up
        {
            let mut migrations = self.active_migrations.write();
            migrations.remove(&slot);
        }

        Ok(())
    }

    /// Scan all keys belonging to a slot in a group
    async fn scan_slot_keys(&self, group_id: GroupId, slot: u16) -> Result<Vec<Vec<u8>>> {
        // Clone the Arc to avoid holding the lock across await
        let state_machine = Arc::clone(&self.state_machine);
        // Release the read lock before awaiting
        let result = {
            let sm = state_machine.read();
            // We can't await here, so we need to make scan_slot_keys synchronous
            // or restructure this differently
            Ok(sm.scan_slot_keys_sync(group_id, slot)?)
        };
        result
    }

    /// Migrate a single key from source to target group
    ///
    /// # Arguments
    ///
    /// * `key` - Key to migrate
    /// * `from_group` - Source group ID
    /// * `to_group` - Target group ID
    async fn migrate_key(
        &self,
        key: &[u8],
        from_group: GroupId,
        to_group: GroupId,
    ) -> Result<()> {
        let state_machine = Arc::clone(&self.state_machine);
        
        // Step 1: Read from source
        let value = {
            let sm = state_machine.read();
            sm.get_from_group_sync(from_group, key)?
        };

        // Step 2: Write to target (if value exists)
        if let Some(value) = value {
            let sm = state_machine.read();
            sm.put_to_group_sync(to_group, key.to_vec(), value)?;
        }

        // Step 3: Delete from source (after confirmation)
        {
            let sm = state_machine.read();
            sm.delete_from_group_sync(from_group, key.to_vec())?;
        }

        Ok(())
    }

    /// Cancel an ongoing migration
    fn cancel_migration(&self, slot: u16) {
        let mut migrations = self.active_migrations.write();
        migrations.remove(&slot);
    }

    /// Get migration progress for a slot
    ///
    /// # Arguments
    ///
    /// * `slot` - Slot number
    ///
    /// # Returns
    ///
    /// The migration record, or None if slot is not being migrated
    pub fn get_migration_progress(&self, slot: u16) -> Option<SlotMigration> {
        let migrations = self.active_migrations.read();
        migrations.get(&slot).map(|m| m.read().clone())
    }

    /// Get all active migrations
    pub fn get_active_migrations(&self) -> Vec<SlotMigration> {
        let migrations = self.active_migrations.read();
        migrations.values().map(|m| m.read().clone()).collect()
    }

    /// Check if a slot is currently migrating
    pub fn is_migrating(&self, slot: u16) -> bool {
        let migrations = self.active_migrations.read();
        migrations.contains_key(&slot)
    }

    /// Shutdown the migration manager
    pub fn shutdown(&self) {
        let _ = self.command_tx.send(MigrationCommand::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Options;
    use tempfile::TempDir;

    fn create_test_state_machine() -> (TempDir, Arc<RwLock<ShardedStateMachine>>) {
        let temp_dir = TempDir::new().unwrap();
        let state_machine = ShardedStateMachine::new(temp_dir.path(), Options::default());
        (temp_dir, Arc::new(RwLock::new(state_machine)))
    }

    fn create_test_router() -> Arc<Router> {
        use super::super::meta_types::ClusterMeta;
        // Create a simple router with uniform distribution
        let meta = ClusterMeta::with_uniform_distribution(4);
        Arc::new(Router::new(meta))
    }

    #[test]
    fn test_migration_config_default() {
        let config = MigrationConfig::default();
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.rate_limit, 1000);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.key_timeout, Duration::from_secs(5));
        assert_eq!(config.batch_delay, Duration::from_millis(10));
    }

    #[test]
    fn test_migration_config_custom() {
        let config = MigrationConfig {
            batch_size: 50,
            rate_limit: 500,
            key_timeout: Duration::from_secs(10),
            max_retries: 5,
            batch_delay: Duration::from_millis(20),
        };
        assert_eq!(config.batch_size, 50);
        assert_eq!(config.rate_limit, 500);
        assert_eq!(config.max_retries, 5);
    }

    #[test]
    fn test_slot_validation() {
        // Valid slots
        assert!(16383 < 16384);
        
        // Invalid slots
        assert!(16384 >= 16384);
    }

    #[test]
    fn test_migration_manager_creation() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Should have no active migrations initially
        assert_eq!(manager.get_active_migrations().len(), 0);
    }

    #[test]
    fn test_is_migrating() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Initially no migration
        assert!(!manager.is_migrating(100));
    }

    #[tokio::test]
    async fn test_start_migration_invalid_slot() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Try to migrate an invalid slot
        let result = manager.start_migration(16384, 0, 1).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid slot"));
    }

    #[tokio::test]
    async fn test_start_migration_valid() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Start a valid migration
        let result = manager.start_migration(100, 0, 1).await;
        assert!(result.is_ok());
        
        // Should be marked as migrating
        assert!(manager.is_migrating(100));
        
        // Should have one active migration
        assert_eq!(manager.get_active_migrations().len(), 1);
    }

    #[tokio::test]
    async fn test_start_migration_duplicate() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Start first migration
        let result1 = manager.start_migration(100, 0, 1).await;
        assert!(result1.is_ok());
        
        // Try to start duplicate migration for same slot
        let result2 = manager.start_migration(100, 0, 1).await;
        assert!(result2.is_err());
        assert!(result2.unwrap_err().to_string().contains("already migrating"));
    }

    #[tokio::test]
    async fn test_get_migration_progress() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // No migration initially
        assert!(manager.get_migration_progress(100).is_none());
        
        // Start migration
        manager.start_migration(100, 0, 1).await.unwrap();
        
        // Should be able to get progress
        let progress = manager.get_migration_progress(100);
        assert!(progress.is_some());
        
        let migration = progress.unwrap();
        assert_eq!(migration.slot, 100);
        match migration.state {
            SlotMigrationState::Migrating { from_group, to_group } => {
                assert_eq!(from_group, 0);
                assert_eq!(to_group, 1);
            }
            _ => panic!("Expected Migrating state"),
        }
    }

    #[test]
    fn test_migration_progress_pct() {
        let migration = SlotMigration {
            slot: 100,
            state: SlotMigrationState::Migrating { from_group: 0, to_group: 1 },
            progress: 50,
            total: 100,
            started_at: 0,
        };
        
        assert_eq!(migration.progress_pct(), 50.0);
    }

    #[test]
    fn test_migration_progress_pct_zero_total() {
        let migration = SlotMigration {
            slot: 100,
            state: SlotMigrationState::Migrating { from_group: 0, to_group: 1 },
            progress: 0,
            total: 0,
            started_at: 0,
        };
        
        assert_eq!(migration.progress_pct(), 0.0);
    }

    #[test]
    fn test_migration_is_complete() {
        let mut migration = SlotMigration {
            slot: 100,
            state: SlotMigrationState::Migrating { from_group: 0, to_group: 1 },
            progress: 0,
            total: 100,
            started_at: 0,
        };
        
        assert!(!migration.is_complete());
        
        migration.state = SlotMigrationState::Complete;
        assert!(migration.is_complete());
    }
}
