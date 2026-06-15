//! Snapshot 可观测性: `db_snapshot` span + `db.snapshot.create` event

use tempfile::tempdir;

use crate::common::dataflow::capture_spans_under_lock;
use crate::common::observability::{capture_events_under_lock, tracing_test_lock};

use super::common::snapshot_opts;
use aidb::DB;

#[test]
fn test_snapshot_observability() {
  let _lock = tracing_test_lock();
  let dir = tempdir().unwrap();
  let db = DB::open(dir.path(), snapshot_opts()).unwrap();
  db.put(b"k", b"v").unwrap();

  let caps = capture_spans_under_lock(|| {
    let _snap = db.snapshot().unwrap();
  });
  assert!(!caps.spans_named("db_snapshot").is_empty());

  let events = capture_events_under_lock(|| {
    let _snap = db.snapshot().unwrap();
  });
  assert!(
    events.iter().any(|e| e.contains("db.snapshot.create")),
    "missing db.snapshot.create, got: {events:?}"
  );
  db.close().unwrap();
}
