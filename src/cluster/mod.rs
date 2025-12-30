//! Cluster module for distributed AiDb
//!
//! This module provides Multi-Raft based distributed clustering including:
//! - OpenRaft consensus for strong consistency
//! - Sharded storage with automatic slot-based routing
//! - Dynamic membership changes (add/remove nodes)
//! - Slot migration for cluster rebalancing
//! - gRPC-based inter-node communication

#[cfg(feature = "cluster")]
pub mod rpc;

#[cfg(feature = "raft-cluster")]
pub mod raft_storage;

#[cfg(feature = "raft-cluster")]
pub mod raft_network;

#[cfg(feature = "raft-cluster")]
pub mod raft_node_new;

#[cfg(feature = "raft-cluster")]
pub mod thin_replication;

#[cfg(feature = "raft-cluster")]
pub mod meta_types;

#[cfg(feature = "raft-cluster")]
pub mod meta_state_machine;

#[cfg(feature = "raft-cluster")]
pub mod meta_raft_node;

#[cfg(feature = "raft-cluster")]
pub mod sharded_storage;

#[cfg(feature = "raft-cluster")]
pub mod multi_raft_network;

#[cfg(feature = "raft-cluster")]
pub mod multi_raft_node;

#[cfg(feature = "raft-cluster")]
pub mod router;

#[cfg(feature = "raft-cluster")]
pub mod sharded_state_machine;

#[cfg(feature = "raft-cluster")]
pub mod replica_allocator;

#[cfg(feature = "raft-cluster")]
pub mod membership_coordinator;

#[cfg(feature = "raft-cluster")]
pub mod slot_migration;

// TODO: Phase 3 - Rewrite for openraft
// #[cfg(feature = "raft-cluster")]
// pub mod raft_transport;

// TODO: Phase 4 - Rewrite for openraft
// #[cfg(feature = "raft-cluster")]
// pub mod raft_node;

// #[cfg(feature = "raft-cluster")]
// pub mod raft_peer;

#[cfg(feature = "raft-cluster")]
pub use raft_storage::{NodeId, OpenRaftStorage, Request, Response, TypeConfig};

#[cfg(feature = "raft-cluster")]
pub use raft_network::{RaftNetworkClient, RaftNetworkClientFactory, RaftServiceImpl};

#[cfg(feature = "raft-cluster")]
pub use raft_node_new::{OpenRaftNode, RaftNodeConfig};

#[cfg(feature = "raft-cluster")]
pub use thin_replication::{WriteBatch as ThinWriteBatch, WriteOp as ThinWriteOp};

#[cfg(feature = "raft-cluster")]
pub use meta_types::{
    ClusterMeta, GroupMeta, MetaRequest, MetaResponse, NodeInfo as MetaNodeInfo, NodeStatus,
    SlotMigration, SlotMigrationState,
};

#[cfg(feature = "raft-cluster")]
pub use meta_state_machine::MetaStateMachine;

#[cfg(feature = "raft-cluster")]
pub use meta_raft_node::MetaRaftNode;

#[cfg(feature = "raft-cluster")]
pub use sharded_storage::{GroupId, ShardedRaftStorage};

#[cfg(feature = "raft-cluster")]
pub use multi_raft_network::{MultiRaftNetworkClient, MultiRaftNetworkFactory};

#[cfg(feature = "raft-cluster")]
pub use multi_raft_node::MultiRaftNode;

#[cfg(feature = "raft-cluster")]
pub use router::{Router, SLOT_COUNT};

#[cfg(feature = "raft-cluster")]
pub use sharded_state_machine::ShardedStateMachine;

#[cfg(feature = "raft-cluster")]
pub use replica_allocator::ReplicaAllocator;

#[cfg(feature = "raft-cluster")]
pub use membership_coordinator::MembershipCoordinator;

#[cfg(feature = "raft-cluster")]
pub use slot_migration::{MigrationConfig, MigrationManager};

// TODO: Phase 3-4 - Re-enable after rewriting for openraft
// #[cfg(feature = "raft-cluster")]
// pub use raft_node::{
//     encode_delete, encode_put, RaftConfig, RaftNode, RaftStateMachine, StateMachine,
// };

// #[cfg(feature = "raft-cluster")]
// pub use raft_transport::{RaftPeer, RaftTransport};

// #[cfg(feature = "raft-cluster")]
// pub use raft_peer::RaftBasedPeer;
