//! SkipMapRep — 基于 crossbeam SkipMap 的 MemTableRep 实现.

use std::ops::Bound;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

use crossbeam_skiplist::map::Entry;
use crossbeam_skiplist::SkipMap;

use super::key_bytes::InternalKeyBytes;
use super::rep::MemTableRep;

/// 基于 crossbeam SkipMap 的 MemTableRep 实现.
pub(crate) struct SkipMapRep {
    map: SkipMap<InternalKeyBytes, Arc<[u8]>>,
    size: AtomicUsize,
}

impl SkipMapRep {
    pub fn new() -> Self {
        Self {
            map: SkipMap::new(),
            size: AtomicUsize::new(0),
        }
    }

    /// 临时暴露底层 SkipMap, 供 inner.rs 的 flush 全表遍历使用.
    /// FIXME: 泛型化 MemTable 时移除此方法, 改用 trait 原语遍历.
    pub(crate) fn inner_map(&self) -> &SkipMap<InternalKeyBytes, Arc<[u8]>> {
        &self.map
    }
}

impl MemTableRep for SkipMapRep {
    type EntryRef<'a> = Entry<'a, InternalKeyBytes, Arc<[u8]>>;

    fn insert(&self, key: InternalKeyBytes, value: Arc<[u8]>) {
        self.size
            .fetch_add(key.as_ref().len() + value.len(), AtomicOrdering::Relaxed);
        self.map.insert(key, value);
    }

    fn lower_bound(
        &self,
        bound: Bound<&InternalKeyBytes>,
    ) -> Option<Entry<'_, InternalKeyBytes, Arc<[u8]>>> {
        self.map.lower_bound(bound)
    }

    fn front(&self) -> Option<Entry<'_, InternalKeyBytes, Arc<[u8]>>> {
        self.map.front()
    }

    fn back(&self) -> Option<Entry<'_, InternalKeyBytes, Arc<[u8]>>> {
        self.map.back()
    }

    fn entry_key<'a>(
        entry: &'a Entry<'a, InternalKeyBytes, Arc<[u8]>>,
    ) -> &'a InternalKeyBytes {
        entry.key()
    }

    fn entry_value(
        entry: &Entry<'_, InternalKeyBytes, Arc<[u8]>>,
    ) -> Arc<[u8]> {
        Arc::clone(entry.value())
    }

    fn entry_next<'a>(
        entry: &'a Entry<'a, InternalKeyBytes, Arc<[u8]>>,
    ) -> Option<Entry<'a, InternalKeyBytes, Arc<[u8]>>> {
        entry.next()
    }

    fn entry_prev<'a>(
        entry: &'a Entry<'a, InternalKeyBytes, Arc<[u8]>>,
    ) -> Option<Entry<'a, InternalKeyBytes, Arc<[u8]>>> {
        entry.prev()
    }

    fn approximate_size(&self) -> usize {
        self.size.load(AtomicOrdering::Relaxed)
    }
}
