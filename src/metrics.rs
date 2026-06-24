//! 集中式 OTel Metrics (`monitoring` feature).
//!
//! aidb 无独立 OTLP 出口; 嵌入方 (aikv) 初始化 global `MeterProvider` 后调用 `init()`.

#[cfg(feature = "monitoring")]
use std::sync::{Arc, OnceLock};

#[cfg(feature = "monitoring")]
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
#[cfg(feature = "monitoring")]
use opentelemetry::KeyValue;

#[cfg(feature = "monitoring")]
static METRICS: OnceLock<Arc<OtelMetrics>> = OnceLock::new();

#[cfg(feature = "monitoring")]
struct OtelMetrics {
    wal_size_bytes: Gauge<f64>,
    memtable_size_bytes: Gauge<f64>,
    sstable_count: Gauge<f64>,
    sstable_size_bytes: Gauge<f64>,
    operations_total: Counter<u64>,
    operation_duration_seconds: Histogram<f64>,
    flush_duration_seconds: Histogram<f64>,
    block_cache_size_bytes: Gauge<f64>,
    block_cache_hits_total: Counter<u64>,
    block_cache_misses_total: Counter<u64>,
    bloom_false_positive_total: Counter<u64>,
    flush_total: Counter<u64>,
    sequence: Gauge<f64>,
    total_key_count: Gauge<f64>,
    compaction_total: Counter<u64>,
    compaction_duration_seconds: Histogram<f64>,
    backup_total: Counter<u64>,
    backup_size_bytes: Gauge<f64>,
    backup_duration_seconds: Histogram<f64>,
    #[cfg(feature = "cluster")]
    raft_rpc_total: Counter<u64>,
    #[cfg(feature = "cluster")]
    raft_log_entries_total: Counter<u64>,
}

#[cfg(feature = "monitoring")]
impl OtelMetrics {
    fn new(meter: Meter) -> Self {
        Self {
            wal_size_bytes: meter
                .f64_gauge("aidb_wal_size_bytes")
                .with_description("WAL 文件总大小")
                .build(),
            memtable_size_bytes: meter
                .f64_gauge("aidb_memtable_size_bytes")
                .with_description("MemTable 近似大小 (user_key+value 字节)")
                .build(),
            sstable_count: meter
                .f64_gauge("aidb_sstable_count")
                .with_description("各层 SSTable 文件数量")
                .build(),
            sstable_size_bytes: meter
                .f64_gauge("aidb_sstable_size_bytes")
                .with_description("各层 SSTable 文件总大小")
                .build(),
            operations_total: meter
                .u64_counter("aidb_operations_total")
                .with_description("DB 操作总数")
                .build(),
            operation_duration_seconds: meter
                .f64_histogram("aidb_operation_duration_seconds")
                .with_description("DB 操作耗时")
                .with_boundaries(vec![
                    0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0,
                ])
                .build(),
            flush_duration_seconds: meter
                .f64_histogram("aidb_flush_duration_seconds")
                .with_description("MemTable flush 耗时")
                .with_boundaries(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
                .build(),
            block_cache_size_bytes: meter
                .f64_gauge("aidb_block_cache_size_bytes")
                .with_description("Block Cache 当前大小")
                .build(),
            block_cache_hits_total: meter
                .u64_counter("aidb_block_cache_hits_total")
                .with_description("Block Cache 命中次数")
                .build(),
            block_cache_misses_total: meter
                .u64_counter("aidb_block_cache_misses_total")
                .with_description("Block Cache 未命中次数")
                .build(),
            bloom_false_positive_total: meter
                .u64_counter("aidb_bloom_false_positive_total")
                .with_description("Bloom Filter 假阳性次数")
                .build(),
            flush_total: meter
                .u64_counter("aidb_flush_total")
                .with_description("MemTable flush 次数")
                .build(),
            sequence: meter
                .f64_gauge("aidb_sequence")
                .with_description("当前 DB sequence")
                .build(),
            total_key_count: meter
                .f64_gauge("aidb_total_key_count")
                .with_description("近似存活 key 数")
                .build(),
            compaction_total: meter
                .u64_counter("aidb_compaction_total")
                .with_description("Compaction 次数")
                .build(),
            compaction_duration_seconds: meter
                .f64_histogram("aidb_compaction_duration_seconds")
                .with_description("Compaction 各阶段耗时")
                .with_boundaries(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
                .build(),
            backup_total: meter
                .u64_counter("aidb_backup_total")
                .with_description("备份操作计数")
                .build(),
            backup_size_bytes: meter
                .f64_gauge("aidb_backup_size_bytes")
                .with_description("备份文件总大小（字节）")
                .build(),
            backup_duration_seconds: meter
                .f64_histogram("aidb_backup_duration_seconds")
                .with_description("备份操作耗时（秒）")
                .with_boundaries(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0])
                .build(),
            #[cfg(feature = "cluster")]
            raft_rpc_total: meter
                .u64_counter("aidb_raft_rpc_total")
                .with_description("Raft RPC 调用次数")
                .build(),
            #[cfg(feature = "cluster")]
            raft_log_entries_total: meter
                .u64_counter("aidb_raft_log_entries_total")
                .with_description("Raft 日志条目累计数")
                .build(),
        }
    }
}

#[cfg(feature = "monitoring")]
fn metrics() -> Option<&'static Arc<OtelMetrics>> {
    METRICS.get()
}

