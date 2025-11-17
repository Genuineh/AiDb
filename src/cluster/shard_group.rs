//! Shard Group management for AiDb distributed cluster
//!
//! A ShardGroup represents a logical unit consisting of:
//! - One Primary node: handles all writes and reads
//! - Multiple Replica nodes: handle reads and provide redundancy
//!
//! The ShardGroupManager manages multiple shard groups and their lifecycles.

use super::consistent_hash::ShardId;
use crate::error::{Error, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// State of a node within a shard group
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// Node is starting up
    Starting,
    /// Node is healthy and serving requests
    Healthy,
    /// Node is unhealthy but still in the group
    Unhealthy,
    /// Node is being removed from the group
    Removing,
    /// Node has been stopped
    Stopped,
}

impl NodeState {
    /// Check if the node can serve requests
    pub fn is_serving(&self) -> bool {
        matches!(self, NodeState::Healthy)
    }
}

/// Information about a node in a shard group
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Node identifier (unique within the group)
    pub id: String,
    /// Network address
    pub address: String,
    /// Current state of the node
    pub state: NodeState,
    /// Number of requests served
    pub request_count: u64,
    /// Whether this is a primary or replica node
    pub is_primary: bool,
}

impl NodeInfo {
    /// Create a new node info
    pub fn new(id: String, address: String, is_primary: bool) -> Self {
        Self { id, address, state: NodeState::Starting, request_count: 0, is_primary }
    }
}

/// State of a shard group
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardGroupState {
    /// Shard group is being initialized
    Initializing,
    /// Shard group is running normally
    Running,
    /// Shard group is in degraded state (e.g., primary down)
    Degraded,
    /// Shard group is being shut down
    ShuttingDown,
    /// Shard group has been stopped
    Stopped,
}

/// A shard group consisting of one primary and multiple replicas
#[derive(Debug)]
pub struct ShardGroup {
    /// Unique identifier for this shard group
    id: ShardId,
    /// Current state of the shard group
    state: ShardGroupState,
    /// The primary node (write + read)
    primary: Option<NodeInfo>,
    /// Replica nodes (read-only)
    replicas: HashMap<String, NodeInfo>,
}

impl ShardGroup {
    /// Create a new shard group
    ///
    /// # Arguments
    /// * `id` - Unique identifier for the shard group
    pub fn new(id: ShardId) -> Self {
        Self {
            id,
            state: ShardGroupState::Initializing,
            primary: None,
            replicas: HashMap::new(),
        }
    }

    /// Get the shard group ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the current state
    pub fn state(&self) -> ShardGroupState {
        self.state
    }

    /// Set the primary node
    ///
    /// # Arguments
    /// * `node_id` - Unique identifier for the node
    /// * `address` - Network address of the node
    pub fn set_primary(&mut self, node_id: String, address: String) -> Result<()> {
        if self.primary.is_some() {
            return Err(Error::ClusterError("Primary node already exists".to_string()));
        }

        let node = NodeInfo::new(node_id, address, true);
        self.primary = Some(node);
        log::info!("Set primary node for shard group {}", self.id);
        Ok(())
    }

    /// Get the primary node
    pub fn primary(&self) -> Option<&NodeInfo> {
        self.primary.as_ref()
    }

    /// Add a replica node
    ///
    /// # Arguments
    /// * `node_id` - Unique identifier for the node
    /// * `address` - Network address of the node
    pub fn add_replica(&mut self, node_id: String, address: String) -> Result<()> {
        if self.replicas.contains_key(&node_id) {
            return Err(Error::ClusterError(format!(
                "Replica {} already exists in shard group {}",
                node_id, self.id
            )));
        }

        let node = NodeInfo::new(node_id.clone(), address, false);
        self.replicas.insert(node_id.clone(), node);
        log::info!("Added replica {} to shard group {}", node_id, self.id);
        Ok(())
    }

