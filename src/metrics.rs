//! 集中式 OTel Metrics (`monitoring` feature).
//!
//! aidb 无独立 OTLP 出口; 嵌入方 (aikv) 初始化 global `MeterProvider` 后调用 `init()`.

#[cfg(feature = "monitoring")]
use parking_lot::RwLock;
#[cfg(feature = "monitoring")]
use std::sync::Arc;

#[cfg(feature = "monitoring")]
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
#[cfg(feature = "monitoring")]
use opentelemetry::KeyValue;

#[cfg(feature = "monitoring")]
static METRICS: RwLock<Option<Arc<OtelMetrics>>> = RwLock::new(None);

#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
const ATTR_OP: &str = "aidb.operation.name";
#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
const ATTR_COMPACTION_PHASE: &str = "aidb.compaction.phase";
#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
const ATTR_MEMTABLE_STATE: &str = "aidb.memtable.state";
#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
const ATTR_SSTABLE_LEVEL: &str = "aidb.sstable.level";
#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
const ATTR_BACKUP_OP: &str = "aidb.backup.operation";
#[cfg(all(feature = "monitoring", feature = "cluster"))]
const ATTR_RAFT_RPC_TYPE: &str = "aidb.raft.rpc.type";
#[cfg(all(feature = "monitoring", feature = "cluster"))]
const ATTR_RAFT_DIRECTION: &str = "aidb.raft.rpc.direction";
#[cfg(all(feature = "monitoring", feature = "cluster"))]
const ATTR_RAFT_GROUP_ID: &str = "aidb.raft.group.id";
#[cfg(all(feature = "monitoring", feature = "cluster"))]
const ATTR_RAFT_RESTART_OUTCOME: &str = "aidb.raft.group.restart.outcome";
#[cfg(feature = "monitoring")]
const ATTR_DB_SYSTEM: &str = "db.system";
#[cfg(feature = "monitoring")]
const ATTR_DB_OPERATION: &str = "db.operation.name";

#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
struct OtelMetrics {
    wal_size_bytes: Gauge<f64>,
    memtable_size_bytes: Gauge<f64>,
    sstable_count: Gauge<f64>,
    sstable_size_bytes: Gauge<f64>,
    operations_total: Counter<u64>,
    operation_duration_seconds: Histogram<f64>,
    db_client_operations: Counter<u64>,
    db_client_operation_duration: Histogram<f64>,
    flush_duration_seconds: Histogram<f64>,
    block_cache_size_bytes: Gauge<f64>,
    block_cache_capacity_bytes: Gauge<f64>,
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
    #[cfg(feature = "cluster")]
    raft_group_fatal_total: Counter<u64>,
    #[cfg(feature = "cluster")]
    raft_group_restart_total: Counter<u64>,
}

