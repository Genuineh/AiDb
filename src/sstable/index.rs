//! Index block implementation for SSTable.
//!
//! The index block maps keys to data blocks, enabling efficient lookup.

use crate::error::{Error, Result};
use crate::sstable::block::{Block, BlockBuilder, BlockIterator};
use crate::sstable::footer::BlockHandle;
use bytes::Bytes;

/// IndexEntry represents a single entry in the index block.
///
/// Each entry contains:
/// - Key: The largest key in the corresponding data block
/// - BlockHandle: Location and size of the data block
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// The largest key in the data block
    pub key: Vec<u8>,
    /// Handle to the data block
    pub handle: BlockHandle,
}

impl IndexEntry {
    /// Create a new IndexEntry
    pub fn new(key: Vec<u8>, handle: BlockHandle) -> Self {
        Self { key, handle }
    }

    /// Encode the entry value (just the BlockHandle)
    pub fn encode_value(&self) -> Vec<u8> {
        self.handle.encode()
    }

    /// Decode the entry value from bytes
    pub fn decode_value(data: &[u8]) -> Result<BlockHandle> {
        BlockHandle::decode(data)
    }
}

/// IndexBlock provides efficient lookup of data blocks by key.
#[derive(Debug)]
pub struct IndexBlock {
    block: Block,
}

impl IndexBlock {
    /// Create a new IndexBlock from raw data
    pub fn new(data: Bytes) -> Result<Self> {
        let block = Block::new(data)?;
        Ok(Self { block })
    }

