//! Snapshot: compaction 不应删除快照可见的旧版本.

use super::common::temp_db_compaction;

/// 回归测试 (compaction dedup 边界穿越 bug): 快照边界 sequence 与被保护
/// key 自身版本的 sequence 不需要精确相等 —— 只要 snapshot 创建前, 中间
/// 有任何一次对*其它* key 的写入推高了全局 sequence, 而这个 key 自己没有
/// 再被写过, 该 key 自身版本的 sequence 就会严格小于 snapshot 注册的边界。
/// 这种情况在真实工作负载里几乎必然发生, 之前 `snapshot_protected()` 用
/// `sequence >= min_snapshot_sequence` 做逐条判断, 语义反了: compaction
/// 会错误丢弃这个边界以下、真正被 snapshot 需要的版本。
#[test]
fn test_snapshot_survives_compaction_when_boundary_not_aligned_to_key_write() {
    let (_dir, db) = temp_db_compaction();
    db.put(b"k", b"original").unwrap();
    db.put(b"other", b"x").unwrap(); // 只推高全局 sequence, 不动 k
    let snap = db.snapshot().unwrap();
    assert_eq!(snap.get(b"k").unwrap(), Some(b"original".to_vec()));

    db.put(b"k", b"new1").unwrap();
    db.flush().unwrap();
    db.put(b"a", b"1").unwrap();
    db.flush().unwrap();
    db.put(b"b", b"2").unwrap();
    db.flush().unwrap();
    db.drain_compactions().unwrap();

    let val = snap.get(b"k").unwrap();
    assert_eq!(
        val,
        Some(b"original".to_vec()),
        "snapshot 边界与 key 自身写入版本不对齐时, compaction 后仍应可见 original, got {val:?}"
    );
    assert_eq!(db.get(b"k").unwrap(), Some(b"new1".to_vec()), "最新值不变");
    drop(snap);
    db.close().unwrap();
}

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
