//! 集中式 Metrics 注册表.
//!
//! 所有模块的 Prometheus 指标统一在此注册, 通过 `monitoring` feature 条件编译.

use std::sync::LazyLock;

/// WAL 文件总字节数
pub static WAL_SIZE: LazyLock<prometheus::Gauge> =
  LazyLock::new(|| prometheus::Gauge::new("aidb_wal_size_bytes", "WAL 文件总大小").unwrap());

/// MemTable 近似内存 (user_key+value), label `state`: `active` | `frozen`
pub static MEMTABLE_SIZE: LazyLock<prometheus::IntGaugeVec> = LazyLock::new(|| {
  prometheus::IntGaugeVec::new(
    prometheus::Opts::new(
      "aidb_memtable_size_bytes",
      "MemTable 近似大小 (user_key+value 字节)",
    ),
    &["state"],
  )
  .unwrap()
});

/// 各层 SSTable 文件数量
pub static SSTABLE_COUNT: LazyLock<prometheus::IntGaugeVec> = LazyLock::new(|| {
  prometheus::IntGaugeVec::new(
    prometheus::Opts::new("aidb_sstable_count", "各层 SSTable 文件数量"),
    &["level"],
  )
  .unwrap()
});

/// DB 操作计数
pub static OPERATIONS_TOTAL: LazyLock<prometheus::IntCounterVec> = LazyLock::new(|| {
  prometheus::IntCounterVec::new(
    prometheus::Opts::new("aidb_operations_total", "DB 操作总数"),
    &["op"],
  )
  .unwrap()
});

/// DB 操作耗时 (秒)
pub static OPERATION_DURATION: LazyLock<prometheus::HistogramVec> = LazyLock::new(|| {
  prometheus::HistogramVec::new(
    prometheus::HistogramOpts::new("aidb_operation_duration_seconds", "DB 操作耗时").buckets(
      vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0],
    ),
    &["op"],
  )
  .unwrap()
});

/// MemTable flush 耗时 (秒)
pub static FLUSH_DURATION: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
  prometheus::Histogram::with_opts(
    prometheus::HistogramOpts::new("aidb_flush_duration_seconds", "MemTable flush 耗时")
      .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
  )
  .unwrap()
});

/// Block Cache 当前占用 (字节)
pub static BLOCK_CACHE_SIZE_BYTES: LazyLock<prometheus::Gauge> = LazyLock::new(|| {
  prometheus::Gauge::new("aidb_block_cache_size_bytes", "Block Cache 当前大小").unwrap()
});

/// Block Cache 命中次数
pub static BLOCK_CACHE_HITS_TOTAL: LazyLock<prometheus::IntCounter> = LazyLock::new(|| {
  prometheus::IntCounter::new("aidb_block_cache_hits_total", "Block Cache 命中次数").unwrap()
});

/// Block Cache 未命中次数
pub static BLOCK_CACHE_MISSES_TOTAL: LazyLock<prometheus::IntCounter> = LazyLock::new(|| {
  prometheus::IntCounter::new(
    "aidb_block_cache_misses_total",
    "Block Cache 未命中次数",
  )
  .unwrap()
});

/// Bloom Filter 假阳性累计次数
pub static BLOOM_FALSE_POSITIVE_TOTAL: LazyLock<prometheus::IntCounter> = LazyLock::new(|| {
  prometheus::IntCounter::new(
    "aidb_bloom_false_positive_total",
    "Bloom Filter 假阳性次数",
  )
  .unwrap()
});

/// Flush 次数
pub static FLUSH_TOTAL: LazyLock<prometheus::IntCounter> = LazyLock::new(|| {
  prometheus::IntCounter::new("aidb_flush_total", "MemTable flush 次数").unwrap()
});

/// 当前 sequence
pub static SEQUENCE: LazyLock<prometheus::IntGauge> =
  LazyLock::new(|| prometheus::IntGauge::new("aidb_sequence", "当前 DB sequence").unwrap());

/// 近似 key 数量
pub static TOTAL_KEY_COUNT: LazyLock<prometheus::IntGauge> = LazyLock::new(|| {
  prometheus::IntGauge::new("aidb_total_key_count", "近似存活 key 数").unwrap()
});

