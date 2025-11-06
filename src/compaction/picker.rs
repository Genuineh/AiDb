//! Compaction file picker.
//!
//! This module selects which files should be compacted based on the
//! Leveled Compaction strategy.

use crate::compaction::{target_size_for_level, MAX_LEVEL0_FILES};
use crate::sstable::SSTableReader;
use std::sync::Arc;

/// A compaction task selected by the picker
#[derive(Debug, Clone)]
pub struct CompactionTask {
    /// Input files for compaction
    pub inputs: Vec<Arc<SSTableReader>>,
    /// Source level
    pub level: usize,
    /// Target level (level + 1)
    pub output_level: usize,
}

/// Picker for selecting files to compact
pub struct CompactionPicker {
    /// Maximum number of levels
    max_levels: usize,
}

impl CompactionPicker {
    /// Create a new compaction picker
    pub fn new(max_levels: usize) -> Self {
        Self { max_levels }
    }

    /// Pick files for compaction
    ///
    /// Returns None if no compaction is needed
    pub fn pick_compaction(&self, levels: &[Vec<Arc<SSTableReader>>]) -> Option<CompactionTask> {
        // Strategy:
        // 1. Check Level 0 first (file count based)
        // 2. Check other levels (size based)

        // Level 0: Trigger if too many files
        if levels[0].len() >= MAX_LEVEL0_FILES {
            return self.pick_level0_compaction(levels);
        }

        // Level 1+: Trigger if size exceeds threshold
        for level in 1..self.max_levels - 1 {
            let total_size = self.calculate_level_size(&levels[level]);
            let target_size = target_size_for_level(level);

            if total_size > target_size {
                return self.pick_level_compaction(levels, level);
            }
        }

        None
    }

    /// Pick files for Level 0 compaction
    ///
    /// Level 0 files may overlap, so we compact all of them into Level 1
    fn pick_level0_compaction(&self, levels: &[Vec<Arc<SSTableReader>>]) -> Option<CompactionTask> {
        if levels[0].is_empty() {
            return None;
        }

        log::info!("Picking Level 0 compaction: {} files at Level 0", levels[0].len());

        // Take all Level 0 files
        let inputs = levels[0].clone();

        Some(CompactionTask { inputs, level: 0, output_level: 1 })
    }

    /// Pick files for Level N compaction (N >= 1)
    ///
    /// For Level N, files don't overlap, so we can pick a subset
    fn pick_level_compaction(
        &self,
        levels: &[Vec<Arc<SSTableReader>>],
        level: usize,
    ) -> Option<CompactionTask> {
        if level >= levels.len() || levels[level].is_empty() {
            return None;
        }

        log::info!(
            "Picking Level {} compaction: {} files, total size: {} bytes",
            level,
            levels[level].len(),
            self.calculate_level_size(&levels[level])
        );

        // Simple strategy: pick the first file at this level
        // In a full implementation, we'd use a more sophisticated strategy
        // (e.g., round-robin, or picking the file that hasn't been compacted recently)
        let inputs = vec![levels[level][0].clone()];

        Some(CompactionTask { inputs, level, output_level: level + 1 })
    }

    /// Calculate total size of a level
    fn calculate_level_size(&self, level: &[Arc<SSTableReader>]) -> u64 {
        level.iter().map(|reader| reader.file_size()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sstable::SSTableBuilder;
    use tempfile::TempDir;

    fn create_sstable_with_size(
        dir: &TempDir,
        file_num: u64,
        num_entries: usize,
    ) -> Arc<SSTableReader> {
        let path = dir.path().join(format!("{:06}.sst", file_num));
        let mut builder = SSTableBuilder::new(&path).unwrap();

        for i in 0..num_entries {
            let key = format!("key{:08}", i);
            let value = format!("value{:08}", i);
            builder.add(key.as_bytes(), value.as_bytes()).unwrap();
        }
        builder.finish().unwrap();

        Arc::new(SSTableReader::open(&path).unwrap())
    }

    #[test]
    fn test_pick_level0_compaction() {
        let temp_dir = TempDir::new().unwrap();
        let picker = CompactionPicker::new(7);

        // Create 4 SSTables at Level 0
        let mut levels: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); 7];
        for i in 0..4 {
            levels[0].push(create_sstable_with_size(&temp_dir, i, 10));
        }

        let task = picker.pick_compaction(&levels);
        assert!(task.is_some());

        let task = task.unwrap();
        assert_eq!(task.level, 0);
        assert_eq!(task.output_level, 1);
        assert_eq!(task.inputs.len(), 4);
    }

    #[test]
    fn test_no_compaction_needed() {
        let temp_dir = TempDir::new().unwrap();
        let picker = CompactionPicker::new(7);

        // Create only 2 SSTables at Level 0 (below threshold)
        let mut levels: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); 7];
        for i in 0..2 {
            levels[0].push(create_sstable_with_size(&temp_dir, i, 10));
        }

        let task = picker.pick_compaction(&levels);
        assert!(task.is_none());
    }

    #[test]
    fn test_pick_level1_compaction() {
        let temp_dir = TempDir::new().unwrap();
        let picker = CompactionPicker::new(7);

        // Create enough data at Level 1 to exceed threshold (10 MB)
        let mut levels: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); 7];

        // Create multiple large SSTables to exceed 10 MB
        // Need many entries to create larger files (each entry is ~30 bytes)
        for i in 0..100 {
            levels[1].push(create_sstable_with_size(&temp_dir, i, 10000));
        }

        // Verify the total size exceeds 10 MB
        let total_size = picker.calculate_level_size(&levels[1]);
        assert!(total_size > 10 * 1024 * 1024, "Total size: {} bytes", total_size);

        let task = picker.pick_compaction(&levels);
        assert!(task.is_some());

        let task = task.unwrap();
        assert_eq!(task.level, 1);
        assert_eq!(task.output_level, 2);
        assert!(!task.inputs.is_empty());
    }

    #[test]
    fn test_level0_priority() {
        let temp_dir = TempDir::new().unwrap();
        let picker = CompactionPicker::new(7);

        let mut levels: Vec<Vec<Arc<SSTableReader>>> = vec![Vec::new(); 7];

        // Create files at both Level 0 and Level 1
        for i in 0..4 {
            levels[0].push(create_sstable_with_size(&temp_dir, i, 10));
        }
        for i in 4..24 {
            levels[1].push(create_sstable_with_size(&temp_dir, i, 1000));
        }

        // Level 0 should have priority
        let task = picker.pick_compaction(&levels);
        assert!(task.is_some());

        let task = task.unwrap();
        assert_eq!(task.level, 0, "Level 0 should be picked first");
    }

    #[test]
    fn test_calculate_level_size() {
        let temp_dir = TempDir::new().unwrap();
        let picker = CompactionPicker::new(7);

        let table1 = create_sstable_with_size(&temp_dir, 1, 100);
        let table2 = create_sstable_with_size(&temp_dir, 2, 100);

        let level = vec![table1.clone(), table2.clone()];
        let total_size = picker.calculate_level_size(&level);

        assert!(total_size > 0);
        assert_eq!(total_size, table1.file_size() + table2.file_size());
    }
}
