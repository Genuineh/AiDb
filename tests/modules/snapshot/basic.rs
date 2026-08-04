//! Snapshot: 基础 MVCC get
//! @component aidb-engine

use super::common::{open_db, temp_db};
use tempfile::tempdir;

/// 验证 Snapshot 基本读视图隔离 (新写入不改变快照读)
#[test]
fn test_snapshot_basic() {
    let (_dir, db) = temp_db();
    db.put(b"k", b"v1").unwrap();
    let snap = db.snapshot().unwrap();
    db.put(b"k", b"v2").unwrap();
    assert_eq!(snap.get(b"k").unwrap(), Some(b"v1".to_vec()));
    assert_eq!(db.get(b"k").unwrap(), Some(b"v2".to_vec()));
    db.close().unwrap();
}

/// 验证 Snapshot 在删除操作发生后的历史数据可读性
#[test]
fn test_snapshot_after_delete() {
    let (_dir, db) = temp_db();
    db.put(b"k", b"gone").unwrap();
    let snap = db.snapshot().unwrap();
    db.delete(b"k").unwrap();
    assert_eq!(snap.get(b"k").unwrap(), Some(b"gone".to_vec()));
    assert_eq!(db.get(b"k").unwrap(), None);
    db.close().unwrap();
}

/// 验证空 DB 下创建 Snapshot
#[test]
fn test_snapshot_empty_db() {
    let (_dir, db) = temp_db();
    let snap = db.snapshot().unwrap();
    assert_eq!(snap.get(b"any").unwrap(), None);
    db.close().unwrap();
}

/// 验证 Snapshot 读不存在的 Key
#[test]
fn test_snapshot_key_not_exists() {
    let (_dir, db) = temp_db();
    db.put(b"exists", b"x").unwrap();
    let snap = db.snapshot().unwrap();
    assert_eq!(snap.get(b"missing").unwrap(), None);
    db.close().unwrap();
}

/// 验证多个不同 Sequence 节点的 Snapshot 独立多版本读
#[test]
fn test_snapshot_multiple() {
    let (_dir, db) = temp_db();
    db.put(b"k", b"v1").unwrap();
    let snap1 = db.snapshot().unwrap();
    db.put(b"k", b"v2").unwrap();
    let snap2 = db.snapshot().unwrap();
    db.put(b"k", b"v3").unwrap();
    assert_eq!(snap1.get(b"k").unwrap(), Some(b"v1".to_vec()));
    assert_eq!(snap2.get(b"k").unwrap(), Some(b"v2".to_vec()));
    assert_eq!(db.get(b"k").unwrap(), Some(b"v3".to_vec()));
    db.close().unwrap();
}

/// (1) 空库 high-water sequence==0; (2) 首条 put 后 snapshot sequence==1; (3) 创建时刻与 db 水位一致.
#[test]
fn test_snapshot_sequence_boundary() {
    let dir = tempdir().unwrap();
    let db = open_db(dir.path());

    let snap0 = db.snapshot().unwrap();
    assert_eq!(snap0.sequence(), 0);
    assert_eq!(snap0.get(b"missing").unwrap(), None);

    db.put(b"k", b"v").unwrap();
    let seq_at_snap = db.current_sequence();
    let snap1 = db.snapshot().unwrap();
    assert_eq!(snap1.sequence(), 1);
    assert_eq!(snap1.sequence(), seq_at_snap);
    assert_eq!(snap1.get(b"k").unwrap(), Some(b"v".to_vec()));
    assert_eq!(db.get(b"k").unwrap(), Some(b"v".to_vec()));

    db.close().unwrap();
}

/// 验证基于 Snapshot 的迭代器遍历与 Range Scan
#[test]
fn test_snapshot_iter_and_scan() {
    let (_dir, db) = temp_db();
    for (k, v) in [(b"a", b"1"), (b"b", b"2"), (b"c", b"3")] {
        db.put(k, v).unwrap();
    }
    let snap = db.snapshot().unwrap();
    db.put(b"d", b"4").unwrap();
    db.delete(b"b").unwrap();

    let mut keys: Vec<Vec<u8>> = snap.iter().unwrap().map(|r| r.unwrap().0).collect();
    keys.sort();
    assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);

    let mut range: Vec<Vec<u8>> = snap
        .scan(Some(b"b"), Some(b"d"))
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    range.sort();
    assert_eq!(range, vec![b"b".to_vec(), b"c".to_vec()]);
    db.close().unwrap();
}