#[cfg(feature = "monitoring")]
impl OtelMetrics {
    fn new(meter: Meter) -> Self {
        Self {
            wal_size_bytes: meter
                .f64_gauge("aidb_wal_size_bytes")
                .with_description("WAL 文件总大小")
                .with_unit("By")
                .build(),
            memtable_size_bytes: meter
                .f64_gauge("aidb_memtable_size_bytes")
                .with_description("MemTable 近似大小 (user_key+value 字节)")
                .with_unit("By")
                .build(),
            sstable_count: meter
                .f64_gauge("aidb_sstable_count")
                .with_description("各层 SSTable 文件数量")
                .with_unit("1")
                .build(),
            sstable_size_bytes: meter
                .f64_gauge("aidb_sstable_size_bytes")
                .with_description("各层 SSTable 文件总大小")
                .with_unit("By")
                .build(),
            operations_total: meter
                .u64_counter("aidb_operations_total")
                .with_description("DB 操作总数")
                .with_unit("1")
                .build(),
            operation_duration_seconds: meter
                .f64_histogram("aidb_operation_duration_seconds")
                .with_description("DB 操作耗时")
                .with_unit("s")
                .with_boundaries(vec![
                    0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0,
                ])
                .build(),
            db_client_operations: meter
                .u64_counter("db.client.operations")
                .with_description("DB client operations (OTel semconv)")
                .with_unit("{operation}")
                .build(),
            db_client_operation_duration: meter
                .f64_histogram("db.client.operation.duration")
                .with_description("DB client operation duration (OTel semconv)")
                .with_unit("s")
                .with_boundaries(vec![
                    0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0,
                ])
                .build(),
            flush_duration_seconds: meter
                .f64_histogram("aidb_flush_duration_seconds")
                .with_description("MemTable flush 耗时")
                .with_unit("s")
                .with_boundaries(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
                .build(),
            block_cache_size_bytes: meter
                .f64_gauge("aidb_block_cache_size_bytes")
                .with_description("Block Cache 当前大小")
                .with_unit("By")
                .build(),
            block_cache_capacity_bytes: meter
                .f64_gauge("aidb_block_cache_capacity_bytes")
                .with_description("Block Cache 配置容量上限")
                .with_unit("By")
                .build(),
            block_cache_hits_total: meter
                .u64_counter("aidb_block_cache_hits_total")
                .with_description("Block Cache 命中次数")
                .with_unit("1")
                .build(),
            block_cache_misses_total: meter
                .u64_counter("aidb_block_cache_misses_total")
                .with_description("Block Cache 未命中次数")
                .with_unit("1")
                .build(),
            bloom_false_positive_total: meter
                .u64_counter("aidb_bloom_false_positive_total")
                .with_description("Bloom Filter 假阳性次数")
                .with_unit("1")
                .build(),
            flush_total: meter
                .u64_counter("aidb_flush_total")
                .with_description("MemTable flush 次数")
                .with_unit("1")
                .build(),
            sequence: meter
                .f64_gauge("aidb_sequence")
                .with_description("当前 DB sequence")
                .with_unit("{sequence}")
                .build(),
            total_key_count: meter
                .f64_gauge("aidb_total_key_count")
                .with_description("近似存活 key 数")
                .with_unit("{key}")
                .build(),
            compaction_total: meter
                .u64_counter("aidb_compaction_total")
                .with_description("Compaction 次数")
                .with_unit("1")
                .build(),
            compaction_duration_seconds: meter
                .f64_histogram("aidb_compaction_duration_seconds")
                .with_description("Compaction 各阶段耗时")
                .with_unit("s")
                .with_boundaries(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
                .build(),
            backup_total: meter
                .u64_counter("aidb_backup_total")
                .with_description("备份操作计数")
                .with_unit("1")
                .build(),
            backup_size_bytes: meter
                .f64_gauge("aidb_backup_size_bytes")
                .with_description("备份文件总大小（字节）")
                .with_unit("By")
                .build(),
            backup_duration_seconds: meter
                .f64_histogram("aidb_backup_duration_seconds")
                .with_description("备份操作耗时（秒）")
                .with_unit("s")
                .with_boundaries(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0])
                .build(),
            #[cfg(feature = "cluster")]
            raft_rpc_total: meter
                .u64_counter("aidb_raft_rpc_total")
                .with_description("Raft RPC 调用次数")
                .with_unit("1")
                .build(),
            #[cfg(feature = "cluster")]
            raft_log_entries_total: meter
                .u64_counter("aidb_raft_log_entries_total")
                .with_description("Raft 日志条目累计数")
                .with_unit("1")
                .build(),
            #[cfg(feature = "cluster")]
            raft_group_fatal_total: meter
                .u64_counter("aidb_raft_group_fatal_total")
                .with_description("检测到 Raft group 进入 Fatal 状态 (apply/存储错误) 的次数")
                .with_unit("1")
                .build(),
            #[cfg(feature = "cluster")]
            raft_group_restart_total: meter
                .u64_counter("aidb_raft_group_restart_total")
                .with_description("Raft group 自愈重启尝试次数 (按结果分类)")
                .with_unit("1")
                .build(),
        }
    }
}

#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
fn metrics() -> Option<Arc<OtelMetrics>> {
    METRICS.read().clone()
}

#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
fn db_client_attrs(op: &str) -> [KeyValue; 2] {
    [
        kv_static(ATTR_DB_SYSTEM, "aidb"),
        kv_static(ATTR_DB_OPERATION, op),
    ]
}

#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
fn kv_static(label: &str, value: impl Into<String>) -> KeyValue {
    KeyValue::new(label.to_string(), value.into())
}

#[cfg(feature = "monitoring")]
#[allow(dead_code)] // Task 4 sync_to_otel 接管后移除
fn kv(label: &str, value: impl Into<String>) -> KeyValue {
    KeyValue::new(label.to_string(), value.into())
}

/// 绑定 OTel Meter (幂等). 通常由 `init()` 在 global provider 就绪后调用.
#[cfg(feature = "monitoring")]
pub fn init_otel(meter: Meter) {
    *METRICS.write() = Some(Arc::new(OtelMetrics::new(meter)));
}

/// 初始化 OTel 指标 (幂等). 需要 global `MeterProvider` 已由嵌入方设置.
pub fn init() {
    #[cfg(feature = "monitoring")]
    {
        if METRICS.read().is_none() {
            let meter = opentelemetry::global::meter("aidb");
            init_otel(meter);
        }
    }
}

#[cfg(all(feature = "monitoring", feature = "cluster"))]
pub fn record_raft_rpc(rpc_type: &str, direction: &str) {
    if let Some(m) = metrics() {
        m.raft_rpc_total.add(
            1,
            &[
                kv(ATTR_RAFT_RPC_TYPE, rpc_type),
                kv(ATTR_RAFT_DIRECTION, direction),
            ],
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

#[cfg(all(feature = "monitoring", feature = "cluster"))]
pub fn record_raft_group_fatal(group_id: u64) {
    if let Some(m) = metrics() {
        m.raft_group_fatal_total
            .add(1, &[kv(ATTR_RAFT_GROUP_ID, group_id.to_string())]);
    }
}

/// `outcome`: "success" | "failure" | "skipped_backoff".
#[cfg(all(feature = "monitoring", feature = "cluster"))]
pub fn record_raft_group_restart(group_id: u64, outcome: &str) {
    if let Some(m) = metrics() {
        m.raft_group_restart_total.add(
            1,
            &[
                kv(ATTR_RAFT_GROUP_ID, group_id.to_string()),
                kv(ATTR_RAFT_RESTART_OUTCOME, outcome),
            ],
        );
    }
}

#[cfg(not(feature = "monitoring"))]
pub fn record_raft_rpc(_rpc_type: &str, _direction: &str) {}

#[cfg(not(feature = "monitoring"))]
pub fn record_raft_log_entries(_count: u64) {}

#[cfg(all(not(feature = "monitoring"), feature = "cluster"))]
pub fn record_raft_group_fatal(_group_id: u64) {}

#[cfg(all(not(feature = "monitoring"), feature = "cluster"))]
pub fn record_raft_group_restart(_group_id: u64, _outcome: &str) {}

#[cfg(feature = "monitoring")]
pub mod testutil {
    use std::sync::{Arc, OnceLock};

    use opentelemetry::global;
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

    static TEST_EXPORTER: OnceLock<InMemoryMetricExporter> = OnceLock::new();
    static TEST_PROVIDER: OnceLock<Arc<SdkMeterProvider>> = OnceLock::new();

    /// 测试用: 安装 InMemory exporter 并 init aidb metrics.
    pub fn init_in_memory() -> InMemoryMetricExporter {
        if let Some(exporter) = TEST_EXPORTER.get() {
            if let Some(provider) = TEST_PROVIDER.get() {
                let meter = provider.meter("aidb");
                super::init_otel(meter);
            }
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
        let meter = provider.meter("aidb");
        super::init_otel(meter);
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
                    total += h.data_points().map(|dp| dp.count()).sum::<u64>();
                }
            }
        }
        total
    }
}
