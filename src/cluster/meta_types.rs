//! MetaRaft cluster metadata types (Phase 13).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cluster::types::NodeId;

/// MetaRaft group ID — data groups use `group_id >= 1`.
pub const METARAFT_GROUP_ID: u64 = 0;

pub const SLOT_COUNT: usize = 16384;

/// 节点信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub rpc_addr: String,
    pub client_addr: Option<String>,
    pub role: NodeRole,
    pub status: NodeStatus,
    pub registered_at: u64,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole {
    Voter,
    Learner,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    Online,
    Offline,
    Draining,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupMeta {
    pub group_id: u64,
    pub replicas: Vec<ReplicaInfo>,
    pub slot_ranges: Vec<(u16, u16)>,
    pub config_version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicaInfo {
    pub node_id: NodeId,
    pub is_leader: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotMigrationState {
    Prepare {
        source_group: u64,
        target_group: u64,
        slots: Vec<u16>,
    },
    Migrating {
        source_group: u64,
        target_group: u64,
        slots: Vec<u16>,
        progress: u64,
        total: u64,
    },
    /// 写冻结: 客户端对该 slot 的写返回 TRYAGAIN; 读仍走 source.
    Frozen {
        source_group: u64,
        target_group: u64,
        slots: Vec<u16>,
    },
    /// verify 通过: 读切到 target; 写仍冻结; 可 Commit. **A2 保证起点**.
    ReadyToCommit {
        source_group: u64,
        target_group: u64,
        slots: Vec<u16>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterMeta {
    pub cluster_id: String,
    pub nodes: HashMap<NodeId, NodeInfo>,
    pub groups: HashMap<u64, GroupMeta>,
    pub version: u64,
    pub format_version: u64,
}

impl Default for ClusterMeta {
    fn default() -> Self {
        Self {
            cluster_id: "uninitialized".into(),
            nodes: HashMap::new(),
            groups: HashMap::new(),
            version: 0,
            format_version: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotStatus {
    Unallocated,
    Assigned(u64),
    Migrating(u64),
}

pub type SlotTable = Vec<SlotStatus>;

pub fn default_slot_table() -> SlotTable {
    vec![SlotStatus::Unallocated; SLOT_COUNT]
}

/// MetaRaft 状态机操作
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetaRequest {
    RegisterNode {
        node_id: NodeId,
        rpc_addr: String,
        client_addr: Option<String>,
        tags: HashMap<String, String>,
    },
    UpdateNodeStatus {
        node_id: NodeId,
        status: NodeStatus,
    },
    ChangeNodeRole {
        node_id: NodeId,
        role: NodeRole,
    },
    UpdateNodeTags {
        node_id: NodeId,
        tags: HashMap<String, String>,
    },
    UpdateNodeClientAddr {
        node_id: NodeId,
        client_addr: Option<String>,
    },
    RemoveNode {
        node_id: NodeId,
    },
    CreateGroup {
        group_id: u64,
        initial_replicas: Vec<(NodeId, bool)>,
    },
    RemoveGroup {
        group_id: u64,
    },
    ChangeGroupMembership {
        group_id: u64,
        new_replicas: Vec<(NodeId, bool)>,
        config_version: u64,
    },
    AssignSlots {
        group_id: u64,
        slots: Vec<u16>,
    },
    UnassignSlots {
        slots: Vec<u16>,
    },
    BeginSlotMigration {
        source_group: u64,
        target_group: u64,
        slots: Vec<u16>,
    },
    UpdateMigrationProgress {
        progress: u64,
        total: u64,
    },
    /// Migrating → Frozen. 仅当 executor 已报告拷贝完成.
    FreezeSlotMigration,
    /// Frozen → ReadyToCommit. 仅当 drain + final_verify 通过.
    MarkMigrationReady,
    CommitSlotMigration,
    CancelSlotMigration,
    /// Bump cluster config epoch (increments ClusterMeta.version).
    /// This is a lightweight consensus operation for CLUSTER BUMPEPOCH.
    BumpEpoch,
}
