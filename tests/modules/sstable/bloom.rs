//! SSTable Bloom Filter 集成测试

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

use aidb::config::CompressionType;
use aidb::engine::filter::{BloomFilter, Filter};
use aidb::engine::memtable::{encode_internal_key, ValueType};
use aidb::engine::sstable::{
    read_block_from_file, sstable_path, IndexBlock, SSTableBuilder, SSTableReader,
};
use aidb::engine::sstable::{Footer, FOOTER_SIZE};
use bytes::Bytes;
use tempfile::tempdir;

pub fn build_sst(
    dir: &std::path::Path,
    file_num: u64,
    entries: &[(&[u8], &[u8], u64)],
    bloom_rate: f64,
) -> std::path::PathBuf {
    let path = sstable_path(dir, file_num, 0);
    let mut b = SSTableBuilder::new(&path, 256, 2, CompressionType::None, bloom_rate).unwrap();
    if bloom_rate > 0.0 {
        b.set_expected_keys(entries.len());
    }
    for (uk, val, seq) in entries {
        let ik = encode_internal_key(uk, *seq, ValueType::TypePut);
        b.add(&ik, val).unwrap();
    }
    b.finish().unwrap();
    path
}

fn meta_index_has_bloom(path: &std::path::Path) -> bool {
    let file = std::fs::File::open(path).unwrap();
    let file_size = file.metadata().unwrap().len();
    let mut footer_buf = [0u8; FOOTER_SIZE];
    let mut f = file.try_clone().unwrap();
    f.seek(SeekFrom::Start(file_size - FOOTER_SIZE as u64))
        .unwrap();
    f.read_exact(&mut footer_buf).unwrap();
    let footer = Footer::decode(&footer_buf).unwrap();
    let meta_bytes = read_block_from_file(&file, &footer.meta_index_handle).unwrap();
    let meta = IndexBlock::new(Bytes::from(meta_bytes)).unwrap();
    meta.entries().unwrap().iter().any(|e| e.key == b"bloom")
}

#[test]
fn test_sstable_with_bloom_filter() {
    let dir = tempdir().unwrap();
    let path = build_sst(dir.path(), 1, &[(b"foo", b"bar", 10)], 0.01);
    assert!(meta_index_has_bloom(&path));
    let r = SSTableReader::open(&path, None).unwrap();
    assert!(r.has_bloom_filter());
    let seek = encode_internal_key(b"foo", 10, ValueType::TypePut);
    assert!(r.get(&seek).unwrap().is_some());
}

#[test]
fn test_sstable_without_bloom_filter() {
    let dir = tempdir().unwrap();
    let path = build_sst(dir.path(), 2, &[(b"k", b"v", 1)], 0.0);
    assert!(!meta_index_has_bloom(&path));
    let r = SSTableReader::open(&path, None).unwrap();
    assert!(!r.has_bloom_filter());
    let seek = encode_internal_key(b"k", 1, ValueType::TypePut);
    assert!(r.get(&seek).unwrap().is_some());
}

#[test]
fn test_sstable_bloom_user_key() {
    let dir = tempdir().unwrap();
    let path = build_sst(
        dir.path(),
        3,
        &[(b"user", b"v2", 5), (b"user", b"v1", 1)],
        0.01,
    );
    let r = SSTableReader::open(&path, None).unwrap();
    let seek_low = encode_internal_key(b"user", 2, ValueType::TypePut);
    assert!(r.get(&seek_low).unwrap().is_some());
    let seek_high = encode_internal_key(b"user", 10, ValueType::TypePut);
    assert!(r.get(&seek_high).unwrap().is_some());
}

#[test]
fn test_sstable_bloom_filter_skip() {
    let dir = tempdir().unwrap();
    let path = build_sst(dir.path(), 4, &[(b"present", b"v", 1)], 0.01);
    let r = SSTableReader::open(&path, None).unwrap();
    let seek = encode_internal_key(b"absent", 1, ValueType::TypePut);
    assert_eq!(r.get(&seek).unwrap(), None);
    let path2 = build_sst(dir.path(), 5, &[(b"present", b"v", 1)], 0.0);
    let r2 = SSTableReader::open(&path2, None).unwrap();
    assert_eq!(r2.get(&seek).unwrap(), None);
}

#[test]
fn test_sstable_bloom_filter_decode_fallback() {
    let dir = tempdir().unwrap();
    let path = build_sst(dir.path(), 6, &[(b"k", b"v", 1)], 0.01);
    let file = std::fs::File::open(&path).unwrap();
    let file_size = file.metadata().unwrap().len();
    let mut footer_buf = [0u8; FOOTER_SIZE];
    let mut f = file.try_clone().unwrap();
    f.seek(SeekFrom::Start(file_size - FOOTER_SIZE as u64))
        .unwrap();
    f.read_exact(&mut footer_buf).unwrap();
    let footer = Footer::decode(&footer_buf).unwrap();
    let meta_bytes = read_block_from_file(&file, &footer.meta_index_handle).unwrap();
    let meta = IndexBlock::new(Bytes::from(meta_bytes)).unwrap();
    let bloom_entry = meta
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| e.key == b"bloom")
        .unwrap();
    let mut out = OpenOptions::new().write(true).open(&path).unwrap();
    out.seek(SeekFrom::Start(bloom_entry.handle.offset))
        .unwrap();
    out.write_all(&[0xFF]).unwrap();
    drop(out);

    let r = SSTableReader::open(&path, None).unwrap();
    assert!(!r.has_bloom_filter());
    let seek = encode_internal_key(b"k", 1, ValueType::TypePut);
    assert!(r.get(&seek).unwrap().is_some());
    let seek_miss = encode_internal_key(b"missing", 1, ValueType::TypePut);
    assert_eq!(r.get(&seek_miss).unwrap(), None);
}

#[test]
fn test_sstable_bloom_filter_roundtrip() {
    let dir = tempdir().unwrap();
    let mut f = BloomFilter::new(3, 0.01);
    f.add(b"a");
    f.add(b"b");
    let decoded = BloomFilter::decode(&f.encode()).unwrap();
    assert!(decoded.may_contain(b"a"));
    assert!(!decoded.may_contain(b"z"));

    let path = build_sst(
        dir.path(),
        7,
        &[(b"a", b"1", 1), (b"b", b"2", 1), (b"c", b"3", 1)],
        0.01,
    );
    let r = SSTableReader::open(&path, None).unwrap();
    for uk in [b"a", b"b", b"c"] {
        let seek = encode_internal_key(uk, 1, ValueType::TypePut);
        assert!(r.get(&seek).unwrap().is_some());
    }
}
