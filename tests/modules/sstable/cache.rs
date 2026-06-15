//! SSTable BlockCache 集成测试 (Phase7.4)

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use bytes::Bytes;
use tempfile::tempdir;
use aidb::config::CompressionType;
use aidb::engine::cache::BlockCache;
use aidb::engine::memtable::{encode_internal_key, ValueType};
use aidb::engine::sstable::{
  find_block_handle, read_block_from_file, sstable_path, Footer, IndexBlock, SSTableBuilder,
  SSTableReader, FOOTER_SIZE,
};
use aidb::error::Error;

fn build_sst(dir: &std::path::Path, file_num: u64, keys: &[(&[u8], &[u8])]) -> std::path::PathBuf {
  build_sst_with_options(dir, file_num, keys, 256, 0.0)
}

fn build_sst_with_options(
  dir: &std::path::Path,
  file_num: u64,
  keys: &[(&[u8], &[u8])],
  block_size: usize,
  bloom_rate: f64,
) -> std::path::PathBuf {
  let path = sstable_path(dir, file_num, 0);
  let mut b = SSTableBuilder::new(&path, block_size, 2, CompressionType::None, bloom_rate).unwrap();
  if bloom_rate > 0.0 {
    b.set_expected_keys(keys.len());
  }
  for (i, (uk, val)) in keys.iter().enumerate() {
    let ik = encode_internal_key(uk, (i + 1) as u64, ValueType::TypePut);
    b.add(&ik, val).unwrap();
  }
  b.finish().unwrap();
  path
}

fn build_multi_block_sst(dir: &std::path::Path, file_num: u64) -> std::path::PathBuf {
  let keys: Vec<(Vec<u8>, Vec<u8>)> = (0..16)
    .map(|i| (format!("key_{i:02}").into_bytes(), vec![b'x'; 200]))
    .collect();
  let refs: Vec<(&[u8], &[u8])> = keys
    .iter()
    .map(|(k, v)| (k.as_slice(), v.as_slice()))
    .collect();
  build_sst_with_options(dir, file_num, &refs, 256, 0.0)
}

fn data_block_offset(path: &std::path::Path, seek_key: &[u8]) -> u64 {
  let file = fs::File::open(path).unwrap();
  let file_size = file.metadata().unwrap().len();
  let mut footer_buf = [0u8; FOOTER_SIZE];
  let mut f = file.try_clone().unwrap();
  f.seek(SeekFrom::Start(file_size - FOOTER_SIZE as u64))
    .unwrap();
  f.read_exact(&mut footer_buf).unwrap();
  let footer = Footer::decode(&footer_buf).unwrap();
  let index_bytes = read_block_from_file(&file, &footer.index_handle).unwrap();
  let index = IndexBlock::new(Bytes::from(index_bytes)).unwrap();
  let entries: Vec<(Vec<u8>, _)> = index
    .entries()
    .unwrap()
    .into_iter()
    .map(|e| (e.key, e.handle))
    .collect();
  find_block_handle(&entries, seek_key).unwrap().offset
}

fn corrupt_byte_at(path: &std::path::Path, offset: u64) {
  let mut bytes = fs::read(path).unwrap();
  bytes[offset as usize] ^= 0xff;
  fs::write(path, bytes).unwrap();
}

fn count_iter(reader: &SSTableReader) -> usize {
  let mut it = reader.iter();
  let mut count = 0;
  while it.valid() {
    count += 1;
    if !it.advance() {
      break;
    }
  }
  count
}

#[test]
fn test_block_cache_hit_miss() {
  let dir = tempdir().unwrap();
  let path = build_sst(
    dir.path(),
    1,
    &[(b"alpha", b"v1"), (b"beta", b"v2"), (b"gamma", b"v3")],
  );
  let cache = Arc::new(BlockCache::new(64 * 1024));
  let reader = SSTableReader::open(&path, Some(Arc::clone(&cache))).unwrap();

  let seek = encode_internal_key(b"alpha", 1, ValueType::TypePut);
  assert!(reader.get(&seek).unwrap().is_some());
  let after_first = cache.stats();
  assert!(after_first.misses >= 1);

  cache.reset_stats();
  assert!(reader.get(&seek).unwrap().is_some());
  let after_second = cache.stats();
  assert_eq!(after_second.hits, 1);
  assert_eq!(after_second.misses, 0);
}

