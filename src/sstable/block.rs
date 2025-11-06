//! Block format implementation for SSTable.
//!
//! A block contains multiple key-value entries and uses restart points
//! for efficient binary search and prefix compression.

use crate::error::{Error, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};

/// Block stores key-value pairs with prefix compression.
///
/// Format:
/// ```text
/// [Entry 1]
/// [Entry 2]
/// ...
/// [Entry N]
/// [Restart Point 1: u32]
/// [Restart Point 2: u32]
/// ...
/// [Restart Point M: u32]
/// [Num Restarts: u32]
/// ```
///
/// Each entry format:
/// ```text
/// [shared_key_len: u32]     // Length of shared prefix with previous key
/// [unshared_key_len: u32]   // Length of unshared key suffix
/// [value_len: u32]          // Length of value
/// [unshared_key: bytes]     // Key suffix
/// [value: bytes]            // Value data
/// ```
#[derive(Debug, Clone)]
pub struct Block {
    data: Bytes,
    restart_offset: usize,
    num_restarts: u32,
}

impl Block {
    /// Create a new Block from raw data
    pub fn new(data: Bytes) -> Result<Self> {
        if data.len() < 4 {
            return Err(Error::corruption("Block too small"));
        }

        // Read number of restarts from the last 4 bytes
        let num_restarts = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap());

        // Calculate restart offset
        // restart_offset = data_len - 4 (num_restarts) - 4 * num_restarts (restart points)
        let restart_offset = data.len() - 4 - (num_restarts as usize * 4);

        if restart_offset > data.len() {
            return Err(Error::corruption("Invalid restart offset"));
        }

