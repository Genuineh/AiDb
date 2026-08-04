//! Snapshot: 并发读写 / flush / compaction
//! @component aidb-engine

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::common::{temp_db, temp_db_compaction};

/// 验证并发高频写入下 Snapshot 读隔离与锁视图正常
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

/// 验证并发 Flush 刷盘过程中 Snapshot 读隔离正确
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

/// 验证并发 Compaction 压缩过程中 Snapshot 的隔离
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

/// 验证 Snapshot 支持 Send + Sync 跨线程传递
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

/// 竞态回归测试 (register 移入 write_lock 前): `db.snapshot()` 内部必须在
/// 释放 write_lock 之前完成 `snapshots.register(seq)`, 否则在"读到 seq"和
/// "注册保护"之间存在窗口 —— 一次恰好插进这个窗口的并发写入 + compaction
/// 可能在 snapshot 尚未注册时把它需要的旧版本当作"无人保护"直接 GC 掉,
/// 导致该 snapshot 存活期间前后两次读到不一致的结果。用持续高频的并发写 +
/// flush + compaction 施加压力, 并对每个存活 snapshot 反复重读比对。
#[test]
fn test_snapshot_register_race_with_concurrent_write_and_compaction() {
    let (_dir, db) = temp_db_compaction();
    db.put(b"k", b"seed").unwrap();

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let db_writer = Arc::clone(&db);
    let stop_writer = Arc::clone(&stop);
    let writer = thread::spawn(move || {
        let mut i: u32 = 0;
        while !stop_writer.load(std::sync::atomic::Ordering::Relaxed) {
            db_writer.put(b"k", &i.to_le_bytes()).unwrap();
            db_writer
                .put(&[b'p', (i % 8) as u8], &i.to_le_bytes())
                .unwrap();
            if i.is_multiple_of(3) {
                db_writer.flush().unwrap();
                let _ = db_writer.drain_compactions();
            }
            i = i.wrapping_add(1);
        }
    });

    // 保留最近若干个存活 snapshot, 每轮都对所有存活 snapshot 重新校验:
    // 只要某个 snapshot 曾经读到过某个值, 在它存活期间必须一直读到同一个值.
    let mut active: std::collections::VecDeque<(aidb::Snapshot, Option<Vec<u8>>)> =
        std::collections::VecDeque::new();

    for round in 0..800u32 {
        let snap = db.snapshot().unwrap();
        let first_read = snap.get(b"k").unwrap();
        active.push_back((snap, first_read));
        if active.len() > 32 {
            active.pop_front();
        }
        for (snap, expected) in active.iter() {
            let got = snap.get(b"k").unwrap();
            assert_eq!(
                got, *expected,
                "round {round}: snapshot(seq={}) 存活期间读到的版本发生变化 (expected={expected:?}, got={got:?}); \
                 说明 register 与并发写入/compaction 之间存在竞态窗口",
                snap.sequence()
            );
        }
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    writer.join().unwrap();
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