#[cfg(feature = "monitoring")]
fn kv(label: &str, value: impl Into<String>) -> KeyValue {
    KeyValue::new(label.to_string(), value.into())
}

/// 绑定 OTel Meter (幂等). 通常由 `init()` 在 global provider 就绪后调用.
#[cfg(feature = "monitoring")]
pub fn init_otel(meter: Meter) {
    let _ = METRICS.set(Arc::new(OtelMetrics::new(meter)));
}

/// 初始化 OTel 指标 (幂等). 需要 global `MeterProvider` 已由嵌入方设置.
pub fn init() {
    #[cfg(feature = "monitoring")]
    {
        if METRICS.get().is_none() {
            let meter = opentelemetry::global::meter("aidb");
            init_otel(meter);
        }
    }
}

#[cfg(feature = "monitoring")]
pub fn set_wal_size(bytes: u64) {
    if let Some(m) = metrics() {
        m.wal_size_bytes.record(bytes as f64, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_operation(op: &str) {
    if let Some(m) = metrics() {
        m.operations_total.add(1, &[kv("op", op)]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_operation_duration(op: &str, secs: f64) {
    if let Some(m) = metrics() {
        m.operation_duration_seconds.record(secs, &[kv("op", op)]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_flush() {
    if let Some(m) = metrics() {
        m.flush_total.add(1, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_flush_duration(secs: f64) {
    if let Some(m) = metrics() {
        m.flush_duration_seconds.record(secs, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn set_block_cache_size(bytes: u64) {
    if let Some(m) = metrics() {
        m.block_cache_size_bytes.record(bytes as f64, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_block_cache_hit() {
    if let Some(m) = metrics() {
        m.block_cache_hits_total.add(1, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_block_cache_miss() {
    if let Some(m) = metrics() {
        m.block_cache_misses_total.add(1, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_bloom_false_positive() {
    if let Some(m) = metrics() {
        m.bloom_false_positive_total.add(1, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn set_sequence(seq: u64) {
    if let Some(m) = metrics() {
        m.sequence.record(seq as f64, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn set_total_key_count(count: usize) {
    if let Some(m) = metrics() {
        m.total_key_count.record(count as f64, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn memtable_set_active(bytes: usize) {
    if let Some(m) = metrics() {
        m.memtable_size_bytes
            .record(bytes as f64, &[kv("state", "active")]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_compaction(phase: &str) {
    if let Some(m) = metrics() {
        m.compaction_total.add(1, &[kv("type", phase)]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_compaction_duration(phase: &str, secs: f64) {
    if let Some(m) = metrics() {
        m.compaction_duration_seconds
            .record(secs, &[kv("phase", phase)]);
    }
}

#[cfg(feature = "monitoring")]
pub fn memtable_on_freeze(frozen_bytes: usize) {
    if let Some(m) = metrics() {
        m.memtable_size_bytes
            .record(frozen_bytes as f64, &[kv("state", "frozen")]);
        m.memtable_size_bytes
            .record(0.0, &[kv("state", "active")]);
    }
}

#[cfg(feature = "monitoring")]
pub fn set_sstable_level(level: &str, count: i64, size_bytes: i64) {
    if let Some(m) = metrics() {
        let attrs = [kv("level", level)];
        m.sstable_count.record(count as f64, &attrs);
        m.sstable_size_bytes.record(size_bytes as f64, &attrs);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_backup_create(size_bytes: u64, duration_secs: f64) {
    if let Some(m) = metrics() {
        m.backup_total.add(1, &[kv("op", "create")]);
        m.backup_size_bytes.record(size_bytes as f64, &[]);
        m.backup_duration_seconds.record(duration_secs, &[]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_backup_delete() {
    if let Some(m) = metrics() {
        m.backup_total.add(1, &[kv("op", "delete")]);
    }
}

#[cfg(feature = "monitoring")]
pub fn record_backup_restore() {
    if let Some(m) = metrics() {
        m.backup_total.add(1, &[kv("op", "restore")]);
    }
}

#[cfg(all(feature = "monitoring", feature = "cluster"))]
pub fn record_raft_rpc(rpc_type: &str, direction: &str) {
    if let Some(m) = metrics() {
        m.raft_rpc_total.add(
            1,
            &[kv("type", rpc_type), kv("direction", direction)],
        );
    }
}

#[cfg(all(feature = "monitoring", feature = "cluster"))]
pub fn record_raft_log_entries(count: u64) {
    if count > 0 {
        if let Some(m) = metrics() {
            m.raft_log_entries_total.add(count, &[]);
        }
    }
}

#[cfg(not(feature = "monitoring"))]
pub fn record_raft_rpc(_rpc_type: &str, _direction: &str) {}

#[cfg(not(feature = "monitoring"))]
pub fn record_raft_log_entries(_count: u64) {}

#[cfg(feature = "monitoring")]
pub mod testutil {
    use std::sync::{Arc, OnceLock};

    use opentelemetry::global;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

    static TEST_EXPORTER: OnceLock<InMemoryMetricExporter> = OnceLock::new();
    static TEST_PROVIDER: OnceLock<Arc<SdkMeterProvider>> = OnceLock::new();

    /// 测试用: 安装 InMemory exporter 并 init aidb metrics.
    pub fn init_in_memory() -> InMemoryMetricExporter {
        if let Some(exporter) = TEST_EXPORTER.get() {
            super::init();
            return exporter.clone();
        }
        let exporter = InMemoryMetricExporter::default();
        let provider = SdkMeterProvider::builder()
            .with_periodic_exporter(exporter.clone())
            .build();
        global::set_meter_provider(provider.clone());
        let provider = Arc::new(provider);
        let _ = TEST_PROVIDER.set(Arc::clone(&provider));
        let _ = TEST_EXPORTER.set(exporter.clone());
        super::init();
        exporter
    }

    fn latest_resource_metrics(
        exporter: &InMemoryMetricExporter,
    ) -> Option<Vec<opentelemetry_sdk::metrics::data::ResourceMetrics>> {
        if let Some(provider) = TEST_PROVIDER.get() {
            provider.force_flush().unwrap();
        }
        exporter.get_finished_metrics().ok()
    }

    pub fn counter_sum(exporter: &InMemoryMetricExporter, name: &str) -> u64 {
        let Some(metrics) = latest_resource_metrics(exporter) else {
            return 0;
        };
        let Some(rm) = metrics.last() else {
            return 0;
        };
        let mut total = 0u64;
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != name {
                    continue;
                }
                if let AggregatedMetrics::U64(MetricData::Sum(sum)) = m.data() {
                    total += sum.data_points().map(|dp| dp.value()).sum::<u64>();
                }
            }
        }
        total
    }

    pub fn gauge_value(exporter: &InMemoryMetricExporter, name: &str) -> f64 {
        let Some(metrics) = latest_resource_metrics(exporter) else {
            return 0.0;
        };
        let Some(rm) = metrics.last() else {
            return 0.0;
        };
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != name {
                    continue;
                }
                if let AggregatedMetrics::F64(MetricData::Gauge(g)) = m.data() {
                    if let Some(dp) = g.data_points().last() {
                        return dp.value();
                    }
                }
            }
        }
        0.0
    }

    pub fn histogram_count(exporter: &InMemoryMetricExporter, name: &str) -> u64 {
        let Some(metrics) = latest_resource_metrics(exporter) else {
            return 0;
        };
        let Some(rm) = metrics.last() else {
            return 0;
        };
        let mut total = 0u64;
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != name {
                    continue;
                }
                if let AggregatedMetrics::F64(MetricData::Histogram(h)) = m.data() {
                    total += h
                        .data_points()
                        .map(|dp| dp.count() as u64)
                        .sum::<u64>();
                }
            }
        }
        total
    }
}
