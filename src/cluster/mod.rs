//! Cluster module for distributed AiDb
//!
//! This module provides RPC-based networking and cluster capabilities including:
//! - Primary nodes that host the full LSM-tree storage
//! - Replica nodes that cache frequently accessed data
//! - Connection pooling and client implementations
//! - Consistent hashing for key routing
//! - Coordinator for cluster management
//! - Health checking and failure detection
//! - Elastic scaling for dynamic cluster resizing

#[cfg(feature = "cluster")]
pub mod rpc;

#[cfg(feature = "cluster")]
pub mod primary;

#[cfg(feature = "cluster")]
pub mod replica;

#[cfg(feature = "cluster")]
pub mod consistent_hash;

#[cfg(feature = "cluster")]
pub mod coordinator;

#[cfg(feature = "cluster")]
pub mod health;

#[cfg(feature = "cluster")]
pub mod shard_group;

#[cfg(feature = "cluster")]
pub mod scaling;

#[cfg(feature = "cluster")]
pub mod autoscaler;

#[cfg(feature = "cluster")]
pub mod peer;

#[cfg(feature = "raft-cluster")]
pub mod raft_storage;

#[cfg(feature = "raft-cluster")]
pub mod raft_network;

#[cfg(feature = "raft-cluster")]
pub mod raft_node_new;

// TODO: Phase 3 - Rewrite for openraft
// #[cfg(feature = "raft-cluster")]
// pub mod raft_transport;

// TODO: Phase 4 - Rewrite for openraft  
// #[cfg(feature = "raft-cluster")]
// pub mod raft_node;

// #[cfg(feature = "raft-cluster")]
// pub mod raft_peer;

#[cfg(feature = "cluster")]
pub use primary::PrimaryNode;

#[cfg(feature = "cluster")]
pub use replica::ReplicaNode;

#[cfg(feature = "cluster")]
pub use consistent_hash::{ConsistentHashRing, ShardId};

#[cfg(feature = "cluster")]
pub use coordinator::{Coordinator, ShardInfo};

#[cfg(feature = "cluster")]
pub use health::{HealthCheckConfig, HealthChecker};

#[cfg(feature = "cluster")]
pub use shard_group::{NodeInfo, NodeState, ShardGroup, ShardGroupManager, ShardGroupState};

#[cfg(feature = "cluster")]
pub use scaling::{ScalingConfig, ScalingManager, ScalingStats};

#[cfg(feature = "cluster")]
pub use autoscaler::{AutoScaler, ScalingDecision, ScalingPolicy, SystemMetrics};

#[cfg(feature = "cluster")]
pub use peer::{PeerInfo, PeerNode, PeerStats};

#[cfg(feature = "raft-cluster")]
pub use raft_storage::{OpenRaftStorage, TypeConfig, NodeId, Request, Response};

#[cfg(feature = "raft-cluster")]
pub use raft_network::{RaftNetworkClient, RaftNetworkClientFactory};

#[cfg(feature = "raft-cluster")]
pub use raft_node_new::{OpenRaftNode, RaftNodeConfig};

// TODO: Phase 3-4 - Re-enable after rewriting for openraft
// #[cfg(feature = "raft-cluster")]
// pub use raft_node::{
//     encode_delete, encode_put, RaftConfig, RaftNode, RaftStateMachine, StateMachine,
// };

// #[cfg(feature = "raft-cluster")]
// pub use raft_transport::{RaftPeer, RaftTransport};

// #[cfg(feature = "raft-cluster")]
// pub use raft_peer::RaftBasedPeer;
