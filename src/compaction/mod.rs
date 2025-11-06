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
        let output_path = self.db_path.join(format!("{:06}.sst", file_number));

        // Create merge iterator
        let merge_iter = MergeIterator::new(self.inputs.clone())?;

        // Create SSTable builder
        let mut builder = SSTableBuilder::new(&output_path)?;
        builder.set_block_size(self.block_size);

        // Merge all entries
        let mut entry_count = 0;
        let mut last_user_key: Option<Vec<u8>> = None;

        for (key, value) in merge_iter {
            // Skip duplicate keys (keep only the newest version)
            if let Some(ref last_key) = last_user_key {
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

            builder.add(&key, &value)?;
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

    #[test]
    fn test_target_size_for_level() {
        assert_eq!(target_size_for_level(1), 10 * 1024 * 1024); // 10 MB
        assert_eq!(target_size_for_level(2), 100 * 1024 * 1024); // 100 MB
        assert_eq!(target_size_for_level(3), 1000 * 1024 * 1024); // 1000 MB (10^3 MB)
    }
}