/// Compaction 次数
pub static COMPACTION_TOTAL: LazyLock<prometheus::IntCounterVec> = LazyLock::new(|| {
  prometheus::IntCounterVec::new(
    prometheus::Opts::new("aidb_compaction_total", "Compaction 次数"),
    &["type"],
  )
  .unwrap()
});

/// Compaction 耗时 (秒)
pub static COMPACTION_DURATION: LazyLock<prometheus::HistogramVec> = LazyLock::new(|| {
  prometheus::HistogramVec::new(
    prometheus::HistogramOpts::new(
      "aidb_compaction_duration_seconds",
      "Compaction 各阶段耗时",
    )
    .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
    &["phase"],
  )
  .unwrap()
});

/// 各层 SSTable 文件总大小 (字节)
pub static SSTABLE_SIZE_BYTES: LazyLock<prometheus::IntGaugeVec> = LazyLock::new(|| {
  prometheus::IntGaugeVec::new(
    prometheus::Opts::new("aidb_sstable_size_bytes", "各层 SSTable 文件总大小"),
    &["level"],
  )
  .unwrap()
});

/// 备份操作计数 (op=create|delete|restore)
pub static BACKUP_TOTAL: LazyLock<prometheus::IntCounterVec> = LazyLock::new(|| {
  prometheus::IntCounterVec::new(
    prometheus::Opts::new("aidb_backup_total", "备份操作计数"),
    &["op"],
  )
  .unwrap()
});

/// 备份文件总大小 (字节)
pub static BACKUP_SIZE_BYTES: LazyLock<prometheus::IntGauge> = LazyLock::new(|| {
  prometheus::IntGauge::new("aidb_backup_size_bytes", "备份文件总大小（字节）").unwrap()
});

/// 备份操作耗时 (秒)
pub static BACKUP_DURATION_SECONDS: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
  prometheus::Histogram::with_opts(
    prometheus::HistogramOpts::new("aidb_backup_duration_seconds", "备份操作耗时（秒）")
      .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]),
  )
  .unwrap()
});

/// 初始化所有指标 (幂等)
pub fn init() {
  let _ = &*WAL_SIZE;
  let _ = &*MEMTABLE_SIZE;
  let _ = &*SSTABLE_COUNT;
  let _ = &*SSTABLE_SIZE_BYTES;
  let _ = &*OPERATIONS_TOTAL;
  let _ = &*OPERATION_DURATION;
  let _ = &*FLUSH_DURATION;
  let _ = &*BLOCK_CACHE_SIZE_BYTES;
  let _ = &*BLOCK_CACHE_HITS_TOTAL;
  let _ = &*BLOCK_CACHE_MISSES_TOTAL;
  let _ = &*BLOOM_FALSE_POSITIVE_TOTAL;
  let _ = &*FLUSH_TOTAL;
  let _ = &*SEQUENCE;
  let _ = &*TOTAL_KEY_COUNT;
  let _ = &*COMPACTION_TOTAL;
  let _ = &*COMPACTION_DURATION;
  let _ = &*BACKUP_TOTAL;
  let _ = &*BACKUP_SIZE_BYTES;
  let _ = &*BACKUP_DURATION_SECONDS;
  MEMTABLE_SIZE.with_label_values(&["active"]).set(0);
  MEMTABLE_SIZE.with_label_values(&["frozen"]).set(0);
}

