//! Cluster module for distributed AiDb
//!
//! This module provides RPC-based networking and cluster capabilities including:
//! - Primary nodes that host the full LSM-tree storage
//! - Replica nodes that cache frequently accessed data
//! - Connection pooling and client implementations

#[cfg(feature = "cluster")]
pub mod rpc;

#[cfg(feature = "cluster")]
pub mod primary;

#[cfg(feature = "cluster")]
pub mod replica;

#[cfg(feature = "cluster")]
pub use primary::PrimaryNode;

#[cfg(feature = "cluster")]
pub use replica::ReplicaNode;
