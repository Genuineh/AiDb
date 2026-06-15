//! Prometheus 指标 wiring 验证

use bytes::Bytes;
use tempfile::tempdir;
use aidb::config::Options;
use aidb::engine::cache::{BlockCache, CacheKey};
use aidb::engine::filter::bloom::bloom_false_positive_count;
use aidb::metrics::{
  self, BLOCK_CACHE_HITS_TOTAL, BLOCK_CACHE_MISSES_TOTAL, BLOCK_CACHE_SIZE_BYTES,
  BLOOM_FALSE_POSITIVE_TOTAL, FLUSH_DURATION, OPERATION_DURATION,
};
use aidb::DB;

use crate::common::observability::assert_gauge_eq;

fn cache_key(file_number: u64, offset: u64) -> CacheKey {
  CacheKey {
    file_number,
    offset,
  }
}

#[test]
fn test_block_cache_prometheus_counters_and_size() {
  metrics::init();
  let cache = BlockCache::new(1024);
  let k = cache_key(1, 0);

  let hits_before = BLOCK_CACHE_HITS_TOTAL.get();
  let misses_before = BLOCK_CACHE_MISSES_TOTAL.get();

  cache.insert(k.clone(), Bytes::from_static(b"hello"));
  assert_gauge_eq(&BLOCK_CACHE_SIZE_BYTES, cache.size() as f64);

  cache.get(k.clone());
  assert_eq!(BLOCK_CACHE_HITS_TOTAL.get(), hits_before + 1);

  cache.get(cache_key(2, 0));
  assert_eq!(BLOCK_CACHE_MISSES_TOTAL.get(), misses_before + 1);

  cache.clear();
  assert_gauge_eq(&BLOCK_CACHE_SIZE_BYTES, 0.0);
}

#[test]
fn test_bloom_false_positive_prometheus_counter() {
  metrics::init();
  let dir = tempdir().unwrap();
  let mut opts = Options::for_testing();
  opts.bloom_false_positive_rate = 0.01;
  opts.use_wal = false;
  let db = DB::open(dir.path(), opts).unwrap();

  for i in 0..500 {
    db.put(format!("key_{i:04x}").as_bytes(), b"v").unwrap();
  }
  db.flush().unwrap();

  let counter_before = bloom_false_positive_count();
  let prom_before = BLOOM_FALSE_POSITIVE_TOTAL.get();
  for i in 0..500 {
    let _ = db.get(format!("absent_{i:04x}").as_bytes());
  }

  assert_eq!(
    BLOOM_FALSE_POSITIVE_TOTAL.get() - prom_before,
    bloom_false_positive_count() - counter_before,
    "prometheus bloom counter should track internal atomic counter"
  );
  db.close().unwrap();
}

#[test]
fn test_db_operation_and_flush_duration_histograms() {
  metrics::init();
  let dir = tempdir().unwrap();
  let mut opts = Options::for_testing();
  opts.memtable_size = 4096;
  opts.background_compaction = false;
  let db = DB::open(dir.path(), opts).unwrap();

  let put_before = OPERATION_DURATION
    .with_label_values(&["put"])
    .get_sample_count();
  let get_before = OPERATION_DURATION
    .with_label_values(&["get"])
    .get_sample_count();
  let flush_before = FLUSH_DURATION.get_sample_count();

  db.put(b"k", b"v").unwrap();
  assert!(
    OPERATION_DURATION
      .with_label_values(&["put"])
      .get_sample_count()
      > put_before
  );

  db.get(b"k").unwrap();
  assert!(
    OPERATION_DURATION
      .with_label_values(&["get"])
      .get_sample_count()
      > get_before
  );

  db.flush().unwrap();
  assert!(FLUSH_DURATION.get_sample_count() > flush_before);

  db.close().unwrap();
}
