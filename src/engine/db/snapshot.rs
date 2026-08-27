//! MVCC 点时间快照: 创建时固定 `sequence` 边界, `Snapshot::get` / `iter` / `scan` 仅可见
//! `seq <= snapshot_seq` 的写入 (Phase5: get; P1: iter/scan).
//!
//! `SnapshotList` 是全局快照注册表: `DB::snapshot` 在 `write_lock` 下读 sequence 并 `register`,
//! `Snapshot` Drop 时 `unregister`; compaction 通过 `min_snapshot_sequence()` 查询活跃快照中
//! 的最小 sequence (无活跃快照时返回 `u64::MAX`), 以决定旧版本可安全丢弃的阈值.
//!
//! # Invariant
//!
//! - register 必须在 `write_lock` 释放前完成, 与 compaction 读 `min_snapshot_sequence()`
//!   之间形成 happens-before, 避免 compaction 误 GC 快照所需版本 (见 `docs/modules/01-engine.md`).

use super::inner::DB;
use super::iterator::DbIterGuard;
use crate::error::Result;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 全局快照注册表. compaction 通过 `min_snapshot_sequence()` 查询阈值.
#[derive(Debug)]
pub struct SnapshotList {
    /// 快照 ID 生成器 (严格递增).
    next_id: AtomicU64,
    /// 活跃快照: (id, sequence).
    active: RwLock<Vec<(u64, u64)>>,
}

impl SnapshotList {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            active: RwLock::new(Vec::new()),
        })
    }

    /// 注册新快照, 返回快照 ID. compaction 通过 `min_snapshot_sequence` 感知.
    pub fn register(self: &Arc<Self>, sequence: u64) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.active.write().push((id, sequence));
        id
    }

    /// 注销快照.
    pub fn unregister(&self, id: u64) {
        self.active.write().retain(|(i, _)| *i != id);
    }

    /// 活跃快照中的最小 sequence. 无活跃快照时返回 `u64::MAX`.
    pub fn min_snapshot_sequence(&self) -> u64 {
        let active = self.active.read();
        active.iter().map(|(_, seq)| *seq).min().unwrap_or(u64::MAX)
    }

    /// 活跃快照数量.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub fn active_count(&self) -> usize {
        self.active.read().len()
    }
}

/// 创建时刻的 sequence 边界; 仅可见 `seq <= sequence` 的写入.
pub struct Snapshot {
    pub(crate) db: Arc<DB>,
    pub(crate) sequence: u64,
    snapshot_id: u64,
}

impl Snapshot {
    pub(crate) fn new(db: Arc<DB>, sequence: u64, snapshot_id: u64) -> Self {
        Self {
            db,
            sequence,
            snapshot_id,
        }
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get_at_sequence(key, self.sequence)
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// 点时间全表迭代 (过滤 tombstone, 仅 `seq <= sequence`).
    pub fn iter(&self) -> Result<DbIterGuard> {
        self.db.iter_at_sequence(self.sequence, None, None)
    }

    /// 点时间范围扫描 `[start, end)`.
    pub fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<DbIterGuard> {
        self.db.iter_at_sequence(self.sequence, start, end)
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        self.db.snapshots.unregister(self.snapshot_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_list_empty() {
        let list = SnapshotList::new();
        assert_eq!(list.min_snapshot_sequence(), u64::MAX);
        assert_eq!(list.active_count(), 0);
    }

    #[test]
    fn test_snapshot_list_register_unregister() {
        let list = SnapshotList::new();
        let id1 = list.register(100);
        let id2 = list.register(200);
        assert_eq!(list.active_count(), 2);
        assert_eq!(list.min_snapshot_sequence(), 100);
        list.unregister(id1);
        assert_eq!(list.min_snapshot_sequence(), 200);
        list.unregister(id2);
        assert_eq!(list.min_snapshot_sequence(), u64::MAX);
    }

    #[test]
    fn test_snapshot_list_id_monotonic() {
        let list = SnapshotList::new();
        let id1 = list.register(10);
        let id2 = list.register(20);
        assert!(id2 > id1, "snapshot IDs must be monotonic");
    }
}
