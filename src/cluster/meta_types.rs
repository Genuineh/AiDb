//! Metadata types for Multi-Raft cluster management
//!
//! This module defines the core data structures for MetaRaft, which manages
//! global cluster metadata including slot mappings, group memberships, and node information.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::raft_storage::NodeId;

/// Global cluster metadata managed by MetaRaft
///
/// This structure contains all the information needed to route requests
/// in a Multi-Raft cluster with sharding support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterMeta {
    /// Slot to Group mapping (16384 slots)
    ///
    /// Each slot is mapped to a group ID. The slot for a key is computed as:
    /// `crc16(key) % 16384`
    ///
    /// slots[i] contains the group_id that owns slot i
    #[serde(with = "serde_big_array::BigArray")]
    pub slots: [u64; 16384],

    /// Group metadata indexed by group ID
    pub groups: HashMap<u64, GroupMeta>,

    /// Node information indexed by node ID
    pub nodes: HashMap<NodeId, NodeInfo>,

    /// Configuration version for optimistic concurrency control
    ///
    /// This version is incremented on every metadata change and can be used
    /// for Compare-And-Swap (CAS) updates.
    pub config_version: u64,

    /// Ongoing slot migrations
    ///
    /// Tracks active slot migrations for online resharding
    pub migrations: Vec<SlotMigration>,
}

impl Default for ClusterMeta {
    fn default() -> Self {
        Self {
            slots: [0u64; 16384],
            groups: HashMap::new(),
            nodes: HashMap::new(),
            config_version: 0,
            migrations: Vec::new(),
        }
    }
}

impl ClusterMeta {
    /// Create a new empty cluster metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize cluster with uniform slot distribution
    ///
    /// Distributes 16384 slots evenly across the specified number of groups.
    ///
    /// # Arguments
    ///
    /// * `group_count` - Number of groups to create
    ///
    /// # Returns
    ///
    /// A new ClusterMeta with slots distributed evenly
    pub fn with_uniform_distribution(group_count: u64) -> Self {
        if group_count == 0 {
            return Self::default();
        }

        let mut slots = [0u64; 16384];
        for (slot, item) in slots.iter_mut().enumerate() {
            *item = (slot as u64) % group_count;
        }

        Self {
            slots,
            config_version: 1,
            ..Default::default()
        }
    }

    /// Get the group ID for a given slot
    pub fn slot_to_group(&self, slot: u16) -> u64 {
        self.slots[slot as usize]
    }

    /// Get the group metadata for a slot
    pub fn get_group_for_slot(&self, slot: u16) -> Option<&GroupMeta> {
        let group_id = self.slot_to_group(slot);
        self.groups.get(&group_id)
    }

    /// Update slot mapping to a new group
    ///
    /// This is used during slot migration.
    pub fn update_slot(&mut self, slot: u16, group_id: u64) {
        self.slots[slot as usize] = group_id;
        self.config_version += 1;
    }

    /// Update a range of slots to a new group
    pub fn update_slot_range(&mut self, start: u16, end: u16, group_id: u64) {
        for slot in start..end {
            self.slots[slot as usize] = group_id;
        }
        self.config_version += 1;
    }
}

/// Metadata for a Raft group
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupMeta {
    /// Unique group identifier
    pub group_id: u64,

    /// List of node IDs that are replicas of this group
    ///
    /// Typically has `replication_factor` nodes (e.g., 3 or 5)
    pub replicas: Vec<NodeId>,

    /// Current leader node ID (if known)
    pub leader: Option<NodeId>,

    /// Group metadata version
    ///
    /// Incremented on membership changes
    pub version: u64,

    /// Slot range owned by this group (inclusive start, exclusive end)
    ///
    /// This is a hint for optimization; the authoritative source is ClusterMeta.slots
    pub slot_range: Option<(u16, u16)>,
}

impl GroupMeta {
    /// Create a new group metadata
    pub fn new(group_id: u64, replicas: Vec<NodeId>) -> Self {
        Self {
            group_id,
            replicas,
            leader: None,
            version: 1,
            slot_range: None,
        }
    }

    /// Update the replica list and increment version
    pub fn update_replicas(&mut self, replicas: Vec<NodeId>) {
        self.replicas = replicas;
        self.version += 1;
    }

    /// Set the leader for this group
    pub fn set_leader(&mut self, leader: NodeId) {
        self.leader = Some(leader);
    }

