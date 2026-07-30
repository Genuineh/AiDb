//! OTel 指标 wiring 验证

use aidb::config::Options;
use aidb::engine::cache::{BlockCache, CacheKey};
use aidb::engine::filter::bloom::bloom_false_positive_count;
use aidb::metrics::testutil;
use aidb::DB;
use bytes::Bytes;
use tempfile::tempdir;

fn cache_key(file_number: u64, offset: u64) -> CacheKey {
    CacheKey {
        file_number,
        offset,
    }
}

#[test]
fn test_block_cache_otel_counters_and_size() {
    let exporter = testutil::init_in_memory();
    let cache = BlockCache::new(1024);
    let k = cache_key(1, 0);

    let hits_before = testutil::counter_sum(&exporter, "aidb_block_cache_hits_total");
    let misses_before = testutil::counter_sum(&exporter, "aidb_block_cache_misses_total");

    cache.insert(k.clone(), Bytes::from_static(b"hello"));
    assert!(
        (testutil::gauge_value(&exporter, "aidb_block_cache_size_bytes") - cache.size() as f64)
            .abs()
            < f64::EPSILON
    );
    assert_eq!(
        testutil::gauge_value(&exporter, "aidb_block_cache_capacity_bytes"),
        cache.capacity() as f64
    );

    cache.get(k.clone());
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_block_cache_hits_total"),
        hits_before + 1
    );

    cache.get(cache_key(2, 0));
    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_block_cache_misses_total"),
        misses_before + 1
    );

    cache.clear();
    assert!(testutil::gauge_value(&exporter, "aidb_block_cache_size_bytes").abs() < f64::EPSILON);
}

#[test]
fn test_bloom_false_positive_otel_counter() {
    let exporter = testutil::init_in_memory();
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
    let prom_before = testutil::counter_sum(&exporter, "aidb_bloom_false_positive_total");
    for i in 0..500 {
        let _ = db.get(format!("absent_{i:04x}").as_bytes());
    }

    assert_eq!(
        testutil::counter_sum(&exporter, "aidb_bloom_false_positive_total") - prom_before,
        bloom_false_positive_count() - counter_before,
        "otel bloom counter should track internal atomic counter"
    );
    db.close().unwrap();
}

#[test]
fn test_db_operation_and_flush_duration_histograms() {
    let exporter = testutil::init_in_memory();
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.memtable_size = 4096;
    opts.background_compaction = false;
    let db = DB::open(dir.path(), opts).unwrap();

    let op_before = testutil::histogram_count(&exporter, "aidb_operation_duration_seconds");
    let flush_before = testutil::histogram_count(&exporter, "aidb_flush_duration_seconds");

    db.put(b"k", b"v").unwrap();
    assert!(testutil::histogram_count(&exporter, "aidb_operation_duration_seconds") > op_before);
    assert!(testutil::counter_sum(&exporter, "db.client.operations") >= 1);

    db.get(b"k").unwrap();
    assert!(
        testutil::histogram_count(&exporter, "aidb_operation_duration_seconds") > op_before + 1
    );

    db.flush().unwrap();
    assert!(testutil::histogram_count(&exporter, "aidb_flush_duration_seconds") > flush_before);

    db.close().unwrap();
}
