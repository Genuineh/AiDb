//! Leveled compaction 文件选择.

use super::helpers::key_ranges_overlap_by_meta_raw;
use crate::config::Options;
use crate::engine::sstable::SSTableReader;
use std::sync::Arc;

#[derive(Clone)]
pub struct CompactionTask {
    pub inputs: Vec<Arc<SSTableReader>>,
    pub level: usize,
    pub output_level: usize,
    pub expanded_inputs: Vec<Arc<SSTableReader>>,
    pub is_trivial_move: bool,
}

pub struct CompactionPicker {
    level0_compaction_trigger: usize,
    max_bytes_for_level_base: u64,
    max_bytes_for_level_multiplier: u64,
    max_levels: usize,
}

impl CompactionPicker {
    pub fn from_options(options: &Options) -> Self {
        Self {
            level0_compaction_trigger: options.level0_compaction_trigger,
            max_bytes_for_level_base: options.max_bytes_for_level_base as u64,
            max_bytes_for_level_multiplier: options.max_bytes_for_level_multiplier as u64,
            max_levels: options.max_levels,
        }
    }

    pub fn target_size_for_level(&self, level: usize) -> u64 {
        if level == 0 {
            return u64::MAX;
        }
        self.max_bytes_for_level_base
            * self
                .max_bytes_for_level_multiplier
                .pow(level.saturating_sub(1) as u32)
    }

    pub fn calculate_level_size(level: &[Arc<SSTableReader>]) -> u64 {
        level.iter().map(|r| r.file_size()).sum()
    }

    #[tracing::instrument(name = "cmp_pick", skip(self, levels))]
    pub fn pick_compaction(&self, levels: &[Vec<Arc<SSTableReader>>]) -> Option<CompactionTask> {
        if levels.is_empty() {
            return None;
        }
        if levels[0].len() >= self.level0_compaction_trigger {
            return self.pick_level0(levels);
        }
        for level in 1..self.max_levels.saturating_sub(1) {
            if level >= levels.len() {
                break;
            }
            if Self::calculate_level_size(&levels[level]) > self.target_size_for_level(level) {
                return self.pick_level_n(levels, level);
            }
        }
        None
    }

    fn pick_level0(&self, levels: &[Vec<Arc<SSTableReader>>]) -> Option<CompactionTask> {
        if levels[0].is_empty() {
            return None;
        }
        let inputs = levels[0].clone();
        let (in_start, in_end) = combined_range(&inputs);
        let expanded = overlap_in_level(levels, 1, in_start, in_end);
        let is_trivial_move = expanded.is_empty() && inputs.len() == 1;
        Some(CompactionTask {
            inputs,
            level: 0,
            output_level: 1,
            expanded_inputs: expanded,
            is_trivial_move,
        })
    }

    fn pick_level_n(
        &self,
        levels: &[Vec<Arc<SSTableReader>>],
        level: usize,
    ) -> Option<CompactionTask> {
        if level + 1 >= levels.len() || levels[level].is_empty() {
            return None;
        }
        let seed = levels[level][0].clone();
        let mut inputs = vec![seed.clone()];
        let mut expanded = overlap_with_reader(levels, level + 1, &seed);

        if expanded.is_empty() {
            // Trivial move: no overlap with target level, promote without rewriting
            return Some(CompactionTask {
                inputs,
                level,
                output_level: level + 1,
                expanded_inputs: Vec::new(),
                is_trivial_move: true,
            });
        }

        let (ex_start, ex_end) = combined_range(&expanded);
        for f in &levels[level] {
            if f.file_number() != seed.file_number()
                && key_ranges_overlap_by_meta_raw(
                    ex_start,
                    ex_end,
                    f.smallest_key(),
                    f.largest_key(),
                )
            {
                inputs.push(f.clone());
            }
        }
        let (in_start, in_end) = combined_range(&inputs);
        expanded = overlap_in_level(levels, level + 1, in_start, in_end);

        Some(CompactionTask {
            inputs,
            level,
            output_level: level + 1,
            expanded_inputs: expanded,
            is_trivial_move: false,
        })
    }
}