#[test]
fn test_block_cache_stats() {
  let dir = tempdir().unwrap();
  let path = build_sst(
    dir.path(),
    2,
    &[
      (b"k1", b"v1"),
      (b"k2", b"v2"),
      (b"k3", b"v3"),
      (b"k4", b"v4"),
    ],
  );
  let cache = Arc::new(BlockCache::new(64 * 1024));
  let reader = SSTableReader::open(&path, Some(Arc::clone(&cache))).unwrap();

  for i in 1..=4 {
    let seek = encode_internal_key(format!("k{i}").as_bytes(), i, ValueType::TypePut);
    assert!(reader.get(&seek).unwrap().is_some());
  }

  let stats = cache.stats();
  assert!(stats.lookups >= 4);
  assert!(stats.insertions >= 1);
}

#[test]
fn test_block_cache_clear() {
  let dir = tempdir().unwrap();
  let path = build_sst(dir.path(), 3, &[(b"key", b"val")]);
  let cache = Arc::new(BlockCache::new(64 * 1024));
  let reader = SSTableReader::open(&path, Some(Arc::clone(&cache))).unwrap();
  let seek = encode_internal_key(b"key", 1, ValueType::TypePut);

  assert!(reader.get(&seek).unwrap().is_some());
  cache.clear();
  cache.reset_stats();

  assert!(reader.get(&seek).unwrap().is_some());
  let stats_after_miss = cache.stats();
  assert!(stats_after_miss.misses >= 1);

  assert!(reader.get(&seek).unwrap().is_some());
  let stats_after_hit = cache.stats();
  assert!(stats_after_hit.hits >= 1);
}

#[test]
fn test_sstable_iterator_with_cache() {
  let dir = tempdir().unwrap();
  let path = build_sst(dir.path(), 4, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
  let cache = Arc::new(BlockCache::new(64 * 1024));
  let reader = SSTableReader::open(&path, Some(Arc::clone(&cache))).unwrap();

  assert_eq!(count_iter(&reader), 3);
  assert!(cache.stats().insertions >= 1);

  cache.reset_stats();
  assert_eq!(count_iter(&reader), 3);
  let stats = cache.stats();
  assert!(
    stats.hits >= 1,
    "second iterator pass should reuse cached blocks: {stats:?}"
  );
}

#[test]
fn test_bloom_fast_path_skips_cache_lookups() {
  let dir = tempdir().unwrap();
  let path = build_sst_with_options(dir.path(), 10, &[(b"present", b"v")], 256, 0.01);
  let cache = Arc::new(BlockCache::new(64 * 1024));
  let reader = SSTableReader::open(&path, Some(Arc::clone(&cache))).unwrap();
  assert!(reader.has_bloom_filter());

  cache.reset_stats();
  let missing = encode_internal_key(b"absent", u64::MAX, ValueType::TypePut);
  assert_eq!(reader.get(&missing).unwrap(), None);
  assert_eq!(
    cache.stats().lookups,
    0,
    "bloom negative must not touch BlockCache"
  );

  assert_eq!(reader.get(&missing).unwrap(), None);
  assert_eq!(cache.stats().lookups, 0);
}

#[test]
fn test_corrupt_block_not_inserted_into_cache() {
  let dir = tempdir().unwrap();
  let path = build_multi_block_sst(dir.path(), 11);
  let cache = Arc::new(BlockCache::new(64 * 1024));
  let reader = SSTableReader::open(&path, Some(Arc::clone(&cache))).unwrap();

  let seek = encode_internal_key(b"key_05", 6, ValueType::TypePut);
  assert!(reader.get(&seek).unwrap().is_some());

  cache.clear();
  cache.reset_stats();
  let offset = data_block_offset(&path, &seek);
  corrupt_byte_at(&path, offset + 1);

  assert!(
    matches!(reader.get(&seek), Err(Error::Corruption(_))),
    "CRC mismatch should surface as Corruption"
  );
  assert_eq!(
    cache.stats().insertions,
    0,
    "failed disk read must not insert into cache"
  );
}
