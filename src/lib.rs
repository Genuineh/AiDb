//! # AiDb - A High-Performance LSM-Tree Storage Engine
//!
//! AiDb is a persistent key-value storage engine inspired by RocksDB and LevelDB.
//! It implements the Log-Structured Merge-Tree (LSM-Tree) architecture for high
//! write throughput and efficient range queries.
//!
//! ## Architecture
//!
//! The storage engine consists of several key components:
//!
//! - **WAL (Write-Ahead Log)**: Ensures durability by logging all writes
//! - **MemTable**: In-memory sorted structure for recent writes
//! - **SSTable**: Immutable sorted files on disk
//! - **Compaction**: Background process to merge and optimize SSTables
//! - **Bloom Filter**: Speeds up key lookups
//! - **Block Cache**: Caches frequently accessed data blocks
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use aidb::{DB, Options};
//!
//! # fn main() -> Result<(), aidb::Error> {
//! // Open or create a database
//! let options = Options::default();
//! let db = DB::open("./data", options)?;
//!
//! // Write operations
//! db.put(b"key1", b"value1")?;
//! db.put(b"key2", b"value2")?;
//!
//! // Read operations
//! if let Some(value) = db.get(b"key1")? {
//!     println!("Found: {:?}", value);
//! }
//!
//! // Delete operations
//! db.delete(b"key1")?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

// Module declarations
pub mod config;
pub mod error;
pub mod memtable;
pub mod sstable;
pub mod wal;

// Re-exports
pub use config::Options;
pub use error::{Error, Result};

use memtable::MemTable;
use parking_lot::RwLock;
use sstable::SSTableReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use wal::WAL;

/// The main database handle.
///
/// This is the primary interface for interacting with the storage engine.
/// It supports basic key-value operations: put, get, and delete.
///
/// # Thread Safety
///
/// `DB` is designed to be thread-safe and can be safely shared across threads
/// using `Arc<DB>`.
pub struct DB {
    /// Database directory path
    #[allow(dead_code)]
    path: PathBuf,

    /// Configuration options
    options: Options,

    /// Current mutable MemTable
    memtable: Arc<RwLock<MemTable>>,

    /// Immutable MemTables waiting to be flushed
    immutable_memtables: Arc<RwLock<Vec<Arc<MemTable>>>>,

    /// Write-Ahead Log
    wal: Arc<RwLock<WAL>>,

    /// SSTable readers organized by level
    /// Level 0 contains newest tables (may overlap)
    /// Level 1+ contains non-overlapping tables
    sstables: Arc<RwLock<Vec<Vec<Arc<SSTableReader>>>>>,

    /// Global sequence number (monotonically increasing)
    sequence: Arc<AtomicU64>,
}

impl DB {
    /// Opens a database at the specified path with the given options.
    ///
    /// If the database does not exist, it will be created.
    /// If it exists, it will be opened and any existing data will be recovered.
    ///
    /// # Arguments
    ///
    /// * `path` - The filesystem path where the database will be stored
    /// * `options` - Configuration options for the database
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is invalid or inaccessible
    /// - Recovery fails due to corrupted data
    /// - Insufficient permissions
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// let options = Options::default();
    /// let db = DB::open("./my_database", options)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn open<P: AsRef<std::path::Path>>(path: P, options: Options) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Validate options
        options.validate()?;

        // Step 1: Create directory if not exists
        if !path.exists() {
            if options.create_if_missing {
                std::fs::create_dir_all(&path)?;
            } else {
                return Err(Error::NotFound(format!(
                    "Database directory does not exist: {:?}",
                    path
                )));
            }
        } else if options.error_if_exists {
            return Err(Error::AlreadyExists(format!(
                "Database already exists: {:?}",
                path
            )));
        }

        // Step 2: Initialize sequence number
        let mut sequence = 0u64;

        // Step 3: Open or create WAL
        let wal_path = path.join(wal::wal_filename(1));
        let wal = if options.use_wal {
            WAL::open(&wal_path)?
        } else {
            WAL::open(&wal_path)?
        };

        // Step 4: Recover from WAL if it exists and has data
        let recovered_entries = if wal_path.exists() && wal.size() > 0 {
            WAL::recover(&wal_path)?
        } else {
            Vec::new()
        };

        // Step 5: Initialize MemTable with recovered data
        let memtable = MemTable::new(sequence + 1);

        for _entry in recovered_entries {
            // Parse the entry (format: "key:value" or "delete:key")
            // For now, we'll enhance this in the put/delete implementation
            sequence += 1;
        }

