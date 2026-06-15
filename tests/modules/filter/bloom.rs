//! Bloom Filter 集成测试 (验收映射)

use aidb::engine::filter::{BloomFilter, Filter};

#[test]
fn test_bloom_filter_basic() {
  let mut f = BloomFilter::new(100, 0.01);
  f.add(b"hello");
  assert!(f.may_contain(b"hello"));
  assert!(!f.may_contain(b"missing"));
}

#[test]
fn test_bloom_filter_no_false_negatives() {
  let mut f = BloomFilter::new(1000, 0.01);
  let keys: Vec<Vec<u8>> = (0..1000).map(|i| format!("key_{i}").into_bytes()).collect();
  for k in &keys {
    f.add(k);
  }
  for k in &keys {
    assert!(f.may_contain(k));
  }
}

#[test]
fn test_bloom_filter_false_positive_rate() {
  let mut f = BloomFilter::new(10000, 0.01);
  for i in 0..10000 {
    f.add(format!("key_{i}").as_bytes());
  }
  let mut false_positives = 0;
  for i in 10000..20000 {
    if f.may_contain(format!("key_{i}").as_bytes()) {
      false_positives += 1;
    }
  }
  let rate = false_positives as f64 / 10000.0;
  assert!(rate < 0.02, "FPR {rate} >= 2%");
}

#[test]
fn test_bloom_filter_encode_decode() {
  let mut f = BloomFilter::new(50, 0.01);
  f.add(b"a");
  f.add(b"b");
  let decoded = BloomFilter::decode(&f.encode()).unwrap();
  assert!(decoded.may_contain(b"a"));
  assert!(!decoded.may_contain(b"z"));
}

#[test]
fn test_bloom_filter_decode_invalid() {
  assert!(BloomFilter::decode(&[1, 2, 3]).is_err());
}
