//! Phase5 → Phase6 迁移回归: 无 CURRENT 的 flush 产物经 bootstrap, 再 MANIFEST reopen.

use std::fs;
use std::path::Path;
use tempfile::tempdir;
use aidb::config::CompressionType;
use aidb::config::Options;
use aidb::engine::compaction::current_exists;
use aidb::engine::memtable::{encode_internal_key, ValueType};
use aidb::engine::sstable::{SSTableBuilder, SSTableReader};
use aidb::DB;

fn small_opts() -> Options {
  let mut o = Options::for_testing();
  o.memtable_size = 4096;
  o.sync_wal = true;
  o
}

/// 模拟仅 Phase5 flush 落盘: 目录中有 `.sst`, 无 CURRENT/MANIFEST.
fn build_phase5_flush_only_dir(path: &Path) {
  fs::create_dir_all(path).unwrap();
  let sst_path = path.join("000001_L0.sst");
  let key = encode_internal_key(b"persist", 1, ValueType::TypePut);
  let mut builder = SSTableBuilder::new(&sst_path, 512, 16, CompressionType::None, 0.0).unwrap();
  builder.add(&key, b"ok").unwrap();
  builder.finish().unwrap();
  assert!(SSTableReader::open(&sst_path, None).is_ok());
  assert!(!current_exists(path));
}

fn strip_manifest(path: &Path) {
  let _ = fs::remove_file(path.join("CURRENT"));
  if let Ok(rd) = fs::read_dir(path) {
    for e in rd.flatten() {
      let name = e.file_name().to_string_lossy().to_string();
      if name.starts_with("MANIFEST-") {
        let _ = fs::remove_file(e.path());
      }
    }
  }
}

#[test]
fn test_bootstrap_from_phase5_sst_flush_persistence() {
  let dir = tempdir().unwrap();
  build_phase5_flush_only_dir(dir.path());

  let opts = small_opts();
  {
    let db = DB::open(dir.path(), opts.clone()).unwrap();
    assert!(
      dir.path().join("CURRENT").exists(),
      "bootstrap 应写入 CURRENT"
    );
    assert_eq!(
      db.get(b"persist").unwrap(),
      Some(b"ok".to_vec()),
      "bootstrap 后应能读到 flush 数据"
    );
    db.close().unwrap();
  }

  let db2 = DB::open(dir.path(), opts).unwrap();
  assert_eq!(
    db2.get(b"persist").unwrap(),
    Some(b"ok".to_vec()),
    "MANIFEST replay reopen 后数据仍在"
  );
  db2.close().unwrap();
}

#[test]
fn test_bootstrap_then_wal_recovery_like_db_recovery() {
  let dir = tempdir().unwrap();
  {
    let db = DB::open(dir.path(), small_opts()).unwrap();
    db.put(b"rec", b"overed").unwrap();
    db.close().unwrap();
  }
  strip_manifest(dir.path());
  assert!(!current_exists(dir.path()));

  {
    let db = DB::open(dir.path(), small_opts()).unwrap();
    assert_eq!(db.get(b"rec").unwrap(), Some(b"overed".to_vec()));
    assert!(dir.path().join("CURRENT").exists());
    db.close().unwrap();
  }

  let db2 = DB::open(dir.path(), small_opts()).unwrap();
  assert_eq!(db2.get(b"rec").unwrap(), Some(b"overed".to_vec()));
  db2.close().unwrap();
}