    /// Remove a replica node
    ///
    /// # Arguments
    /// * `node_id` - Identifier of the replica to remove
    pub fn remove_replica(&mut self, node_id: &str) -> Result<()> {
        if let Some(node) = self.replicas.get_mut(node_id) {
            node.state = NodeState::Removing;
        }

        self.replicas.remove(node_id).ok_or_else(|| {
            Error::ClusterError(format!("Replica {} not found in shard group {}", node_id, self.id))
        })?;

        log::info!("Removed replica {} from shard group {}", node_id, self.id);
        Ok(())
    }

    /// Get all replicas
    pub fn replicas(&self) -> Vec<&NodeInfo> {
        self.replicas.values().collect()
    }

    /// Get a specific replica
    pub fn get_replica(&self, node_id: &str) -> Option<&NodeInfo> {
        self.replicas.get(node_id)
    }

    /// Get the number of replicas
    pub fn replica_count(&self) -> usize {
        self.replicas.len()
    }

    /// Update node state
    ///
    /// # Arguments
    /// * `node_id` - Identifier of the node
    /// * `state` - New state for the node
    pub fn update_node_state(&mut self, node_id: &str, state: NodeState) -> Result<()> {
        // Check if it's the primary
        if let Some(ref mut primary) = self.primary {
            if primary.id == node_id {
                primary.state = state;
                self.update_group_state();
                return Ok(());
            }
        }

        // Check replicas
        if let Some(replica) = self.replicas.get_mut(node_id) {
            replica.state = state;
            self.update_group_state();
            return Ok(());
        }

        Err(Error::ClusterError(format!(
            "Node {} not found in shard group {}",
            node_id, self.id
        )))
    }

    /// Update the overall shard group state based on node states
    fn update_group_state(&mut self) {
        // If shutting down or stopped, don't change state
        if matches!(self.state, ShardGroupState::ShuttingDown | ShardGroupState::Stopped) {
            return;
        }

        // Check primary state
        let primary_healthy = self.primary.as_ref().map(|p| p.state.is_serving()).unwrap_or(false);

        if !primary_healthy {
            self.state = ShardGroupState::Degraded;
            log::warn!("Shard group {} is degraded: primary not healthy", self.id);
        } else if self.primary.is_some() {
            self.state = ShardGroupState::Running;
        }
    }

    /// Start the shard group
    pub fn start(&mut self) -> Result<()> {
        if !matches!(self.state, ShardGroupState::Initializing | ShardGroupState::Stopped) {
            return Err(Error::ClusterError(format!(
                "Cannot start shard group {} in state {:?}",
                self.id, self.state
            )));
        }

        // Mark primary as healthy if exists
        if let Some(ref mut primary) = self.primary {
            primary.state = NodeState::Healthy;
        }

        // Mark all replicas as healthy
        for replica in self.replicas.values_mut() {
            replica.state = NodeState::Healthy;
        }

        self.update_group_state();
        log::info!("Started shard group {}", self.id);
        Ok(())
    }

    /// Stop the shard group
    pub fn stop(&mut self) -> Result<()> {
        self.state = ShardGroupState::ShuttingDown;

        // Mark primary as stopped
        if let Some(ref mut primary) = self.primary {
            primary.state = NodeState::Stopped;
        }

        // Mark all replicas as stopped
        for replica in self.replicas.values_mut() {
            replica.state = NodeState::Stopped;
        }

        self.state = ShardGroupState::Stopped;
        log::info!("Stopped shard group {}", self.id);
        Ok(())
    }

    /// Get all nodes (primary + replicas)
    pub fn all_nodes(&self) -> Vec<&NodeInfo> {
        let mut nodes = Vec::new();
        if let Some(ref primary) = self.primary {
            nodes.push(primary);
        }
        nodes.extend(self.replicas.values());
        nodes
    }

    /// Get the number of healthy nodes
    pub fn healthy_node_count(&self) -> usize {
        self.all_nodes().iter().filter(|n| n.state.is_serving()).count()
    }
}

