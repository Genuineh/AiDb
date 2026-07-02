//! 集群 Raft 指标 — 实现见 [`crate::metrics`].

pub use crate::metrics::{
    record_raft_group_fatal, record_raft_group_restart, record_raft_log_entries, record_raft_rpc,
};
