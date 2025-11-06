//! SSTable builder implementation.
//!
//! Builds an SSTable file from a sequence of sorted key-value pairs.

use crate::error::{Error, Result};
use crate::sstable::block::BlockBuilder;
use crate::sstable::footer::{BlockHandle, Footer};
use crate::sstable::index::{IndexBlockBuilder, IndexEntry};
use crate::sstable::{CompressionType, DEFAULT_BLOCK_SIZE, FOOTER_SIZE};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// SSTableBuilder builds an SSTable file.
///
/// Usage:
/// ```no_run
/// use aidb::sstable::SSTableBuilder;
///
/// let mut builder = SSTableBuilder::new("table.sst").unwrap();
/// builder.add(b"key1", b"value1").unwrap();
/// builder.add(b"key2", b"value2").unwrap();
/// builder.finish().unwrap();
/// ```
pub struct SSTableBuilder {
    writer: BufWriter<File>,
    data_block_builder: BlockBuilder,
    index_block_builder: IndexBlockBuilder,
    last_key: Vec<u8>,
    data_block_offset: u64,
    num_entries: u64,
    block_size: usize,
    compression: CompressionType,
    pending_handle: Option<BlockHandle>,
}

impl SSTableBuilder {
    /// Create a new SSTableBuilder
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            writer,
            data_block_builder: BlockBuilder::new(16), // 16 restart interval
            index_block_builder: IndexBlockBuilder::new(),
            last_key: Vec::new(),
            data_block_offset: 0,
            num_entries: 0,
            block_size: DEFAULT_BLOCK_SIZE,
            compression: CompressionType::None,
            pending_handle: None,
        })
    }

    /// Set the block size (default: 4KB)
    pub fn set_block_size(&mut self, size: usize) {
        self.block_size = size;
    }

    /// Set the compression type
    pub fn set_compression(&mut self, compression: CompressionType) {
        self.compression = compression;
    }

    /// Add a key-value pair to the SSTable.
    ///
    /// Keys must be added in sorted order.
    pub fn add(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if key.is_empty() {
            return Err(Error::invalid_argument("Key cannot be empty"));
        }

        // Verify keys are in sorted order
        if !self.last_key.is_empty() && key <= self.last_key.as_slice() {
            return Err(Error::invalid_argument("Keys must be added in sorted order"));
        }

        // If we have a pending index entry, add it now
        if let Some(handle) = self.pending_handle.take() {
            let entry = IndexEntry::new(self.last_key.clone(), handle);
            self.index_block_builder.add_entry(&entry);
        }

        // Add to current data block
        self.data_block_builder.add(key, value);
        self.last_key.clear();
        self.last_key.extend_from_slice(key);
        self.num_entries += 1;

        // Flush block if it's large enough
        if self.data_block_builder.current_size() >= self.block_size {
            self.flush_data_block()?;
        }

        Ok(())
    }

    /// Flush the current data block to disk
    fn flush_data_block(&mut self) -> Result<()> {
        if self.data_block_builder.is_empty() {
            return Ok(());
        }

        // Build the block by replacing with a new builder
        let old_builder = std::mem::replace(&mut self.data_block_builder, BlockBuilder::new(16));
        let block_data = old_builder.finish();
        let mut compressed_data = block_data.to_vec();

        // Apply compression if enabled
        #[cfg(feature = "snappy")]
        if self.compression == CompressionType::Snappy {
            compressed_data = snap::raw::Encoder::new()
                .compress_vec(&block_data)
                .map_err(|e| Error::internal(format!("Compression failed: {}", e)))?;
        }

        // Write block data
        let block_offset = self.data_block_offset;
        let block_size = compressed_data.len() as u64;

        self.writer.write_all(&compressed_data)?;

        // Write compression type trailer (1 byte)
        self.writer.write_all(&[self.compression as u8])?;

        // Write CRC32 checksum (4 bytes)
        let checksum = crc32fast::hash(&compressed_data);
        self.writer.write_all(&checksum.to_le_bytes())?;

        // Update offset (data + 1 byte compression + 4 bytes crc)
        self.data_block_offset += block_size + 5;

        // Save handle for the index
        let handle = BlockHandle::new(block_offset, block_size + 5);
        self.pending_handle = Some(handle);

        // Note: data_block_builder was already replaced with a new one above

        Ok(())
    }

    /// Finish building the SSTable.
    ///
    /// This writes the index block, meta index block, and footer.
    pub fn finish(mut self) -> Result<u64> {
        // Flush any remaining data block
        self.flush_data_block()?;

        // Add the last pending index entry
        if let Some(handle) = self.pending_handle.take() {
            let entry = IndexEntry::new(self.last_key.clone(), handle);
            self.index_block_builder.add_entry(&entry);
        }

        // Write meta index block (empty for now, but reserved for bloom filters)
        let meta_index_offset = self.data_block_offset;
        let meta_index_data = vec![0u8; 8]; // Empty meta block with just num_restarts=0
        self.writer.write_all(&meta_index_data)?;
        // Write compression type and checksum for meta index block
        self.writer.write_all(&[CompressionType::None as u8])?;
        let meta_checksum = crc32fast::hash(&meta_index_data);
        self.writer.write_all(&meta_checksum.to_le_bytes())?;
        let meta_index_size = meta_index_data.len() as u64 + 5; // data + compression + checksum
        let meta_index_handle = BlockHandle::new(meta_index_offset, meta_index_size);

        // Write index block
        let index_offset = self.data_block_offset + meta_index_size;
        let index_data = self.index_block_builder.finish();
        self.writer.write_all(&index_data)?;
        // Write compression type and checksum for index block
        self.writer.write_all(&[CompressionType::None as u8])?;
        let index_checksum = crc32fast::hash(&index_data);
        self.writer.write_all(&index_checksum.to_le_bytes())?;
        let index_size = index_data.len() as u64 + 5; // data + compression + checksum
        let index_handle = BlockHandle::new(index_offset, index_size);

        // Write footer
        let footer = Footer::new(meta_index_handle, index_handle);
        footer.write_to(&mut self.writer)?;

        // Flush to disk
        self.writer.flush()?;

        let total_size = index_offset + index_size + FOOTER_SIZE as u64;
        Ok(total_size)
    }

    /// Get the number of entries added
    pub fn num_entries(&self) -> u64 {
        self.num_entries
    }

    /// Get the current file size
    pub fn current_size(&self) -> u64 {
        self.data_block_offset + self.data_block_builder.current_size() as u64
    }

    /// Abandon the SSTable (don't write footer)
    pub fn abandon(self) -> Result<()> {
        // Just drop the writer without finishing
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_sstable_builder_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let builder = SSTableBuilder::new(temp_file.path()).unwrap();

        assert_eq!(builder.num_entries(), 0);
    }

    #[test]
    fn test_sstable_builder_single_entry() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();

        builder.add(b"key1", b"value1").unwrap();
        assert_eq!(builder.num_entries(), 1);

        let size = builder.finish().unwrap();
        assert!(size > 0);
    }

    #[test]
    fn test_sstable_builder_multiple_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();

        builder.add(b"apple", b"red").unwrap();
        builder.add(b"banana", b"yellow").unwrap();
        builder.add(b"cherry", b"red").unwrap();

        assert_eq!(builder.num_entries(), 3);

        let size = builder.finish().unwrap();
        assert!(size > 0);
    }

    #[test]
    fn test_sstable_builder_large_dataset() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
        builder.set_block_size(1024); // Small blocks to force multiple blocks

        // Add enough entries to create multiple data blocks
        for i in 0..1000 {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            builder.add(key.as_bytes(), value.as_bytes()).unwrap();
        }

        assert_eq!(builder.num_entries(), 1000);

        let size = builder.finish().unwrap();
        assert!(size > 1024); // Should be larger than one block
    }

    #[test]
    fn test_sstable_builder_sorted_keys() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();

        builder.add(b"a", b"1").unwrap();
        builder.add(b"b", b"2").unwrap();

        // Try to add out of order - should fail
        let result = builder.add(b"a", b"3");
        assert!(result.is_err());
    }

    #[test]
    fn test_sstable_builder_empty_key() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();

        let result = builder.add(b"", b"value");
        assert!(result.is_err());
    }

    #[test]
    fn test_sstable_builder_abandon() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();

        builder.add(b"key1", b"value1").unwrap();
        builder.abandon().unwrap();

        // File should exist but not be a valid SSTable (no footer)
        assert!(temp_file.path().exists());
    }
}
