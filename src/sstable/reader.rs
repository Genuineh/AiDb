//! SSTable reader implementation.
//!
//! Reads data from an SSTable file with efficient caching and lookup.

use crate::error::{Error, Result};
use crate::sstable::block::Block;
use crate::sstable::footer::{BlockHandle, Footer};
use crate::sstable::index::IndexBlock;
use crate::sstable::{CompressionType, FOOTER_SIZE};
use bytes::Bytes;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

/// SSTableReader provides read access to an SSTable file.
///
/// Usage:
/// ```no_run
/// use aidb::sstable::SSTableReader;
///
/// let reader = SSTableReader::open("table.sst").unwrap();
/// if let Some(value) = reader.get(b"key1").unwrap() {
///     println!("Found: {:?}", value);
/// }
/// ```
#[derive(Debug)]
pub struct SSTableReader {
    file: Arc<File>,
    index_block: IndexBlock,
    #[allow(dead_code)]
    footer: Footer,
    file_size: u64,
}

impl SSTableReader {
    /// Open an SSTable file for reading
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = File::open(path)?;

        // Get file size
        let file_size = file.metadata()?.len();
        if file_size < FOOTER_SIZE as u64 {
            return Err(Error::corruption("File too small to be a valid SSTable"));
        }

        // Read footer from the end of the file
        file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
        let footer = Footer::read_from(&mut file)?;

        // Read index block
        let index_data = Self::read_block_data(&mut file, &footer.index_handle)?;
        let index_block = IndexBlock::new(index_data)?;

        Ok(Self { file: Arc::new(file), index_block, footer, file_size })
    }

    /// Get the value for a key
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Find the data block that may contain the key
        let handle = match self.index_block.find_block(key)? {
            Some(h) => h,
            None => return Ok(None),
        };

        // Read and search the data block
        let block_data = Self::read_block_with_handle(&self.file, &handle)?;
        let block = Block::new(block_data)?;

        // Search for the key in the block
        let mut iter = block.iter();
        iter.seek_to_first();

        while iter.advance() {
            if iter.key() == key {
                let value = iter.value().to_vec();
                // Empty value means tombstone (deleted)
                if value.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(value));
            }
            if iter.key() > key {
                // Key doesn't exist
                return Ok(None);
            }
        }

        Ok(None)
    }

    /// Read raw block data from the file
    fn read_block_data(file: &mut File, handle: &BlockHandle) -> Result<Bytes> {
        // Seek to block offset
        file.seek(SeekFrom::Start(handle.offset))?;

        // Read block data + compression type (1 byte) + checksum (4 bytes)
        let total_size = handle.size as usize;
        if total_size < 5 {
            return Err(Error::corruption("Block size too small"));
        }

        let mut buffer = vec![0u8; total_size];
        file.read_exact(&mut buffer)?;

        // Extract components
        // Layout: [data...][compression_type: 1 byte][checksum: 4 bytes]
        let data_size = total_size - 5;
        let data = &buffer[..data_size];
        let compression_type = buffer[data_size];
        let checksum_bytes = &buffer[data_size + 1..data_size + 5];
        let stored_checksum = u32::from_le_bytes(checksum_bytes.try_into().unwrap());

        // Verify checksum (computed on the compressed data)
        let computed_checksum = crc32fast::hash(data);
        if computed_checksum != stored_checksum {
            return Err(Error::ChecksumMismatch {
                expected: stored_checksum,
                actual: computed_checksum,
            });
        }

        // Decompress if needed
        let compression = CompressionType::from_u8(compression_type)
            .ok_or_else(|| Error::corruption("Invalid compression type"))?;

        let decompressed = match compression {
            CompressionType::None => data.to_vec(),
            #[cfg(feature = "snappy")]
            CompressionType::Snappy => snap::raw::Decoder::new()
                .decompress_vec(data)
                .map_err(|e| Error::internal(format!("Decompression failed: {}", e)))?,
            #[cfg(not(feature = "snappy"))]
            CompressionType::Snappy => {
                return Err(Error::internal("Snappy compression not enabled"));
            }
        };

        Ok(Bytes::from(decompressed))
    }

    /// Read block data using an Arc<File> (for concurrent access)
    fn read_block_with_handle(file: &Arc<File>, handle: &BlockHandle) -> Result<Bytes> {
        // Clone the file descriptor for this read operation
        let mut file_clone = file.try_clone().map_err(Error::Io)?;

        Self::read_block_data(&mut file_clone, handle)
    }

    /// Get the number of data blocks
    pub fn num_blocks(&self) -> usize {
        self.index_block.len()
    }

    /// Get the file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Get the smallest key in the SSTable
    pub fn smallest_key(&self) -> Result<Option<Vec<u8>>> {
        let mut iter = self.index_block.iter();
        iter.seek_to_first();

        if !iter.advance() {
            return Ok(None);
        }

        let entry = iter.entry()?;
        let handle = entry.handle;

        // Read the first data block
        let block_data = Self::read_block_with_handle(&self.file, &handle)?;
        let block = Block::new(block_data)?;

        let mut block_iter = block.iter();
        block_iter.seek_to_first();

        if !block_iter.advance() {
            return Ok(None);
        }

        Ok(Some(block_iter.key().to_vec()))
    }

    /// Get the largest key in the SSTable
    pub fn largest_key(&self) -> Result<Option<Vec<u8>>> {
        let mut iter = self.index_block.iter();
        iter.seek_to_first();

        let mut last_entry = None;
        while iter.advance() {
            last_entry = Some(iter.entry()?);
        }

        let entry = match last_entry {
            Some(e) => e,
            None => return Ok(None),
        };

        Ok(Some(entry.key))
    }

    /// Create an iterator over all key-value pairs
    pub fn iter(&self) -> SSTableIterator {
        SSTableIterator::new(self)
    }
}

