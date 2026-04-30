//! Compaction module for managing SSTable compaction.
//!
//! This module implements the compaction process that merges multiple SSTables
//! into a single SSTable, removing deleted entries and old versions.
//!
//! ## Compaction Strategy
//!
//! We use Leveled Compaction inspired by RocksDB:
//! - Level 0: New SSTables from flush (may overlap)
//! - Level 1+: Non-overlapping SSTables
//! - Each level has a size threshold
//!
//! ## Compaction Triggers
//!
//! - Level 0: When number of files >= 4
//! - Level N: When total size >= target_size(N)
//!
//! ## Process
//!
//! 1. Pick files for compaction (picker.rs)
//! 2. Merge using multi-way merge iterator (merge.rs)
//! 3. Write to new SSTable in next level
//! 4. Update version (version.rs)
//! 5. Delete old files

pub mod merge;
pub mod picker;
pub mod version;

pub use merge::MergeIterator;
pub use picker::{CompactionPicker, CompactionTask};
pub use version::{Version, VersionEdit, VersionSet};

use crate::error::Result;
use crate::sstable::{SSTableBuilder, SSTableReader};
use std::path::PathBuf;
use std::sync::Arc;

/// Compaction job that executes the compaction process
pub struct CompactionJob {
    /// Input SSTables to compact
    pub inputs: Vec<Arc<SSTableReader>>,
    /// Target level for output
    pub output_level: usize,
    /// Database directory
    pub db_path: PathBuf,
    /// Block size for output SSTables
    pub block_size: usize,
}

impl CompactionJob {
    /// Create a new compaction job
    pub fn new(
        inputs: Vec<Arc<SSTableReader>>,
        output_level: usize,
        db_path: PathBuf,
        block_size: usize,
    ) -> Self {
        Self { inputs, output_level, db_path, block_size }
    }

    /// Execute the compaction
    ///
    /// This will:
    /// 1. Create a merge iterator over all input SSTables
    /// 2. Write merged data to a new SSTable
    /// 3. Return the file number of the new SSTable
    pub fn run(&self, file_number: u64) -> Result<CompactionResult> {
        log::info!(
            "Starting compaction: {} input files -> level {}",
            self.inputs.len(),
            self.output_level
        );

        // Create output SSTable path
        let output_path = crate::sstable::sstable_path(&self.db_path, file_number, self.output_level);

        // Create merge iterator
        let mut merge_iter = MergeIterator::new(self.inputs.clone())?;

        // Create SSTable builder
        let mut builder = SSTableBuilder::new(&output_path)?;
        builder.set_block_size(self.block_size);

        // Merge all entries
        let mut entry_count = 0;
        let mut last_user_key: Option<Vec<u8>> = None;

        while let Some((key, value)) = merge_iter.next_entry()? {
            // Skip duplicate keys (keep only the newest version)
            if let Some(ref last_key) = last_user_key {
                if key.as_slice() < last_key.as_slice() {
                    log::error!(
                        "Compaction output order violation for {:?}: prev_key={:?}, current_key={:?}, output_level={}",
                        output_path,
                        String::from_utf8_lossy(last_key),
                        String::from_utf8_lossy(&key),
                        self.output_level
                    );
                }
                if last_key.as_slice() == key.as_slice() {
                    continue;
                }
            }

            // Skip tombstones (empty values) during compaction to level 1+
            // This removes deleted keys from the database
            if self.output_level > 0 && value.is_empty() {
                last_user_key = Some(key.to_vec());
                continue;
            }

            if let Err(err) = builder.add(&key, &value) {
                log::error!(
                    "Compaction write failed for {:?}: prev_key={:?}, current_key={:?}, output_level={}, entry_count={}",
                    output_path,
                    last_user_key
                        .as_ref()
                        .map(|prev| String::from_utf8_lossy(prev).into_owned()),
                    String::from_utf8_lossy(&key),
                    self.output_level,
                    entry_count
                );
                return Err(err);
            }
            entry_count += 1;
            last_user_key = Some(key.to_vec());
        }

        // If no entries were written, clean up and return
        if entry_count == 0 {
            builder.abandon()?;
            if output_path.exists() {
                std::fs::remove_file(&output_path)?;
            }
            return Ok(CompactionResult { file_number: 0, entry_count: 0, output_path });
        }

        // Finish building the SSTable
        let file_size = builder.finish()?;

        log::info!(
            "Compaction completed: {} entries written, file size: {} bytes",
            entry_count,
            file_size
        );

        Ok(CompactionResult { file_number, entry_count, output_path })
    }
}

/// Result of a compaction operation
pub struct CompactionResult {
    /// File number of the output SSTable (0 if no file was created)
    pub file_number: u64,
    /// Number of entries written
    pub entry_count: usize,
    /// Path to the output file
    pub output_path: PathBuf,
}

/// Target size for each level (in bytes)
pub fn target_size_for_level(level: usize) -> u64 {
    if level == 0 {
        // Level 0 is controlled by file count, not size
        return u64::MAX;
    }
    // Level 1: 10 MB
    // Level 2: 100 MB
    // Level 3: 1 GB
    // Level N: 10^N MB
    10u64.pow(level as u32) * 1024 * 1024
}

