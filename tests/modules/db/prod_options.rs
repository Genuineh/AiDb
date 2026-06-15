//! 生产-like Options 集成测试

use tempfile::tempdir;
use aidb::config::Options;
use aidb::DB;

/// 生产-like Options: bloom_fp=0.01, block_cache=64 MiB, 禁用后台 compaction
fn prod_opts() -> Options {
  Options {
    bloom_false_positive_rate: 0.01,
    block_cache_size: 64 * 1024 * 1024,
    memtable_size: 4096,
    sync_wal: true,
    background_compaction: false,
    ..Options::default()
  }
}

/// 用生产-like Options 写入 100 个 key, flush, get 验证全部正确
#[test]
fn test_prod_opts_write_read_100_keys() {
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), prod_opts()).unwrap();

  for i in 0u32..100 {
    let key = format!("key_{i:03}");
    let val = format!("val_{i:03}");
    db.put(key.as_bytes(), val.as_bytes()).unwrap();
  }
  db.flush().unwrap();

  for i in 0u32..100 {
    let key = format!("key_{i:03}");
    let expected = format!("val_{i:03}");
    assert_eq!(
      db.get(key.as_bytes()).unwrap(),
      Some(expected.into_bytes()),
      "key {key} should have correct value after flush"
    );
  }

  db.close().unwrap();
}

/// 验证 cache stats 读取不 panic, hit_rate() 可以正常调用
#[test]
fn test_prod_opts_cache_stats_no_panic() {
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), prod_opts()).unwrap();

  for i in 0u32..100 {
    let key = format!("key_{i:03}");
    let val = format!("val_{i:03}");
    db.put(key.as_bytes(), val.as_bytes()).unwrap();
  }
  db.flush().unwrap();

  // 第一轮读取: 预热 cache
  for i in 0u32..100 {
    let key = format!("key_{i:03}");
    let _ = db.get(key.as_bytes()).unwrap();
  }

  // cache stats 读取不 panic
  let stats = db.cache_stats();
  let hit_rate = stats.hit_rate(); // 不 panic
  assert!(
    (0.0..=1.0).contains(&hit_rate),
    "hit_rate should be in [0, 1], got {hit_rate}"
  );
  assert!(
    stats.lookups >= 100,
    "should have at least 100 cache lookups, got {}",
    stats.lookups
  );

  db.close().unwrap();
}

/// 验证 bloom filter 路径被执行 (存在 key 和不存在 key 都不 panic)
#[test]
fn test_prod_opts_bloom_filter_no_panic() {
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), prod_opts()).unwrap();

  for i in 0u32..50 {
    let key = format!("bloom_key_{i:03}");
    let val = format!("bloom_val_{i}");
    db.put(key.as_bytes(), val.as_bytes()).unwrap();
  }
  db.flush().unwrap();

  // 查询存在的 key (bloom filter true positive path)
  for i in 0u32..50 {
    let key = format!("bloom_key_{i:03}");
    assert!(
      db.get(key.as_bytes()).unwrap().is_some(),
      "existing key {key} should be found via bloom filter path"
    );
  }

  // 查询不存在的 key (bloom filter negative path, 可能过滤掉 SST 查找)
  for i in 50u32..100 {
    let key = format!("nonexistent_{i:03}");
    assert_eq!(
      db.get(key.as_bytes()).unwrap(),
      None,
      "nonexistent key {key} should return None"
    );
  }

  db.close().unwrap();
}
