//! Metrics definitions and collection for AiDb
//!
//! This module defines all Prometheus metrics used by AiDb and provides
//! helper functions for recording measurements.

#![allow(missing_docs)]

use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge_vec, register_histogram_vec, CounterVec, Encoder,
    GaugeVec, HistogramVec, TextEncoder,
};
use std::sync::Arc;

lazy_static! {
    // Request metrics - operations per second and latency
    pub static ref REQUEST_COUNTER: CounterVec = register_counter_vec!(
        "aidb_requests_total",
        "Total number of requests",
        &["operation", "status"]
    )
    .unwrap();

    pub static ref REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_request_duration_seconds",
        "Request duration in seconds",
        &["operation"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .unwrap();

    // System metrics
    pub static ref MEMORY_USAGE: GaugeVec = register_gauge_vec!(
        "aidb_memory_bytes",
        "Memory usage in bytes",
        &["component"]
    )
    .unwrap();

    pub static ref DISK_USAGE: GaugeVec = register_gauge_vec!(
        "aidb_disk_bytes",
        "Disk usage in bytes",
        &["path"]
    )
    .unwrap();

    // Business metrics
    pub static ref CACHE_HITS: CounterVec = register_counter_vec!(
        "aidb_cache_hits_total",
        "Total cache hits",
        &["cache_type"]
    )
    .unwrap();

    pub static ref CACHE_MISSES: CounterVec = register_counter_vec!(
        "aidb_cache_misses_total",
        "Total cache misses",
        &["cache_type"]
    )
    .unwrap();

    pub static ref COMPACTION_COUNT: CounterVec = register_counter_vec!(
        "aidb_compactions_total",
        "Total number of compactions",
        &["level"]
    )
    .unwrap();

    pub static ref COMPACTION_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_compaction_duration_seconds",
        "Compaction duration in seconds",
        &["level"],
        vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]
    )
    .unwrap();

    pub static ref FLUSH_COUNT: CounterVec = register_counter_vec!(
        "aidb_flushes_total",
        "Total number of memtable flushes",
        &["status"]
    )
    .unwrap();

    pub static ref FLUSH_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_flush_duration_seconds",
        "Flush duration in seconds",
        &[],
        vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
    )
    .unwrap();

    // Error metrics
    pub static ref ERROR_COUNTER: CounterVec = register_counter_vec!(
        "aidb_errors_total",
        "Total number of errors",
        &["operation", "error_type"]
    )
    .unwrap();

    // SSTable metrics
    pub static ref SSTABLE_COUNT: GaugeVec = register_gauge_vec!(
        "aidb_sstables_total",
        "Total number of SSTables",
        &["level"]
    )
    .unwrap();

    pub static ref SSTABLE_SIZE: GaugeVec = register_gauge_vec!(
        "aidb_sstable_size_bytes",
        "SSTable size in bytes",
        &["level"]
    )
    .unwrap();

    // WAL metrics
    pub static ref WAL_SIZE: GaugeVec = register_gauge_vec!(
        "aidb_wal_size_bytes",
        "WAL size in bytes",
        &[]
    )
    .unwrap();

    pub static ref WAL_SYNC_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_wal_sync_duration_seconds",
        "WAL sync duration in seconds",
        &[],
        vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1]
    )
    .unwrap();

    // Backup metrics
    pub static ref BACKUP_COUNT: CounterVec = register_counter_vec!(
        "aidb_backups_total",
        "Total number of backups",
        &["status"]
    )
    .unwrap();

    pub static ref BACKUP_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_backup_duration_seconds",
        "Backup duration in seconds",
        &[],
        vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0]
    )
    .unwrap();

    pub static ref RESTORE_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_restore_duration_seconds",
        "Restore duration in seconds",
        &[],
        vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0]
    )
    .unwrap();

    // Cluster metrics (if using cluster feature)
    pub static ref CLUSTER_NODES: GaugeVec = register_gauge_vec!(
        "aidb_cluster_nodes",
        "Number of nodes in cluster",
        &["node_type", "status"]
    )
    .unwrap();

    pub static ref CLUSTER_REQUESTS: CounterVec = register_counter_vec!(
        "aidb_cluster_requests_total",
        "Total cluster RPC requests",
        &["method", "status"]
    )
    .unwrap();

    // Multi-Raft per-group metrics
    pub static ref RAFT_GROUP_REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "aidb_raft_group_request_duration_seconds",
        "Per-group request duration in seconds",
        &["group_id", "operation"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .unwrap();

    pub static ref RAFT_GROUP_REPLICATION_LAG: GaugeVec = register_gauge_vec!(
        "aidb_raft_group_replication_lag",
        "Replication lag (committed index - applied index) per group",
        &["group_id"]
    )
    .unwrap();

    pub static ref RAFT_GROUP_LOG_SIZE: GaugeVec = register_gauge_vec!(
        "aidb_raft_group_log_size",
        "Total log entries per group",
        &["group_id"]
    )
    .unwrap();

    pub static ref RAFT_GROUP_SNAPSHOT_COUNT: CounterVec = register_counter_vec!(
        "aidb_raft_group_snapshots_total",
        "Total snapshots created per group",
        &["group_id"]
    )
    .unwrap();

    pub static ref RAFT_GROUP_LOG_COMPACTION_COUNT: CounterVec = register_counter_vec!(
        "aidb_raft_group_log_compactions_total",
        "Total log compactions per group",
        &["group_id"]
    )
    .unwrap();
}

