//! @component aidb-cluster
//! 集成测试: 集群副本自动对账.
#![cfg(feature = "cluster")]

#[path = "modules/cluster/replica_reconcile.rs"]
mod replica_reconcile;