        Ok(Self {
            data,
            restart_offset,
            num_restarts,
        })
    }

    /// Get the number of restart points
    pub fn num_restarts(&self) -> u32 {
        self.num_restarts
    }

    /// Get a restart point by index
    fn get_restart_point(&self, index: u32) -> u32 {
        let offset = self.restart_offset + (index as usize * 4);
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    /// Create an iterator over the block
    pub fn iter(&self) -> BlockIterator {
        BlockIterator::new(self.clone())
    }

    /// Get the raw data
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// BlockBuilder builds a block with prefix compression.
pub struct BlockBuilder {
    buffer: BytesMut,
    restarts: Vec<u32>,
    counter: usize,
    last_key: Vec<u8>,
    block_restart_interval: usize,
}

impl BlockBuilder {
    /// Create a new BlockBuilder
    pub fn new(block_restart_interval: usize) -> Self {
        let mut restarts = Vec::new();
        restarts.push(0); // First restart point at offset 0

        Self {
            buffer: BytesMut::new(),
            restarts,
            counter: 0,
            last_key: Vec::new(),
            block_restart_interval,
        }
    }

    /// Add a key-value pair to the block
    pub fn add(&mut self, key: &[u8], value: &[u8]) {
        assert!(!key.is_empty(), "Key cannot be empty");

        // Keys must be added in sorted order
        if !self.last_key.is_empty() {
            assert!(
                key > self.last_key.as_slice(),
                "Keys must be added in sorted order"
            );
        }

        let mut shared = 0;

        // Add a restart point if needed
        if self.counter >= self.block_restart_interval {
            self.restarts.push(self.buffer.len() as u32);
            self.counter = 0;
            self.last_key.clear();
        } else if !self.last_key.is_empty() {
            // Calculate shared prefix length
            shared = self.shared_prefix_len(&self.last_key, key);
        }

        let unshared = key.len() - shared;

        // Write entry: shared | unshared | value_len | key_suffix | value
        self.buffer.put_u32_le(shared as u32);
        self.buffer.put_u32_le(unshared as u32);
        self.buffer.put_u32_le(value.len() as u32);
        self.buffer.put_slice(&key[shared..]);
        self.buffer.put_slice(value);

        // Update state
        self.last_key.clear();
        self.last_key.extend_from_slice(key);
        self.counter += 1;
    }

    /// Calculate the length of the shared prefix
    fn shared_prefix_len(&self, a: &[u8], b: &[u8]) -> usize {
        let min_len = a.len().min(b.len());
        for i in 0..min_len {
            if a[i] != b[i] {
                return i;
            }
        }
        min_len
    }

    /// Finish building and return the block data
    pub fn finish(mut self) -> Bytes {
        // Write restart points
        for restart in &self.restarts {
            self.buffer.put_u32_le(*restart);
        }

        // Write number of restarts
        self.buffer.put_u32_le(self.restarts.len() as u32);

        self.buffer.freeze()
    }

    /// Get the current size of the block
    pub fn current_size(&self) -> usize {
        self.buffer.len() + self.restarts.len() * 4 + 4
    }

    /// Check if the block is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Iterator over entries in a block
pub struct BlockIterator {
    block: Block,
    current: usize,
    restart_index: u32,
    key: Vec<u8>,
    value: Vec<u8>,
    valid: bool,
}

impl BlockIterator {
    /// Create a new BlockIterator
    fn new(block: Block) -> Self {
        Self {
            block,
            current: 0,
            restart_index: 0,
            key: Vec::new(),
            value: Vec::new(),
            valid: false,
        }
    }

    /// Seek to the first entry
    pub fn seek_to_first(&mut self) {
        self.seek_to_restart_point(0);
    }

    /// Seek to a restart point
    fn seek_to_restart_point(&mut self, index: u32) {
        self.key.clear();
        self.restart_index = index;
        self.current = self.block.get_restart_point(index) as usize;
        self.valid = self.current < self.block.restart_offset;
    }

    /// Move to the next entry
    pub fn next(&mut self) -> bool {
        if !self.valid {
            return false;
        }

        self.parse_next_entry();
        self.valid
    }

    /// Parse the next entry
    fn parse_next_entry(&mut self) {
        if self.current >= self.block.restart_offset {
            self.valid = false;
            return;
        }

        let data = &self.block.data[self.current..];
        if data.len() < 12 {
            self.valid = false;
            return;
        }

        let mut cursor = std::io::Cursor::new(data);

        let shared = cursor.get_u32_le() as usize;
        let unshared = cursor.get_u32_le() as usize;
        let value_len = cursor.get_u32_le() as usize;

        let offset = cursor.position() as usize;

        if data.len() < offset + unshared + value_len {
            self.valid = false;
            return;
        }

        // Reconstruct key
        self.key.truncate(shared);
        self.key.extend_from_slice(&data[offset..offset + unshared]);

        // Extract value
        self.value.clear();
        self.value
            .extend_from_slice(&data[offset + unshared..offset + unshared + value_len]);

        self.current += 12 + unshared + value_len;
        self.valid = true;
    }

    /// Check if the iterator is valid
    pub fn valid(&self) -> bool {
        self.valid
    }

    /// Get the current key
    pub fn key(&self) -> &[u8] {
        assert!(self.valid, "Iterator not valid");
        &self.key
    }

    /// Get the current value
    pub fn value(&self) -> &[u8] {
        assert!(self.valid, "Iterator not valid");
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_builder_empty() {
        let builder = BlockBuilder::new(16);
        assert!(builder.is_empty());
    }

    #[test]
    fn test_block_builder_single_entry() {
        let mut builder = BlockBuilder::new(16);
        builder.add(b"key1", b"value1");

        let data = builder.finish();
        let block = Block::new(data).unwrap();

        assert_eq!(block.num_restarts(), 1);
    }

    #[test]
    fn test_block_builder_multiple_entries() {
        let mut builder = BlockBuilder::new(2);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");

        let data = builder.finish();
        let block = Block::new(data).unwrap();

        // Should have 2 restart points (at entry 0 and entry 2)
        assert_eq!(block.num_restarts(), 2);
    }

    #[test]
    fn test_block_iterator() {
        let mut builder = BlockBuilder::new(16);
        builder.add(b"apple", b"red");
        builder.add(b"banana", b"yellow");
        builder.add(b"cherry", b"red");

        let data = builder.finish();
        let block = Block::new(data).unwrap();

        let mut iter = block.iter();
        iter.seek_to_first();

        // First entry
        assert!(iter.next());
        assert!(iter.valid());
        assert_eq!(iter.key(), b"apple");
        assert_eq!(iter.value(), b"red");

        // Second entry
        assert!(iter.next());
        assert!(iter.valid());
        assert_eq!(iter.key(), b"banana");
        assert_eq!(iter.value(), b"yellow");

        // Third entry
        assert!(iter.next());
        assert!(iter.valid());
        assert_eq!(iter.key(), b"cherry");
        assert_eq!(iter.value(), b"red");

        // No more entries
        assert!(!iter.next());
        assert!(!iter.valid());
    }

    #[test]
    fn test_prefix_compression() {
        let mut builder = BlockBuilder::new(16);
        builder.add(b"apple_a", b"1");
        builder.add(b"apple_b", b"2");
        builder.add(b"apple_c", b"3");

        let size_with_compression = builder.current_size();

        // Without compression, each entry would be at least:
        // 12 bytes (header) + 7 bytes (key) + 1 byte (value) = 20 bytes per entry
        // With compression, later entries should be smaller
        let size_without_compression = 20 * 3 + 8; // 8 for restart info

        assert!(size_with_compression < size_without_compression);
    }

    #[test]
    #[should_panic(expected = "Keys must be added in sorted order")]
    fn test_block_builder_unsorted_keys() {
        let mut builder = BlockBuilder::new(16);
        builder.add(b"key2", b"value2");
        builder.add(b"key1", b"value1"); // This should panic
    }
}
