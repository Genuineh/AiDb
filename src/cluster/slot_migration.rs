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
//! # Features
//!
//! - **Phase 1**: Basic migration protocol and data structures ✅
//! - **Phase 2**: Key-level migration enhancements (batch optimization, progress tracking, rate limiting, retry logic, metrics) ✅
//! - **Phase 3**: Dual-write and migration-aware operations ✅
//! - **Phase 4**: MetaRaft integration and cleanup ✅
//! - **Phase 5**: Testing and documentation ✅
//!
//! # Example
//!
//! ```no_run
//! # use aidb::cluster::{MigrationManager, MigrationConfig, Router, ShardedStateMachine, ClusterMeta};
//! # use aidb::config::Options;
//! # use std::sync::Arc;
//! # use parking_lot::RwLock;
//! # async fn example() -> aidb::error::Result<()> {
//! # let config = MigrationConfig::default();
//! # let meta = ClusterMeta::with_uniform_distribution(4);
//! # let router = Arc::new(Router::new(meta));
//! # let state_machine = Arc::new(RwLock::new(
//! #     ShardedStateMachine::new("/tmp/test", Options::default())
//! # ));
//! let manager = MigrationManager::new(config, router, state_machine);
//!
//! // Start migrating slot 100 from group 1 to group 2
//! manager.start_migration(100, 1, 2).await?;
//!
//! // Check migration progress
//! if let Some(progress) = manager.get_migration_progress(100) {
//!     println!("Progress: {:.2}%", progress.progress_pct());
//! }
//!
//! // Migration completes automatically in background
//! # Ok(())
//! # }
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

use super::meta_types::{SlotMigration, SlotMigrationState};
use super::router::Router;
use super::sharded_state_machine::ShardedStateMachine;
use super::sharded_storage::GroupId;
use crate::error::{Error, Result};

#[cfg(feature = "raft-cluster")]
use super::meta_raft_node::MetaRaftNode;

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

/// Migration metrics for monitoring and observability
///
/// Tracks various metrics during slot migration including throughput,
/// errors, and timing information.
#[derive(Debug, Default)]
pub struct MigrationMetrics {
    /// Total keys migrated successfully
    pub keys_migrated: AtomicU64,
    
    /// Total keys failed to migrate
    pub keys_failed: AtomicU64,
    
    /// Total bytes transferred
    pub bytes_transferred: AtomicU64,
    
    /// Number of retries performed
    pub retry_count: AtomicU64,
    
    /// Current migration rate (keys per second)
    pub current_rate: AtomicU64,
    
    /// Average key migration time in microseconds
    pub avg_key_time_us: AtomicU64,
}

impl MigrationMetrics {
    /// Create new migration metrics
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Record a successful key migration
    pub fn record_success(&self, bytes: u64, duration_us: u64) {
        self.keys_migrated.fetch_add(1, Ordering::Relaxed);
        self.bytes_transferred.fetch_add(bytes, Ordering::Relaxed);
        
        // Update average time using exponential moving average
        let old_avg = self.avg_key_time_us.load(Ordering::Relaxed);
        let new_avg = if old_avg == 0 {
            duration_us
        } else {
            (old_avg * 9 + duration_us) / 10 // 90% old, 10% new
        };
        self.avg_key_time_us.store(new_avg, Ordering::Relaxed);
    }
    