/// Iterator over all entries in an SSTable
pub struct SSTableIterator {
    file: Arc<File>,
    index_iter_entries: Vec<(Vec<u8>, BlockHandle)>,
    current_block_index: usize,
    current_block: Option<Block>,
    current_block_iter: Option<crate::sstable::block::BlockIterator>,
}

impl SSTableIterator {
    fn new(reader: &SSTableReader) -> Self {
        // Collect all index entries upfront
        let mut entries = Vec::new();
        let mut index_iter = reader.index_block.iter();
        index_iter.seek_to_first();

        while index_iter.advance() {
            if let Ok(entry) = index_iter.entry() {
                entries.push((entry.key, entry.handle));
            }
        }

        Self {
            file: Arc::clone(&reader.file),
            index_iter_entries: entries,
            current_block_index: 0,
            current_block: None,
            current_block_iter: None,
        }
    }

    /// Seek to the first entry
    pub fn seek_to_first(&mut self) -> Result<()> {
        self.current_block_index = 0;
        self.load_current_block()?;
        Ok(())
    }

    /// Load the current data block
    fn load_current_block(&mut self) -> Result<()> {
        if self.current_block_index >= self.index_iter_entries.len() {
            self.current_block = None;
            self.current_block_iter = None;
            return Ok(());
        }

        let (_, handle) = &self.index_iter_entries[self.current_block_index];
        let block_data = SSTableReader::read_block_with_handle(&self.file, handle)?;
        let block = Block::new(block_data)?;

        let mut iter = block.iter();
        iter.seek_to_first();

        self.current_block = Some(block);
        self.current_block_iter = Some(iter);

        Ok(())
    }

    /// Move to the next entry
    pub fn advance(&mut self) -> Result<bool> {
        if let Some(ref mut iter) = self.current_block_iter {
            if iter.advance() {
                return Ok(true);
            }
        }

        // Move to next block
        self.current_block_index += 1;
        self.load_current_block()?;

        if let Some(ref mut iter) = self.current_block_iter {
            Ok(iter.advance())
        } else {
            Ok(false)
        }
    }

    /// Check if the iterator is valid
    pub fn valid(&self) -> bool {
        self.current_block_iter.as_ref().map(|i| i.valid()).unwrap_or(false)
    }

    /// Get the current key
    pub fn key(&self) -> &[u8] {
        self.current_block_iter.as_ref().unwrap().key()
    }