/// Manager for multiple shard groups
pub struct ShardGroupManager {
    /// All managed shard groups
    groups: Arc<RwLock<HashMap<ShardId, ShardGroup>>>,
}

impl ShardGroupManager {
    /// Create a new shard group manager
    pub fn new() -> Self {
        Self { groups: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Create a new shard group
    ///
    /// # Arguments
    /// * `shard_id` - Unique identifier for the shard group
    pub fn create_group(&self, shard_id: ShardId) -> Result<()> {
        let mut groups = self.groups.write();

        if groups.contains_key(&shard_id) {
            return Err(Error::ClusterError(format!("Shard group {} already exists", shard_id)));
        }

        let group = ShardGroup::new(shard_id.clone());
        groups.insert(shard_id.clone(), group);
        log::info!("Created shard group {}", shard_id);
        Ok(())
    }

    /// Remove a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Identifier of the shard group to remove
    pub fn remove_group(&self, shard_id: &str) -> Result<()> {
        let mut groups = self.groups.write();

        // Stop the group first if it exists
        if let Some(group) = groups.get_mut(shard_id) {
            group.stop()?;
        }

        groups
            .remove(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        log::info!("Removed shard group {}", shard_id);
        Ok(())
    }

    /// Set the primary node for a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    /// * `node_id` - Node identifier
    /// * `address` - Network address of the node
    pub fn set_primary(&self, shard_id: &str, node_id: String, address: String) -> Result<()> {
        let mut groups = self.groups.write();
        let group = groups
            .get_mut(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        group.set_primary(node_id, address)
    }

    /// Add a replica to a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    /// * `node_id` - Node identifier
    /// * `address` - Network address of the node
    pub fn add_replica(&self, shard_id: &str, node_id: String, address: String) -> Result<()> {
        let mut groups = self.groups.write();
        let group = groups
            .get_mut(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        group.add_replica(node_id, address)
    }

    /// Remove a replica from a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    /// * `node_id` - Node identifier to remove
    pub fn remove_replica(&self, shard_id: &str, node_id: &str) -> Result<()> {
        let mut groups = self.groups.write();
        let group = groups
            .get_mut(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        group.remove_replica(node_id)
    }

    /// Start a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    pub fn start_group(&self, shard_id: &str) -> Result<()> {
        let mut groups = self.groups.write();
        let group = groups
            .get_mut(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        group.start()
    }

    /// Stop a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    pub fn stop_group(&self, shard_id: &str) -> Result<()> {
        let mut groups = self.groups.write();
        let group = groups
            .get_mut(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        group.stop()
    }

    /// Update node state in a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    /// * `node_id` - Node identifier
    /// * `state` - New state for the node
    pub fn update_node_state(&self, shard_id: &str, node_id: &str, state: NodeState) -> Result<()> {
        let mut groups = self.groups.write();
        let group = groups
            .get_mut(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        group.update_node_state(node_id, state)
    }

    /// Get information about a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    pub fn get_group(&self, shard_id: &str) -> Option<ShardGroupState> {
        let groups = self.groups.read();
        groups.get(shard_id).map(|g| g.state())
    }

    /// Get all nodes in a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    pub fn get_group_nodes(&self, shard_id: &str) -> Result<Vec<NodeInfo>> {
        let groups = self.groups.read();
        let group = groups
            .get(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        Ok(group.all_nodes().into_iter().cloned().collect())
    }

    /// List all shard groups
    pub fn list_groups(&self) -> Vec<ShardId> {
        let groups = self.groups.read();
        groups.keys().cloned().collect()
    }

    /// Get the number of shard groups
    pub fn group_count(&self) -> usize {
        let groups = self.groups.read();
        groups.len()
    }

    /// Get primary node for a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    pub fn get_primary(&self, shard_id: &str) -> Result<Option<NodeInfo>> {
        let groups = self.groups.read();
        let group = groups
            .get(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        Ok(group.primary().cloned())
    }

    /// Get all replicas in a shard group
    ///
    /// # Arguments
    /// * `shard_id` - Shard group identifier
    pub fn get_replicas(&self, shard_id: &str) -> Result<Vec<NodeInfo>> {
        let groups = self.groups.read();
        let group = groups
            .get(shard_id)
            .ok_or_else(|| Error::ClusterError(format!("Shard group {} not found", shard_id)))?;

        Ok(group.replicas().into_iter().cloned().collect())
    }
}

impl Default for ShardGroupManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_state() {
        assert!(NodeState::Healthy.is_serving());
        assert!(!NodeState::Unhealthy.is_serving());
        assert!(!NodeState::Starting.is_serving());
        assert!(!NodeState::Stopped.is_serving());
    }

    #[test]
    fn test_shard_group_creation() {
        let group = ShardGroup::new("shard1".to_string());
        assert_eq!(group.id(), "shard1");
        assert_eq!(group.state(), ShardGroupState::Initializing);
        assert!(group.primary().is_none());
        assert_eq!(group.replica_count(), 0);
    }

    #[test]
    fn test_shard_group_set_primary() {
        let mut group = ShardGroup::new("shard1".to_string());

        group
            .set_primary("primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();

        assert!(group.primary().is_some());
        let primary = group.primary().unwrap();
        assert_eq!(primary.id, "primary1");
        assert_eq!(primary.address, "127.0.0.1:50051");
        assert!(primary.is_primary);

        // Cannot set primary twice
        let result = group.set_primary("primary2".to_string(), "127.0.0.1:50052".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_shard_group_add_remove_replica() {
        let mut group = ShardGroup::new("shard1".to_string());

        // Add replicas
        group
            .add_replica("replica1".to_string(), "127.0.0.1:50052".to_string())
            .unwrap();
        group
            .add_replica("replica2".to_string(), "127.0.0.1:50053".to_string())
            .unwrap();

        assert_eq!(group.replica_count(), 2);

        // Cannot add duplicate
        let result = group.add_replica("replica1".to_string(), "127.0.0.1:50054".to_string());
        assert!(result.is_err());

        // Remove replica
        group.remove_replica("replica1").unwrap();
        assert_eq!(group.replica_count(), 1);
        assert!(group.get_replica("replica1").is_none());
        assert!(group.get_replica("replica2").is_some());
    }

    #[test]
    fn test_shard_group_state_transitions() {
        let mut group = ShardGroup::new("shard1".to_string());
        group
            .set_primary("primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();

        assert_eq!(group.state(), ShardGroupState::Initializing);

        // Start the group
        group.start().unwrap();
        assert_eq!(group.state(), ShardGroupState::Running);

        // Primary should be healthy
        let primary = group.primary().unwrap();
        assert_eq!(primary.state, NodeState::Healthy);

        // Stop the group
        group.stop().unwrap();
        assert_eq!(group.state(), ShardGroupState::Stopped);
    }

    #[test]
    fn test_shard_group_update_node_state() {
        let mut group = ShardGroup::new("shard1".to_string());
        group
            .set_primary("primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();
        group
            .add_replica("replica1".to_string(), "127.0.0.1:50052".to_string())
            .unwrap();

        group.start().unwrap();
        assert_eq!(group.state(), ShardGroupState::Running);

        // Mark primary as unhealthy
        group.update_node_state("primary1", NodeState::Unhealthy).unwrap();
        assert_eq!(group.state(), ShardGroupState::Degraded);

        // Mark primary as healthy again
        group.update_node_state("primary1", NodeState::Healthy).unwrap();
        assert_eq!(group.state(), ShardGroupState::Running);
    }

    #[test]
    fn test_shard_group_manager_creation() {
        let manager = ShardGroupManager::new();
        assert_eq!(manager.group_count(), 0);
    }

    #[test]
    fn test_shard_group_manager_create_remove() {
        let manager = ShardGroupManager::new();

        // Create group
        manager.create_group("shard1".to_string()).unwrap();
        assert_eq!(manager.group_count(), 1);

        // Cannot create duplicate
        let result = manager.create_group("shard1".to_string());
        assert!(result.is_err());

        // Remove group
        manager.remove_group("shard1").unwrap();
        assert_eq!(manager.group_count(), 0);
    }

    #[test]
    fn test_shard_group_manager_operations() {
        let manager = ShardGroupManager::new();

        // Create and configure group
        manager.create_group("shard1".to_string()).unwrap();
        manager
            .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();
        manager
            .add_replica("shard1", "replica1".to_string(), "127.0.0.1:50052".to_string())
            .unwrap();
        manager
            .add_replica("shard1", "replica2".to_string(), "127.0.0.1:50053".to_string())
            .unwrap();

        // Start group
        manager.start_group("shard1").unwrap();
        assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Running));

        // Check nodes
        let nodes = manager.get_group_nodes("shard1").unwrap();
        assert_eq!(nodes.len(), 3);

        let primary = manager.get_primary("shard1").unwrap();
        assert!(primary.is_some());

        let replicas = manager.get_replicas("shard1").unwrap();
        assert_eq!(replicas.len(), 2);

        // Stop group
        manager.stop_group("shard1").unwrap();
        assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Stopped));
    }

    #[test]
    fn test_shard_group_manager_list_groups() {
        let manager = ShardGroupManager::new();

        manager.create_group("shard1".to_string()).unwrap();
        manager.create_group("shard2".to_string()).unwrap();
        manager.create_group("shard3".to_string()).unwrap();

        let groups = manager.list_groups();
        assert_eq!(groups.len(), 3);
        assert!(groups.contains(&"shard1".to_string()));
        assert!(groups.contains(&"shard2".to_string()));
        assert!(groups.contains(&"shard3".to_string()));
    }

    #[test]
    fn test_shard_group_manager_update_node_state() {
        let manager = ShardGroupManager::new();

        manager.create_group("shard1".to_string()).unwrap();
        manager
            .set_primary("shard1", "primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();
        manager.start_group("shard1").unwrap();

        // Update node state
        manager.update_node_state("shard1", "primary1", NodeState::Unhealthy).unwrap();

        assert_eq!(manager.get_group("shard1"), Some(ShardGroupState::Degraded));
    }

    #[test]
    fn test_node_info_creation() {
        let node = NodeInfo::new("node1".to_string(), "127.0.0.1:50051".to_string(), true);

        assert_eq!(node.id, "node1");
        assert_eq!(node.address, "127.0.0.1:50051");
        assert!(node.is_primary);
        assert_eq!(node.state, NodeState::Starting);
        assert_eq!(node.request_count, 0);
    }

    #[test]
    fn test_shard_group_all_nodes() {
        let mut group = ShardGroup::new("shard1".to_string());
        group
            .set_primary("primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();
        group
            .add_replica("replica1".to_string(), "127.0.0.1:50052".to_string())
            .unwrap();
        group
            .add_replica("replica2".to_string(), "127.0.0.1:50053".to_string())
            .unwrap();

        let all_nodes = group.all_nodes();
        assert_eq!(all_nodes.len(), 3);
    }

    #[test]
    fn test_shard_group_healthy_node_count() {
        let mut group = ShardGroup::new("shard1".to_string());
        group
            .set_primary("primary1".to_string(), "127.0.0.1:50051".to_string())
            .unwrap();
        group
            .add_replica("replica1".to_string(), "127.0.0.1:50052".to_string())
            .unwrap();

        // Initially no healthy nodes (all starting)
        assert_eq!(group.healthy_node_count(), 0);

        // Start the group
        group.start().unwrap();
        assert_eq!(group.healthy_node_count(), 2);

        // Mark primary as unhealthy
        group.update_node_state("primary1", NodeState::Unhealthy).unwrap();
        assert_eq!(group.healthy_node_count(), 1);
    }
}
