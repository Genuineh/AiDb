//! SSTable reader implementation.
//!
//! Reads data from an SSTable file with efficient caching and lookup.

use crate::cache::{BlockCache, CacheKey};
use crate::error::{Error, Result};
use crate::filter::{BloomFilter, Filter};
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
/// # Basic Usage
///
/// ```no_run
/// use aidb::sstable::SSTableReader;
///
/// // Open without cache
/// let reader = SSTableReader::open("table.sst").unwrap();
/// if let Some(value) = reader.get(b"key1").unwrap() {
///     println!("Found: {:?}", value);
/// }
/// ```
///
/// # With Block Cache
///
/// ```no_run
/// use aidb::sstable::SSTableReader;
/// use aidb::cache::BlockCache;
/// use std::sync::Arc;
///
/// // Create a shared cache
/// let cache = Arc::new(BlockCache::new(8 * 1024 * 1024)); // 8MB
///
/// // Open with cache
/// let reader = SSTableReader::open_with_cache("table.sst", Some(cache)).unwrap();
/// if let Some(value) = reader.get(b"key1").unwrap() {
///     println!("Found: {:?}", value);
/// }
/// ```
#[derive(Debug)]
pub struct SSTableReader {
    file: Arc<File>,
    file_number: u64,
    index_block: IndexBlock,
    bloom_filter: Option<BloomFilter>,
    #[allow(dead_code)]
    footer: Footer,
    file_size: u64,
    file_path: std::path::PathBuf,
    block_cache: Option<Arc<BlockCache>>,
}

