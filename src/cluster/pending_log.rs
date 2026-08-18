//! PendingLogOverlay — 尚未 flush 到 DB 的 log entries 内存暂存.
//!
//! `LogCommitter` 将 entries append 到 DB 前, 先写入 PendingLogOverlay.
//! 生成代 (generation) 防护防止过时 flush 覆盖新 entry: 每轮 flush cycle
//! 递增 generation, `mark_durable` 只删除匹配 generation 的 entry.

use std::collections::BTreeMap;

/// 内存 pending log entries, 按 index 索引.
///
/// `Ent` 是 entry 的具体类型, 由调用方在构造时提供 index.
/// 本层不依赖 `RaftEntry` trait, 保持最小化泛型约束.
pub struct PendingLogOverlay<Ent> {
    /// index → (generation, Entry)
    pending: BTreeMap<u64, (u64, Ent)>,
    /// 单调递增 flush generation
    generation: u64,
}

impl<Ent> Default for PendingLogOverlay<Ent> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Ent> PendingLogOverlay<Ent> {
    pub fn new() -> Self {
        Self {
            pending: BTreeMap::new(),
            generation: 0,
        }
    }

    /// 当前 generation 值.
    pub fn current_generation(&self) -> u64 {
        self.generation
    }

    /// 开始新一轮 flush cycle, 递增 generation 并返回新值.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// 以当前 generation 插入 entry.
    ///
    /// `index` 是 entry 的 log index, 由调用方提供.
    pub fn insert_at(&mut self, index: u64, entry: Ent) {
        self.pending.insert(index, (self.generation, entry));
    }

    /// 按 index 查询 entry.
    pub fn get(&self, index: u64) -> Option<&Ent> {
        self.pending.get(&index).map(|(_, entry)| entry)
    }

    /// 获取 [start, end) 范围内的 entry 引用, 按 index 升序.
    pub fn get_range(&self, start: u64, end: u64) -> Vec<&Ent> {
        self.pending
            .range(start..end)
            .map(|(_, (_, entry))| entry)
            .collect()
    }

    /// 提取 [start, end) 范围内的 (generation, entry) 对并移出 overlay.
    pub fn drain_range(&mut self, start: u64, end: u64) -> Vec<(u64, Ent)> {
        let keys: Vec<_> = self.pending.range(start..end).map(|(&k, _)| k).collect();
        keys.into_iter()
            .filter_map(|k| self.pending.remove(&k))
            .collect()
    }

    /// 标记一批 entry 已持久化 — 仅移除 generation 匹配的 entry.
    ///
    /// 生成代防护: 当一轮慢 flush 在下一轮新 flush 之后完成时,
    /// 旧 generation 的 `mark_durable` 不会误删新 generation 的 entry.
    pub fn mark_durable(&mut self, indices: &[u64], expected_generation: u64) {
        for &index in indices {
            if let Some((gen, _)) = self.pending.get(&index) {
                if *gen == expected_generation {
                    self.pending.remove(&index);
                }
            }
        }
    }

    /// 删除 index >= after 的所有 entry (exclusive truncation).
    pub fn truncate_after(&mut self, after: u64) {
        let keys: Vec<_> = self.pending.range(after..).map(|(&k, _)| k).collect();
        for k in keys {
            self.pending.remove(&k);
        }
    }

    /// 删除 index <= upto 的所有 entry (inclusive purge).
    pub fn purge_upto(&mut self, upto: u64) {
        let keys: Vec<_> = self.pending.range(..=upto).map(|(&k, _)| k).collect();
        for k in keys {
            self.pending.remove(&k);
        }
    }

    /// 清空所有 pending entry.
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 使用 u64 作为 entry 类型测试 overlay 核心逻辑.
    // 实际使用中 Ent = openraft::entry::Entry<...>.

    fn make_entry(index: u64) -> u64 {
        index
    }

    #[test]
    fn test_insert_and_get() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, make_entry(1));
        overlay.insert_at(2, make_entry(2));
        assert_eq!(overlay.len(), 2);
        assert_eq!(*overlay.get(1).unwrap(), 1);
        assert_eq!(*overlay.get(2).unwrap(), 2);
        assert!(overlay.get(3).is_none());
    }

    #[test]
    fn test_get_range() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 1);
        overlay.insert_at(2, 2);
        overlay.insert_at(3, 3);
        let range = overlay.get_range(2, 4);
        assert_eq!(range.len(), 2);
        assert_eq!(*range[0], 2);
        assert_eq!(*range[1], 3);
    }

    #[test]
    fn test_drain_range() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 1);
        overlay.insert_at(2, 2);
        overlay.insert_at(3, 3);
        let drained = overlay.drain_range(2, 4);
        assert_eq!(drained.len(), 2);
        assert_eq!(overlay.len(), 1);
        assert_eq!(*overlay.get(1).unwrap(), 1);
    }

    #[test]
    fn test_mark_durable_generation_guard() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 100);
        assert_eq!(overlay.current_generation(), 0);

        let gen1 = overlay.next_generation();
        overlay.insert_at(2, 200);
        assert_eq!(gen1, 1);

        // old generation 0 不应删除 index 2 (generation 1)
        overlay.mark_durable(&[1, 2], 0);
        assert_eq!(overlay.len(), 1, "entry 2 should remain");
        assert!(overlay.get(2).is_some());

        // 用正确 generation 删除
        overlay.mark_durable(&[2], 1);
        assert!(overlay.get(2).is_none());
    }

    #[test]
    fn test_truncate_after() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 1);
        overlay.insert_at(2, 2);
        overlay.insert_at(3, 3);
        overlay.truncate_after(2);
        assert_eq!(overlay.len(), 1);
        assert!(overlay.get(1).is_some());
        assert!(overlay.get(2).is_none());
        assert!(overlay.get(3).is_none());
    }

    #[test]
    fn test_purge_upto() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 1);
        overlay.insert_at(2, 2);
        overlay.insert_at(3, 3);
        overlay.purge_upto(2);
        assert_eq!(overlay.len(), 1);
        assert!(overlay.get(3).is_some());
    }

    #[test]
    fn test_clear() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 1);
        overlay.insert_at(2, 2);
        overlay.clear();
        assert!(overlay.is_empty());
    }

    #[test]
    fn test_generation_increment() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        assert_eq!(overlay.current_generation(), 0);
        assert_eq!(overlay.next_generation(), 1);
        assert_eq!(overlay.next_generation(), 2);
        assert_eq!(overlay.current_generation(), 2);
    }

    #[test]
    fn test_mark_durable_partial_generation() {
        let mut overlay = PendingLogOverlay::<u64>::new();
        overlay.insert_at(1, 10); // gen 0
        let _ = overlay.next_generation();
        overlay.insert_at(2, 20); // gen 1
        overlay.insert_at(3, 30); // gen 1

        // 只标记 gen 0 为 durable — 只应删除 index 1
        overlay.mark_durable(&[1, 2], 0);
        assert_eq!(overlay.len(), 2);
        assert!(overlay.get(2).is_some());
        assert!(overlay.get(3).is_some());
    }
}