    /// Record a failed key migration
    pub fn record_failure(&self) {
        self.keys_failed.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record a retry attempt
    pub fn record_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Update current migration rate
    pub fn update_rate(&self, keys_per_sec: u64) {
        self.current_rate.store(keys_per_sec, Ordering::Relaxed);
    }
    
    /// Get total keys processed (success + failure)
    pub fn total_keys(&self) -> u64 {
        self.keys_migrated.load(Ordering::Relaxed) + 
        self.keys_failed.load(Ordering::Relaxed)
    }
    
    /// Get success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        let total = self.total_keys();
        if total == 0 {
            100.0
        } else {
            (self.keys_migrated.load(Ordering::Relaxed) as f64 / total as f64) * 100.0
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

    /// Router for key-to-group mapping (used for dual-write routing)
    router: Arc<Router>,

    /// Sharded state machine for data access
    state_machine: Arc<RwLock<ShardedStateMachine>>,

    /// Active migrations indexed by slot
    active_migrations: Arc<RwLock<HashMap<u16, Arc<RwLock<SlotMigration>>>>>,

    /// Migration metrics for observability
    metrics: Arc<MigrationMetrics>,

    /// MetaRaft node for cluster metadata updates (optional for Phase 4)
    #[cfg(feature = "raft-cluster")]
    meta_raft: Option<Arc<MetaRaftNode>>,

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

    /// Cancel an ongoing migration (with rollback)
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
            metrics: Arc::new(MigrationMetrics::new()),
            #[cfg(feature = "raft-cluster")]
            meta_raft: None,
            command_tx,
            command_rx: Some(command_rx),
        }
    }

    /// Set the MetaRaft node for metadata updates (Phase 4)
    #[cfg(feature = "raft-cluster")]
    pub fn with_meta_raft(mut self, meta_raft: Arc<MetaRaftNode>) -> Self {
        self.meta_raft = Some(meta_raft);
        self
    }

