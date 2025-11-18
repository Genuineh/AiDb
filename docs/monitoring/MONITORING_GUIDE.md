# AiDb Monitoring Guide

This guide explains how to monitor AiDb using Prometheus metrics and how to set up monitoring infrastructure.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Metrics Reference](#metrics-reference)
- [Grafana Dashboards](#grafana-dashboards)
- [Alert Rules](#alert-rules)
- [Best Practices](#best-practices)

## Overview

AiDb provides comprehensive monitoring through Prometheus-compatible metrics. The monitoring system tracks:

- **Request Metrics**: Operations per second, latency, error rates
- **System Metrics**: CPU, memory, disk usage
- **Business Metrics**: Cache hit rates, compaction statistics
- **Error Metrics**: Operation failures by type

## Quick Start

### 1. Enable Monitoring Feature

Add the `monitoring` feature to your `Cargo.toml`:

```toml
[dependencies]
aidb = { version = "0.1", features = ["monitoring"] }
```

### 2. Start Metrics Server

```rust
use aidb::monitoring::MetricsServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create metrics server
    let addr = "127.0.0.1:9090".parse()?;
    let server = MetricsServer::new(addr);
    
    // Get collector for recording metrics
    let collector = server.collector();
    
    // Start server
    tokio::spawn(async move {
        server.run().await
    });
    
    // Metrics available at http://127.0.0.1:9090/metrics
    
    Ok(())
}
```

### 3. Record Metrics

```rust
use aidb::monitoring::MetricsCollector;

let collector = MetricsCollector::new();

// Record operations
let start = std::time::Instant::now();
db.put(b"key", b"value")?;
collector.record_put_success(start.elapsed().as_secs_f64());

// Update system metrics
collector.update_memory_usage("memtable", 1024 * 1024);
collector.update_disk_usage("/data", 100 * 1024 * 1024);

// Export metrics
let metrics_text = collector.export()?;
println!("{}", metrics_text);
```

### 4. Configure Prometheus

Create `prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'aidb'
    static_configs:
      - targets: ['localhost:9090']
```

Start Prometheus:

```bash
prometheus --config.file=prometheus.yml
```

## Metrics Reference

### Request Metrics

#### `aidb_requests_total`
Counter of total requests by operation and status.

**Labels:**
- `operation`: get, put, delete
- `status`: success, error

**Example:**
```promql
rate(aidb_requests_total{operation="get",status="success"}[5m])
```

#### `aidb_request_duration_seconds`
Histogram of request duration in seconds.

**Labels:**
- `operation`: get, put, delete

**Buckets:** 1ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, 2.5s, 5s, 10s

**Example:**
```promql
histogram_quantile(0.99, rate(aidb_request_duration_seconds_bucket[5m]))
```

### System Metrics

#### `aidb_memory_bytes`
Gauge of memory usage in bytes by component.

**Labels:**
- `component`: memtable, block_cache, etc.

**Example:**
```promql
aidb_memory_bytes{component="memtable"}
```

#### `aidb_disk_bytes`
Gauge of disk usage in bytes by path.

**Labels:**
- `path`: filesystem path

**Example:**
```promql
aidb_disk_bytes{path="/data"}
```

### Business Metrics

#### `aidb_cache_hits_total` / `aidb_cache_misses_total`
Counters for cache hits and misses.

**Labels:**
- `cache_type`: block_cache

**Example (hit rate):**
```promql
rate(aidb_cache_hits_total[5m]) / 
(rate(aidb_cache_hits_total[5m]) + rate(aidb_cache_misses_total[5m]))
```

#### `aidb_compactions_total`
Counter of total compactions by level.

**Labels:**
- `level`: 0, 1, 2, etc.

**Example:**
```promql
rate(aidb_compactions_total[5m])
```

#### `aidb_compaction_duration_seconds`
Histogram of compaction duration.

**Labels:**
- `level`: 0, 1, 2, etc.

**Buckets:** 100ms, 500ms, 1s, 5s, 10s, 30s, 60s, 120s, 300s

### SSTable Metrics

#### `aidb_sstables_total`
Gauge of total SSTables by level.

**Labels:**
- `level`: 0, 1, 2, etc.

#### `aidb_sstable_size_bytes`
Gauge of SSTable total size by level.

**Labels:**
- `level`: 0, 1, 2, etc.

### WAL Metrics

#### `aidb_wal_size_bytes`
Gauge of current WAL size in bytes.

#### `aidb_wal_sync_duration_seconds`
Histogram of WAL sync duration.

**Buckets:** 100µs, 500µs, 1ms, 5ms, 10ms, 50ms, 100ms

### Backup Metrics

#### `aidb_backups_total`
Counter of total backups by status.

**Labels:**
- `status`: success, error

#### `aidb_backup_duration_seconds`
Histogram of backup duration.

**Buckets:** 1s, 5s, 10s, 30s, 60s, 300s, 600s, 1800s

#### `aidb_restore_duration_seconds`
Histogram of restore duration (same buckets as backup).

### Cluster Metrics

#### `aidb_cluster_nodes`
Gauge of cluster nodes by type and status.

**Labels:**
- `node_type`: primary, replica
- `status`: healthy, unhealthy

#### `aidb_cluster_requests_total`
Counter of cluster RPC requests.

**Labels:**
- `method`: RPC method name
- `status`: success, error

### Error Metrics

#### `aidb_errors_total`
Counter of errors by operation and type.

**Labels:**
- `operation`: get, put, delete, flush, compaction, etc.
- `error_type`: io_error, not_found, corruption, etc.

**Example:**
```promql
rate(aidb_errors_total[5m])
```

## Grafana Dashboards

### System Overview Dashboard

Key panels:
1. **Request Rate**: `rate(aidb_requests_total[5m])`
2. **Latency (P99)**: `histogram_quantile(0.99, rate(aidb_request_duration_seconds_bucket[5m]))`
3. **Error Rate**: `rate(aidb_errors_total[5m])`
4. **Memory Usage**: `aidb_memory_bytes`
5. **Disk Usage**: `aidb_disk_bytes`

### Performance Dashboard

Key panels:
1. **Throughput**: Operations per second by type
2. **Latency Distribution**: Heatmap of request duration
3. **Cache Hit Rate**: Cache efficiency over time
4. **Compaction Activity**: Compactions per level

### Cluster Dashboard

Key panels:
1. **Node Status**: Number of healthy/unhealthy nodes
2. **RPC Request Rate**: Cluster communication volume
3. **Data Distribution**: SSTable distribution across nodes
4. **Replication Lag**: Time difference between primary and replicas

## Alert Rules

### Critical Alerts

```yaml
groups:
  - name: aidb_critical
    interval: 10s
    rules:
      # High error rate
      - alert: HighErrorRate
        expr: rate(aidb_errors_total[5m]) > 10
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High error rate detected"
          description: "Error rate is {{ $value }} errors/sec"
      
      # Node down
      - alert: NodeDown
        expr: aidb_cluster_nodes{status="healthy"} < 1
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "No healthy nodes available"
      
      # Disk full
      - alert: DiskAlmostFull
        expr: aidb_disk_bytes / (100 * 1024 * 1024 * 1024) > 0.9
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Disk usage over 90%"
```

### Warning Alerts

```yaml
      # High latency
      - alert: HighLatency
        expr: histogram_quantile(0.99, rate(aidb_request_duration_seconds_bucket[5m])) > 1.0
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "P99 latency over 1 second"
      
      # High memory usage
      - alert: HighMemoryUsage
        expr: aidb_memory_bytes{component="memtable"} > (1024 * 1024 * 1024)
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "MemTable memory usage over 1GB"
      
      # Low cache hit rate
      - alert: LowCacheHitRate
        expr: |
          rate(aidb_cache_hits_total[5m]) / 
          (rate(aidb_cache_hits_total[5m]) + rate(aidb_cache_misses_total[5m])) < 0.5
        for: 15m
        labels:
          severity: warning
        annotations:
          summary: "Cache hit rate below 50%"
```

### Info Alerts

```yaml
      # Compaction needed
      - alert: ManyLevel0Tables
        expr: aidb_sstables_total{level="0"} > 10
        for: 5m
        labels:
          severity: info
        annotations:
          summary: "Many Level 0 SSTables, compaction recommended"
      
      # Large WAL
      - alert: LargeWAL
        expr: aidb_wal_size_bytes > (100 * 1024 * 1024)
        for: 10m
        labels:
          severity: info
        annotations:
          summary: "WAL size over 100MB, consider flush"
```

## Best Practices

### 1. Metrics Collection

- **Use helper functions**: Prefer `record_get_operation()` over manual counter updates
- **Measure duration accurately**: Use `std::time::Instant` for timing
- **Update periodically**: Update gauge metrics (memory, disk) every 5-10 seconds
- **Don't over-instrument**: Focus on critical paths

### 2. Prometheus Configuration

- **Scrape interval**: 15 seconds is a good default
- **Retention**: Keep at least 15 days of data
- **Storage**: Allocate adequate disk space (estimate: 1-2KB per sample)
- **Labels**: Use labels wisely, avoid high cardinality

### 3. Grafana Dashboards

- **Organize by concern**: Separate system, performance, and cluster dashboards
- **Use templating**: Create variables for time ranges, instances, operations
- **Set refresh rate**: 5-30 seconds for operational dashboards
- **Add annotations**: Mark deployments and incidents

### 4. Alerting

- **Start conservative**: Begin with fewer, critical alerts
- **Use runbooks**: Document response procedures for each alert
- **Avoid alert fatigue**: Tune thresholds to reduce false positives
- **Test alerts**: Verify alerts fire correctly before production

### 5. Performance Impact

- **Metrics are cheap**: Prometheus counters/gauges have minimal overhead
- **Histograms cost more**: Use appropriate bucket counts (10-15 buckets)
- **Sampling**: Consider sampling for extremely high-frequency operations
- **Async export**: Metrics server runs independently, no DB blocking

## Example Queries

### Top 5 slowest operations
```promql
topk(5, 
  histogram_quantile(0.99, 
    rate(aidb_request_duration_seconds_bucket[5m])
  ) by (operation)
)
```

### Error rate by operation
```promql
sum(rate(aidb_errors_total[5m])) by (operation)
```

### Memory breakdown
```promql
sum(aidb_memory_bytes) by (component)
```

### Compaction efficiency
```promql
rate(aidb_compactions_total[5m]) / 
sum(rate(aidb_compaction_duration_seconds_count[5m]))
```

### Cache efficiency
```promql
(
  rate(aidb_cache_hits_total[5m]) /
  (rate(aidb_cache_hits_total[5m]) + rate(aidb_cache_misses_total[5m]))
) * 100
```

## Troubleshooting

### Metrics not appearing

1. Check feature is enabled: `--features monitoring`
2. Verify server is running: `curl http://localhost:9090/health`
3. Check Prometheus scrape config
4. Look for errors in logs

### High memory usage

1. Check MemTable size: `aidb_memory_bytes{component="memtable"}`
2. Check cache size: `aidb_memory_bytes{component="block_cache"}`
3. Consider reducing cache size in options
4. Increase flush frequency

### Slow queries

1. Check P99 latency: `histogram_quantile(0.99, ...)`
2. Look for cache misses: `aidb_cache_misses_total`
3. Check for excessive Level 0 tables
4. Consider enabling compaction

### High error rate

1. Group errors by type: `sum by (error_type)`
2. Check disk space: `aidb_disk_bytes`
3. Verify file permissions
4. Check for corrupted data files

## Next Steps

- Set up [Grafana dashboards](#grafana-dashboards)
- Configure [alert rules](#alert-rules)
- Integrate with your monitoring stack
- Review [best practices](#best-practices)
- Explore the [metrics reference](#metrics-reference)