impl SSTableReader {
    /// Open an SSTable file for reading
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_cache(path, None)
    }

    /// Open an SSTable file for reading with optional block cache
    pub fn open_with_cache<P: AsRef<Path>>(
        path: P,
        block_cache: Option<Arc<BlockCache>>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path)?;

        // Get file size
        let file_size = file.metadata()?.len();
        if file_size < FOOTER_SIZE as u64 {
            return Err(Error::corruption("File too small to be a valid SSTable"));
        }

        // Extract file number from filename
        // Use path hash as fallback to ensure unique cache keys
        let file_number = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|s| s.strip_suffix(".sst"))
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or_else(|| {
                // Fallback: use hash of full path to ensure uniqueness
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                path.hash(&mut hasher);
                hasher.finish()
            });

        // Read footer from the end of the file
        file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
        let footer = Footer::read_from(&mut file)?;

        // Read index block
        let index_data = Self::read_block_data(&mut file, &footer.index_handle)?;
        let index_block = IndexBlock::new(index_data)?;

        // Read bloom filter from meta block
        let bloom_filter = if footer.meta_index_handle.size > 5 {
            // Try to read meta block (it should point to the bloom filter)
            // For now, we directly read using the meta index handle's offset minus meta block size
            // This is a simplification; in a full implementation, we'd parse the meta index

            // Read the actual meta block (bloom filter data)
            // The meta block comes before the meta index block
            // We need to calculate its position from the footer
            let _meta_block_handle = BlockHandle::new(
                0,                               // Will be calculated
                footer.meta_index_handle.offset, // Size before meta index
            );

            // For simplicity, we'll read it from the known position
            // In the builder, we write: [meta_block][meta_index_block][index_block][footer]
            // The footer.meta_index_handle points to meta_index_block
            // We need to find meta_block, which comes before it

            // Let's try a different approach: read from the start of meta section
            // The meta section starts after all data blocks
            // We can estimate this from the index block entries

            // For now, try to read the meta block assuming it's before the meta index
            // This is a simplified implementation
            match Self::try_read_bloom_filter(&mut file, &footer) {
                Ok(Some(filter)) => Some(filter),
                Ok(None) => None,
                Err(e) => {
                    log::warn!("Failed to read bloom filter: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            file: Arc::new(file),
            file_number,
            index_block,
            bloom_filter,
            footer,
            file_size,
            file_path: path.to_path_buf(),
            block_cache,
        })
    }

    /// Get the value for a key
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check bloom filter first (if available)
        if let Some(ref filter) = self.bloom_filter {
            if !filter.may_contain(key) {
                // Definitely not in the SSTable
                return Ok(None);
            }
        }
        // Find the data block that may contain the key
        let handle = match self.index_block.find_block(key)? {
            Some(h) => h,
            None => return Ok(None),
        };

        // Read block with cache support
        let block_data = self.read_block_cached(&handle)?;
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

        #[allow(unused_mut)]
        let mut decompressed = match compression {
            CompressionType::None => data.to_vec(),
            #[cfg(feature = "snappy")]
            CompressionType::Snappy => snap::raw::Decoder::new()
                .decompress_vec(data)
                .map_err(|e| Error::internal(format!("Decompression failed: {}", e)))?,
            #[cfg(not(feature = "snappy"))]
            CompressionType::Snappy => {
                return Err(Error::internal("Snappy compression not enabled"));
            }
            #[allow(unreachable_patterns)]
            _ => {
                // This handles any compression type not explicitly matched above
                // Including Lz4 when the feature is not enabled
                return Err(Error::internal(format!(
                    "Unsupported compression type: {}",
                    compression_type
                )));
            }
        };

        // Handle Lz4 compression if the feature is enabled
        #[cfg(feature = "lz4-compression")]
        if let CompressionType::Lz4 = compression {
            decompressed = lz4::block::decompress(data, None)
                .map_err(|e| Error::internal(format!("LZ4 decompression failed: {}", e)))?;
        }

        Ok(Bytes::from(decompressed))
    }

    /// Try to read the bloom filter from the meta block
    fn try_read_bloom_filter(file: &mut File, footer: &Footer) -> Result<Option<BloomFilter>> {
        // The meta block handle is stored in the footer, but it points to the meta index
        // We need to read the actual meta block which comes before the meta index

        // Calculate meta block position: it's before the meta index block
        // From the builder: meta_block_offset = self.data_block_offset
        // meta_index_offset = self.data_block_offset + meta_block_size

        // We can calculate the meta block offset from:
        // meta_index_offset = footer.meta_index_handle.offset
        // So meta_block_offset = meta_index_offset - meta_block_size

        // But we don't know meta_block_size yet. Let's try a different approach:
        // Read backward from meta_index_offset to find the meta block

        // For now, let's try to read the meta index to get the meta block handle
        // But in our simple implementation, meta index is empty

        // Simpler approach: try to read meta block at a known offset
        // The meta block starts right after the last data block
        // We can get the offset from the last index entry

        let mut index_iter =
            IndexBlock::new(Self::read_block_data(file, &footer.index_handle)?)?.iter();
        index_iter.seek_to_first();

        let mut last_data_block_end = 0u64;
        while index_iter.advance() {
            if let Ok(entry) = index_iter.entry() {
                last_data_block_end = entry.handle.offset + entry.handle.size;
            }
        }

        if last_data_block_end == 0 {
            return Ok(None);
        }

        // Meta block should start at last_data_block_end
        let meta_block_offset = last_data_block_end;
        let meta_block_size = footer.meta_index_handle.offset - meta_block_offset;

        if !(5..=100_000_000).contains(&meta_block_size) {
            // Sanity check
            return Ok(None);
        }

        let meta_block_handle = BlockHandle::new(meta_block_offset, meta_block_size);

        // Try to read the meta block
        let meta_data = Self::read_block_data(file, &meta_block_handle)?;

        // Try to decode as bloom filter
        if meta_data.len() > 12 {
            match BloomFilter::decode(&meta_data) {
                Ok(filter) => Ok(Some(filter)),
                Err(_) => Ok(None), // Not a valid bloom filter
            }
        } else {
            Ok(None) // Empty meta block
        }
    }

    /// Read block data using an Arc<File> (for concurrent access)
    fn read_block_with_handle(file: &Arc<File>, handle: &BlockHandle) -> Result<Bytes> {
        // Clone the file descriptor for this read operation
        let mut file_clone = file.try_clone().map_err(Error::Io)?;

        Self::read_block_data(&mut file_clone, handle)
    }

    /// Read a block with caching support
    fn read_block_cached(&self, handle: &BlockHandle) -> Result<Bytes> {
        if let Some(ref cache) = self.block_cache {
            let cache_key = CacheKey::new(self.file_number, handle.offset);

            // Check cache
            if let Some(cached_data) = cache.get(&cache_key) {
                return Ok(cached_data);
            }

            // Cache miss - read from file
            let data = Self::read_block_with_handle(&self.file, handle)?;
            // Insert into cache for future reads
            cache.insert(cache_key, data.clone());
            Ok(data)
        } else {
            // No cache - read directly from file
            Self::read_block_with_handle(&self.file, handle)
        }
    }

    /// Get the number of data blocks
    pub fn num_blocks(&self) -> usize {
        self.index_block.len()
    }

    /// Get the file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Get the file path
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get the file number from the filename
    ///
    /// Extracts the file number from filenames like "000001.sst"
    /// Returns None if the filename doesn't match the expected pattern
    pub fn file_number(&self) -> Option<u64> {
        let filename = self.file_path.file_name()?.to_str()?;
        let num_str = filename.strip_suffix(".sst")?;
        num_str.parse::<u64>().ok()
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

        // Read the first data block with cache support
        let block_data = self.read_block_cached(&handle)?;
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

    /// Check if bloom filter is available
    pub fn has_bloom_filter(&self) -> bool {
        self.bloom_filter.is_some()
    }

    /// Returns all keys in the SSTable.
    ///
    /// This collects all unique keys from the SSTable.
    pub fn keys(&self) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();
        let mut iter = self.iter();
        
        iter.seek_to_first()?;
        while iter.valid() {
            keys.push(iter.key().to_vec());
            iter.advance()?;
        }
        
        Ok(keys)
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
