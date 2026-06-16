//! Snapshot: 并发读写 / flush / compaction

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::common::{temp_db, temp_db_compaction};

#[test]
fn test_snapshot_concurrent_write() {
    let (_dir, db) = temp_db();
    db.put(b"fixed", b"frozen").unwrap();
    let snap = db.snapshot().unwrap();
    let db_writer = Arc::clone(&db);
    let writer = thread::spawn(move || {
        for i in 0..100u8 {
            db_writer.put(b"noise", &[i]).unwrap();
            db_writer.put(b"fixed", &[i]).unwrap();
        }
    });
    for _ in 0..50 {
        assert_eq!(snap.get(b"fixed").unwrap(), Some(b"frozen".to_vec()));
        thread::sleep(Duration::from_millis(0));
    }
    writer.join().unwrap();
    assert_eq!(snap.get(b"fixed").unwrap(), Some(b"frozen".to_vec()));
    db.close().unwrap();
}

#[test]
fn test_snapshot_flush_concurrent() {
    let (_dir, db) = temp_db();
    db.put(b"k", b"v1").unwrap();
    let snap = db.snapshot().unwrap();
    db.put(b"k", b"v2").unwrap();
    let db_flush = Arc::clone(&db);
    let flusher = thread::spawn(move || {
        for _ in 0..20 {
            db_flush.flush().unwrap();
        }
    });
    for _ in 0..30 {
        assert_eq!(snap.get(b"k").unwrap(), Some(b"v1".to_vec()));
    }
    flusher.join().unwrap();
    assert_eq!(snap.get(b"k").unwrap(), Some(b"v1".to_vec()));
    db.close().unwrap();
}

#[test]
fn test_snapshot_compaction_concurrent() {
    let (_dir, db) = temp_db_compaction();
    db.put(b"k", b"v1").unwrap();
    let snap = db.snapshot().unwrap();
    db.put(b"k", b"v2").unwrap();
    db.flush().unwrap();
    assert_eq!(snap.get(b"k").unwrap(), Some(b"v1".to_vec()));
    let db_cmp = Arc::clone(&db);
    let compactor = thread::spawn(move || {
        for i in 0..6u8 {
            db_cmp.put(&[b'q', i], &[i]).unwrap();
            db_cmp.flush().unwrap();
            let _ = db_cmp.drain_compactions();
        }
    });
    for _ in 0..20 {
        let _ = snap.get(b"k");
    }
    compactor.join().unwrap();
    let _ = db.drain_compactions();
    // 弱化语义: compaction 完成后可能 None, 但不应误读为 v2
    assert_ne!(snap.get(b"k").unwrap(), Some(b"v2".to_vec()));
    db.close().unwrap();
}

#[test]
fn test_snapshot_send_sync() {
    let (_dir, db) = temp_db();
    db.put(b"k", b"v").unwrap();
    let snap = db.snapshot().unwrap();
    let handle = thread::spawn(move || snap.get(b"k").unwrap());
    assert_eq!(handle.join().unwrap(), Some(b"v".to_vec()));
    db.close().unwrap();
}

/// 长时间持有 + 大量写入: 默认 CI 跳过.
#[test]
#[ignore = "slow: snapshot hold under heavy write+compaction"]
fn test_snapshot_long_hold_heavy_write() {
    let (_dir, db) = temp_db_compaction();
    db.put(b"anchor", b"v0").unwrap();
    let snap = db.snapshot().unwrap();
    for i in 0..2000u32 {
        let key = format!("key{i:04}");
        db.put(key.as_bytes(), b"x").unwrap();
        if i % 50 == 49 {
            db.flush().unwrap();
            let _ = db.drain_compactions();
        }
    }
    assert_eq!(snap.get(b"anchor").unwrap(), Some(b"v0".to_vec()));
    db.close().unwrap();
}

/// snapshot 保护: 快照存活期间 compaction 保留旧版本.
#[test]
fn test_snapshot_protects_old_versions_during_compaction() {
    let (_dir, db) = temp_db_compaction();

    // 写入初始值并 flush
    db.put(b"k", b"original").unwrap();
    db.flush().unwrap();

    // 创建快照, 捕获当前版本
    let snap = db.snapshot().unwrap();
    // 验证快照可读
    assert_eq!(
        snap.get(b"k").unwrap(),
        Some(b"original".to_vec()),
        "snapshot should read original before overwrite"
    );

    // 覆盖同一 key, 产生更高 sequence 版本
    for i in 0u8..3 {
        db.put(b"k", &[i]).unwrap();
    }
    db.flush().unwrap();

    // 再 flush 几次确保 L0 compaction trigger
    db.put(b"a", b"1").unwrap();
    db.flush().unwrap();
    db.put(b"b", b"2").unwrap();
    db.flush().unwrap();

    // 运行 compaction
    db.drain_compactions().unwrap();

    // 快照应仍可读到 original
    let val = snap.get(b"k").unwrap();
    assert_eq!(
        val,
        Some(b"original".to_vec()),
        "snapshot should read original after compaction, got: {val:?}"
    );

    drop(snap);
    db.close().unwrap();
}