fn combined_range(files: &[Arc<SSTableReader>]) -> (&[u8], &[u8]) {
    let mut start = files[0].smallest_key();
    let mut end = files[0].largest_key();
    for f in files.iter().skip(1) {
        if f.smallest_key() < start {
            start = f.smallest_key();
        }
        if f.largest_key() > end {
            end = f.largest_key();
        }
    }
    (start, end)
}

fn overlap_in_level(
    levels: &[Vec<Arc<SSTableReader>>],
    level: usize,
    start: &[u8],
    end: &[u8],
) -> Vec<Arc<SSTableReader>> {
    if level >= levels.len() {
        return Vec::new();
    }
    levels[level]
        .iter()
        .filter(|f| key_ranges_overlap_by_meta_raw(start, end, f.smallest_key(), f.largest_key()))
        .cloned()
        .collect()
}

fn overlap_with_reader(
    levels: &[Vec<Arc<SSTableReader>>],
    level: usize,
    seed: &SSTableReader,
) -> Vec<Arc<SSTableReader>> {
    overlap_in_level(levels, level, seed.smallest_key(), seed.largest_key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Options;
    use crate::engine::memtable::{encode_internal_key, ValueType};
    use crate::engine::sstable::{sstable_path, SSTableBuilder, SSTableReader};
    use tempfile::tempdir;

    fn file(dir: &std::path::Path, num: u64, level: usize, key: &[u8]) -> Arc<SSTableReader> {
        let path = sstable_path(dir, num, level);
        let ik = encode_internal_key(key, 1, ValueType::TypePut);
        let mut b =
            SSTableBuilder::new(&path, 512, 16, crate::config::CompressionType::None, 0.0).unwrap();
        b.add(&ik, b"v").unwrap();
        b.finish().unwrap();
        Arc::new(SSTableReader::open(&path, None).unwrap())
    }

    fn empty_levels(n: usize) -> Vec<Vec<Arc<SSTableReader>>> {
        vec![Vec::new(); n]
    }

    #[test]
    fn test_pick_level0() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.level0_compaction_trigger = 2;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[0].push(file(dir.path(), 1, 0, b"a"));
        levels[0].push(file(dir.path(), 2, 0, b"b"));
        let task = picker.pick_compaction(&levels).unwrap();
        assert_eq!(task.level, 0);
        assert_eq!(task.inputs.len(), 2);
    }

    #[test]
    fn test_no_compaction_when_below_threshold() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.level0_compaction_trigger = 4;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[0].push(file(dir.path(), 1, 0, b"a"));
        assert!(picker.pick_compaction(&levels).is_none());
    }

    fn big_file(
        dir: &std::path::Path,
        num: u64,
        level: usize,
        user_key: &[u8],
        value_len: usize,
    ) -> Arc<SSTableReader> {
        let path = sstable_path(dir, num, level);
        let ik = encode_internal_key(user_key, 1, ValueType::TypePut);
        let val = vec![0u8; value_len];
        let mut b =
            SSTableBuilder::new(&path, 512, 16, crate::config::CompressionType::None, 0.0).unwrap();
        b.add(&ik, &val).unwrap();
        b.finish().unwrap();
        Arc::new(SSTableReader::open(&path, None).unwrap())
    }

    #[test]
    fn test_calculate_level_size() {
        let dir = tempdir().unwrap();
        let a = big_file(dir.path(), 1, 1, b"a", 400);
        let b = big_file(dir.path(), 2, 1, b"b", 400);
        let total = CompactionPicker::calculate_level_size(&[a, b]);
        assert!(total > 800);
    }

    #[test]
    fn test_pick_level1_compaction() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.max_bytes_for_level_base = 500;
        opts.max_bytes_for_level_multiplier = 10;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[1].push(big_file(dir.path(), 1, 1, b"a", 400));
        levels[1].push(big_file(dir.path(), 2, 1, b"b", 400));
        let task = picker
            .pick_compaction(&levels)
            .expect("level1 should trigger");
        assert_eq!(task.level, 1);
        assert_eq!(task.output_level, 2);
    }

    #[test]
    fn test_level0_priority_over_level1() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.level0_compaction_trigger = 2;
        opts.max_bytes_for_level_base = 100;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[0].push(file(dir.path(), 1, 0, b"a"));
        levels[0].push(file(dir.path(), 2, 0, b"b"));
        levels[1].push(big_file(dir.path(), 3, 1, b"c", 500));
        let task = picker.pick_compaction(&levels).unwrap();
        assert_eq!(task.level, 0);
    }

    #[test]
    fn test_pick_last_level_not_compacted() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.max_levels = 3;
        opts.max_bytes_for_level_base = 100;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(3);
        levels[2].push(big_file(dir.path(), 1, 2, b"z", 10_000));
        assert!(picker.pick_compaction(&levels).is_none());
    }

    #[test]
    fn test_pick_level_n_key_range_expansion() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.max_bytes_for_level_base = 200;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[1].push(big_file(dir.path(), 1, 1, b"a", 300));
        levels[1].push(file(dir.path(), 2, 1, b"m"));
        levels[2].push(file(dir.path(), 3, 2, b"a"));
        let task = picker.pick_compaction(&levels).expect("level1 overflow");
        assert!(!task.inputs.is_empty());
        assert_eq!(task.output_level, 2);
    }

    #[test]
    fn test_pick_level_n_trivial_move() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.max_bytes_for_level_base = 200;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        // Level 1 has one file with key "a", level 2 has NO overlapping files
        levels[1].push(big_file(dir.path(), 1, 1, b"a", 300));
        let task = picker.pick_compaction(&levels).expect("level1 overflow");
        assert!(
            task.is_trivial_move,
            "should be trivial move when no overlap"
        );
        assert_eq!(task.inputs.len(), 1);
        assert!(task.expanded_inputs.is_empty());
    }

    #[test]
    fn test_pick_level_n_no_trivial_move_when_overlap() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.max_bytes_for_level_base = 200;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[1].push(big_file(dir.path(), 1, 1, b"a", 300));
        levels[2].push(file(dir.path(), 2, 2, b"a")); // overlaps with seed "a"
        let task = picker.pick_compaction(&levels).expect("level1 overflow");
        assert!(
            !task.is_trivial_move,
            "should NOT be trivial move when overlap exists"
        );
    }

    #[test]
    fn test_pick_level0_no_trivial_move_multiple_files() {
        let dir = tempdir().unwrap();
        let mut opts = Options::for_testing();
        opts.level0_compaction_trigger = 2;
        let picker = CompactionPicker::from_options(&opts);
        let mut levels = empty_levels(7);
        levels[0].push(file(dir.path(), 1, 0, b"a"));
        levels[0].push(file(dir.path(), 2, 0, b"z"));
        // L1 empty, but multiple L0 files may overlap each other -> not trivial
        let task = picker.pick_compaction(&levels).unwrap();
        assert!(
            !task.is_trivial_move,
            "multiple L0 files should not be trivial move even when L1 is empty"
        );
    }

    #[test]
    fn test_key_ranges_overlap() {
        use crate::engine::compaction::helpers::key_ranges_overlap_by_meta_raw;
        use crate::engine::memtable::{encode_internal_key, ValueType};
        let a0 = encode_internal_key(b"a", 1, ValueType::TypePut);
        let a1 = encode_internal_key(b"c", 1, ValueType::TypePut);
        let b0 = encode_internal_key(b"b", 1, ValueType::TypePut);
        let b1 = encode_internal_key(b"d", 1, ValueType::TypePut);
        assert!(key_ranges_overlap_by_meta_raw(&a0, &a1, &b0, &b1));
        let z0 = encode_internal_key(b"x", 1, ValueType::TypePut);
        let z1 = encode_internal_key(b"y", 1, ValueType::TypePut);
        assert!(!key_ranges_overlap_by_meta_raw(&a0, &a1, &z0, &z1));
    }
}
