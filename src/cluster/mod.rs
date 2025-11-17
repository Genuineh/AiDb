//! Cluster module for distributed AiDb
//!
//! This module provides RPC-based networking and cluster capabilities including:
//! - Primary nodes that host the full LSM-tree storage
//! - Replica nodes that cache frequently accessed data
//! - Connection pooling and client implementations
//! - Consistent hashing for key routing
//! - Coordinator for cluster management
//! - Health checking and failure detection

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
