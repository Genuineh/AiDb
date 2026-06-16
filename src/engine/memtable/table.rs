//! MemTable — 内存写入缓冲 (SkipMap).

use super::internal_key::{
    check_sequence, encode_internal_key, extract_sequence, extract_user_key, extract_value_type,
    ValueType, K_MAX_SEQUENCE,
};
use super::iterator::MemTableIterator;
use super::key_bytes::InternalKeyBytes;
use crate::error::Result;
use crossbeam_skiplist::SkipMap;
use std::ops::Bound;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

/// 已冻结、等待 flush 的 MemTable (无 put/delete, 仅读).
pub struct ImmutableMemTable {
    table: MemTable,
    flush_seq: u64,
}

impl ImmutableMemTable {
    pub fn flush_seq(&self) -> u64 {
        self.flush_seq
    }

    pub fn approximate_size(&self) -> usize {
        self.table.approximate_size()
    }

    pub fn get(&self, key: &[u8], snapshot_seq: u64) -> Result<Option<Vec<u8>>> {
        self.table.get(key, snapshot_seq)
    }

    pub fn get_latest(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.table.get_latest(key)
    }

    pub fn search(&self, seek_key: &[u8]) -> Result<Option<(Vec<u8>, ValueType)>> {
        self.table.search(seek_key)
    }

    pub fn iter(&self) -> MemTableIterator<'_> {
        self.table.iter()
    }

    /// 路径遍历: Chunk -> MemTable
    pub(crate) fn inner(&self) -> &MemTable {
        &self.table
    }
}

/// 可变 MemTable.
pub struct MemTable {
    table: SkipMap<InternalKeyBytes, Arc<[u8]>>,
    size: AtomicUsize,
}

impl Default for MemTable {
    fn default() -> Self {
        Self::new()
    }
}

impl MemTable {
    pub fn new() -> Self {
        Self {
            table: SkipMap::new(),
            size: AtomicUsize::new(0),
        }
    }

    pub(crate) fn map(&self) -> &SkipMap<InternalKeyBytes, Arc<[u8]>> {
        &self.table
    }

    pub fn approximate_size(&self) -> usize {
        self.size.load(AtomicOrdering::Relaxed)
    }

    #[tracing::instrument(
    name = "mem_put",
    skip(self, key, value),
    fields(key_size = key.len(), value_size = value.len())
  )]
    pub fn put(&self, key: &[u8], value: &[u8], sequence: u64) -> Result<()> {
        check_sequence(sequence)?;
        let encoded = encode_internal_key(key, sequence, ValueType::TypePut);
        let ik = InternalKeyBytes::from_slice(&encoded);
        let val = Arc::from(value);
        self.table.insert(ik, val);
        self.size
            .fetch_add(key.len() + value.len(), AtomicOrdering::Relaxed);
        tracing::debug!(target: "mem", "mem.put");
        self.sync_active_metric();
        Ok(())
    }

    #[tracing::instrument(name = "mem_delete", skip(self, key))]
    pub fn delete(&self, key: &[u8], sequence: u64) -> Result<()> {
        check_sequence(sequence)?;
        let encoded = encode_internal_key(key, sequence, ValueType::TypeDelete);
        let ik = InternalKeyBytes::from_slice(&encoded);
        self.table.insert(ik, Arc::from(&[] as &[u8]));
        self.size.fetch_add(key.len(), AtomicOrdering::Relaxed);
        tracing::debug!(target: "mem", "mem.delete");
        self.sync_active_metric();
        Ok(())
    }

    #[tracing::instrument(name = "mem_get", skip(self, key))]
    pub fn get(&self, key: &[u8], snapshot_seq: u64) -> Result<Option<Vec<u8>>> {
        let seek_key = encode_internal_key(key, snapshot_seq, ValueType::TypePut);
        match self.search(&seek_key)? {
            Some((value, ValueType::TypePut)) => {
                tracing::debug!(target: "mem", "mem.get.hit");
                Ok(Some(value))
            }
            Some((_, ValueType::TypeDelete)) => {
                tracing::debug!(target: "mem", "mem.get.miss");
                Ok(None)
            }
            None => {
                tracing::debug!(target: "mem", "mem.get.miss");
                Ok(None)
            }
        }
    }

    #[tracing::instrument(name = "mem_search", skip(self, seek_key))]
    pub fn search(&self, seek_key: &[u8]) -> Result<Option<(Vec<u8>, ValueType)>> {
        let bound = InternalKeyBytes::from_slice(seek_key);
        let Some(entry) = self.table.lower_bound(Bound::Included(&bound)) else {
            return Ok(None);
        };
        let entry_key = entry.key().as_ref();
        if extract_user_key(entry_key) != extract_user_key(seek_key) {
            return Ok(None);
        }
        let entry_seq = extract_sequence(entry_key)?;
        let seek_seq = extract_sequence(seek_key)?;
        if entry_seq > seek_seq {
            return Ok(None);
        }
        let value_type = extract_value_type(entry_key)?;
        Ok(Some((entry.value().as_ref().to_vec(), value_type)))
    }

    #[tracing::instrument(
    name = "mem_freeze",
    skip(self),
    fields(approximate_size = self.approximate_size())
  )]
    pub fn freeze(self, flush_seq: u64) -> ImmutableMemTable {
        tracing::info!(target: "mem", "mem.freeze");
        #[cfg(feature = "monitoring")]
        crate::metrics::memtable_on_freeze(self.approximate_size());
        ImmutableMemTable {
            table: self,
            flush_seq,
        }
    }

    fn sync_active_metric(&self) {
        #[cfg(feature = "monitoring")]
        crate::metrics::memtable_set_active(self.approximate_size());
    }

    pub fn iter(&self) -> MemTableIterator<'_> {
        MemTableIterator::new(self)
    }

    /// 非 snapshot 读: 见所有已写入版本.
    pub fn get_latest(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.get(key, K_MAX_SEQUENCE)
    }
}