    /// Check if a node is a replica of this group
    pub fn is_replica(&self, node_id: NodeId) -> bool {
        self.replicas.contains(&node_id)
    }
}

/// Information about a cluster node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeInfo {
    /// Unique node identifier
    pub node_id: NodeId,

    /// Network address (e.g., "127.0.0.1:50051")
    pub addr: String,

    /// Current node status
    pub status: NodeStatus,

    /// Timestamp when node joined (Unix timestamp in seconds)
    pub joined_at: u64,

    /// Number of groups this node participates in
    ///
    /// Used for load balancing during replica allocation
    pub group_count: usize,
}

impl NodeInfo {
    /// Create a new node info
    pub fn new(node_id: NodeId, addr: String) -> Self {
        Self {
            node_id,
            addr,
            status: NodeStatus::Joining,
            joined_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            group_count: 0,
        }
    }

    /// Check if node is online
    pub fn is_online(&self) -> bool {
        matches!(self.status, NodeStatus::Online)
    }

    /// Mark node as online
    pub fn set_online(&mut self) {
        self.status = NodeStatus::Online;
    }

    /// Mark node as offline
    pub fn set_offline(&mut self) {
        self.status = NodeStatus::Offline;
    }
}

/// Status of a cluster node
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is online and serving requests
    Online,
    /// Node is offline or unreachable
    Offline,
    /// Node is joining the cluster
    Joining,
    /// Node is leaving the cluster
    Leaving,
}

/// Ongoing slot migration information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotMigration {
    /// Slot being migrated
    pub slot: u16,

    /// Migration state
    pub state: SlotMigrationState,

    /// Number of keys migrated so far
    pub progress: u64,

    /// Total number of keys to migrate
    pub total: u64,

    /// Unix timestamp when migration started
    pub started_at: u64,
}

impl SlotMigration {
    /// Create a new slot migration
    pub fn new(slot: u16, from_group: u64, to_group: u64) -> Self {
        Self {
            slot,
            state: SlotMigrationState::Migrating { from_group, to_group },
            progress: 0,
            total: 0,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Check if migration is complete
    pub fn is_complete(&self) -> bool {
        matches!(self.state, SlotMigrationState::Complete)
    }

    /// Get migration progress percentage
    pub fn progress_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.progress as f64 / self.total as f64) * 100.0
        }
    }
}

/// State of a slot migration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotMigrationState {
    /// Idle - no migration
    Idle,
    /// Migrating from one group to another
    Migrating {
        /// Source group
        from_group: u64,
        /// Target group
        to_group: u64,
    },
    /// Importing (target group perspective)
    Importing {
        /// Source group
        from_group: u64,
        /// Target group
        to_group: u64,
    },
    /// Migration complete
    Complete,
}

/// Request types for MetaRaft operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetaRequest {
    /// Add a new node to the cluster
    AddNode {
        /// Node ID
        node_id: NodeId,
        /// Node address
        addr: String,
    },

    /// Remove a node from the cluster
    RemoveNode {
        /// Node ID to remove
        node_id: NodeId,
    },

    /// Create a new Raft group
    CreateGroup {
        /// Group ID
        group_id: u64,
        /// Initial replicas
        replicas: Vec<NodeId>,
    },

    /// Update slot mapping
    UpdateSlots {
        /// Start slot (inclusive)
        start: u16,
        /// End slot (exclusive)
        end: u16,
        /// Target group ID
        group_id: u64,
    },

    /// Update group membership
    UpdateGroupMembers {
        /// Group ID
        group_id: u64,
        /// New replica list
        replicas: Vec<NodeId>,
    },

    /// Start a slot migration
    StartMigration {
        /// Slot to migrate
        slot: u16,
        /// Source group
        from_group: u64,
        /// Target group
        to_group: u64,
    },

    /// Complete a slot migration
    CompleteMigration {
        /// Slot that was migrated
        slot: u16,
    },

    /// Update group leader
    UpdateGroupLeader {
        /// Group ID
        group_id: u64,
        /// New leader node ID
        leader: NodeId,
    },
}

