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
pub mod compaction;
pub mod config;
pub mod error;
pub mod memtable;
pub mod sstable;
pub mod wal;

// Re-exports
pub use config::Options;
pub use error::{Error, Result};

use compaction::{CompactionJob, CompactionPicker, VersionEdit, VersionSet};
use memtable::MemTable;
use parking_lot::RwLock;
use sstable::{SSTableBuilder, SSTableReader};
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
            return Err(Error::AlreadyExists(format!("Database already exists: {:?}", path)));
        }

        // Step 2: Initialize sequence number
        let mut sequence = 0u64;

        // Step 3: Find and open the latest WAL file
        let mut wal_number = 1u64;
        let mut latest_wal_path = path.join(wal::wal_filename(1));

        // Scan for the latest WAL file
        if path.exists() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    if let Some(filename) = entry.file_name().to_str() {
                        if let Some(num) = wal::parse_wal_filename(filename) {
                            if num >= wal_number {
                                wal_number = num;
                                latest_wal_path = entry.path();
                            }
                        }
                    }
                }
            }
        }

        let wal = WAL::open(&latest_wal_path)?;

        // Step 4: Recover from WAL if it exists and has data
        let recovered_entries = if latest_wal_path.exists() && wal.size() > 0 {
            WAL::recover(&latest_wal_path)?
        } else {
            Vec::new()
        };

        // Step 5: Initialize MemTable with recovered data
        let memtable = MemTable::new(sequence + 1);

        for entry in recovered_entries {
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

        // Step 6: Load existing SSTables
        let mut sstables: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); options.max_levels];

        // Scan directory for SSTable files (*.sst)
        if path.exists() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                let mut sst_files = Vec::new();

                for entry in entries.flatten() {
                    if let Some(filename) = entry.file_name().to_str() {
                        if filename.ends_with(".sst") {
                            sst_files.push(entry.path());
                        }
                    }
                }

                // Sort SSTable files by file number (newest last)
                sst_files.sort();

                // Load all SSTables into Level 0
                for sst_path in sst_files {
                    match SSTableReader::open(&sst_path) {
                        Ok(reader) => {
                            sstables[0].push(Arc::new(reader));
                            log::info!("Loaded SSTable: {:?}", sst_path);
                        }
                        Err(e) => {
                            log::warn!("Failed to load SSTable {:?}: {}", sst_path, e);
                        }
                    }
                }

                log::info!("Loaded {} SSTables at Level 0", sstables[0].len());
            }
        }

        // Step 7: Initialize VersionSet
        let version_set = VersionSet::new(&path, options.max_levels)?;

        // Step 8: Initialize CompactionPicker
        let compaction_picker = CompactionPicker::new(options.max_levels);

        // Step 9: Construct DB instance
        Ok(DB {
            path,
            options,
            memtable: Arc::new(RwLock::new(memtable)),
            immutable_memtables: Arc::new(RwLock::new(Vec::new())),
            wal: Arc::new(RwLock::new(wal)),
            sstables: Arc::new(RwLock::new(sstables)),
            sequence: Arc::new(AtomicU64::new(sequence)),
            next_file_number: Arc::new(AtomicU64::new(2)), // Start from 2 (1 is for WAL)
            wal_file_number: Arc::new(AtomicU64::new(wal_number)),
            version_set: Arc::new(RwLock::new(version_set)),
            compaction_picker: Arc::new(compaction_picker),
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
            log::info!(
                "MemTable is full ({} bytes >= {}), triggering freeze",
                memtable_size,
                self.options.memtable_size
            );
            // Freeze the current MemTable
            // The actual flush will happen in the background or on next flush() call
            self.freeze_memtable()?;
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
                // For Level 0, search all tables (may overlap)
                // For other levels, tables don't overlap, so we can binary search
                for table in level_tables.iter().rev() {
                    // Since we store user_key only in SSTables (simplified version),
                    // we can directly search for the key
                    if let Some(value) = table.get(key)? {
                        return Ok(Some(value));
                    }
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
        let sstable_path = self.path.join(format!("{:06}.sst", file_number));

        log::info!("Starting flush of MemTable to SSTable: {:?}", sstable_path);

        // Create SSTable builder
        let mut builder = SSTableBuilder::new(&sstable_path)?;
        builder.set_block_size(self.options.block_size);

        // Iterate through MemTable and add entries to SSTable
        // We only keep the latest version of each user key (skip older versions)
        let mut entry_count = 0;
        let mut last_user_key: Option<Vec<u8>> = None;

        for entry in memtable.iter() {
            let user_key = entry.user_key();
            let value = entry.value();
            let value_type = entry.value_type();

            // Skip if this is an older version of the same key
            if let Some(ref last_key) = last_user_key {
                if last_key.as_slice() == user_key {
                    continue; // Skip older versions
                }
            }

            // For SSTable at Level 0, we store both values and tombstones
            // Tombstones will be removed during compaction
            builder.add(user_key, value)?;
            entry_count += 1;

            last_user_key = Some(user_key.to_vec());
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

        // Open the SSTable for reading
        let reader = Arc::new(SSTableReader::open(&sstable_path)?);

        // Add to Level 0 at the front (newest files first)
        {
            let mut sstables = self.sstables.write();
            sstables[0].insert(0, reader);
        }

        Ok(file_number)
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

        // Step 2: Flush all immutable MemTables
        loop {
            // Get the oldest immutable MemTable
            let memtable_to_flush = {
                let mut immutable = self.immutable_memtables.write();
                if immutable.is_empty() {
                    break;
                }
                immutable.remove(0) // Remove from front (FIFO)
            };

            // Flush it to SSTable
            self.flush_memtable_to_sstable(&memtable_to_flush)?;
        }

        // Step 3: Rotate WAL after successful flush
        self.rotate_wal()?;

        // Step 4: Check if compaction is needed
        self.maybe_trigger_compaction()?;

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

        // Close and delete the old WAL file
        let old_path = old_wal.path().to_path_buf();
        drop(old_wal);

        // Remove old WAL file
        if old_path.exists() {
            std::fs::remove_file(&old_path)?;
            log::info!("Removed old WAL file: {:?}", old_path);
        }

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

        // Update version set
        {
            let mut version_set = self.version_set.write();

            // Read smallest and largest keys from the new SSTable
            let new_reader = Arc::new(SSTableReader::open(&result.output_path)?);
            let smallest_key = new_reader
                .smallest_key()?
                .ok_or_else(|| Error::internal("New SSTable has no keys"))?;
            let largest_key = new_reader
                .largest_key()?
                .ok_or_else(|| Error::internal("New SSTable has no keys"))?;

            // Add new file to version
            let add_edit = VersionEdit::AddFile {
                level: task.output_level,
                file_number: result.file_number,
                file_size: new_reader.file_size(),
                smallest_key,
                largest_key,
            };
            version_set.log_edit(&add_edit)?;

            // Delete input files from version
            // We need to find the file numbers for the input files
            // For now, we'll scan all files in the directory
            let mut deleted_files = Vec::new();
            for input in &task.inputs {
                // Find matching file by size
                if let Ok(entries) = std::fs::read_dir(&self.path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                            if filename.ends_with(".sst") {
                                if let Ok(reader) = SSTableReader::open(&path) {
                                    if reader.file_size() == input.file_size() {
                                        // Found a match
                                        if let Some(num_str) = filename.strip_suffix(".sst") {
                                            if let Ok(file_num) = num_str.parse::<u64>() {
                                                deleted_files.push((file_num, path.clone()));
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Log deletions and delete files
            for (file_num, file_path) in deleted_files {
                let delete_edit = VersionEdit::DeleteFile {
                    level: task.level,
                    file_number: file_num,
                };
                version_set.log_edit(&delete_edit)?;

                // Delete the physical file
                if file_path.exists() {
                    std::fs::remove_file(&file_path)?;
                    log::info!("Deleted compacted file: {:?}", file_path);
                }
            }
        }

        // Update in-memory SSTable list
        {
            let mut sstables = self.sstables.write();

            // Remove input files from source level
            sstables[task.level].retain(|reader| {
                !task.inputs.iter().any(|input| Arc::ptr_eq(reader, input))
            });

            // Add new file to output level
            let new_reader = Arc::new(SSTableReader::open(&result.output_path)?);
            // For Level 0, insert at front (newest first), for other levels, append
            if task.output_level == 0 {
                sstables[task.output_level].insert(0, new_reader);
            } else {
                sstables[task.output_level].push(new_reader);
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

        log::info!("Database closed successfully");

        Ok(())
    }
}

impl Drop for DB {
    fn drop(&mut self) {
        // Attempt to flush and close cleanly
        // Ignore errors during drop as we can't propagate them
        if let Err(e) = self.flush() {
            eprintln!("Error flushing database during drop: {}", e);
        }

        if self.options.use_wal {
            let mut wal = self.wal.write();
            if let Err(e) = wal.sync() {
                eprintln!("Error syncing WAL during drop: {}", e);
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

        assert_eq!(
            sst_files.len(),
            1,
            "SSTable with tombstones should be created at Level 0"
        );

        // Reopen and verify all keys are deleted
        {
            let db = DB::open(&db_path, Options::default()).unwrap();
            for i in 0..10 {
                let key = format!("key{}", i);
                assert_eq!(db.get(key.as_bytes()).unwrap(), None);
            }
        }
    }
}
