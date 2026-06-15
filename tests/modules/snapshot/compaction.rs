//! Snapshot: compaction 不应删除快照可见的旧版本.

use super::common::temp_db_compaction;

/// snapshot 保护: compaction 不会删除快照可见的旧版本.
#[test]
fn test_snapshot_after_compaction() {
  let (_dir, db) = temp_db_compaction();
  db.put(b"k", b"v1").unwrap();
  let snap = db.snapshot().unwrap();
  db.put(b"k", b"v2").unwrap();
  db.flush().unwrap();
  assert_eq!(
    snap.get(b"k").unwrap(),
    Some(b"v1".to_vec()),
    "compaction 前 snapshot 可见 v1"
  );
  for i in 0..4u8 {
    db.put(&[b'p', i], &[i]).unwrap();
  }
  db.flush().unwrap();
  db.drain_compactions().unwrap();
  // snapshot 保护: v1 应仍可见
  assert_eq!(
    snap.get(b"k").unwrap(),
    Some(b"v1".to_vec()),
    "snapshot 保护: compaction 后旧版本仍应可见"
  );
  assert_eq!(db.get(b"k").unwrap(), Some(b"v2".to_vec()), "最新值不变");
  db.close().unwrap();
}