    /// Get the current value
    pub fn value(&self) -> &[u8] {
        self.current_block_iter.as_ref().unwrap().value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sstable::SSTableBuilder;
    use tempfile::NamedTempFile;

    fn create_test_sstable(entries: &[(&[u8], &[u8])]) -> NamedTempFile {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();

        for (key, value) in entries {
            builder.add(key, value).unwrap();
        }

        builder.finish().unwrap();
        temp_file
    }

    #[test]
    fn test_sstable_reader_open() {
        let entries = vec![
            (b"key1" as &[u8], b"value1" as &[u8]),
            (b"key2", b"value2"),
            (b"key3", b"value3"),
        ];

        let temp_file = create_test_sstable(&entries);
        let reader = SSTableReader::open(temp_file.path()).unwrap();

        assert_eq!(reader.num_blocks(), 1);
    }

    #[test]
    fn test_sstable_reader_get() {
        let entries = vec![
            (b"apple" as &[u8], b"red" as &[u8]),
            (b"banana", b"yellow"),
            (b"cherry", b"red"),
        ];

        let temp_file = create_test_sstable(&entries);
        let reader = SSTableReader::open(temp_file.path()).unwrap();

        // Test exact matches
        assert_eq!(reader.get(b"apple").unwrap(), Some(b"red".to_vec()));
        assert_eq!(reader.get(b"banana").unwrap(), Some(b"yellow".to_vec()));
        assert_eq!(reader.get(b"cherry").unwrap(), Some(b"red".to_vec()));

        // Test non-existent key
        assert_eq!(reader.get(b"durian").unwrap(), None);
        assert_eq!(reader.get(b"aaa").unwrap(), None);
    }

    #[test]
    fn test_sstable_reader_smallest_largest() {
        let entries =
            vec![(b"apple" as &[u8], b"1" as &[u8]), (b"banana", b"2"), (b"cherry", b"3")];

        let temp_file = create_test_sstable(&entries);
        let reader = SSTableReader::open(temp_file.path()).unwrap();

        assert_eq!(reader.smallest_key().unwrap(), Some(b"apple".to_vec()));
        assert_eq!(reader.largest_key().unwrap(), Some(b"cherry".to_vec()));
    }

    #[test]
    fn test_sstable_reader_large_dataset() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
        builder.set_block_size(1024); // Small blocks

        // Create a large dataset
        for i in 0..1000 {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            builder.add(key.as_bytes(), value.as_bytes()).unwrap();
        }
        builder.finish().unwrap();

        let reader = SSTableReader::open(temp_file.path()).unwrap();

        // Should have multiple blocks
        assert!(reader.num_blocks() > 1);

        // Test random access
        assert_eq!(reader.get(b"key00000500").unwrap(), Some(b"value00000500".to_vec()));
        assert_eq!(reader.get(b"key00000000").unwrap(), Some(b"value00000000".to_vec()));
        assert_eq!(reader.get(b"key00000999").unwrap(), Some(b"value00000999".to_vec()));
    }

    #[test]
    fn test_sstable_iterator() {
        let entries = vec![
            (b"apple" as &[u8], b"red" as &[u8]),
            (b"banana", b"yellow"),
            (b"cherry", b"red"),
        ];

        let temp_file = create_test_sstable(&entries);
        let reader = SSTableReader::open(temp_file.path()).unwrap();

        let mut iter = reader.iter();
        iter.seek_to_first().unwrap();

        let mut collected = Vec::new();
        while iter.advance().unwrap() {
            if iter.valid() {
                collected.push((iter.key().to_vec(), iter.value().to_vec()));
            }
        }

        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], (b"apple".to_vec(), b"red".to_vec()));
        assert_eq!(collected[1], (b"banana".to_vec(), b"yellow".to_vec()));
        assert_eq!(collected[2], (b"cherry".to_vec(), b"red".to_vec()));
    }

    #[test]
    fn test_sstable_corrupted_checksum() {
        let entries = vec![(b"key1" as &[u8], b"value1" as &[u8])];

        let temp_file = create_test_sstable(&entries);

        // Corrupt the file by modifying a byte
        let mut file = std::fs::OpenOptions::new().write(true).open(temp_file.path()).unwrap();

        use std::io::{Seek, SeekFrom, Write};
        file.seek(SeekFrom::Start(10)).unwrap();
        file.write_all(&[0xFF]).unwrap();
        drop(file);

        let reader = SSTableReader::open(temp_file.path()).unwrap();
        let result = reader.get(b"key1");

        // Should detect corruption
        assert!(result.is_err());
    }
}