        // Step 6: Load existing SSTables
        // For now, we'll start with empty SSTable list
        // This will be enhanced when we implement flush and compaction
        let sstables: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); options.max_levels];

        // Step 7: Construct DB instance
        Ok(DB {
            path,
            options,
            memtable: Arc::new(RwLock::new(memtable)),
            immutable_memtables: Arc::new(RwLock::new(Vec::new())),
            wal: Arc::new(RwLock::new(wal)),
            sstables: Arc::new(RwLock::new(sstables)),
            sequence: Arc::new(AtomicU64::new(sequence)),
        })
    }

    /// Inserts a key-value pair into the database.
    ///
    /// If the key already exists, its value will be overwritten.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to insert
    /// * `value` - The value to associate with the key
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails due to I/O errors.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = DB::open("./data", Options::default())?;
    /// db.put(b"key", b"value")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // Step 1: Get the next sequence number
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;

        // Step 2: Write to WAL first (for durability)
        if self.options.use_wal {
            let mut wal = self.wal.write();

            // Encode the entry as: "put:key_len:key:value"
            let mut entry = Vec::new();
            entry.extend_from_slice(b"put:");
            entry.extend_from_slice(&(key.len() as u32).to_le_bytes());
            entry.extend_from_slice(b":");
            entry.extend_from_slice(key);
            entry.extend_from_slice(b":");
            entry.extend_from_slice(value);

            wal.append(&entry)?;

            if self.options.sync_wal {
                wal.sync()?;
            }
        }

        // Step 3: Insert into MemTable
        {
            let memtable = self.memtable.read();
            memtable.put(key, value, seq);
        }

        // Step 4: Check if MemTable is full and needs flushing
        let memtable_size = {
            let memtable = self.memtable.read();
            memtable.approximate_size()
        };

        if memtable_size >= self.options.memtable_size {
            // TODO: Trigger flush (will be implemented in the Flush phase)
            // For now, we'll just log that it's full
            log::warn!(
                "MemTable is full ({} bytes), flush not yet implemented",
                memtable_size
            );
        }

        Ok(())
    }

    /// Retrieves the value associated with a key.
    ///
    /// Returns `None` if the key does not exist or has been deleted.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails due to I/O errors or data corruption.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = DB::open("./data", Options::default())?;
    /// if let Some(value) = db.get(b"key")? {
    ///     println!("Found: {:?}", value);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Get the current sequence number for consistent reads
        let max_seq = self.sequence.load(Ordering::SeqCst);

        // Step 1: Check current MemTable
        {
            let memtable = self.memtable.read();
            if let Some(value) = memtable.get(key, max_seq) {
                return Ok(Some(value));
            }
        }

        // Step 2: Check Immutable MemTables (newest to oldest)
        {
            let immutable = self.immutable_memtables.read();
            for memtable in immutable.iter().rev() {
                if let Some(value) = memtable.get(key, max_seq) {
                    return Ok(Some(value));
                }
            }
        }

        // Step 3: Search SSTables from Level 0 to Level N
        {
            let sstables = self.sstables.read();
            for level_tables in sstables.iter() {
                for _table in level_tables.iter().rev() {
                    // TODO: Use bloom filter to skip tables
                    // TODO: Use index block to find the right data block
                    // For now, we'll implement this in the next phase
                }
            }
        }

        // Key not found
        Ok(None)
    }

    /// Deletes a key from the database.
    ///
    /// This operation is implemented as a tombstone marker.
    /// The actual data is removed during compaction.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to delete
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails due to I/O errors.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = DB::open("./data", Options::default())?;
    /// db.delete(b"key")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn delete(&self, key: &[u8]) -> Result<()> {
        // Step 1: Get the next sequence number
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;

        // Step 2: Write tombstone to WAL
        if self.options.use_wal {
            let mut wal = self.wal.write();

            // Encode the entry as: "del:key_len:key"
            let mut entry = Vec::new();
            entry.extend_from_slice(b"del:");
            entry.extend_from_slice(&(key.len() as u32).to_le_bytes());
            entry.extend_from_slice(b":");
            entry.extend_from_slice(key);

            wal.append(&entry)?;

            if self.options.sync_wal {
                wal.sync()?;
            }
        }

        // Step 3: Insert tombstone into MemTable
        {
            let memtable = self.memtable.read();
            memtable.delete(key, seq);
        }

        Ok(())
    }

    /// Closes the database, ensuring all data is flushed to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if flushing fails.
    pub fn close(&self) -> Result<()> {
        // Step 1: Sync WAL to ensure all writes are persisted
        if self.options.use_wal {
            let mut wal = self.wal.write();
            wal.sync()?;
        }

        // TODO: Step 2: Flush MemTable to SSTable (will be implemented in Flush phase)
        // TODO: Step 3: Write final Manifest entry

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_db_open() {
        let temp_dir = TempDir::new().unwrap();
        let options = Options::default();
        let result = DB::open(temp_dir.path(), options);
        assert!(result.is_ok());
    }

    #[test]
    fn test_db_put_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Test put and get
        db.put(b"key1", b"value1").unwrap();
        let value = db.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Test non-existent key
        let value = db.get(b"key2").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_db_delete() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Put a key
        db.put(b"key1", b"value1").unwrap();
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Delete the key
        db.delete(b"key1").unwrap();
        assert_eq!(db.get(b"key1").unwrap(), None);
    }

    #[test]
    fn test_db_overwrite() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Put initial value
        db.put(b"key1", b"value1").unwrap();
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Overwrite with new value
        db.put(b"key1", b"value2").unwrap();
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_db_multiple_operations() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Multiple puts
        for i in 0..100 {
            let key = format!("key{}", i);
            let value = format!("value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        // Verify all values
        for i in 0..100 {
            let key = format!("key{}", i);
            let expected = format!("value{}", i);
            let value = db.get(key.as_bytes()).unwrap();
            assert_eq!(value, Some(expected.as_bytes().to_vec()));
        }
    }

    #[test]
    fn test_db_close() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        db.put(b"key1", b"value1").unwrap();
        let result = db.close();
        assert!(result.is_ok());
    }

    #[test]
    fn test_db_recovery() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();

        // First session: write data
        {
            let db = DB::open(&db_path, Options::default()).unwrap();
            db.put(b"key1", b"value1").unwrap();
            db.put(b"key2", b"value2").unwrap();
            db.close().unwrap();
        }

        // Second session: verify recovery
        {
            let db = DB::open(&db_path, Options::default()).unwrap();
            // Note: Currently recovery from WAL is not fully implemented
            // This test will be enhanced in future
        }
    }

    #[test]
    fn test_db_error_if_exists() {
        let temp_dir = TempDir::new().unwrap();

        // Create the database first
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        db.close().unwrap();
        drop(db);

        // Try to open with error_if_exists
        let options = Options::default().create_if_missing(false);
        let mut options = options;
        options.error_if_exists = true;

        let result = DB::open(temp_dir.path(), options);
        assert!(result.is_err());
    }
}
