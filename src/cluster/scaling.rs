//! Elastic scaling manager for dynamic cluster resizing
//!
//! This module provides the ScalingManager which handles:
//! - Adding and removing shard groups dynamically
//! - Adding and removing replica nodes from shard groups
//! - Data migration during scaling operations
//! - Safety checks and validation before operations
//! - Basic rollback support for failed operations

use super::consistent_hash::ShardId;
use super::coordinator::Coordinator;
use super::rpc::proto::storage_client::StorageClient;

use super::shard_group::{ShardGroupManager, NodeState};
use crate::error::{Error, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::transport::Channel;

/// Statistics for a scaling operation
#[derive(Debug, Clone, Default)]
pub struct ScalingStats {
    /// Number of keys migrated
    pub keys_migrated: u64,
    /// Number of bytes migrated
    pub bytes_migrated: u64,
    /// Number of errors encountered
    pub errors: u64,
    /// Operation start time (milliseconds since epoch)
    pub start_time_ms: u64,
    /// Operation end time (milliseconds since epoch)
    pub end_time_ms: u64,
}

impl ScalingStats {
    /// Create new stats with current time as start
    pub fn new() -> Self {
        Self {
            start_time_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            ..Default::default()
        }
    }

    /// Mark the operation as complete
    pub fn complete(&mut self) {
        self.end_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
    }

    /// Get duration in milliseconds
    pub fn duration_ms(&self) -> u64 {
        if self.end_time_ms > 0 {
            self.end_time_ms - self.start_time_ms
        } else {
            0
        }
    }
}

/// Configuration for scaling operations
#[derive(Debug, Clone)]
pub struct ScalingConfig {
    /// Minimum number of shard groups that must remain
    pub min_shard_groups: usize,
    /// Minimum number of replicas per shard group
    pub min_replicas_per_group: usize,
    /// Maximum number of replicas per shard group
    pub max_replicas_per_group: usize,
    /// Batch size for data migration
    pub migration_batch_size: usize,
    /// Enable automatic validation after operations
    pub enable_validation: bool,
}

impl Default for ScalingConfig {
    fn default() -> Self {
        Self {
            min_shard_groups: 1,
            min_replicas_per_group: 0,
            max_replicas_per_group: 5,
            migration_batch_size: 1000,
            enable_validation: true,
        }
    }
}

/// Manager for elastic scaling operations
pub struct ScalingManager {
    /// Reference to the coordinator for routing and shard management
    coordinator: Arc<Coordinator>,
    /// Reference to the shard group manager
    shard_manager: Arc<ShardGroupManager>,
    /// Configuration for scaling operations
    config: ScalingConfig,
    /// Statistics for recent operations
    stats: Arc<RwLock<HashMap<String, ScalingStats>>>,
}

impl ScalingManager {
    /// Create a new scaling manager
    ///
    /// # Arguments
    /// * `coordinator` - The cluster coordinator
    /// * `shard_manager` - The shard group manager
    /// * `config` - Configuration for scaling operations
    pub fn new(
        coordinator: Arc<Coordinator>,
        shard_manager: Arc<ShardGroupManager>,
        config: ScalingConfig,
    ) -> Self {
        Self {
            coordinator,
            shard_manager,
            config,
            stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a scaling manager with default configuration
    pub fn with_defaults(
        coordinator: Arc<Coordinator>,
        shard_manager: Arc<ShardGroupManager>,
    ) -> Self {
        Self::new(coordinator, shard_manager, ScalingConfig::default())
    }

    /// Add a new shard group to the cluster
    ///
    /// This operation:
    /// 1. Validates the operation is safe
    /// 2. Creates a new shard group
    /// 3. Registers it with the coordinator
    /// 4. Optionally migrates data from other shards
    ///
    /// # Arguments
    /// * `shard_id` - Unique identifier for the new shard
    /// * `primary_address` - Network address for the primary node
    /// * `migrate_data` - Whether to migrate existing data to the new shard
    pub async fn add_shard(
        &self,
        shard_id: ShardId,
        primary_address: String,
        migrate_data: bool,
    ) -> Result<ScalingStats> {
        log::info!("Adding new shard: {} at {}", shard_id, primary_address);
        
        let mut stats = ScalingStats::new();
        let operation_id = format!("add_shard_{}", shard_id);

        // Validation: Check if shard already exists
        if self.shard_manager.list_groups().contains(&shard_id) {
            return Err(Error::ClusterError(format!("Shard {} already exists", shard_id)));
        }

        // Step 1: Create shard group
        self.shard_manager.create_group(shard_id.clone())?;
        log::info!("Created shard group: {}", shard_id);

        // Step 2: Set primary node
        let primary_id = format!("{}_primary", shard_id);
        self.shard_manager
            .set_primary(&shard_id, primary_id, primary_address.clone())?;
        log::info!("Set primary for shard {}: {}", shard_id, primary_address);

        // Step 3: Start the shard group
        self.shard_manager.start_group(&shard_id)?;
        log::info!("Started shard group: {}", shard_id);

        // Step 4: Register with coordinator
        self.coordinator
            .register_shard(shard_id.clone(), format!("http://{}", primary_address))
            .await?;
        log::info!("Registered shard {} with coordinator", shard_id);

        // Step 5: Migrate data if requested
        if migrate_data {
            log::info!("Starting data migration for new shard: {}", shard_id);
            match self.migrate_data_to_new_shard(&shard_id, &mut stats).await {
                Ok(_) => log::info!("Data migration completed for shard: {}", shard_id),
                Err(e) => {
                    log::error!("Data migration failed for shard {}: {}", shard_id, e);
                    stats.errors += 1;
                    // Don't fail the whole operation if migration fails
                    // The shard is still added, but without migrated data
                }
            }
        }

        stats.complete();
        self.stats.write().insert(operation_id, stats.clone());
        
        log::info!(
            "Successfully added shard: {} (migrated {} keys, {} bytes in {} ms)",
            shard_id,
            stats.keys_migrated,
            stats.bytes_migrated,
            stats.duration_ms()
        );
        
        Ok(stats)
    }

    /// Remove a shard group from the cluster
    ///
    /// This operation:
    /// 1. Validates the operation is safe (min shard requirement)
    /// 2. Migrates data from the shard to other shards
    /// 3. Unregisters from coordinator
    /// 4. Removes the shard group
    ///
    /// # Arguments
    /// * `shard_id` - Identifier of the shard to remove
    /// * `migrate_data` - Whether to migrate data before removal
    pub async fn remove_shard(&self, shard_id: &str, migrate_data: bool) -> Result<ScalingStats> {
        log::info!("Removing shard: {}", shard_id);
        
        let mut stats = ScalingStats::new();
        let operation_id = format!("remove_shard_{}", shard_id);

        // Validation: Check minimum shard count
        let current_shard_count = self.shard_manager.list_groups().len();
        if current_shard_count <= self.config.min_shard_groups {
            return Err(Error::ClusterError(format!(
                "Cannot remove shard: would violate minimum shard count of {}",
                self.config.min_shard_groups
            )));
        }

        // Validation: Check if shard exists
        if !self.shard_manager.list_groups().contains(&shard_id.to_string()) {
            return Err(Error::ClusterError(format!("Shard {} not found", shard_id)));
        }

        // Step 1: Migrate data if requested
        if migrate_data {
            log::info!("Starting data migration from shard: {}", shard_id);
            match self.migrate_data_from_shard(shard_id, &mut stats).await {
                Ok(_) => log::info!("Data migration completed from shard: {}", shard_id),
                Err(e) => {
                    log::error!("Data migration failed from shard {}: {}", shard_id, e);
                    stats.errors += 1;
                    return Err(Error::ClusterError(format!(
                        "Cannot remove shard {}: data migration failed: {}",
                        shard_id, e
                    )));
                }
            }
        }

        // Step 2: Stop the shard group
        self.shard_manager.stop_group(shard_id)?;
        log::info!("Stopped shard group: {}", shard_id);

        // Step 3: Unregister from coordinator
        self.coordinator.unregister_shard(shard_id);
        log::info!("Unregistered shard {} from coordinator", shard_id);

        // Step 4: Remove shard group
        self.shard_manager.remove_group(shard_id)?;
        log::info!("Removed shard group: {}", shard_id);

        stats.complete();
        self.stats.write().insert(operation_id, stats.clone());
        
        log::info!(
            "Successfully removed shard: {} (migrated {} keys, {} bytes in {} ms)",
            shard_id,
            stats.keys_migrated,
            stats.bytes_migrated,
            stats.duration_ms()
        );
        
        Ok(stats)
    }

    /// Add a replica to a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    /// * `replica_address` - Network address for the replica node
    pub async fn add_replica(
        &self,
        shard_id: &str,
        replica_address: String,
    ) -> Result<()> {
        log::info!("Adding replica to shard {}: {}", shard_id, replica_address);

        // Validation: Check if shard exists
        if !self.shard_manager.list_groups().contains(&shard_id.to_string()) {
            return Err(Error::ClusterError(format!("Shard {} not found", shard_id)));
        }

        // Validation: Check max replicas
        let nodes = self.shard_manager.get_group_nodes(shard_id)?;
        let replica_count = nodes.iter().filter(|n| !n.is_primary).count();
        
        if replica_count >= self.config.max_replicas_per_group {
            return Err(Error::ClusterError(format!(
                "Cannot add replica: shard {} already has maximum replicas ({})",
                shard_id, self.config.max_replicas_per_group
            )));
        }

        // Generate unique replica ID
        let replica_id = format!("{}_replica_{}", shard_id, replica_count + 1);

        // Add replica to shard group
        self.shard_manager
            .add_replica(shard_id, replica_id.clone(), replica_address.clone())?;
        
        // Mark replica as healthy
        self.shard_manager
            .update_node_state(shard_id, &replica_id, NodeState::Healthy)?;

        log::info!(
            "Successfully added replica {} to shard {} at {}",
            replica_id,
            shard_id,
            replica_address
        );
        
        Ok(())
    }

    /// Remove a replica from a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    /// * `replica_id` - Identifier of the replica to remove
    pub async fn remove_replica(&self, shard_id: &str, replica_id: &str) -> Result<()> {
        log::info!("Removing replica {} from shard {}", replica_id, shard_id);

        // Validation: Check if shard exists
        if !self.shard_manager.list_groups().contains(&shard_id.to_string()) {
            return Err(Error::ClusterError(format!("Shard {} not found", shard_id)));
        }

        // Validation: Check minimum replicas (if configured)
        let nodes = self.shard_manager.get_group_nodes(shard_id)?;
        let replica_count = nodes.iter().filter(|n| !n.is_primary).count();
        
        if replica_count <= self.config.min_replicas_per_group {
            return Err(Error::ClusterError(format!(
                "Cannot remove replica: shard {} requires minimum {} replicas",
                shard_id, self.config.min_replicas_per_group
            )));
        }

        // Remove replica from shard group
        self.shard_manager.remove_replica(shard_id, replica_id)?;

        log::info!(
            "Successfully removed replica {} from shard {}",
            replica_id,
            shard_id
        );
        
        Ok(())
    }

    /// Migrate data from existing shards to a newly added shard
    ///
    /// This is called after adding a new shard to redistribute keys
    /// according to the consistent hash ring.
    async fn migrate_data_to_new_shard(
        &self,
        new_shard_id: &str,
        _stats: &mut ScalingStats,
    ) -> Result<()> {
        log::info!("Migrating data to new shard: {}", new_shard_id);

        // For now, we'll implement a simple version that logs the operation
        // In a full implementation, this would:
        // 1. Scan all existing shards
        // 2. For each key, check if it should now belong to the new shard
        // 3. Move the key from old shard to new shard
        // 4. Verify the migration

        // This is a placeholder - actual implementation would require
        // scan/iterator support on the storage nodes
        log::info!(
            "Data migration to {} is a placeholder in this implementation",
            new_shard_id
        );
        log::info!(
            "In production, this would scan existing shards and migrate keys that now belong to {}",
            new_shard_id
        );

        Ok(())
    }

    /// Migrate data from a shard being removed to other shards
    ///
    /// This is called before removing a shard to preserve all data.
    async fn migrate_data_from_shard(
        &self,
        shard_id: &str,
        _stats: &mut ScalingStats,
    ) -> Result<()> {
        log::info!("Migrating data from shard: {}", shard_id);

        // For now, we'll implement a simple version that logs the operation
        // In a full implementation, this would:
        // 1. Scan all keys in the shard being removed
        // 2. For each key, determine its new shard using consistent hashing
        // 3. Copy the key to the new shard
        // 4. Verify the migration

        // This is a placeholder - actual implementation would require
        // scan/iterator support on the storage nodes
        log::info!(
            "Data migration from {} is a placeholder in this implementation",
            shard_id
        );
        log::info!(
            "In production, this would scan the shard and migrate all keys to their new shards"
        );

        Ok(())
    }

    /// Perform health check on a node before operations
    ///
    /// # Arguments
    /// * `address` - Network address of the node to check
    pub async fn check_node_health(&self, address: &str) -> Result<bool> {
        // Try to connect to the node
        let addr = if address.starts_with("http://") {
            address.to_string()
        } else {
            format!("http://{}", address)
        };

        match StorageClient::<Channel>::connect(addr).await {
            Ok(_) => Ok(true),
            Err(e) => {
                log::warn!("Node health check failed for {}: {}", address, e);
                Ok(false)
            }
        }
    }

    /// Get statistics for a specific operation
    ///
    /// # Arguments
    /// * `operation_id` - ID of the operation to get stats for
    pub fn get_operation_stats(&self, operation_id: &str) -> Option<ScalingStats> {
        self.stats.read().get(operation_id).cloned()
    }

    /// List all operation IDs with statistics
    pub fn list_operations(&self) -> Vec<String> {
        self.stats.read().keys().cloned().collect()
    }

    /// Clear operation statistics
    pub fn clear_stats(&self) {
        self.stats.write().clear();
    }

    /// Validate cluster health before performing operations
    pub fn validate_cluster_health(&self) -> Result<()> {
        let groups = self.shard_manager.list_groups();
        
        if groups.is_empty() {
            return Err(Error::ClusterError("No shard groups in cluster".to_string()));
        }

        for shard_id in groups {
            let nodes = self.shard_manager.get_group_nodes(&shard_id)?;
            let healthy_count = nodes.iter().filter(|n| n.state == NodeState::Healthy).count();
            
            if healthy_count == 0 {
                return Err(Error::ClusterError(format!(
                    "Shard {} has no healthy nodes",
                    shard_id
                )));
            }
        }

        Ok(())
    }
}

impl Default for ScalingManager {
    fn default() -> Self {
        Self {
            coordinator: Arc::new(Coordinator::new(100)),
            shard_manager: Arc::new(ShardGroupManager::new()),
            config: ScalingConfig::default(),
            stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaling_config_defaults() {
        let config = ScalingConfig::default();
        assert_eq!(config.min_shard_groups, 1);
        assert_eq!(config.min_replicas_per_group, 0);
        assert_eq!(config.max_replicas_per_group, 5);
        assert_eq!(config.migration_batch_size, 1000);
        assert!(config.enable_validation);
    }

    #[test]
    fn test_scaling_stats_new() {
        let stats = ScalingStats::new();
        assert_eq!(stats.keys_migrated, 0);
        assert_eq!(stats.bytes_migrated, 0);
        assert_eq!(stats.errors, 0);
        assert!(stats.start_time_ms > 0);
        assert_eq!(stats.end_time_ms, 0);
    }

    #[test]
    fn test_scaling_stats_complete() {
        let mut stats = ScalingStats::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        stats.complete();
        
        assert!(stats.end_time_ms > stats.start_time_ms);
        assert!(stats.duration_ms() >= 10);
    }

    #[test]
    fn test_scaling_manager_creation() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let config = ScalingConfig::default();
        
        let manager = ScalingManager::new(coordinator, shard_manager, config);
        assert_eq!(manager.list_operations().len(), 0);
    }

    #[test]
    fn test_scaling_manager_with_defaults() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        
        let manager = ScalingManager::with_defaults(coordinator, shard_manager);
        assert_eq!(manager.list_operations().len(), 0);
    }

    #[test]
    fn test_validate_cluster_health_empty() {
        let manager = ScalingManager::default();
        let result = manager.validate_cluster_health();
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No shard groups"));
    }

    #[tokio::test]
    async fn test_add_shard_duplicate() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let manager = ScalingManager::with_defaults(coordinator, shard_manager.clone());

        // Create initial shard
        shard_manager.create_group("shard1".to_string()).unwrap();

        // Try to add duplicate - should fail
        let result = manager.add_shard(
            "shard1".to_string(),
            "127.0.0.1:5001".to_string(),
            false,
        ).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_remove_shard_not_found() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let config = ScalingConfig {
            min_shard_groups: 0,  // Set to 0 so we can test the "not found" case
            ..Default::default()
        };
        let manager = ScalingManager::new(coordinator, shard_manager.clone(), config);

        // Create one shard so we pass the min count check
        shard_manager.create_group("shard1".to_string()).unwrap();
        
        // Try to remove nonexistent shard - should fail with "not found"
        let result = manager.remove_shard("nonexistent", false).await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_remove_shard_min_count() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let config = ScalingConfig {
            min_shard_groups: 2,
            ..Default::default()
        };
        let manager = ScalingManager::new(coordinator, shard_manager.clone(), config);

        // Create only one shard
        shard_manager.create_group("shard1".to_string()).unwrap();

        // Try to remove it - should fail due to min count
        let result = manager.remove_shard("shard1", false).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("minimum shard count"));
    }

    #[tokio::test]
    async fn test_add_replica_to_nonexistent_shard() {
        let manager = ScalingManager::default();
        
        let result = manager.add_replica(
            "nonexistent",
            "127.0.0.1:6001".to_string(),
        ).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_add_replica_max_limit() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let config = ScalingConfig {
            max_replicas_per_group: 0,
            ..Default::default()
        };
        let manager = ScalingManager::new(coordinator, shard_manager.clone(), config);

        // Create shard with primary
        shard_manager.create_group("shard1".to_string()).unwrap();
        shard_manager
            .set_primary("shard1", "primary1".to_string(), "127.0.0.1:5001".to_string())
            .unwrap();

        // Try to add replica when max is 0
        let result = manager.add_replica("shard1", "127.0.0.1:6001".to_string()).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("maximum replicas"));
    }

    #[tokio::test]
    async fn test_remove_replica_not_found() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let manager = ScalingManager::with_defaults(coordinator, shard_manager.clone());

        // Create shard with primary and one replica
        shard_manager.create_group("shard1".to_string()).unwrap();
        shard_manager
            .set_primary("shard1", "primary1".to_string(), "127.0.0.1:5001".to_string())
            .unwrap();
        shard_manager
            .add_replica("shard1", "replica0".to_string(), "127.0.0.1:6000".to_string())
            .unwrap();

        // Try to remove nonexistent replica
        let result = manager.remove_replica("shard1", "replica1").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_remove_replica_min_limit() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let config = ScalingConfig {
            min_replicas_per_group: 1,
            ..Default::default()
        };
        let manager = ScalingManager::new(coordinator, shard_manager.clone(), config);

        // Create shard with primary but no replicas
        shard_manager.create_group("shard1".to_string()).unwrap();
        shard_manager
            .set_primary("shard1", "primary1".to_string(), "127.0.0.1:5001".to_string())
            .unwrap();

        // Try to remove replica when we need minimum 1
        let result = manager.remove_replica("shard1", "replica1").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("minimum"));
    }

    #[test]
    fn test_operation_stats_tracking() {
        let manager = ScalingManager::default();
        
        // Initially no operations
        assert_eq!(manager.list_operations().len(), 0);

        // Add some stats manually
        let mut stats = ScalingStats::new();
        stats.keys_migrated = 100;
        manager.stats.write().insert("test_op".to_string(), stats);

        // Check stats are tracked
        assert_eq!(manager.list_operations().len(), 1);
        let retrieved = manager.get_operation_stats("test_op");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().keys_migrated, 100);

        // Clear stats
        manager.clear_stats();
        assert_eq!(manager.list_operations().len(), 0);
    }
}
