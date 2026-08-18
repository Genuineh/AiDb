//! Phase7: DB flush 产出带 Bloom Filter 的 SST.
//! @component aidb-engine

use std::path::Path;

use aidb::config::Options;
use aidb::engine::sstable::SSTableReader;
use aidb::DB;
use tempfile::tempdir;

fn bloom_testing_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 4096;
    o.sync_wal = true;
    o.bloom_false_positive_rate = 0.01;
    o
}

fn first_sst_path(db_dir: &Path) -> std::path::PathBuf {
    std::fs::read_dir(db_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|ext| ext == "sst"))
        .expect("expected at least one .sst after flush")
}

#[test]
fn test_db_flush_writes_bloom_filter() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), bloom_testing_opts()).unwrap();
    db.put(b"flush_bloom_key", b"value").unwrap();
    db.flush().unwrap();
    assert_eq!(db.get(b"flush_bloom_key").unwrap(), Some(b"value".to_vec()));

    let sst_path = first_sst_path(dir.path());
    let reader = SSTableReader::open(&sst_path, None).unwrap();
    assert!(
        reader.has_bloom_filter(),
        "flush with bloom_false_positive_rate > 0 should write Meta Index bloom entry"
    );

    db.close().unwrap();
}

#[test]
fn test_db_flush_without_bloom_when_disabled() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    db.put(b"k", b"v").unwrap();
    db.flush().unwrap();

    let sst_path = first_sst_path(dir.path());
    let reader = SSTableReader::open(&sst_path, None).unwrap();
    assert!(
        !reader.has_bloom_filter(),
        "for_testing() defaults bloom_false_positive_rate to 0.0"
    );

    db.close().unwrap();
}
