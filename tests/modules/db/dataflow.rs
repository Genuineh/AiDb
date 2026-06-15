//! DB 层跨模块 dataflow — put/get/flush/delete 的 span 树 (Mode A)

use crate::common::dataflow::{capture_spans, capture_spans_and_events};
use crate::common::observability::tracing_test_lock;
use tempfile::tempdir;
use aidb::config::Options;
use aidb::DB;

fn dataflow_opts() -> Options {
  let mut o = Options::for_testing();
  o.memtable_size = 4096;
  o.block_cache_size = 512 * 1024;
  o.bloom_false_positive_rate = 0.01;
  o.sync_wal = true;
  o.background_compaction = false;
  o
}

#[test]
#[serial_test::serial]
fn test_db_put_span_tree() {
  let _lock = tracing_test_lock();
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), dataflow_opts()).unwrap();

  let tree = capture_spans(|| {
    db.put(b"k", b"v").unwrap();
  });

  assert!(!tree.spans_named("db_put").is_empty());
  tree.assert_ancestor("wal_write", "db_put");
  tree.assert_ancestor("mem_put", "db_put");
  assert!(tree.all_same_trace());
  db.close().unwrap();
}

#[test]
#[serial_test::serial]
fn test_db_get_memtable_span_tree() {
  let _lock = tracing_test_lock();
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), dataflow_opts()).unwrap();
  db.put(b"k", b"v").unwrap();

  let tree = capture_spans(|| {
    assert_eq!(db.get(b"k").unwrap(), Some(b"v".to_vec()));
  });

  assert!(!tree.spans_named("db_get").is_empty());
  tree.assert_ancestor("mem_search", "db_get");
  db.close().unwrap();
}

#[test]
#[serial_test::serial]
fn test_db_get_sstable_span_tree() {
  let _lock = tracing_test_lock();
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), dataflow_opts()).unwrap();
  db.put(b"sst", b"on_disk").unwrap();
  db.flush().unwrap();

  let tree = capture_spans(|| {
    assert_eq!(db.get(b"sst").unwrap(), Some(b"on_disk".to_vec()));
  });

  assert!(!tree.spans_named("db_get").is_empty());
  tree.assert_ancestor("sst_seek", "db_get");
  db.close().unwrap();
}

#[test]
fn test_db_delete_span_tree() {
  // capture_spans 内部已调用 tracing_test_lock()
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), dataflow_opts()).unwrap();
  db.put(b"k", b"v").unwrap();

  let tree = capture_spans(|| {
    db.delete(b"k").unwrap();
  });

  assert!(!tree.spans_named("db_delete").is_empty());
  tree.assert_ancestor("wal_write", "db_delete");
  tree.assert_ancestor("mem_delete", "db_delete");
  db.close().unwrap();
}

#[test]
#[serial_test::serial]
fn test_db_flush_span_tree() {
  let _lock = tracing_test_lock();
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), dataflow_opts()).unwrap();
  db.put(b"f", b"lush").unwrap();

  let tree = capture_spans(|| {
    db.flush().unwrap();
  });

  assert!(!tree.spans_named("db_flush").is_empty());
  tree.assert_ancestor("db_flush_sst", "db_flush");
  tree.assert_ancestor("sst_build_add", "db_flush");
  db.close().unwrap();
}

#[test]
#[serial_test::serial]
fn test_db_put_event_chain() {
  let _lock = tracing_test_lock();
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), dataflow_opts()).unwrap();

  let (tree, events) = capture_spans_and_events(|| {
    db.put(b"ev", b"1").unwrap();
  });

  tree.assert_ancestor("mem_put", "db_put");
  let write_start = events.iter().position(|e| e.contains("wal.write.start"));
  let write_done = events.iter().position(|e| e.contains("wal.write.complete"));
  let mem_put = events.iter().position(|e| e.contains("mem.put"));
  assert!(write_start.is_some() && write_done.is_some());
  assert!(mem_put.is_some());
  assert!(write_start.unwrap() < write_done.unwrap());
  db.close().unwrap();
}