/// MetricsCollector provides a convenient interface for collecting metrics
#[derive(Clone)]
pub struct MetricsCollector {
    _marker: Arc<()>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self { _marker: Arc::new(()) }
    }

    /// Export metrics in Prometheus text format
    pub fn export(&self) -> Result<String, Box<dyn std::error::Error>> {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    /// Record a successful get operation
    pub fn record_get_success(&self, duration: f64) {
        REQUEST_COUNTER.with_label_values(&["get", "success"]).inc();
        REQUEST_DURATION.with_label_values(&["get"]).observe(duration);
    }

    /// Record a failed get operation
    pub fn record_get_error(&self, error_type: &str) {
        REQUEST_COUNTER.with_label_values(&["get", "error"]).inc();
        ERROR_COUNTER.with_label_values(&["get", error_type]).inc();
    }

    /// Record a successful put operation
    pub fn record_put_success(&self, duration: f64) {
        REQUEST_COUNTER.with_label_values(&["put", "success"]).inc();
        REQUEST_DURATION.with_label_values(&["put"]).observe(duration);
    }

    /// Record a failed put operation
    pub fn record_put_error(&self, error_type: &str) {
        REQUEST_COUNTER.with_label_values(&["put", "error"]).inc();
        ERROR_COUNTER.with_label_values(&["put", error_type]).inc();
    }

    /// Record a successful delete operation
    pub fn record_delete_success(&self, duration: f64) {
        REQUEST_COUNTER.with_label_values(&["delete", "success"]).inc();
        REQUEST_DURATION.with_label_values(&["delete"]).observe(duration);
    }

    /// Record a failed delete operation
    pub fn record_delete_error(&self, error_type: &str) {
        REQUEST_COUNTER.with_label_values(&["delete", "error"]).inc();
        ERROR_COUNTER.with_label_values(&["delete", error_type]).inc();
    }

    /// Record cache hit
    pub fn record_cache_hit(&self, cache_type: &str) {
        CACHE_HITS.with_label_values(&[cache_type]).inc();
    }

    /// Record cache miss
    pub fn record_cache_miss(&self, cache_type: &str) {
        CACHE_MISSES.with_label_values(&[cache_type]).inc();
    }

    /// Record compaction
    pub fn record_compaction(&self, level: usize, duration: f64) {
        COMPACTION_COUNT.with_label_values(&[&level.to_string()]).inc();
        COMPACTION_DURATION.with_label_values(&[&level.to_string()]).observe(duration);
    }

    /// Record flush
    pub fn record_flush(&self, duration: f64, success: bool) {
        let status = if success { "success" } else { "error" };
        FLUSH_COUNT.with_label_values(&[status]).inc();
        if success {
            FLUSH_DURATION.with_label_values(&[] as &[&str]).observe(duration);
        }
    }

    /// Update memory usage
    pub fn update_memory_usage(&self, component: &str, bytes: u64) {
        MEMORY_USAGE.with_label_values(&[component]).set(bytes as f64);
    }

    /// Update disk usage
    pub fn update_disk_usage(&self, path: &str, bytes: u64) {
        DISK_USAGE.with_label_values(&[path]).set(bytes as f64);
    }

    /// Update SSTable count and size
    pub fn update_sstable_stats(&self, level: usize, count: usize, total_size: u64) {
        let level_str = level.to_string();
        SSTABLE_COUNT.with_label_values(&[&level_str]).set(count as f64);
        SSTABLE_SIZE.with_label_values(&[&level_str]).set(total_size as f64);
    }

    /// Update WAL size
    pub fn update_wal_size(&self, bytes: u64) {
        WAL_SIZE.with_label_values(&[] as &[&str]).set(bytes as f64);
    }

    /// Record WAL sync duration
    pub fn record_wal_sync(&self, duration: f64) {
        WAL_SYNC_DURATION.with_label_values(&[] as &[&str]).observe(duration);
    }

    /// Record backup operation
    pub fn record_backup(&self, duration: f64, success: bool) {
        let status = if success { "success" } else { "error" };
        BACKUP_COUNT.with_label_values(&[status]).inc();
        if success {
            BACKUP_DURATION.with_label_values(&[] as &[&str]).observe(duration);
        }
    }

    /// Record restore operation
    pub fn record_restore(&self, duration: f64, success: bool) {
        if success {
            RESTORE_DURATION.with_label_values(&[] as &[&str]).observe(duration);
        }
    }

    /// Update cluster node count
    pub fn update_cluster_nodes(&self, node_type: &str, status: &str, count: usize) {
        CLUSTER_NODES.with_label_values(&[node_type, status]).set(count as f64);
    }

    /// Record cluster RPC request
    pub fn record_cluster_request(&self, method: &str, success: bool) {
        let status = if success { "success" } else { "error" };
        CLUSTER_REQUESTS.with_label_values(&[method, status]).inc();
    }

    /// Record per-group request duration
    pub fn record_raft_group_request(&self, group_id: u64, operation: &str, duration: f64) {
        let group_id_str = group_id.to_string();
        RAFT_GROUP_REQUEST_DURATION
            .with_label_values(&[&group_id_str, operation])
            .observe(duration);
    }

    /// Update per-group replication lag
    ///
    /// Replication lag is defined as: committed_index - applied_index
    pub fn update_raft_group_replication_lag(&self, group_id: u64, lag: u64) {
        let group_id_str = group_id.to_string();
        RAFT_GROUP_REPLICATION_LAG.with_label_values(&[&group_id_str]).set(lag as f64);
    }

    /// Update per-group log size
    pub fn update_raft_group_log_size(&self, group_id: u64, size: u64) {
        let group_id_str = group_id.to_string();
        RAFT_GROUP_LOG_SIZE.with_label_values(&[&group_id_str]).set(size as f64);
    }

    /// Record per-group snapshot creation
    pub fn record_raft_group_snapshot(&self, group_id: u64) {
        let group_id_str = group_id.to_string();
        RAFT_GROUP_SNAPSHOT_COUNT.with_label_values(&[&group_id_str]).inc();
    }

    /// Record per-group log compaction
    pub fn record_raft_group_log_compaction(&self, group_id: u64) {
        let group_id_str = group_id.to_string();
        RAFT_GROUP_LOG_COMPACTION_COUNT.with_label_values(&[&group_id_str]).inc();
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Register all metrics (called automatically via lazy_static)
pub fn register_metrics() {
    // Metrics are registered automatically via lazy_static
    // This function exists for explicit initialization if needed
    let _ = &*REQUEST_COUNTER;
    let _ = &*REQUEST_DURATION;
    let _ = &*MEMORY_USAGE;
    let _ = &*DISK_USAGE;
    let _ = &*CACHE_HITS;
    let _ = &*CACHE_MISSES;
    let _ = &*COMPACTION_COUNT;
    let _ = &*COMPACTION_DURATION;
    let _ = &*FLUSH_COUNT;
    let _ = &*FLUSH_DURATION;
    let _ = &*ERROR_COUNTER;
    let _ = &*SSTABLE_COUNT;
    let _ = &*SSTABLE_SIZE;
    let _ = &*WAL_SIZE;
    let _ = &*WAL_SYNC_DURATION;
    let _ = &*BACKUP_COUNT;
    let _ = &*BACKUP_DURATION;
    let _ = &*RESTORE_DURATION;
    let _ = &*CLUSTER_NODES;
    let _ = &*CLUSTER_REQUESTS;
}

// Helper functions for convenience

/// Record a get operation
pub fn record_get_operation(duration: f64, success: bool, error_type: Option<&str>) {
    if success {
        REQUEST_COUNTER.with_label_values(&["get", "success"]).inc();
        REQUEST_DURATION.with_label_values(&["get"]).observe(duration);
    } else if let Some(err) = error_type {
        REQUEST_COUNTER.with_label_values(&["get", "error"]).inc();
        ERROR_COUNTER.with_label_values(&["get", err]).inc();
    }
}

/// Record a put operation
pub fn record_put_operation(duration: f64, success: bool, error_type: Option<&str>) {
    if success {
        REQUEST_COUNTER.with_label_values(&["put", "success"]).inc();
        REQUEST_DURATION.with_label_values(&["put"]).observe(duration);
    } else if let Some(err) = error_type {
        REQUEST_COUNTER.with_label_values(&["put", "error"]).inc();
        ERROR_COUNTER.with_label_values(&["put", err]).inc();
    }
}

/// Record a delete operation
pub fn record_delete_operation(duration: f64, success: bool, error_type: Option<&str>) {
    if success {
        REQUEST_COUNTER.with_label_values(&["delete", "success"]).inc();
        REQUEST_DURATION.with_label_values(&["delete"]).observe(duration);
    } else if let Some(err) = error_type {
        REQUEST_COUNTER.with_label_values(&["delete", "error"]).inc();
        ERROR_COUNTER.with_label_values(&["delete", err]).inc();
    }
}

/// Record a flush operation
pub fn record_flush_operation(duration: f64, success: bool) {
    let status = if success { "success" } else { "error" };
    FLUSH_COUNT.with_label_values(&[status]).inc();
    if success {
        FLUSH_DURATION.with_label_values(&[] as &[&str]).observe(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        assert!(collector.export().is_ok());
    }

    #[test]
    fn test_record_operations() {
        let collector = MetricsCollector::new();

        // Record various operations
        collector.record_get_success(0.001);
        collector.record_put_success(0.002);
        collector.record_delete_success(0.001);
        collector.record_get_error("not_found");

        // Should be able to export
        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_requests_total"));
        assert!(metrics.contains("aidb_request_duration_seconds"));
    }

    #[test]
    fn test_cache_metrics() {
        let collector = MetricsCollector::new();

        collector.record_cache_hit("block_cache");
        collector.record_cache_miss("block_cache");

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_cache_hits_total"));
        assert!(metrics.contains("aidb_cache_misses_total"));
    }

    #[test]
    fn test_compaction_metrics() {
        let collector = MetricsCollector::new();

        collector.record_compaction(0, 1.5);
        collector.record_compaction(1, 2.5);

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_compactions_total"));
        assert!(metrics.contains("aidb_compaction_duration_seconds"));
    }

    #[test]
    fn test_flush_metrics() {
        let collector = MetricsCollector::new();

        collector.record_flush(0.5, true);
        collector.record_flush(0.0, false);

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_flushes_total"));
    }

    #[test]
    fn test_system_metrics() {
        let collector = MetricsCollector::new();

        collector.update_memory_usage("memtable", 1024 * 1024);
        collector.update_disk_usage("/data", 1024 * 1024 * 1024);

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_memory_bytes"));
        assert!(metrics.contains("aidb_disk_bytes"));
    }

    #[test]
    fn test_sstable_metrics() {
        let collector = MetricsCollector::new();

        collector.update_sstable_stats(0, 10, 1024 * 1024 * 10);
        collector.update_sstable_stats(1, 5, 1024 * 1024 * 5);

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_sstables_total"));
        assert!(metrics.contains("aidb_sstable_size_bytes"));
    }

    #[test]
    fn test_wal_metrics() {
        let collector = MetricsCollector::new();

        collector.update_wal_size(1024 * 1024);
        collector.record_wal_sync(0.001);

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_wal_size_bytes"));
        assert!(metrics.contains("aidb_wal_sync_duration_seconds"));
    }

    #[test]
    fn test_backup_metrics() {
        let collector = MetricsCollector::new();

        collector.record_backup(10.0, true);
        collector.record_restore(15.0, true);

        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_backups_total"));
        assert!(metrics.contains("aidb_backup_duration_seconds"));
        assert!(metrics.contains("aidb_restore_duration_seconds"));
    }

    #[test]
    fn test_helper_functions() {
        record_get_operation(0.001, true, None);
        record_put_operation(0.002, true, None);
        record_delete_operation(0.001, true, None);
        record_flush_operation(0.5, true);

        // Test error cases
        record_get_operation(0.0, false, Some("io_error"));
        record_put_operation(0.0, false, Some("disk_full"));

        let collector = MetricsCollector::new();
        let metrics = collector.export().unwrap();
        assert!(metrics.contains("aidb_requests_total"));
    }
}
