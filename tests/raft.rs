//! @component aidb-cluster
//! 集群 Raft 集成测入口.
#![cfg(feature = "cluster")]

#[path = "modules/cluster/mod.rs"]
mod cluster;