/// 将所有 AiDb 指标注册到外部 Registry.
/// 在进程的早期调用, 确保在 record_* 被调用前完成.
pub fn register_into(registry: &prometheus::Registry) -> Result<(), prometheus::Error> {
  registry.register(Box::new(WAL_SIZE.clone()))?;
  registry.register(Box::new(MEMTABLE_SIZE.clone()))?;
  registry.register(Box::new(SSTABLE_COUNT.clone()))?;
  registry.register(Box::new(SSTABLE_SIZE_BYTES.clone()))?;
  registry.register(Box::new(OPERATIONS_TOTAL.clone()))?;
  registry.register(Box::new(OPERATION_DURATION.clone()))?;
  registry.register(Box::new(FLUSH_DURATION.clone()))?;
  registry.register(Box::new(BLOCK_CACHE_SIZE_BYTES.clone()))?;
  registry.register(Box::new(BLOCK_CACHE_HITS_TOTAL.clone()))?;
  registry.register(Box::new(BLOCK_CACHE_MISSES_TOTAL.clone()))?;
  registry.register(Box::new(BLOOM_FALSE_POSITIVE_TOTAL.clone()))?;
  registry.register(Box::new(FLUSH_TOTAL.clone()))?;
  registry.register(Box::new(SEQUENCE.clone()))?;
  registry.register(Box::new(TOTAL_KEY_COUNT.clone()))?;
  registry.register(Box::new(COMPACTION_TOTAL.clone()))?;
  registry.register(Box::new(COMPACTION_DURATION.clone()))?;
  registry.register(Box::new(BACKUP_TOTAL.clone()))?;
  registry.register(Box::new(BACKUP_SIZE_BYTES.clone()))?;
  registry.register(Box::new(BACKUP_DURATION_SECONDS.clone()))?;
  #[cfg(all(feature = "monitoring", feature = "cluster"))]
  crate::cluster::metrics::register_into(registry)?;
  Ok(())
}

#[cfg(feature = "monitoring")]
pub fn record_operation(op: &str) {
  OPERATIONS_TOTAL.with_label_values(&[op]).inc();
}

#[cfg(feature = "monitoring")]
pub fn record_operation_duration(op: &str, secs: f64) {
  OPERATION_DURATION.with_label_values(&[op]).observe(secs);
}

#[cfg(feature = "monitoring")]
pub fn record_flush() {
  FLUSH_TOTAL.inc();
}

#[cfg(feature = "monitoring")]
pub fn record_flush_duration(secs: f64) {
  FLUSH_DURATION.observe(secs);
}

#[cfg(feature = "monitoring")]
pub fn set_block_cache_size(bytes: u64) {
  BLOCK_CACHE_SIZE_BYTES.set(bytes as f64);
}

#[cfg(feature = "monitoring")]
pub fn record_block_cache_hit() {
  BLOCK_CACHE_HITS_TOTAL.inc();
}

#[cfg(feature = "monitoring")]
pub fn record_block_cache_miss() {
  BLOCK_CACHE_MISSES_TOTAL.inc();
}

#[cfg(feature = "monitoring")]
pub fn record_bloom_false_positive() {
  BLOOM_FALSE_POSITIVE_TOTAL.inc();
}

#[cfg(feature = "monitoring")]
pub fn set_sequence(seq: u64) {
  SEQUENCE.set(seq as i64);
}

#[cfg(feature = "monitoring")]
pub fn set_total_key_count(count: usize) {
  TOTAL_KEY_COUNT.set(count as i64);
}

#[cfg(feature = "monitoring")]
pub fn memtable_set_active(bytes: usize) {
  MEMTABLE_SIZE
    .with_label_values(&["active"])
    .set(bytes as i64);
}

#[cfg(feature = "monitoring")]
pub fn record_compaction(phase: &str) {
  COMPACTION_TOTAL.with_label_values(&[phase]).inc();
}

#[cfg(feature = "monitoring")]
pub fn record_compaction_duration(phase: &str, secs: f64) {
  COMPACTION_DURATION
    .with_label_values(&[phase])
    .observe(secs);
}

/// freeze 时: 将字节数计入 frozen, active 置 0 (新 active 由 Engine 创建后再 set)
#[cfg(feature = "monitoring")]
pub fn memtable_on_freeze(frozen_bytes: usize) {
  MEMTABLE_SIZE
    .with_label_values(&["frozen"])
    .add(frozen_bytes as i64);
  MEMTABLE_SIZE.with_label_values(&["active"]).set(0);
}

#[cfg(feature = "monitoring")]
pub fn record_backup_create(size_bytes: u64, duration_secs: f64) {
  BACKUP_TOTAL.with_label_values(&["create"]).inc();
  BACKUP_SIZE_BYTES.set(size_bytes as i64);
  BACKUP_DURATION_SECONDS.observe(duration_secs);
}

#[cfg(feature = "monitoring")]
pub fn record_backup_delete() {
  BACKUP_TOTAL.with_label_values(&["delete"]).inc();
}

#[cfg(feature = "monitoring")]
pub fn record_backup_restore() {
  BACKUP_TOTAL.with_label_values(&["restore"]).inc();
}
