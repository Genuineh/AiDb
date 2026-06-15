//! 集群 Raft Prometheus 指标 (`monitoring` feature).

use std::sync::LazyLock;

/// Raft RPC 计数 (type= vote | append_entries | install_snapshot, direction= incoming | outgoing)
pub static RAFT_RPC_TOTAL: LazyLock<prometheus::IntCounterVec> = LazyLock::new(|| {
  prometheus::IntCounterVec::new(
    prometheus::Opts::new("aidb_raft_rpc_total", "Raft RPC 调用次数"),
    &["type", "direction"],
  )
  .unwrap()
});

/// 累计处理的 Raft 日志条目数 (AppendEntries payload)
pub static RAFT_LOG_ENTRIES_TOTAL: LazyLock<prometheus::IntCounter> = LazyLock::new(|| {
  prometheus::IntCounter::new("aidb_raft_log_entries_total", "Raft 日志条目累计数").unwrap()
});

pub fn init() {
  let _ = &*RAFT_RPC_TOTAL;
  let _ = &*RAFT_LOG_ENTRIES_TOTAL;
}

pub fn register_into(registry: &prometheus::Registry) -> Result<(), prometheus::Error> {
  registry.register(Box::new(RAFT_RPC_TOTAL.clone()))?;
  registry.register(Box::new(RAFT_LOG_ENTRIES_TOTAL.clone()))?;
  Ok(())
}

#[cfg(feature = "monitoring")]
pub fn record_raft_rpc(rpc_type: &str, direction: &str) {
  RAFT_RPC_TOTAL
    .with_label_values(&[rpc_type, direction])
    .inc();
}

#[cfg(feature = "monitoring")]
pub fn record_raft_log_entries(count: u64) {
  if count > 0 {
    RAFT_LOG_ENTRIES_TOTAL.inc_by(count);
  }
}

#[cfg(not(feature = "monitoring"))]
pub fn record_raft_rpc(_rpc_type: &str, _direction: &str) {}

#[cfg(not(feature = "monitoring"))]
pub fn record_raft_log_entries(_count: u64) {}
