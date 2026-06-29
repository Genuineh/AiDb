//! IndexBlock — Data Block 最大 key → BlockHandle.

use std::cmp::Ordering;

use bytes::Bytes;

use crate::engine::memtable::compare_internal_key;
use crate::error::{Error, Result};

use super::block::{Block, BlockBuilder};
use super::handle::BlockHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub handle: BlockHandle,
}

pub struct IndexBlockBuilder {
    builder: BlockBuilder,
}

impl Default for IndexBlockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexBlockBuilder {
    pub fn new() -> Self {
        Self {
            builder: BlockBuilder::new(1),
        }
    }

    pub fn add_entry(&mut self, entry: &IndexEntry) -> Result<()> {
        let value = entry.handle.encode();
        self.builder.add(&entry.key, &value)
    }

    pub fn finish(&mut self) -> Bytes {
        self.builder.finish()
    }

    pub fn is_empty(&self) -> bool {
        self.builder.is_empty()
    }
}

pub struct IndexBlock {
    block: Block,
}

impl IndexBlock {
    pub fn new(data: Bytes) -> Result<Self> {
        Ok(Self {
            block: Block::new(data)?,
        })
    }

    pub fn entries(&self) -> Result<Vec<IndexEntry>> {
        let mut out = Vec::new();
        let mut it = self.block.iter();
        while it.valid() {
            let key = it.key().to_vec();
            let handle = BlockHandle::decode(it.value())?;
            out.push(IndexEntry { key, handle });
            if !it.advance() {
                break;
            }
        }
        Ok(out)
    }

    /// 定位可能包含 `seek_key` 的 Data Block.
    pub fn find_block(&self, seek_key: &[u8]) -> Result<BlockHandle> {
        let entries = self.entries()?;
        find_block_handle(
            &entries
                .iter()
                .map(|e| (e.key.clone(), e.handle))
                .collect::<Vec<_>>(),
            seek_key,
        )
    }
}

/// 在 Index 条目上二分定位 Data Block.
pub fn find_block_handle(
    entries: &[(Vec<u8>, BlockHandle)],
    seek_key: &[u8],
) -> Result<BlockHandle> {
    if entries.is_empty() {
        return Err(Error::Corruption("empty index block".into()));
    }
    let mut lo = 0usize;
    let mut hi = entries.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if compare_internal_key(&entries[mid].0, seek_key) == Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let idx = if lo < entries.len() {
        lo
    } else {
        entries.len() - 1
    };
    Ok(entries[idx].1)
}

/// 从 Index Block 解析 entries (供 Iterator 复用).
pub fn load_index_entries(block: &Block) -> Result<Vec<(Vec<u8>, BlockHandle)>> {
    let mut out = Vec::new();
    let mut it = block.iter();
    while it.valid() {
        out.push((it.key().to_vec(), BlockHandle::decode(it.value())?));
        if !it.advance() {
            break;
        }
    }
    Ok(out)
}