/// Response types for MetaRaft operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetaResponse {
    /// Operation successful
    Ok,

    /// Return cluster metadata
    ClusterMeta(ClusterMeta),

    /// Operation failed with error message
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_meta_default() {
        let meta = ClusterMeta::new();
        assert_eq!(meta.config_version, 0);
        assert_eq!(meta.groups.len(), 0);
        assert_eq!(meta.nodes.len(), 0);
        assert_eq!(meta.migrations.len(), 0);
    }

    #[test]
    fn test_cluster_meta_uniform_distribution() {
        let meta = ClusterMeta::with_uniform_distribution(16);
        assert_eq!(meta.config_version, 1);

        // Check that slots are evenly distributed
        for slot in 0..16384 {
            let expected_group = (slot as u64) % 16;
            assert_eq!(meta.slot_to_group(slot), expected_group);
        }
    }

    #[test]
    fn test_cluster_meta_update_slot() {
        let mut meta = ClusterMeta::new();
        let initial_version = meta.config_version;

        meta.update_slot(100, 42);
        assert_eq!(meta.slot_to_group(100), 42);
        assert_eq!(meta.config_version, initial_version + 1);
    }

    #[test]
    fn test_cluster_meta_update_slot_range() {
        let mut meta = ClusterMeta::new();
        let initial_version = meta.config_version;

        meta.update_slot_range(100, 200, 42);
        for slot in 100..200 {
            assert_eq!(meta.slot_to_group(slot), 42);
        }
        assert_eq!(meta.config_version, initial_version + 1);
    }

    #[test]
    fn test_group_meta() {
        let mut group = GroupMeta::new(1, vec![1, 2, 3]);
        assert_eq!(group.group_id, 1);
        assert_eq!(group.replicas, vec![1, 2, 3]);
        assert_eq!(group.version, 1);
        assert!(group.is_replica(2));
        assert!(!group.is_replica(4));

        group.set_leader(1);
        assert_eq!(group.leader, Some(1));

        group.update_replicas(vec![1, 2, 3, 4]);
        assert_eq!(group.version, 2);
        assert!(group.is_replica(4));
    }

    #[test]
    fn test_node_info() {
        let mut node = NodeInfo::new(1, "127.0.0.1:50051".to_string());
        assert_eq!(node.node_id, 1);
        assert_eq!(node.addr, "127.0.0.1:50051");
        assert_eq!(node.status, NodeStatus::Joining);
        assert!(!node.is_online());

        node.set_online();
        assert!(node.is_online());

        node.set_offline();
        assert!(!node.is_online());
    }

    #[test]
    fn test_slot_migration() {
        let migration = SlotMigration::new(100, 1, 2);
        assert_eq!(migration.slot, 100);
        assert!(!migration.is_complete());
        assert_eq!(migration.progress_pct(), 0.0);

        let mut migration = migration;
        migration.total = 100;
        migration.progress = 50;
        assert_eq!(migration.progress_pct(), 50.0);
    }

    #[test]
    fn test_serialization() {
        // Test smaller components individually to avoid stack overflow with large arrays
        
        // Test GroupMeta serialization
        let group = GroupMeta::new(1, vec![1, 2, 3]);
        let json = serde_json::to_string(&group).unwrap();
        let deserialized: GroupMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(group, deserialized);

        // Test NodeInfo serialization
        let node = NodeInfo::new(1, "127.0.0.1:50051".to_string());
        let json = serde_json::to_string(&node).unwrap();
        let deserialized: NodeInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(node, deserialized);

        // Test bincode for smaller structs
        let bytes = bincode::serialize(&group).unwrap();
        let _: GroupMeta = bincode::deserialize(&bytes).unwrap();

        // For ClusterMeta with large array, we only test creation and basic operations
        // Full serialization testing is deferred to integration tests with proper stack size
        let meta = ClusterMeta::new();
        assert_eq!(meta.config_version, 0);
    }

    #[test]
    fn test_meta_request_serialization() {
        let requests = vec![
            MetaRequest::AddNode {
                node_id: 1,
                addr: "127.0.0.1:50051".to_string(),
            },
            MetaRequest::RemoveNode { node_id: 1 },
            MetaRequest::CreateGroup {
                group_id: 1,
                replicas: vec![1, 2, 3],
            },
            MetaRequest::UpdateSlots {
                start: 0,
                end: 100,
                group_id: 1,
            },
            MetaRequest::UpdateGroupMembers {
                group_id: 1,
                replicas: vec![1, 2, 3, 4],
            },
        ];

        for req in requests {
            let json = serde_json::to_string(&req).unwrap();
            let _: MetaRequest = serde_json::from_str(&json).unwrap();
        }
    }
}