/// Maximum number of files at Level 0
pub const MAX_LEVEL0_FILES: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sstable::SSTableBuilder;
    use crate::sstable::SSTableReader;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn create_sstable(dir: &Path, number: u64, entries: &[(&[u8], &[u8])]) -> Arc<SSTableReader> {
        let path = crate::sstable::sstable_path(dir, number, 0);
        let mut builder = SSTableBuilder::new(&path).unwrap();
        for (k, v) in entries {
            builder.add(k, v).unwrap();
        }
        builder.finish().unwrap();
        Arc::new(SSTableReader::open(&path).unwrap())
    }

    #[test]
    fn test_target_size_for_level() {
        assert_eq!(target_size_for_level(1), 10 * 1024 * 1024);
        assert_eq!(target_size_for_level(2), 100 * 1024 * 1024);
        assert_eq!(target_size_for_level(3), 1000 * 1024 * 1024);
    }

    #[test]
    fn test_compaction_basic_merge() {
        let dir = TempDir::new().unwrap();

        let sst1 = create_sstable(dir.path(), 1, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
        let sst2 = create_sstable(dir.path(), 2, &[(b"d", b"4"), (b"e", b"5"), (b"f", b"6")]);

        let job = CompactionJob::new(
            vec![sst1, sst2],
            1,
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();

        assert_eq!(result.entry_count, 6);
        let output_path = dir.path().join("000100_L1.sst");
        assert!(output_path.exists());

        let reader = SSTableReader::open(&output_path).unwrap();
        assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(reader.get(b"b").unwrap(), Some(b"2".to_vec()));
        assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
        assert_eq!(reader.get(b"d").unwrap(), Some(b"4".to_vec()));
        assert_eq!(reader.get(b"e").unwrap(), Some(b"5".to_vec()));
        assert_eq!(reader.get(b"f").unwrap(), Some(b"6".to_vec()));
    }

    #[test]
    fn test_compaction_removes_duplicates() {
        let dir = TempDir::new().unwrap();

        let sst1 = create_sstable(dir.path(), 1, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
        let sst2 = create_sstable(dir.path(), 2, &[(b"b", b"20"), (b"c", b"30"), (b"d", b"40")]);

        let job = CompactionJob::new(
            vec![sst1, sst2],
            1,
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();
        assert_eq!(result.entry_count, 4); // a, b, c, d (no duplicates)

        let reader = SSTableReader::open(&dir.path().join("000100_L1.sst")).unwrap();
        assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(reader.get(b"b").unwrap(), Some(b"2".to_vec()));
        assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
        assert_eq!(reader.get(b"d").unwrap(), Some(b"40".to_vec()));
    }

    #[test]
    fn test_compaction_removes_tombstones_at_level1() {
        let dir = TempDir::new().unwrap();

        // "b" and "d" have empty values (tombstones)
        let sst1 = create_sstable(
            dir.path(),
            1,
            &[(b"a", b"1"), (b"b", b""), (b"c", b"3"), (b"d", b""), (b"e", b"5")],
        );

        let job = CompactionJob::new(
            vec![sst1],
            1, // level 1+ removes tombstones
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();
        assert_eq!(result.entry_count, 3); // a, c, e (tombstones removed)

        let reader = SSTableReader::open(&dir.path().join("000100_L1.sst")).unwrap();
        assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(reader.get(b"b").unwrap(), None); // tombstone removed
        assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
        assert_eq!(reader.get(b"d").unwrap(), None); // tombstone removed
        assert_eq!(reader.get(b"e").unwrap(), Some(b"5".to_vec()));
    }

    #[test]
    fn test_compaction_preserves_tombstones_at_level0() {
        let dir = TempDir::new().unwrap();

        let sst1 = create_sstable(
            dir.path(),
            1,
            &[(b"a", b"1"), (b"b", b""), (b"c", b"3")],
        );

        let job = CompactionJob::new(
            vec![sst1],
            0, // level 0 preserves tombstones
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();
        assert_eq!(result.entry_count, 3); // all entries kept

        let reader = SSTableReader::open(&dir.path().join("000100_L0.sst")).unwrap();
        assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
        // At level 0, tombstone is preserved — entry exists with empty value
        assert_eq!(reader.get(b"b").unwrap(), Some(b"".to_vec()));
        assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
    }

    #[test]
    fn test_compaction_single_input() {
        let dir = TempDir::new().unwrap();

        let sst1 = create_sstable(dir.path(), 1, &[(b"x", b"10"), (b"y", b"20"), (b"z", b"30")]);

        let job = CompactionJob::new(
            vec![sst1],
            1,
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();
        assert_eq!(result.entry_count, 3);

        let reader = SSTableReader::open(&dir.path().join("000100_L1.sst")).unwrap();
        assert_eq!(reader.get(b"x").unwrap(), Some(b"10".to_vec()));
        assert_eq!(reader.get(b"y").unwrap(), Some(b"20".to_vec()));
        assert_eq!(reader.get(b"z").unwrap(), Some(b"30".to_vec()));
    }

    #[test]
    fn test_compaction_all_entries_removed() {
        let dir = TempDir::new().unwrap();

        // All entries are tombstones
        let sst1 = create_sstable(dir.path(), 1, &[(b"a", b""), (b"b", b""), (b"c", b"")]);

        let job = CompactionJob::new(
            vec![sst1],
            1, // level 1+ removes tombstones
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();
        assert_eq!(result.file_number, 0);
        assert_eq!(result.entry_count, 0);
        // Output file should not exist
        assert!(!dir.path().join("000100_L1.sst").exists());
    }
}
