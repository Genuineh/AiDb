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
pub mod backup;
pub mod cache;
pub mod compaction;
pub mod config;
pub mod error;
pub mod filter;
pub mod iterator;
pub mod memtable;
pub mod snapshot;
pub mod sstable;
pub mod wal;
pub mod write_batch;

// Cluster module (optional, enabled with "cluster" feature)
#[cfg(feature = "cluster")]
pub mod cluster;

// Monitoring module (optional, enabled with "monitoring" feature)
#[cfg(feature = "monitoring")]
pub mod monitoring;

// Re-exports
pub use config::Options;
pub use error::{Error, Result};
pub use iterator::DBIterator;
pub use snapshot::Snapshot;
pub use write_batch::WriteBatch;

use cache::BlockCache;
use compaction::{CompactionJob, CompactionPicker, VersionEdit, VersionSet};
use memtable::MemTable;
use parking_lot::{Mutex, RwLock};
use sstable::{SSTableBuilder, SSTableReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

    /// File number generator for SSTables and WAL
    next_file_number: Arc<AtomicU64>,

    /// Current WAL file number
    wal_file_number: Arc<AtomicU64>,

    /// Version set for managing SSTable metadata
    version_set: Arc<RwLock<VersionSet>>,

    /// Compaction picker
    compaction_picker: Arc<CompactionPicker>,

    /// Block cache for SSTable data blocks
    block_cache: Arc<BlockCache>,

    /// Serializes flush work so manual and background flush do not race.
    flush_lock: Mutex<()>,

    /// Set to true when the DB is being dropped to signal the background flush thread to exit.
    flush_thread_shutdown: Arc<AtomicBool>,

    /// Global key count - maintains exact count of visible keys across all layers.
    /// This is O(1) for dbsize() operation.
    total_key_count: Arc<AtomicUsize>,
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
    pub fn open<P: AsRef<std::path::Path>>(path: P, options: Options) -> Result<Arc<Self>> {
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
            return Err(Error::AlreadyExists(format!("Database already exists: {:?}", path)));
        }

        // Step 2: Initialize sequence number
        let mut sequence = 0u64;

        // Step 3: Find all WAL files in the directory for recovery
        let mut wal_numbers: Vec<u64> = Vec::new();

        if path.exists() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    if let Some(filename) = entry.file_name().to_str() {
                        if let Some(num) = wal::parse_wal_filename(filename) {
                            wal_numbers.push(num);
                        }
                    }
                }
            }
        }

        wal_numbers.sort();

        // Step 4: Recover from all WAL files.
        // WAL files accumulate across rotations — we must recover from ALL of them
        // in order because the old WAL (with earlier entries) may contain data from
        // the active MemTable that was never flushed to SSTable.
        let latest_wal_number = wal_numbers.last().copied().unwrap_or(1);
        let latest_wal_path = path.join(wal::wal_filename(latest_wal_number));

        let memtable = MemTable::new(sequence + 1);

        for &wal_num in &wal_numbers {
            let wal_path = path.join(wal::wal_filename(wal_num));
            if !wal_path.exists() {
                continue;
            }

            let recovered = match WAL::recover(&wal_path) {
                Ok(entries) => entries,
                Err(e) => {
                    log::warn!("Skipping corrupted WAL {:?}: {}", wal_path, e);
                    continue;
                }
            };

            if recovered.is_empty() {
                continue;
            }

            log::info!("Recovering {} entries from WAL {:?}", recovered.len(), wal_path);

            for entry in recovered {
                sequence += 1;

                // Parse WAL entry format
                if entry.starts_with(b"put:") {
                    // Format: "put:key_len:key:value"
                    let entry = &entry[4..]; // Skip "put:"

                    // Read key length
                    if entry.len() < 4 {
                        log::warn!("Invalid WAL entry: too short");
                        continue;
                    }

                    let key_len = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
                    let entry = &entry[4..]; // Skip key_len

                    if entry.is_empty() || entry[0] != b':' {
                        log::warn!("Invalid WAL entry: missing separator");
                        continue;
                    }

                    let entry = &entry[1..]; // Skip ':'

                    if entry.len() < key_len + 1 {
                        log::warn!("Invalid WAL entry: key too short");
                        continue;
                    }

                    let key = &entry[..key_len];
                    let entry = &entry[key_len..];

                    if entry.is_empty() || entry[0] != b':' {
                        log::warn!("Invalid WAL entry: missing value separator");
                        continue;
                    }

                    let value = &entry[1..];

                    // Insert into memtable
                    memtable.put(key, value, sequence);
                } else if entry.starts_with(b"del:") {
                    // Format: "del:key_len:key"
                    let entry = &entry[4..]; // Skip "del:"

                    if entry.len() < 4 {
                        log::warn!("Invalid WAL entry: too short");
                        continue;
                    }

                    let key_len = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
                    let entry = &entry[4..]; // Skip key_len

                    if entry.is_empty() || entry[0] != b':' {
                        log::warn!("Invalid WAL entry: missing separator");
                        continue;
                    }

                    let entry = &entry[1..]; // Skip ':'

                    if entry.len() < key_len {
                        log::warn!("Invalid WAL entry: key too short");
                        continue;
                    }

                    let key = &entry[..key_len];

                    // Insert tombstone into memtable
                    memtable.delete(key, sequence);
                } else {
                    log::warn!("Unknown WAL entry type");
                }
            }
        }

        // Open the latest WAL as the active WAL for subsequent writes.
        let wal = WAL::open(&latest_wal_path)?;

        // Step 6: Load existing SSTables
        let mut sstables: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); options.max_levels];

        // Step 6a: Create block cache (needed before loading SSTables)
        let block_cache = Arc::new(BlockCache::new(options.block_cache_size));

        // Scan directory for SSTable files (*.sst) and assign to correct levels
        if path.exists() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                let mut sst_files: Vec<(u64, usize, std::path::PathBuf)> = Vec::new();

                for entry in entries.flatten() {
                    if let Some(filename) = entry.file_name().to_str() {
                        if let Some((num, level)) = crate::sstable::parse_sstable_filename(filename) {
                            sst_files.push((num, level, entry.path()));
                        }
                    }
                }

                sst_files.sort_by_key(|&(num, _, _)| num);

                for (_, level, sst_path) in sst_files {
                    match SSTableReader::open_with_cache(&sst_path, Some(Arc::clone(&block_cache)))
                    {
                        Ok(reader) => {
                            if level < sstables.len() {
                                sstables[level].push(Arc::new(reader));
                            } else {
                                sstables[0].push(Arc::new(reader));
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to load SSTable {:?}: {}", sst_path, e);
                        }
                    }
                }

                for (i, level_ssts) in sstables.iter().enumerate() {
                    if !level_ssts.is_empty() {
                        log::info!("Loaded {} SSTables at Level {}", level_ssts.len(), i);
                    }
                }
            }
        }

        // Step 7: Initialize VersionSet
        let version_set = VersionSet::new(&path, options.max_levels)?;

        // Step 8: Initialize CompactionPicker
        let compaction_picker = CompactionPicker::new(options.max_levels);

        // Step 9: Calculate initial key count from MemTable after recovery
        // MemTable now contains all WAL entries applied, so its unique_key_count is accurate
        let initial_key_count = memtable.approximate_unique_key_count();

        // Step 10: Construct DB instance
        let flush_thread_shutdown = Arc::new(AtomicBool::new(false));
        let total_key_count = Arc::new(AtomicUsize::new(initial_key_count));

        let db = Arc::new(DB {
            path,
            options,
            memtable: Arc::new(RwLock::new(memtable)),
            immutable_memtables: Arc::new(RwLock::new(Vec::new())),
            wal: Arc::new(RwLock::new(wal)),
            sstables: Arc::new(RwLock::new(sstables)),
            sequence: Arc::new(AtomicU64::new(sequence)),
            next_file_number: Arc::new(AtomicU64::new(2)), // Start from 2 (1 is for WAL)
            wal_file_number: Arc::new(AtomicU64::new(latest_wal_number)),
            version_set: Arc::new(RwLock::new(version_set)),
            compaction_picker: Arc::new(compaction_picker),
            block_cache,
            flush_lock: Mutex::new(()),
            flush_thread_shutdown: Arc::clone(&flush_thread_shutdown),
            total_key_count: Arc::clone(&total_key_count),
        });

        // Step 10: Spawn background flush thread.
        //
        // This thread periodically flushes immutable MemTables to SSTable files.
        // Without this, immutable MemTables accumulate in memory indefinitely when
        // callers never invoke `flush()` explicitly.
        //
        // The thread holds a Weak reference so it exits automatically when the last
        // Arc<DB> is dropped (Weak::upgrade() returns None).
        let db_weak = Arc::downgrade(&db);
        let shutdown_flag = Arc::clone(&flush_thread_shutdown);
        std::thread::Builder::new()
            .name("aidb-flush".to_string())
            .spawn(move || {
                log::info!("Background flush thread started");
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));

                    if shutdown_flag.load(Ordering::Acquire) {
                        log::info!("Background flush thread: shutdown signal received, exiting");
                        break;
                    }

                    let db = match db_weak.upgrade() {
                        Some(db) => db,
                        None => {
                            log::info!("Background flush thread: DB dropped, exiting");
                            break;
                        }
                    };

                    if db.immutable_memtables.read().is_empty() {
                        continue;
                    }

                    if let Err(e) = db.flush_pending() {
                        log::error!("Background flush error: {}", e);
                    }
                }
                log::info!("Background flush thread exited");
            })
            .expect("Failed to spawn aidb-flush thread");

        Ok(db)
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

        // Step 1b: Check if this is a new key (for accurate key counting)
        // Uses bloom filter in SSTables for fast negative lookups - if bloom filter says
        // "definitely not present", we skip the SSTable lookup entirely.
        let is_new_key = !self.key_exists(key);

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

        // Step 3b: Increment key count if this is a new key
        // Use fetch_update with saturating_add to prevent overflow and ensure atomicity
        if is_new_key {
            let _ = self.total_key_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                Some(count.saturating_add(1))
            });
        }

        // Step 4: Check if MemTable is full and needs flushing
        let memtable_size = {
            let memtable = self.memtable.read();
            memtable.approximate_size()
        };

        if memtable_size >= self.options.memtable_size {
            log::info!(
                "MemTable is full ({} bytes >= {}), triggering freeze",
                memtable_size,
                self.options.memtable_size
            );
            // Freeze the current MemTable
            // The actual flush will happen in the background or on next flush() call
            self.freeze_memtable_if_full()?;
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
                // Empty value means tombstone (deleted)
                if value.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(value));
            }
        }

        // Step 2: Check Immutable MemTables (newest to oldest)
        {
            let immutable = self.immutable_memtables.read();
            for memtable in immutable.iter().rev() {
                if let Some(value) = memtable.get(key, max_seq) {
                    // Empty value means tombstone (deleted)
                    if value.is_empty() {
                        return Ok(None);
                    }
                    return Ok(Some(value));
                }
            }
        }

        // Step 3: Search SSTables from Level 0 to Level N
        {
            let sstables = self.sstables.read();
            for level_tables in sstables.iter() {
                // For Level 0, search all tables (may overlap)
                // Tables are stored newest-first, so iterate forward
                // For other levels, tables don't overlap, so we can binary search
                for table in level_tables.iter() {
                    // Since we store user_key only in SSTables (simplified version),
                    // we can directly search for the key
                    if let Some(value) = table.get(key)? {
                        // Empty value means tombstone (deleted)
                        if value.is_empty() {
                            return Ok(None);
                        }
                        return Ok(Some(value));
                    }
                }
            }
        }

        // Key not found
        Ok(None)
    }

    /// Checks if a key exists in MemTable layers only (fast, no SSTable scan).
    ///
    /// Returns `true` if the key has any entry (value or tombstone) in active or
    /// immutable MemTables. Used for delete key counting where we skip SSTable scan
    /// for performance.
    fn key_exists_in_memtables(&self, key: &[u8]) -> bool {
        // Skip internal expiration metadata keys
        if key.starts_with(b"__exp__:") {
            return false;
        }

        // Check active MemTable
        if self.memtable.read().key_map_contains(key) {
            return true;
        }

        // Check immutable MemTables
        let immutable = self.immutable_memtables.read();
        for memtable in immutable.iter() {
            if memtable.key_map_contains(key) {
                return true;
            }
        }

        false
    }

    /// Checks if a key exists in any layer (MemTable or SSTable).
    ///
    /// NOTE: This function performs expensive SSTable scans and should only be
    /// used for background operations or debugging, NOT in hot paths.
    ///
    /// Returns `true` if the key has any entry (value or tombstone) in any layer.
    #[allow(dead_code)]
    fn key_exists(&self, key: &[u8]) -> bool {
        // Skip internal expiration metadata keys
        if key.starts_with(b"__exp__:") {
            return false;
        }

        // Check MemTable layers first (fast path)
        if self.key_exists_in_memtables(key) {
            return true;
        }

        // Check SSTables (slow path - full scan)
        let sstables = self.sstables.read();
        for level_tables in sstables.iter() {
            for table in level_tables.iter() {
                if table.get(key).ok().flatten().is_some() {
                    return true;
                }
            }
        }

        false
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

        // Step 1b: Check if this key exists in any layer (for accurate key counting)
        // Uses bloom filter in SSTables for fast negative lookups.
        let key_existed = self.key_exists(key);

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

        // Step 3b: Decrement key count if the key existed
        // Use saturating_sub to prevent negative count (can happen if delete is called
        // more times than put for new keys, e.g., due to benchmark bulk operations)
        if key_existed {
            let _ = self.total_key_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                Some(count.saturating_sub(1))
            });
        }

        Ok(())
    }

    /// Creates a snapshot of the database at the current point in time.
    ///
    /// A snapshot provides a consistent, point-in-time view of the database.
    /// All read operations through the snapshot will see data as it existed
    /// at the time the snapshot was created, regardless of subsequent modifications.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// let db = DB::open("./data", Options::default())?;
    /// let db = Arc::new(db);
    ///
    /// db.put(b"key", b"value1")?;
    ///
    /// // Create a snapshot
    /// let snapshot = db.snapshot();
    ///
    /// // Modify the database
    /// db.put(b"key", b"value2")?;
    ///
    /// // Snapshot still sees old value
    /// assert_eq!(snapshot.get(b"key")?, Some(b"value1".to_vec()));
    ///
    /// // Current DB sees new value
    /// assert_eq!(db.get(b"key")?, Some(b"value2".to_vec()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn snapshot(self: &Arc<Self>) -> crate::snapshot::Snapshot {
        let seq = self.sequence.load(Ordering::SeqCst);
        crate::snapshot::Snapshot::new(Arc::clone(self), seq)
    }

    /// Internal method to get a value at a specific sequence number.
    ///
    /// This is used by snapshots to implement point-in-time reads.
    /// Only entries with sequence numbers <= max_seq are visible.
    pub(crate) fn get_at_sequence(&self, key: &[u8], max_seq: u64) -> Result<Option<Vec<u8>>> {
        // Step 1: Check current MemTable
        {
            let memtable = self.memtable.read();
            if let Some(value) = memtable.get(key, max_seq) {
                // Empty value means tombstone (deleted)
                if value.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(value));
            }
        }

        // Step 2: Check Immutable MemTables (newest to oldest)
        {
            let immutable = self.immutable_memtables.read();
            for memtable in immutable.iter().rev() {
                if let Some(value) = memtable.get(key, max_seq) {
                    // Empty value means tombstone (deleted)
                    if value.is_empty() {
                        return Ok(None);
                    }
                    return Ok(Some(value));
                }
            }
        }

        // Step 3: Search SSTables from Level 0 to Level N
        {
            let sstables = self.sstables.read();
            for level_tables in sstables.iter() {
                // For Level 0, search all tables (may overlap)
                // Tables are stored newest-first, so iterate forward
                // For other levels, tables don't overlap, so we can binary search
                for table in level_tables.iter() {
                    // Since we store user_key only in SSTables (simplified version),
                    // we can directly search for the key
                    if let Some(value) = table.get(key)? {
                        // Empty value means tombstone (deleted)
                        if value.is_empty() {
                            return Ok(None);
                        }
                        return Ok(Some(value));
                    }
                }
            }
        }

        // Key not found
        Ok(None)
    }

    /// Applies a batch of write operations atomically.
    ///
    /// All operations in the batch are applied together as a single atomic unit.
    /// All operations will be written to WAL first for durability, then applied to
    /// the MemTable. All operations in a batch share the same base sequence number
    /// for consistency.
    ///
    /// # Durability Guarantees
    ///
    /// - All operations are written to WAL before being applied to MemTable
    /// - A single WAL sync occurs after all batch entries are written
    /// - On recovery, all WAL entries for the batch will be replayed together
    /// - If any operation fails during WAL write, the entire batch fails and no
    ///   operations are applied to MemTable
    ///
    /// # Arguments
    ///
    /// * `batch` - The WriteBatch containing operations to apply
    ///
    /// # Errors
    ///
    /// Returns an error if WAL writing or MemTable operations fail.
    /// If WAL writing fails, no operations are applied to MemTable.
    /// If MemTable operations fail after WAL writing succeeds, the operations
    /// will be recovered from WAL on next database open.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options, WriteBatch};
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = DB::open("./data", Options::default())?;
    /// let mut batch = WriteBatch::new();
    /// batch.put(b"key1", b"value1");
    /// batch.put(b"key2", b"value2");
    /// batch.delete(b"key3");
    ///
    /// // Apply all operations atomically
    /// db.write(batch)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn write(&self, batch: WriteBatch) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        // Allocate sequence numbers for the entire batch upfront
        let batch_size = batch.len() as u64;
        let base_seq = self.sequence.fetch_add(batch_size, Ordering::SeqCst) + 1;

        // Write all operations to WAL first (for durability)
        if self.options.use_wal {
            let mut wal = self.wal.write();

            for op in batch.iter() {
                match op {
                    write_batch::WriteOp::Put { key, value } => {
                        // Encode as: "put:key_len:key:value"
                        let mut entry = Vec::new();
                        entry.extend_from_slice(b"put:");
                        entry.extend_from_slice(&(key.len() as u32).to_le_bytes());
                        entry.extend_from_slice(b":");
                        entry.extend_from_slice(key);
                        entry.extend_from_slice(b":");
                        entry.extend_from_slice(value);
                        wal.append(&entry)?;
                    }
                    write_batch::WriteOp::Delete { key } => {
                        // Encode as: "del:key_len:key"
                        let mut entry = Vec::new();
                        entry.extend_from_slice(b"del:");
                        entry.extend_from_slice(&(key.len() as u32).to_le_bytes());
                        entry.extend_from_slice(b":");
                        entry.extend_from_slice(key);
                        wal.append(&entry)?;
                    }
                }
            }

            if self.options.sync_wal {
                wal.sync()?;
            }
        }

        // Apply all operations to MemTable with consecutive sequence numbers
        {
            let memtable = self.memtable.read();
            let mut seq = base_seq;

            for op in batch.iter() {
                match op {
                    write_batch::WriteOp::Put { key, value } => {
                        memtable.put(key, value, seq);
                    }
                    write_batch::WriteOp::Delete { key } => {
                        memtable.delete(key, seq);
                    }
                }
                seq += 1;
            }
        }

        // Check if MemTable is full and needs flushing
        let memtable_size = {
            let memtable = self.memtable.read();
            memtable.approximate_size()
        };

        if memtable_size >= self.options.memtable_size {
            log::info!(
                "MemTable is full ({} bytes >= {}), triggering freeze after batch write",
                memtable_size,
                self.options.memtable_size
            );
            self.freeze_memtable_if_full()?;
        }

        Ok(())
    }

    /// Freezes the current MemTable and creates a new one.
    ///
    /// This moves the current mutable MemTable to the immutable list
    /// and creates a fresh MemTable for new writes.
    fn freeze_memtable(&self) -> Result<()> {
        let mut memtable = self.memtable.write();
        let mut immutable = self.immutable_memtables.write();

        // Get current sequence number for the new MemTable
        let current_seq = self.sequence.load(Ordering::SeqCst);

        // Move current memtable to immutable list
        let old_memtable = std::mem::replace(&mut *memtable, MemTable::new(current_seq + 1));
        immutable.push(Arc::new(old_memtable));

        log::info!("MemTable frozen, {} immutable memtables waiting for flush", immutable.len());

        Ok(())
    }

    /// Freeze the current MemTable only if it is still full after acquiring the write lock.
    fn freeze_memtable_if_full(&self) -> Result<bool> {
        let mut memtable = self.memtable.write();
        if memtable.approximate_size() < self.options.memtable_size {
            return Ok(false);
        }

        let mut immutable = self.immutable_memtables.write();

        // Get current sequence number for the new MemTable
        let current_seq = self.sequence.load(Ordering::SeqCst);

        // Move current memtable to immutable list
        let old_memtable = std::mem::replace(&mut *memtable, MemTable::new(current_seq + 1));
        immutable.push(Arc::new(old_memtable));

        log::info!(
            "MemTable frozen after full check, {} immutable memtables waiting for flush",
            immutable.len()
        );

        Ok(true)
    }

    /// Flushes an immutable MemTable to an SSTable file.
    ///
    /// This method:
    /// 1. Iterates through all entries in the MemTable
    /// 2. Writes them to an SSTable using SSTableBuilder
    /// 3. Adds the new SSTable to Level 0
    /// 4. Returns the file number of the created SSTable
    fn flush_memtable_to_sstable(&self, memtable: &MemTable) -> Result<u64> {
        // Generate a new file number
        let file_number = self.next_file_number.fetch_add(1, Ordering::SeqCst);

        // Create SSTable file path
        let sstable_path = crate::sstable::sstable_path(&self.path, file_number, 0);

        log::info!("Starting flush of MemTable to SSTable: {:?}", sstable_path);

        // Create SSTable builder
        let mut builder = SSTableBuilder::new(&sstable_path)?;
        builder.set_block_size(self.options.block_size);
        builder.set_compression(self.options.compression);

        // Iterate through MemTable and add entries to SSTable
        // We only keep the latest version of each user key (skip older versions)
        //
        // BUG FIX: The previous implementation assumed entries for the same user_key
        // were adjacent in iteration order. But SkipMap is sorted by InternalKey which
        // includes sequence number, so entries for the same user_key with different
        // sequences are NOT necessarily adjacent. We now use a HashMap to track
        // the latest entry for each user_key.
        use std::collections::HashMap;

        // Structure to hold entry data we need for writing
        #[derive(Clone)]
        struct EntryData {
            user_key: Vec<u8>,
            sequence: u64,
            value: Vec<u8>,
            value_type: memtable::ValueType,
        }

        // First pass: collect all entries and find the latest for each user_key
        let mut latest_entries: HashMap<Vec<u8>, EntryData> = HashMap::new();
        for entry in memtable.iter() {
            let user_key = entry.user_key().to_vec();
            let sequence = entry.key().sequence();
            let value_type = entry.key().value_type();
            let value = entry.value().to_vec();

            match latest_entries.get(&user_key) {
                Some(existing) => {
                    if sequence > existing.sequence {
                        latest_entries.insert(user_key.clone(), EntryData {
                            user_key,
                            sequence,
                            value,
                            value_type,
                        });
                    }
                }
                None => {
                    latest_entries.insert(user_key.clone(), EntryData {
                        user_key,
                        sequence,
                        value,
                        value_type,
                    });
                }
            }
        }

        // Second pass: write latest entries to SSTable in sorted order by user_key
        let mut entry_count = 0;
        let mut entries: Vec<_> = latest_entries.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (_, entry_data) in entries {
            // For SSTable at Level 0, we store both values and tombstones
            // Tombstones are represented as empty Vec, actual values are non-empty
            // Tombstones will be removed during compaction
            let value: &[u8] = if entry_data.value_type == crate::memtable::ValueType::Deletion {
                // Write empty value for tombstones
                &[]
            } else {
                &entry_data.value
            };

            if let Err(err) = builder.add(&entry_data.user_key, value) {
                log::error!(
                    "Flush write to SSTable failed for {:?}: user_key={:?}, value_type={:?}, entry_count={}",
                    sstable_path,
                    String::from_utf8_lossy(&entry_data.user_key),
                    entry_data.value_type,
                    entry_count
                );
                return Err(err);
            }
            entry_count += 1;
        }

        // Check if we have any entries to flush
        if entry_count == 0 {
            // No entries to flush - abandon the builder and clean up
            log::info!(
                "MemTable contains no entries to flush (only tombstones or duplicates), skipping SSTable creation"
            );

            // Abandon the builder (don't write footer)
            builder.abandon()?;

            // Remove the incomplete SSTable file
            if sstable_path.exists() {
                std::fs::remove_file(&sstable_path)?;
            }

            // Return a special value to indicate no file was created
            // (we still consumed the file number, which is fine)
            return Ok(0);
        }

        // Finish building the SSTable
        let file_size = builder.finish()?;

        log::info!(
            "Flush completed: {} entries written, file size: {} bytes",
            entry_count,
            file_size
        );

        // Open the SSTable for reading with block cache
        let reader = Arc::new(SSTableReader::open_with_cache(
            &sstable_path,
            Some(Arc::clone(&self.block_cache)),
        )?);

        // Add to Level 0 at the front (newest files first)
        {
            let mut sstables = self.sstables.write();
            sstables[0].insert(0, reader);
        }

        Ok(file_number)
    }

    /// Flushes immutable MemTables in FIFO order without removing them first.
    ///
    /// Callers must hold `flush_lock` so the queue cannot be processed concurrently.
    fn flush_immutable_memtables(&self) -> Result<u32> {
        let mut flushed = 0u32;

        loop {
            let memtable_to_flush = {
                let immutable = self.immutable_memtables.read();
                immutable.first().cloned()
            };

            let Some(memtable_to_flush) = memtable_to_flush else {
                break;
            };

            self.flush_memtable_to_sstable(&memtable_to_flush)?;

            let mut immutable = self.immutable_memtables.write();
            if let Some(pos) = immutable
                .iter()
                .position(|memtable| Arc::ptr_eq(memtable, &memtable_to_flush))
            {
                immutable.remove(pos);
                flushed += 1;
            } else {
                log::warn!("Flushed immutable MemTable was already removed from the queue");
            }
        }

        Ok(flushed)
    }

    /// Manually triggers a flush of the current MemTable.
    ///
    /// This will freeze the current MemTable and flush all immutable MemTables
    /// to SSTable files.
    ///
    /// # Errors
    ///
    /// Returns an error if the flush fails.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = DB::open("./data", Options::default())?;
    /// db.put(b"key", b"value")?;
    /// db.flush()?; // Manually flush to disk
    /// # Ok(())
    /// # }
    /// ```
    pub fn flush(&self) -> Result<()> {
        // Step 1: Freeze the current MemTable if it's not empty
        {
            let memtable = self.memtable.read();
            if !memtable.is_empty() {
                drop(memtable); // Release read lock before freeze
                self.freeze_memtable()?;
            }
        }

        let _flush_guard = self.flush_lock.lock();

        // Step 2: Flush all immutable MemTables
        self.flush_immutable_memtables()?;

        // Step 3: Rotate WAL after successful flush
        self.rotate_wal()?;

        // Step 4: Check if compaction is needed
        self.maybe_trigger_compaction()?;

        Ok(())
    }

    /// Flush only the pending immutable MemTables without freezing the current (mutable) MemTable.
    ///
    /// This is called by the background flush thread. Unlike `flush()`, it does NOT
    /// freeze the currently active MemTable - that happens automatically inside `put()`
    /// when the MemTable reaches its size limit. This separation ensures writes are
    /// never blocked by the background flush.
    fn flush_pending(&self) -> Result<()> {
        let _flush_guard = self.flush_lock.lock();
        let flushed = self.flush_immutable_memtables()?;

        if flushed > 0 {
            log::info!("Background flush: {} MemTable(s) flushed to SSTable", flushed);

            // Rotate WAL after flushing immutable MemTables to SSTable.
            // The flushed entries are now durable in SSTable, so their WAL entries
            // are redundant and can be safely removed.
            // New writes will go to the new WAL file.
            self.rotate_wal()?;

            self.maybe_trigger_compaction()?;
        }

        Ok(())
    }

    /// Rotates the WAL file.
    ///
    /// This creates a new WAL file and removes the old one after a successful flush.
    fn rotate_wal(&self) -> Result<()> {
        let new_wal_number = self.wal_file_number.fetch_add(1, Ordering::SeqCst) + 1;
        let new_wal_path = self.path.join(wal::wal_filename(new_wal_number));

        log::info!("Rotating WAL to {:?}", new_wal_path);

        // Create new WAL
        let new_wal = WAL::open(&new_wal_path)?;

        // Replace the old WAL
        let old_wal = {
            let mut wal = self.wal.write();
            std::mem::replace(&mut *wal, new_wal)
        };

        // Close the old WAL (flushes buffered data to disk) but do NOT delete it.
        // The old WAL is kept for crash recovery since it may contain entries from
        // the active MemTable that haven't been flushed to SSTable yet. On next
        // DB::open(), ALL WAL files are scanned and recovered in order.
        let old_path = old_wal.path().to_path_buf();
        drop(old_wal);

        log::info!("Old WAL preserved for recovery: {:?}", old_path);

        Ok(())
    }

    /// Check if compaction is needed and trigger it if necessary
    ///
    /// This is called after flush to check if any level needs compaction
    pub fn maybe_trigger_compaction(&self) -> Result<()> {
        let sstables = self.sstables.read();

        // Check if compaction is needed
        let task = {
            let task = self.compaction_picker.pick_compaction(&sstables);
            match task {
                Some(t) => t,
                None => {
                    log::debug!("No compaction needed");
                    return Ok(());
                }
            }
        };

        // Drop the read lock before compaction
        drop(sstables);

        log::info!(
            "Triggering compaction: level {} -> level {}, {} input files",
            task.level,
            task.output_level,
            task.inputs.len()
        );

        // Execute compaction
        self.compact(task)?;

        Ok(())
    }

    /// Execute a compaction task
    fn compact(&self, task: compaction::CompactionTask) -> Result<()> {
        // Allocate file number for output SSTable
        let file_number = self.next_file_number.fetch_add(1, Ordering::SeqCst);

        // Create compaction job
        let job = CompactionJob::new(
            task.inputs.clone(),
            task.output_level,
            self.path.clone(),
            self.options.block_size,
        );

        // Run compaction
        let result = job.run(file_number)?;

        // If no file was created, nothing to update
        if result.file_number == 0 {
            log::info!("Compaction produced no output (all tombstones or duplicates)");
            return Ok(());
        }

        // Open the new SSTable reader once and reuse it (fixes duplicate Arc bug)
        let new_reader = Arc::new(SSTableReader::open_with_cache(
            &result.output_path,
            Some(Arc::clone(&self.block_cache)),
        )?);

        // Get metadata from the new reader
        let smallest_key = new_reader
            .smallest_key()?
            .ok_or_else(|| Error::internal("New SSTable has no keys"))?;
        let largest_key = new_reader
            .largest_key()?
            .ok_or_else(|| Error::internal("New SSTable has no keys"))?;

        // Collect input file numbers and paths using reliable file_number() method
        // This fixes the unreliable file-size matching bug
        // We fail fast if any file has an invalid filename to prevent state inconsistencies
        let mut input_file_info: Vec<(u64, std::path::PathBuf)> = Vec::new();
        for input in &task.inputs {
            let file_num = input.file_number().ok_or_else(|| {
                Error::internal(format!(
                    "Input SSTable has invalid filename: {:?}",
                    input.file_path()
                ))
            })?;
            let file_path = input.file_path().to_path_buf();
            input_file_info.push((file_num, file_path));
        }

        // Update both version set and in-memory SSTable list atomically
        // This fixes the desynchronized state bug
        {
            // Acquire both locks to ensure atomic update
            let mut version_set = self.version_set.write();
            let mut sstables = self.sstables.write();

            // Add new file to version set
            let add_edit = VersionEdit::AddFile {
                level: task.output_level,
                file_number: result.file_number,
                file_size: new_reader.file_size(),
                smallest_key,
                largest_key,
            };
            version_set.log_edit(&add_edit)?;

            // Delete input files from version set
            for (file_num, _) in &input_file_info {
                let delete_edit =
                    VersionEdit::DeleteFile { level: task.level, file_number: *file_num };
                version_set.log_edit(&delete_edit)?;
            }

            // Update in-memory SSTable list BEFORE physical deletion
            // This fixes the race condition bug where Arc::ptr_eq could fail

            // Remove input files from source level using Arc::ptr_eq
            sstables[task.level]
                .retain(|reader| !task.inputs.iter().any(|input| Arc::ptr_eq(reader, input)));

            // Add new file to output level (reuse the same Arc instance)
            // For Level 0, insert at front (newest first), for other levels, append
            if task.output_level == 0 {
                sstables[task.output_level].insert(0, Arc::clone(&new_reader));
            } else {
                sstables[task.output_level].push(Arc::clone(&new_reader));
            }
        }
        // Locks are released here

        // Now delete physical files AFTER updating in-memory structures
        // This ensures consistency if deletion fails
        for (_file_num, file_path) in input_file_info {
            if file_path.exists() {
                std::fs::remove_file(&file_path)?;
                log::info!("Deleted compacted file: {:?}", file_path);
            }
        }

        log::info!(
            "Compaction completed: wrote {} entries to level {}",
            result.entry_count,
            task.output_level
        );

        Ok(())
    }

    /// Closes the database, ensuring all data is flushed to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if flushing fails.
    pub fn close(&self) -> Result<()> {
        // Step 1: Flush all data to disk
        self.flush()?;

        // Step 2: Sync WAL to ensure all writes are persisted
        if self.options.use_wal {
            let mut wal = self.wal.write();
            wal.sync()?;
        }

        // Step 3: Clean up old WAL files. Since flush() above ensured all data is
        // in SSTables, any pre-rotation WAL files can be safely removed.
        self.cleanup_old_wals()?;

        log::info!("Database closed successfully");

        Ok(())
    }

    /// Remove old WAL files that are no longer needed.
    ///
    /// Only the latest WAL file is kept. All older WAL files are safe to delete
    /// because their entries have been fully flushed to SSTables.
    fn cleanup_old_wals(&self) -> Result<()> {
        let current_wal = self.wal_file_number.load(Ordering::SeqCst);

        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if let Some(num) = wal::parse_wal_filename(filename) {
                        if num < current_wal {
                            let path = entry.path();
                            if let Err(e) = std::fs::remove_file(&path) {
                                log::warn!("Failed to remove old WAL {:?}: {}", path, e);
                            } else {
                                log::info!("Removed old WAL file: {:?}", path);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get block cache statistics.
    ///
    /// Returns statistics about cache hits, misses, and evictions.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// let db = DB::open("./data", Options::default())?;
    ///
    /// // Perform some operations
    /// db.put(b"key1", b"value1")?;
    /// db.get(b"key1")?;
    ///
    /// // Check cache statistics
    /// let stats = db.cache_stats();
    /// println!("Cache hit rate: {:.2}%", stats.hit_rate() * 100.0);
    /// println!("Total lookups: {}", stats.lookups);
    /// println!("Hits: {}, Misses: {}", stats.hits, stats.misses);
    /// # Ok(())
    /// # }
    /// ```
    pub fn cache_stats(&self) -> cache::CacheStats {
        self.block_cache.stats()
    }

    /// Clear the block cache.
    ///
    /// This removes all cached blocks, which may temporarily reduce read performance
    /// but can be useful for benchmarking or memory management.
    pub fn clear_cache(&self) {
        self.block_cache.clear();
    }

    /// Reset cache statistics.
    ///
    /// Resets hits, misses, and other cache statistics to zero while preserving
    /// cached data.
    pub fn reset_cache_stats(&self) {
        self.block_cache.reset_stats();
    }

    // Backup-related helper methods

    /// Get the database path.
    pub fn get_path(&self) -> &std::path::Path {
        &self.path
    }

    /// Get the current sequence number.
    pub fn get_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    /// List all SSTable files in the database.
    pub fn list_sstable_files(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();
        let sstables = self.sstables.read();

        for level_tables in sstables.iter() {
            for table in level_tables.iter() {
                if let Some(filename) = table.file_path().file_name() {
                    if let Some(name) = filename.to_str() {
                        files.push(name.to_string());
                    }
                }
            }
        }

        Ok(files)
    }

    /// List all WAL files in the database.
    pub fn list_wal_files(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if wal::parse_wal_filename(filename).is_some() {
                        files.push(filename.to_string());
                    }
                }
            }
        }

        Ok(files)
    }

    /// Get the approximate size of the current MemTable in bytes.
    ///
    /// This includes the size of all entries in the active (mutable) MemTable.
    /// Immutable MemTables are not included.
    pub fn memtable_size(&self) -> u64 {
        self.memtable.read().approximate_size() as u64
    }

    /// Get the total size of all MemTables (active + immutable) in bytes.
    pub fn total_memtable_size(&self) -> u64 {
        let mut total = self.memtable.read().approximate_size() as u64;
        for imm in self.immutable_memtables.read().iter() {
            total += imm.approximate_size() as u64;
        }
        total
    }

    /// Get the current WAL size in bytes.
    ///
    /// This is the size of the WAL file on disk.
    pub fn wal_size(&self) -> u64 {
        self.wal.read().size()
    }

    /// Get the current block cache size in bytes.
    pub fn block_cache_size(&self) -> u64 {
        self.block_cache.size() as u64
    }

    /// Get an estimated number of keys in the database.
    ///
    /// This uses the sequence number as an upper bound estimate.
    /// The sequence number counts all put/delete operations, so this is an
    /// over-estimate when keys are updated multiple times. However, it provides
    /// a fast O(1) estimate without scanning the database.
    ///
    /// For exact key count, use the `iter()` method which performs a full scan.
    pub fn estimated_dbsize(&self) -> usize {
        self.sequence.load(Ordering::SeqCst) as usize
    }

    /// Get an estimated number of entries in all MemTables (active + immutable).
    ///
    /// This is a faster estimate than the sequence number because it only counts
    /// entries currently in memory, not historical operations.
    pub fn estimated_memtable_entries(&self) -> usize {
        let mut total = self.memtable.read().len();
        for imm in self.immutable_memtables.read().iter() {
            total += imm.len();
        }
        total
    }

    /// Get an estimated number of unique keys in all MemTables (active + immutable).
    ///
    /// This uses the incrementally maintained unique key count from each MemTable,
    /// providing O(1) lookup instead of scanning all entries.
    ///
    /// Note: This only counts unique keys in MemTables, not in SSTables.
    /// The SSTable key count is estimated separately.
    pub fn estimated_memtable_unique_keys(&self) -> usize {
        let mut total = self.memtable.read().approximate_unique_key_count();
        for imm in self.immutable_memtables.read().iter() {
            total += imm.approximate_unique_key_count();
        }
        total
    }

    /// Get the exact number of keys in the database.
    ///
    /// This returns the exact key count maintained by the database.
    /// This is O(1) operation because the count is maintained incrementally.
    ///
    /// The count is updated on every put/delete operation:
    /// - put with new key: count + 1
    /// - delete existing key: count - 1
    ///
    /// Note: The count uses saturating arithmetic to prevent underflow.
    pub fn dbsize(&self) -> usize {
        let count = self.total_key_count.load(Ordering::SeqCst);
        log::info!("dbsize called, returning: {}", count);
        count
    }

    /// Reset the key count to zero.
    ///
    /// This should be called when flushing the database to ensure accurate counting.
    pub fn reset_key_count(&self) {
        let old = self.total_key_count.load(Ordering::SeqCst);
        self.total_key_count.store(0, Ordering::SeqCst);
        log::info!("reset key_count from {} to 0", old);
    }

    /// Clear all SSTables and immutable MemTables.
    ///
    /// This should be called when flushing the database to ensure all data is also cleared.
    /// SSTable files will be deleted from disk and all in-memory data structures will be cleared.
    pub fn clear_all_data(&self) -> Result<()> {
        // Clear immutable MemTables
        {
            let mut immutable = self.immutable_memtables.write();
            immutable.clear();
            log::info!("Cleared immutable MemTables");
        }

        // Clear current MemTable by replacing with a fresh one
        {
            let mut memtable = self.memtable.write();
            let current_seq = self.sequence.load(Ordering::SeqCst);
            *memtable = MemTable::new(current_seq + 1);
            log::info!("Reset current MemTable");
        }

        let mut sstables = self.sstables.write();

        // Delete all SSTable files from disk and clear memory references
        for level_tables in sstables.iter_mut() {
            for table in level_tables.iter() {
                let path = table.file_path().to_path_buf();
                // Delete the file from disk
                if path.exists() {
                    std::fs::remove_file(&path)?;
                    log::info!("Deleted SSTable file: {:?}", path);
                }
            }
            // Clear this level
            level_tables.clear();
        }

        // Delete all WAL files to ensure complete data wipe
        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.ends_with(".log") {
                        let wal_path = self.path.join(filename);
                        std::fs::remove_file(&wal_path)?;
                        log::info!("Deleted WAL file: {:?}", wal_path);
                    }
                }
            }
        }

        log::info!("Cleared all SSTables, MemTables and WAL files");
        Ok(())
    }

    /// Get the exact number of keys in the database.
    ///
    /// This performs a full scan of MemTables and SSTables to count unique keys.
    /// This is O(n) in the number of keys and should not be called frequently.
    ///
    /// Note: This counts keys visible at the current sequence number (snapshot).
    /// Deleted keys (tombstones) are not counted.
    pub fn count_keys(self: &Arc<Self>) -> Result<usize> {
        let seq = self.sequence.load(Ordering::SeqCst);
        let mut iter = DBIterator::new(Arc::clone(self), seq)?;
        let mut count = 0;

        while iter.valid() {
            count += 1;
            iter.next();
        }

        Ok(count)
    }

    /// Get the block cache capacity in bytes.
    pub fn block_cache_capacity(&self) -> u64 {
        self.block_cache.capacity() as u64
    }
}