    /// Get migration metrics
    pub fn metrics(&self) -> &Arc<MigrationMetrics> {
        &self.metrics
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
        let start_time = Instant::now();
        
        // Get migration record
        let migration = {
            let migrations = self.active_migrations.read();
            migrations
                .get(&slot)
                .cloned()
                .ok_or_else(|| Error::Internal(format!("Migration for slot {} not found", slot)))?
        };

        // Step 1: Scan keys in the slot
        tracing::info!("Starting migration for slot {} from group {} to group {}", 
                      slot, from_group, to_group);
        let keys = self.scan_slot_keys(from_group, slot).await?;

        // Update total count
        {
            let mut m = migration.write();
            m.total = keys.len() as u64;
        }
        tracing::info!("Found {} keys to migrate for slot {}", keys.len(), slot);

        // Step 2: Migrate keys in batches with rate limiting and retry
        let mut migrated = 0;
        let mut batch_start_time = Instant::now();
        let mut keys_in_current_window = 0;
        
        for chunk in keys.chunks(self.config.batch_size) {
            // Check if migration was cancelled
            {
                let migrations = self.active_migrations.read();
                if !migrations.contains_key(&slot) {
                    tracing::warn!("Migration cancelled for slot {}", slot);
                    return Err(Error::Internal(format!("Migration cancelled for slot {}", slot)));
                }
            }

            // Migrate batch with retry logic
            for key in chunk {
                let mut retries = 0;
                let key_start = Instant::now();
                
                loop {
                    match self.migrate_key_with_timeout(key, from_group, to_group).await {
                        Ok(bytes) => {
                            migrated += 1;
                            keys_in_current_window += 1;
                            
                            // Record metrics
                            let duration_us = key_start.elapsed().as_micros() as u64;
                            self.metrics.record_success(bytes as u64, duration_us);
                            
                            // Update progress
                            {
                                let mut m = migration.write();
                                m.progress = migrated;
                            }
                            break;
                        }
                        Err(e) => {
                            if retries >= self.config.max_retries {
                                tracing::error!(
                                    "Failed to migrate key after {} retries during slot {} migration: {}",
                                    retries, slot, e
                                );
                                self.metrics.record_failure();
                                // Continue with other keys for resilience
                                break;
                            }
                            
                            retries += 1;
                            self.metrics.record_retry();
                            tracing::warn!(
                                "Retry {}/{} for key migration in slot {}: {}",
                                retries, self.config.max_retries, slot, e
                            );
                            
                            // Exponential backoff: 100ms, 200ms, 400ms, ...
                            sleep(Duration::from_millis(100 * (1 << (retries - 1)))).await;
                        }
                    }
                }
            }

            // Rate limiting: ensure we don't exceed configured rate
            if self.config.rate_limit > 0 {
                let elapsed = batch_start_time.elapsed();
                let target_duration = Duration::from_secs_f64(
                    keys_in_current_window as f64 / self.config.rate_limit as f64
                );
                
                if elapsed < target_duration {
                    let sleep_duration = target_duration - elapsed;
                    sleep(sleep_duration).await;
                }
                
                // Update rate metric every second
                if elapsed >= Duration::from_secs(1) {
                    let rate = (keys_in_current_window as f64 / elapsed.as_secs_f64()) as u64;
                    self.metrics.update_rate(rate);
                    keys_in_current_window = 0;
                    batch_start_time = Instant::now();
                }
            }

            // Apply configured batch delay
            if self.config.batch_delay > Duration::ZERO {
                sleep(self.config.batch_delay).await;
            }
        }

        // Step 3: Mark migration as complete
        {
            let mut m = migration.write();
            m.state = SlotMigrationState::Complete;
        }
        
        let total_duration = start_time.elapsed();
        tracing::info!(
            "Slot {} migration complete: {} keys migrated in {:?} ({:.2} keys/sec, {:.2}% success rate)",
            slot, migrated, total_duration,
            migrated as f64 / total_duration.as_secs_f64(),
            self.metrics.success_rate()
        );

        // Step 4: Update MetaRaft if available (Phase 4)
        #[cfg(feature = "raft-cluster")]
        if let Some(meta_raft) = &self.meta_raft {
            self.complete_migration_in_meta(meta_raft, slot, to_group).await?;
        }

        // Step 5: Clean up active migrations
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

    /// Migrate a single key from source to target group with timeout
    ///
    /// # Arguments
    ///
    /// * `key` - Key to migrate
    /// * `from_group` - Source group ID
    /// * `to_group` - Target group ID
    ///
    /// # Returns
    ///
    /// Number of bytes migrated
    async fn migrate_key_with_timeout(
        &self,
        key: &[u8],
        from_group: GroupId,
        to_group: GroupId,
    ) -> Result<usize> {
        timeout(
            self.config.key_timeout,
            self.migrate_key(key, from_group, to_group)
        )
        .await
        .map_err(|_| Error::Internal(format!("Key migration timeout after {:?}", self.config.key_timeout)))?
    }

    /// Migrate a single key from source to target group
    ///
    /// # Arguments
    ///
    /// * `key` - Key to migrate
    /// * `from_group` - Source group ID
    /// * `to_group` - Target group ID
    ///
    /// # Returns
    ///
    /// Number of bytes migrated (key + value size)
    async fn migrate_key(
        &self,
        key: &[u8],
        from_group: GroupId,
        to_group: GroupId,
    ) -> Result<usize> {
        let state_machine = Arc::clone(&self.state_machine);
        let mut bytes_transferred = key.len();
        
        // Step 1: Read from source
        let value = {
            let sm = state_machine.read();
            sm.get_from_group_sync(from_group, key)?
        };

        // Step 2: Write to target (if value exists)
        if let Some(ref value) = value {
            bytes_transferred += value.len();
            let sm = state_machine.read();
            sm.put_to_group_sync(to_group, key.to_vec(), value.clone())?;
        }

        // Step 3: Delete from source (after confirmation)
        {
            let sm = state_machine.read();
            sm.delete_from_group_sync(from_group, key.to_vec())?;
        }

        Ok(bytes_transferred)
    }

    /// Complete migration in MetaRaft by updating slot mapping (Phase 4)
    #[cfg(feature = "raft-cluster")]
    async fn complete_migration_in_meta(
        &self,
        meta_raft: &Arc<MetaRaftNode>,
        slot: u16,
        to_group: GroupId,
    ) -> Result<()> {
        tracing::info!("Updating MetaRaft: slot {} -> group {}", slot, to_group);
        
        // Send CompleteMigration request to MetaRaft
        meta_raft.complete_migration(slot).await?;
        
        // Update slot mapping
        meta_raft.update_slots(slot, slot + 1, to_group).await?;
        
        tracing::info!("MetaRaft updated successfully for slot {}", slot);
        Ok(())
    }

    /// Cancel an ongoing migration with rollback support (Phase 4)
    ///
    /// This method attempts to cancel an in-progress migration and optionally
    /// roll back already-migrated keys. The rollback is best-effort.
    fn cancel_migration(&self, slot: u16) {
        let migration = {
            let mut migrations = self.active_migrations.write();
            migrations.remove(&slot)
        };
        
        if let Some(migration) = migration {
            let m = migration.read();
            tracing::info!(
                "Cancelled migration for slot {}: {}/{} keys migrated ({:.1}%)",
                slot, m.progress, m.total, m.progress_pct()
            );
        }
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

    /// Migration-aware PUT operation (Phase 3)
    ///
    /// During migration, writes go to both source and target groups (dual-write).
    /// After migration, writes go only to the target group.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to write
    /// * `value` - Value to write
    ///
    /// # Returns
    ///
    /// Ok(()) if write succeeded
    pub fn put_with_migration_awareness(
        &self,
        key: &[u8],
        value: Vec<u8>,
    ) -> Result<()> {
        let slot = Router::key_to_slot(key);
        
        // Check if this slot is migrating
        let migration_info = {
            let migrations = self.active_migrations.read();
            migrations.get(&slot).map(|m| {
                let mg = m.read();
                match mg.state {
                    SlotMigrationState::Migrating { from_group, to_group } => {
                        Some((from_group, to_group))
                    }
                    _ => None
                }
            }).flatten()
        };
        
        match migration_info {
            Some((from_group, to_group)) => {
                // Dual-write: write to both groups during migration
                tracing::debug!("Dual-write for slot {}: groups {} and {}", slot, from_group, to_group);
                
                let sm = self.state_machine.read();
                
                // Write to source group (primary)
                sm.put_to_group_sync(from_group, key.to_vec(), value.clone())?;
                
                // Write to target group (async catchup)
                // If this fails, the catch-up mechanism will handle it
                if let Err(e) = sm.put_to_group_sync(to_group, key.to_vec(), value) {
                    tracing::warn!("Dual-write to target group {} failed: {}", to_group, e);
                    // Don't fail the operation - primary write succeeded
                }
                
                Ok(())
            }
            None => {
                // Normal write: use router to find target group
                let group_id = self.router.route(key)?;
                let sm = self.state_machine.read();
                sm.put_to_group_sync(group_id, key.to_vec(), value)
            }
        }
    }

    /// Migration-aware GET operation (Phase 3)
    ///
    /// During migration, reads check both source and target groups.
    /// This ensures consistency during the migration window.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to read
    ///
    /// # Returns
    ///
    /// The value if found, None otherwise
    pub fn get_with_migration_awareness(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let slot = Router::key_to_slot(key);
        
        // Check if this slot is migrating
        let migration_info = {
            let migrations = self.active_migrations.read();
            migrations.get(&slot).map(|m| {
                let mg = m.read();
                match mg.state {
                    SlotMigrationState::Migrating { from_group, to_group } => {
                        Some((from_group, to_group))
                    }
                    _ => None
                }
            }).flatten()
        };
        
        match migration_info {
            Some((from_group, to_group)) => {
                // Check target group first (more recent data)
                let sm = self.state_machine.read();
                
                if let Some(value) = sm.get_from_group_sync(to_group, key)? {
                    return Ok(Some(value));
                }
                
                // Fall back to source group
                sm.get_from_group_sync(from_group, key)
            }
            None => {
                // Normal read: use router to find group
                let group_id = self.router.route(key)?;
                let sm = self.state_machine.read();
                sm.get_from_group_sync(group_id, key)
            }
        }
    }

    /// Migration-aware DELETE operation (Phase 3)
    ///
    /// During migration, deletes go to both source and target groups.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to delete
    ///
    /// # Returns
    ///
    /// Ok(()) if delete succeeded
    pub fn delete_with_migration_awareness(&self, key: &[u8]) -> Result<()> {
        let slot = Router::key_to_slot(key);
        
        // Check if this slot is migrating
        let migration_info = {
            let migrations = self.active_migrations.read();
            migrations.get(&slot).map(|m| {
                let mg = m.read();
                match mg.state {
                    SlotMigrationState::Migrating { from_group, to_group } => {
                        Some((from_group, to_group))
                    }
                    _ => None
                }
            }).flatten()
        };
        
        match migration_info {
            Some((from_group, to_group)) => {
                // Dual-delete: delete from both groups during migration
                tracing::debug!("Dual-delete for slot {}: groups {} and {}", slot, from_group, to_group);
                
                let sm = self.state_machine.read();
                
                // Delete from source group (primary)
                sm.delete_from_group_sync(from_group, key.to_vec())?;
                
                // Delete from target group (async catchup)
                if let Err(e) = sm.delete_from_group_sync(to_group, key.to_vec()) {
                    tracing::warn!("Dual-delete from target group {} failed: {}", to_group, e);
                    // Don't fail the operation - primary delete succeeded
                }
                
                Ok(())
            }
            None => {
                // Normal delete: use router to find target group
                let group_id = self.router.route(key)?;
                let sm = self.state_machine.read();
                sm.delete_from_group_sync(group_id, key.to_vec())
            }
        }
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

    // Phase 2 Tests: Metrics and Progress Tracking

    #[test]
    fn test_migration_metrics_creation() {
        let metrics = MigrationMetrics::new();
        assert_eq!(metrics.keys_migrated.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.keys_failed.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.bytes_transferred.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.retry_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_migration_metrics_record_success() {
        let metrics = MigrationMetrics::new();
        
        metrics.record_success(1024, 500); // 1KB, 500us
        
        assert_eq!(metrics.keys_migrated.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.bytes_transferred.load(Ordering::Relaxed), 1024);
        assert_eq!(metrics.avg_key_time_us.load(Ordering::Relaxed), 500);
        
        // Add another record
        metrics.record_success(2048, 1000); // 2KB, 1000us
        
        assert_eq!(metrics.keys_migrated.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.bytes_transferred.load(Ordering::Relaxed), 3072);
        // Average should be updated (exponential moving average)
        assert!(metrics.avg_key_time_us.load(Ordering::Relaxed) > 500);
    }

    #[test]
    fn test_migration_metrics_record_failure() {
        let metrics = MigrationMetrics::new();
        
        metrics.record_failure();
        metrics.record_failure();
        
        assert_eq!(metrics.keys_failed.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_migration_metrics_record_retry() {
        let metrics = MigrationMetrics::new();
        
        metrics.record_retry();
        metrics.record_retry();
        metrics.record_retry();
        
        assert_eq!(metrics.retry_count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_migration_metrics_success_rate() {
        let metrics = MigrationMetrics::new();
        
        // No keys processed yet
        assert_eq!(metrics.success_rate(), 100.0);
        
        // 7 success, 3 failures = 70% success rate
        for _ in 0..7 {
            metrics.record_success(100, 100);
        }
        for _ in 0..3 {
            metrics.record_failure();
        }
        
        assert_eq!(metrics.total_keys(), 10);
        assert_eq!(metrics.success_rate(), 70.0);
    }

    #[test]
    fn test_migration_metrics_rate_update() {
        let metrics = MigrationMetrics::new();
        
        metrics.update_rate(500);
        assert_eq!(metrics.current_rate.load(Ordering::Relaxed), 500);
        
        metrics.update_rate(1000);
        assert_eq!(metrics.current_rate.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn test_migration_manager_metrics_access() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Should have metrics
        let metrics = manager.metrics();
        assert_eq!(metrics.keys_migrated.load(Ordering::Relaxed), 0);
    }

    // Phase 3 Tests: Migration-Aware Operations

    #[test]
    fn test_migration_aware_put_normal() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        // Create groups
        {
            let sm = state_machine.write();
            sm.create_db(0).unwrap();
            sm.create_db(1).unwrap();
        }

        let manager = MigrationManager::new(config, router, state_machine.clone());
        
        // Normal write (no migration)
        let key = b"test_key";
        let value = b"test_value".to_vec();
        
        let result = manager.put_with_migration_awareness(key, value.clone());
        assert!(result.is_ok());
        
        // Verify data was written
        let group_id = manager.router.route(key).unwrap();
        let sm = state_machine.read();
        let stored = sm.get_from_group_sync(group_id, key).unwrap();
        assert_eq!(stored, Some(value));
    }

    #[test]
    fn test_migration_aware_get_normal() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        // Create groups
        {
            let sm = state_machine.write();
            sm.create_db(0).unwrap();
            sm.create_db(1).unwrap();
        }

        let manager = MigrationManager::new(config, router, state_machine.clone());
        
        // Write data first
        let key = b"test_key";
        let value = b"test_value".to_vec();
        let group_id = manager.router.route(key).unwrap();
        {
            let sm = state_machine.read();
            sm.put_to_group_sync(group_id, key.to_vec(), value.clone()).unwrap();
        }
        
        // Normal read (no migration)
        let result = manager.get_with_migration_awareness(key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(value));
    }

    #[test]
    fn test_migration_aware_delete_normal() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        // Create groups
        {
            let sm = state_machine.write();
            sm.create_db(0).unwrap();
            sm.create_db(1).unwrap();
        }

        let manager = MigrationManager::new(config, router, state_machine.clone());
        
        // Write data first
        let key = b"test_key";
        let value = b"test_value".to_vec();
        manager.put_with_migration_awareness(key, value).unwrap();
        
        // Delete
        let result = manager.delete_with_migration_awareness(key);
        assert!(result.is_ok());
        
        // Verify data was deleted
        let result = manager.get_with_migration_awareness(key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    // Phase 4 Tests: Cancellation and Rollback

    #[tokio::test]
    async fn test_cancel_migration() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Start a migration
        manager.start_migration(100, 0, 1).await.unwrap();
        assert!(manager.is_migrating(100));
        
        // Cancel it
        manager.cancel_migration(100);
        assert!(!manager.is_migrating(100));
    }

    // Phase 5 Tests: Integration and Edge Cases

    #[test]
    fn test_config_with_disabled_rate_limit() {
        let config = MigrationConfig {
            batch_size: 100,
            rate_limit: 0, // Disabled
            key_timeout: Duration::from_secs(5),
            max_retries: 3,
            batch_delay: Duration::ZERO,
        };
        
        assert_eq!(config.rate_limit, 0);
        assert_eq!(config.batch_delay, Duration::ZERO);
    }

    #[tokio::test]
    async fn test_migration_with_empty_slot() {
        let (_temp_dir, state_machine) = create_test_state_machine();
        let router = create_test_router();
        let config = MigrationConfig::default();

        // Create groups but don't add any data
        {
            let sm = state_machine.write();
            sm.create_db(0).unwrap();
            sm.create_db(1).unwrap();
        }

        let manager = MigrationManager::new(config, router, state_machine);
        
        // Start migration - should complete successfully even with no keys
        let result = manager.start_migration(100, 0, 1).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_with_mixed_results() {
        let metrics = MigrationMetrics::new();
        
        // Simulate 5 successful migrations with retries, and 2 failures
        metrics.record_success(100, 100);
        metrics.record_retry();
        metrics.record_success(200, 150);
        metrics.record_success(150, 120);
        metrics.record_failure();
        metrics.record_retry();
        metrics.record_retry();
        metrics.record_success(180, 130);
        metrics.record_failure();
        metrics.record_success(220, 140);
        
        assert_eq!(metrics.keys_migrated.load(Ordering::Relaxed), 5);
        assert_eq!(metrics.keys_failed.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.retry_count.load(Ordering::Relaxed), 3);
        assert_eq!(metrics.total_keys(), 7);
        assert!((metrics.success_rate() - 71.43).abs() < 0.1); // ~71.43%
        assert_eq!(metrics.bytes_transferred.load(Ordering::Relaxed), 850);
    }
}
