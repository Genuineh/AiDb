//! Snapshot: flush 后旧版本仍可见 (数据进 SST)

use super::common::temp_db;

#[test]
fn test_snapshot_after_flush() {
  let (_dir, db) = temp_db();
  db.put(b"k", b"v1").unwrap();
  let snap = db.snapshot().unwrap();
  db.put(b"k", b"v2").unwrap();
  // 小 memtable: overwrite 后再写 filler 并 flush, 逼 v1/v2 落 SST.
  for i in 0..8u8 {
    db.put(&[b'f', i], &[i]).unwrap();
  }
  db.flush().unwrap();
  assert_eq!(snap.get(b"k").unwrap(), Some(b"v1".to_vec()));
  assert_eq!(db.get(b"k").unwrap(), Some(b"v2".to_vec()));
  db.close().unwrap();
}