    /// Find the block handle for a given key.
    ///
    /// Returns the handle of the data block that may contain the key.
    /// Uses binary search on restart points for efficiency.
    pub fn find_block(&self, key: &[u8]) -> Result<Option<BlockHandle>> {
        let num_restarts = self.block.num_restarts();
        if num_restarts == 0 {
            return Ok(None);
        }

        // Binary search through restart points to find the right segment
        let mut left = 0;
        let mut right = num_restarts;

        while left < right {
            let mid = (left + right) / 2;
            let mut iter = self.block.iter();
            iter.seek_to_first();

            // Skip to the restart point
            for _ in 0..mid {
                if !iter.advance() {
                    break;
                }
            }

            if !iter.valid() {
                right = mid;
                continue;
            }

            if iter.key() < key {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        // Linear search from the restart point
        if left == 0 {
            left = 0;
        } else {
            left -= 1;
        }

        let mut iter = self.block.iter();
        iter.seek_to_first();

        // Skip to the starting position
        for _ in 0..left {
            if !iter.advance() {
                return Ok(None);
            }
        }

        // Find the first entry >= key
        let mut last_handle: Option<BlockHandle> = None;
        loop {
            if !iter.advance() {
                break;
            }

            if !iter.valid() {
                break;
            }

            let entry_key = iter.key();
            let handle = BlockHandle::decode(iter.value())?;

            if entry_key >= key {
                return Ok(Some(handle));
            }

            last_handle = Some(handle);
        }

        // Return the last block we saw (key might be in it)
        Ok(last_handle)
    }

    /// Create an iterator over all index entries
    pub fn iter(&self) -> IndexIterator {
        IndexIterator::new(self.block.iter())
    }

    /// Get the number of entries in the index
    pub fn len(&self) -> usize {
        let mut count = 0;
        let mut iter = self.block.iter();
        iter.seek_to_first();
        while iter.advance() {
            count += 1;
        }
        count
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// IndexBlockBuilder builds an index block.
pub struct IndexBlockBuilder {
    builder: BlockBuilder,
}

impl IndexBlockBuilder {
    /// Create a new IndexBlockBuilder
    pub fn new() -> Self {
        // Index blocks use a larger restart interval since they're typically smaller
        Self { builder: BlockBuilder::new(1) }
    }

    /// Add an index entry
    pub fn add_entry(&mut self, entry: &IndexEntry) {
        let value = entry.encode_value();
        self.builder.add(&entry.key, &value);
    }

    /// Finish building and return the block data
    pub fn finish(self) -> Bytes {
        self.builder.finish()
    }

    /// Check if the builder is empty
    pub fn is_empty(&self) -> bool {
        self.builder.is_empty()
    }

    /// Get the current size
    pub fn current_size(&self) -> usize {
        self.builder.current_size()
    }
}

impl Default for IndexBlockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator over index entries
pub struct IndexIterator {
    iter: BlockIterator,
}

impl IndexIterator {
    fn new(iter: BlockIterator) -> Self {
        Self { iter }
    }

    /// Seek to the first entry
    pub fn seek_to_first(&mut self) {
        self.iter.seek_to_first();
    }

    /// Move to the next entry
    pub fn advance(&mut self) -> bool {
        self.iter.advance()
    }

    /// Check if the iterator is valid
    pub fn valid(&self) -> bool {
        self.iter.valid()
    }

    /// Get the current entry
    pub fn entry(&self) -> Result<IndexEntry> {
        if !self.valid() {
            return Err(Error::InvalidState("Iterator not valid".to_string()));
        }

        let key = self.iter.key().to_vec();
        let handle = BlockHandle::decode(self.iter.value())?;

        Ok(IndexEntry::new(key, handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_entry() {
        let entry = IndexEntry::new(b"key1".to_vec(), BlockHandle::new(100, 50));

        let encoded = entry.encode_value();
        let decoded = IndexEntry::decode_value(&encoded).unwrap();

        assert_eq!(decoded, entry.handle);
    }

    #[test]
    fn test_index_block_builder() {
        let mut builder = IndexBlockBuilder::new();
        assert!(builder.is_empty());

        builder.add_entry(&IndexEntry::new(b"apple".to_vec(), BlockHandle::new(0, 100)));
        builder.add_entry(&IndexEntry::new(b"banana".to_vec(), BlockHandle::new(100, 150)));
        builder.add_entry(&IndexEntry::new(b"cherry".to_vec(), BlockHandle::new(250, 200)));

        let data = builder.finish();
        assert!(!data.is_empty());

        let index = IndexBlock::new(data).unwrap();
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn test_index_block_find() {
        let mut builder = IndexBlockBuilder::new();
        builder.add_entry(&IndexEntry::new(b"apple".to_vec(), BlockHandle::new(0, 100)));
        builder.add_entry(&IndexEntry::new(b"banana".to_vec(), BlockHandle::new(100, 150)));
        builder.add_entry(&IndexEntry::new(b"cherry".to_vec(), BlockHandle::new(250, 200)));

        let data = builder.finish();
        let index = IndexBlock::new(data).unwrap();

        // Find exact match
        let handle = index.find_block(b"banana").unwrap().unwrap();
        assert_eq!(handle.offset, 100);

        // Find key in first block
        let handle = index.find_block(b"aaa").unwrap().unwrap();
        assert_eq!(handle.offset, 0);

        // Find key between blocks
        let handle = index.find_block(b"avocado").unwrap().unwrap();
        assert_eq!(handle.offset, 100);

        // Find key in last block
        let handle = index.find_block(b"carrot").unwrap().unwrap();
        assert_eq!(handle.offset, 250);

        // Find key after all blocks
        let handle = index.find_block(b"durian").unwrap();
        assert!(handle.is_some());
    }

    #[test]
    fn test_index_iterator() {
        let mut builder = IndexBlockBuilder::new();
        builder.add_entry(&IndexEntry::new(b"apple".to_vec(), BlockHandle::new(0, 100)));
        builder.add_entry(&IndexEntry::new(b"banana".to_vec(), BlockHandle::new(100, 150)));

        let data = builder.finish();
        let index = IndexBlock::new(data).unwrap();

        let mut iter = index.iter();
        iter.seek_to_first();

        // First entry
        assert!(iter.advance());
        let entry = iter.entry().unwrap();
        assert_eq!(entry.key, b"apple");
        assert_eq!(entry.handle.offset, 0);

        // Second entry
        assert!(iter.advance());
        let entry = iter.entry().unwrap();
        assert_eq!(entry.key, b"banana");
        assert_eq!(entry.handle.offset, 100);

        // No more entries
        assert!(!iter.advance());
    }
}