impl Drop for DB {
    fn drop(&mut self) {
        // Signal the background flush thread to stop.
        // It will exit on its next iteration when Weak::upgrade() returns None
        // (strong count is 0 at this point) or the shutdown flag is set.
        self.flush_thread_shutdown.store(true, Ordering::Release);

        // Attempt to flush and close cleanly
        // Ignore errors during drop as we can't propagate them
        if let Err(e) = self.flush() {
            log::error!("event=db_drop_flush_failed error={}", e);
        }

        if self.options.use_wal {
            let mut wal = self.wal.write();
            if let Err(e) = wal.sync() {
                log::error!("event=db_drop_wal_sync_failed error={}", e);
            }
        }
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
            let _db = DB::open(&db_path, Options::default()).unwrap();
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

    // ===== Flush Tests =====

    #[test]
    fn test_manual_flush() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write some data
        for i in 0..100 {
            let key = format!("key{}", i);
            let value = format!("value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        // Manually flush
        db.flush().unwrap();

        // Verify data is still accessible
        for i in 0..100 {
            let key = format!("key{}", i);
            let expected = format!("value{}", i);
            let value = db.get(key.as_bytes()).unwrap();
            assert_eq!(value, Some(expected.as_bytes().to_vec()));
        }

        // Check that SSTable was created
        let sstables = db.sstables.read();
        assert!(!sstables[0].is_empty(), "Level 0 should have SSTables after flush");
    }

    #[test]
    fn test_auto_flush_on_memtable_full() {
        let temp_dir = TempDir::new().unwrap();

        // Use a small memtable size to trigger auto-flush
        let options = Options::default().memtable_size(1024); // 1KB
        let db = DB::open(temp_dir.path(), options).unwrap();

        // Write enough data to exceed memtable size
        for i in 0..200 {
            let key = format!("key{:08}", i);
            let value = vec![b'x'; 100]; // 100 bytes value
            db.put(key.as_bytes(), &value).unwrap();
        }

        // Check that immutable memtables were created
        let immutable = db.immutable_memtables.read();
        assert!(!immutable.is_empty(), "Should have frozen memtables");
    }

    #[test]
    fn test_flush_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();

        // First session: write and flush
        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            for i in 0..50 {
                let key = format!("persist_key{}", i);
                let value = format!("persist_value{}", i);
                db.put(key.as_bytes(), value.as_bytes()).unwrap();
            }

            db.flush().unwrap();
            db.close().unwrap();
        }

        // Second session: verify data from SSTables
        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            for i in 0..50 {
                let key = format!("persist_key{}", i);
                let expected = format!("persist_value{}", i);
                let value = db.get(key.as_bytes()).unwrap();
                assert_eq!(
                    value,
                    Some(expected.as_bytes().to_vec()),
                    "Data should persist after flush and reopen"
                );
            }
        }
    }

    #[test]
    fn test_flush_with_deletes() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write and delete some keys
        for i in 0..100 {
            let key = format!("key{}", i);
            let value = format!("value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        // Delete every other key
        for i in (0..100).step_by(2) {
            let key = format!("key{}", i);
            db.delete(key.as_bytes()).unwrap();
        }

        // Flush
        db.flush().unwrap();

        // Verify deleted keys are gone
        for i in 0..100 {
            let key = format!("key{}", i);
            let value = db.get(key.as_bytes()).unwrap();

            if i % 2 == 0 {
                assert_eq!(value, None, "Deleted keys should not be found");
            } else {
                let expected = format!("value{}", i);
                assert_eq!(value, Some(expected.as_bytes().to_vec()));
            }
        }
    }

    #[test]
    fn test_flush_empty_memtable() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Flush without any data
        let result = db.flush();
        assert!(result.is_ok(), "Flushing empty memtable should succeed");

        // Verify no SSTables were created
        let sstables = db.sstables.read();
        assert!(sstables[0].is_empty(), "No SSTables should be created for empty memtable");
    }

    #[test]
    fn test_multiple_flushes() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // First batch
        for i in 0..50 {
            let key = format!("batch1_key{}", i);
            let value = format!("batch1_value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        // Second batch
        for i in 0..50 {
            let key = format!("batch2_key{}", i);
            let value = format!("batch2_value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        // Third batch
        for i in 0..50 {
            let key = format!("batch3_key{}", i);
            let value = format!("batch3_value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        // Verify all SSTables exist
        let sstables = db.sstables.read();
        assert_eq!(sstables[0].len(), 3, "Should have 3 SSTables at Level 0");

        // Verify all data is accessible
        for i in 0..50 {
            let key1 = format!("batch1_key{}", i);
            let key2 = format!("batch2_key{}", i);
            let key3 = format!("batch3_key{}", i);

            assert!(db.get(key1.as_bytes()).unwrap().is_some());
            assert!(db.get(key2.as_bytes()).unwrap().is_some());
            assert!(db.get(key3.as_bytes()).unwrap().is_some());
        }
    }

    #[test]
    fn test_close_triggers_flush() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();

        // Write data and close (should auto-flush)
        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            for i in 0..100 {
                let key = format!("key{}", i);
                let value = format!("value{}", i);
                db.put(key.as_bytes(), value.as_bytes()).unwrap();
            }

            db.close().unwrap(); // Should trigger flush
        }

        // Reopen and verify data
        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            for i in 0..100 {
                let key = format!("key{}", i);
                let expected = format!("value{}", i);
                let value = db.get(key.as_bytes()).unwrap();
                assert_eq!(
                    value,
                    Some(expected.as_bytes().to_vec()),
                    "Data should be persisted after close"
                );
            }
        }
    }

    #[test]
    fn test_concurrent_writes_during_freeze() {
        use std::sync::Arc;
        use std::thread;

        let temp_dir = TempDir::new().unwrap();
        let options = Options::default().memtable_size(1024); // Small memtable
        let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

        let mut handles = vec![];

        // Spawn multiple writer threads
        for thread_id in 0..5 {
            let db_clone = db.clone();
            let handle = thread::spawn(move || {
                for i in 0..50 {
                    let key = format!("thread{}_key{}", thread_id, i);
                    let value = vec![b'x'; 50];
                    db_clone.put(key.as_bytes(), &value).unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Flush and verify
        db.flush().unwrap();

        for thread_id in 0..5 {
            for i in 0..50 {
                let key = format!("thread{}_key{}", thread_id, i);
                let value = db.get(key.as_bytes()).unwrap();
                assert!(value.is_some(), "All concurrent writes should succeed");
            }
        }
    }

    // ===== Bug Fix Tests: Empty SSTable Prevention =====

    #[test]
    fn test_flush_only_tombstones_creates_sstable() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write and then delete keys (only tombstones remain)
        for i in 0..50 {
            let key = format!("key{}", i);
            db.put(key.as_bytes(), b"value").unwrap();
            db.delete(key.as_bytes()).unwrap();
        }

        // Get initial SSTable count
        let initial_sstable_count = {
            let sstables = db.sstables.read();
            sstables[0].len()
        };

        // Flush SHOULD create an SSTable (tombstones are preserved at Level 0)
        db.flush().unwrap();

        // Verify new SSTable was created
        let final_sstable_count = {
            let sstables = db.sstables.read();
            sstables[0].len()
        };

        assert_eq!(
            final_sstable_count,
            initial_sstable_count + 1,
            "SSTable should be created even with only tombstones at Level 0"
        );

        // Verify all deleted keys return None
        for i in 0..50 {
            let key = format!("key{}", i);
            assert_eq!(db.get(key.as_bytes()).unwrap(), None);
        }
    }

    #[test]
    fn test_flush_mixed_tombstones_and_values() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write some values
        for i in 0..25 {
            let key = format!("keep{}", i);
            db.put(key.as_bytes(), b"value").unwrap();
        }

        // Write and delete other keys (tombstones)
        for i in 0..25 {
            let key = format!("delete{}", i);
            db.put(key.as_bytes(), b"value").unwrap();
            db.delete(key.as_bytes()).unwrap();
        }

        // Flush should create an SSTable (has valid entries)
        db.flush().unwrap();

        // Verify SSTable was created
        let sstable_count = {
            let sstables = db.sstables.read();
            sstables[0].len()
        };

        assert_eq!(
            sstable_count, 1,
            "One SSTable should be created when MemTable has valid entries"
        );

        // Verify only valid keys are readable
        for i in 0..25 {
            let keep_key = format!("keep{}", i);
            let delete_key = format!("delete{}", i);

            assert!(
                db.get(keep_key.as_bytes()).unwrap().is_some(),
                "Valid entries should be in SSTable"
            );
            assert!(
                db.get(delete_key.as_bytes()).unwrap().is_none(),
                "Deleted entries should not be in SSTable"
            );
        }
    }

    #[test]
    fn test_flush_empty_memtable_no_sstable() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Flush empty MemTable
        db.flush().unwrap();

        // Verify no SSTable was created
        let sstable_count = {
            let sstables = db.sstables.read();
            sstables[0].len()
        };

        assert_eq!(sstable_count, 0, "No SSTable should be created for empty MemTable");
    }

    #[test]
    fn test_flush_duplicate_overwrites() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write the same key multiple times
        for i in 0..100 {
            db.put(b"same_key", format!("value{}", i).as_bytes()).unwrap();
        }

        // Flush should create SSTable with only one entry
        db.flush().unwrap();

        // Verify SSTable was created
        let sstable_count = {
            let sstables = db.sstables.read();
            sstables[0].len()
        };

        assert_eq!(sstable_count, 1, "One SSTable should be created");

        // Verify we get the latest value
        let value = db.get(b"same_key").unwrap();
        assert_eq!(value, Some(b"value99".to_vec()));
    }

    #[test]
    fn test_tombstone_sstable_files_created() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();

        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            // Create a MemTable with only tombstones
            for i in 0..10 {
                let key = format!("key{}", i);
                db.put(key.as_bytes(), b"value").unwrap();
                db.delete(key.as_bytes()).unwrap();
            }

            db.flush().unwrap();
            db.close().unwrap();
        }

        // Check for .sst files (should exist with tombstones)
        let sst_files: Vec<_> = std::fs::read_dir(&db_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sst"))
            .collect();

        assert_eq!(sst_files.len(), 1, "SSTable with tombstones should be created at Level 0");

        // Reopen and verify all keys are deleted
        {
            let db = DB::open(&db_path, Options::default()).unwrap();
            for i in 0..10 {
                let key = format!("key{}", i);
                assert_eq!(db.get(key.as_bytes()).unwrap(), None);
            }
        }
    }

    #[test]
    fn test_block_cache_hit_miss() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write some data and flush to create SSTables
        for i in 0..100 {
            let key = format!("key{:04}", i);
            let value = format!("value{:04}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();

        // Clear cache stats
        db.reset_cache_stats();

        // First read - should be cache misses
        let _ = db.get(b"key0001").unwrap();
        let stats = db.cache_stats();
        assert!(stats.misses > 0, "Should have cache misses");

        // Second read of same key - should hit cache
        let initial_hits = stats.hits;
        let _ = db.get(b"key0001").unwrap();
        let stats = db.cache_stats();
        assert!(stats.hits > initial_hits, "Should have cache hits on second read");

        // Verify hit rate increases
        assert!(stats.hit_rate() > 0.0);
    }

    #[test]
    fn test_block_cache_stats() {
        let temp_dir = TempDir::new().unwrap();
        let opts = Options::default().block_cache_size(1024 * 1024); // 1MB cache
        let db = DB::open(temp_dir.path(), opts).unwrap();

        // Initial stats should be zero
        let stats = db.cache_stats();
        assert_eq!(stats.lookups, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);

        // Write and flush
        for i in 0..50 {
            db.put(format!("key{}", i).as_bytes(), b"value").unwrap();
        }
        db.flush().unwrap();

        // Read some keys
        for i in 0..10 {
            let _ = db.get(format!("key{}", i).as_bytes()).unwrap();
        }

        let stats = db.cache_stats();
        assert!(stats.lookups > 0, "Should have cache lookups");
        assert!(stats.hits + stats.misses == stats.lookups, "Hits + misses should equal lookups");
    }

    #[test]
    fn test_block_cache_clear() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Write and flush
        for i in 0..50 {
            db.put(format!("key{}", i).as_bytes(), b"value").unwrap();
        }
        db.flush().unwrap();

        // Read to populate cache
        for i in 0..10 {
            let _ = db.get(format!("key{}", i).as_bytes()).unwrap();
        }

        // Cache should have entries
        assert!(!db.block_cache.is_empty(), "Cache should have entries");

        // Clear cache
        db.clear_cache();

        // Cache should be empty
        assert_eq!(db.block_cache.len(), 0, "Cache should be empty after clear");
    }

    #[test]
    fn test_block_cache_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let opts = Options::default().block_cache_size(0); // Disable cache
        let db = DB::open(temp_dir.path(), opts).unwrap();

        // Write and flush
        for i in 0..50 {
            db.put(format!("key{}", i).as_bytes(), b"value").unwrap();
        }
        db.flush().unwrap();

        // Read some keys
        for i in 0..10 {
            let _ = db.get(format!("key{}", i).as_bytes()).unwrap();
        }

        // With cache disabled, should always have zero cache entries
        assert_eq!(db.block_cache.len(), 0, "Cache should be empty when disabled");
    }

    #[test]
    fn test_block_cache_shared_across_sstables() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Create multiple SSTables
        for batch in 0..3 {
            for i in 0..20 {
                let key = format!("key{:02}_{:03}", batch, i);
                db.put(key.as_bytes(), b"value").unwrap();
            }
            db.flush().unwrap();
        }

        db.reset_cache_stats();

        // Read from different SSTables
        let _ = db.get(b"key00_001").unwrap(); // From first SSTable
        let _ = db.get(b"key01_001").unwrap(); // From second SSTable
        let _ = db.get(b"key02_001").unwrap(); // From third SSTable

        // All should share the same cache
        let stats = db.cache_stats();
        assert!(stats.lookups > 0, "Should have lookups across multiple SSTables");
    }

    // ===== WriteBatch Tests =====

    #[test]
    fn test_write_batch_empty() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        let batch = WriteBatch::new();
        let result = db.write(batch);
        assert!(result.is_ok(), "Writing empty batch should succeed");
    }

    #[test]
    fn test_write_batch_single_put() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        let mut batch = WriteBatch::new();
        batch.put(b"key1", b"value1");

        db.write(batch).unwrap();

        let value = db.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_write_batch_multiple_puts() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        let mut batch = WriteBatch::new();
        for i in 0..100 {
            let key = format!("key{}", i);
            let value = format!("value{}", i);
            batch.put(key.as_bytes(), value.as_bytes());
        }

        db.write(batch).unwrap();

        // Verify all values
        for i in 0..100 {
            let key = format!("key{}", i);
            let expected = format!("value{}", i);
            let value = db.get(key.as_bytes()).unwrap();
            assert_eq!(value, Some(expected.as_bytes().to_vec()));
        }
    }

    #[test]
    fn test_write_batch_delete() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // First put a key
        db.put(b"key1", b"value1").unwrap();
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Delete it using batch
        let mut batch = WriteBatch::new();
        batch.delete(b"key1");
        db.write(batch).unwrap();

        // Verify it's deleted
        assert_eq!(db.get(b"key1").unwrap(), None);
    }

    #[test]
    fn test_write_batch_mixed_operations() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Pre-populate some data
        db.put(b"key1", b"old_value1").unwrap();
        db.put(b"key2", b"old_value2").unwrap();
        db.put(b"key3", b"old_value3").unwrap();

        // Create batch with mixed operations
        let mut batch = WriteBatch::new();
        batch.put(b"key1", b"new_value1"); // Overwrite
        batch.delete(b"key2"); // Delete
        batch.put(b"key4", b"new_value4"); // New key

        db.write(batch).unwrap();

        // Verify results
        assert_eq!(db.get(b"key1").unwrap(), Some(b"new_value1".to_vec()));
        assert_eq!(db.get(b"key2").unwrap(), None);
        assert_eq!(db.get(b"key3").unwrap(), Some(b"old_value3".to_vec()));
        assert_eq!(db.get(b"key4").unwrap(), Some(b"new_value4".to_vec()));
    }

    #[test]
    fn test_write_batch_atomicity() {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();

        // Create a large batch
        let mut batch = WriteBatch::new();
        for i in 0..1000 {
            let key = format!("batch_key{}", i);
            let value = format!("batch_value{}", i);
            batch.put(key.as_bytes(), value.as_bytes());
        }

        // Write atomically
        db.write(batch).unwrap();

        // All keys should be present
        for i in 0..1000 {
            let key = format!("batch_key{}", i);
            let value = db.get(key.as_bytes()).unwrap();
            assert!(value.is_some(), "Key {} should be present after batch write", i);
        }
    }

    #[test]
    fn test_write_batch_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();

        // First session: write batch and close
        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            let mut batch = WriteBatch::new();
            for i in 0..50 {
                let key = format!("persist_key{}", i);
                let value = format!("persist_value{}", i);
                batch.put(key.as_bytes(), value.as_bytes());
            }

            db.write(batch).unwrap();
            db.close().unwrap();
        }

        // Second session: verify data persists
        {
            let db = DB::open(&db_path, Options::default()).unwrap();

            for i in 0..50 {
                let key = format!("persist_key{}", i);
                let expected = format!("persist_value{}", i);
                let value = db.get(key.as_bytes()).unwrap();
                assert_eq!(
                    value,
                    Some(expected.as_bytes().to_vec()),
                    "Batch data should persist after close and reopen"
                );
            }
        }
    }

    #[test]
    fn test_write_batch_triggers_flush() {
        let temp_dir = TempDir::new().unwrap();

        // Use small memtable to trigger flush
        let options = Options::default().memtable_size(1024);
        let db = DB::open(temp_dir.path(), options).unwrap();

        // Create a batch that exceeds memtable size
        let mut batch = WriteBatch::new();
        for i in 0..100 {
            let key = format!("large_key{:08}", i);
            let value = vec![b'x'; 100]; // 100 bytes
            batch.put(key.as_bytes(), &value);
        }

        db.write(batch).unwrap();

        // Check that immutable memtables were created or flush happened
        let immutable = db.immutable_memtables.read();
        assert!(!immutable.is_empty() || !db.sstables.read()[0].is_empty());
    }
}
