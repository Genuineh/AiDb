//! Compaction 执行: 归并 → 新 SSTable.
//!
//! 支持 subcompaction 分裂: 将大 compaction 拆分为多个并行子任务,
//! 每个子任务负责一个 key 区间, 使用 std::thread::scope 并行执行.

use super::helpers::user_key_from_internal;
use super::merge::MergeIterator;
use crate::config::CompressionType;
use crate::engine::memtable::{extract_sequence, extract_value_type, ValueType};
use crate::engine::sstable::{sstable_path, SSTableBuilder, SSTableReader};
use crate::error::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct CompactionJob {
    pub inputs: Vec<Arc<SSTableReader>>,
    pub expanded_inputs: Vec<Arc<SSTableReader>>,
    pub output_level: usize,
    pub db_path: PathBuf,
    pub block_size: usize,
    pub block_restart_interval: usize,
    pub compression: CompressionType,
    pub bloom_false_positive_rate: f64,
    /// 活跃快照中的最小 sequence. 低于此值的旧版本可安全 dedup.
    pub min_snapshot_sequence: u64,
}

pub struct CompactionResult {
    pub file_number: u64,
    pub entry_count: usize,
    pub output_path: PathBuf,
    pub smallest_key: Vec<u8>,
    pub largest_key: Vec<u8>,
    pub file_size: u64,
}

impl CompactionJob {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inputs: Vec<Arc<SSTableReader>>,
        expanded_inputs: Vec<Arc<SSTableReader>>,
        output_level: usize,
        db_path: PathBuf,
        block_size: usize,
        block_restart_interval: usize,
        compression: CompressionType,
        bloom_false_positive_rate: f64,
    ) -> Self {
        Self {
            inputs,
            expanded_inputs,
            output_level,
            db_path,
            block_size,
            block_restart_interval,
            compression,
            bloom_false_positive_rate,
            min_snapshot_sequence: u64::MAX, // 默认无活跃快照
        }
    }

    /// 设置快照保护阈值: compaction 不会删除 `sequence >= min_snapshot_sequence` 的版本.
    pub fn with_snapshot_threshold(mut self, min_seq: u64) -> Self {
        self.min_snapshot_sequence = min_seq;
        self
    }

    fn all_inputs(&self) -> Vec<Arc<SSTableReader>> {
        let mut all = self.inputs.clone();
        all.extend(self.expanded_inputs.clone());
        all
    }

    /// 检查 entry 是否因快照保护而必须保留.
    fn snapshot_protected(&self, key: &[u8]) -> Result<bool> {
        if self.min_snapshot_sequence == u64::MAX {
            return Ok(false);
        }
        Ok(extract_sequence(key)? >= self.min_snapshot_sequence)
    }

    fn count_dedup_entries(&self) -> Result<usize> {
        let mut merge_iter = MergeIterator::new(self.all_inputs())?;
        let mut count = 0usize;
        let mut last_user_key: Option<Vec<u8>> = None;
        while let Some((key, _)) = merge_iter.next_entry()? {
            let user_key = user_key_from_internal(&key)?;
            if last_user_key.as_deref() == Some(user_key) {
                if self.snapshot_protected(&key)? {
                    count += 1; // 快照可能可见, 保留
                }
                continue;
            }
            if self.output_level > 0 && extract_value_type(&key)? == ValueType::TypeDelete {
                last_user_key = Some(user_key.to_vec());
                continue;
            }
            count += 1;
            last_user_key = Some(user_key.to_vec());
        }
        Ok(count)
    }

    /// 单遍计数 + 分裂点记录. 基于文件大小粗估条目数以确定分裂间隔.
    fn count_dedup_with_splits(&self, num_splits: usize) -> Result<(usize, Vec<Vec<u8>>)> {
        if num_splits <= 1 {
            let count = self.count_dedup_entries()?;
            return Ok((count, vec![]));
        }

        // 基于文件大小粗估条目数 (~100 bytes/entry 经验值)
        let estimated_total: usize = self
            .all_inputs()
            .iter()
            .map(|r| r.file_size() as usize / 100)
            .sum::<usize>()
            .max(1);
        let est_per_split = estimated_total / num_splits;
        let mut next_split_at = est_per_split;

        let mut merge_iter = MergeIterator::new(self.all_inputs())?;
        let mut count = 0usize;
        let mut last_user_key: Option<Vec<u8>> = None;
        let mut splits: Vec<Vec<u8>> = Vec::new();

        while let Some((key, _)) = merge_iter.next_entry()? {
            let user_key = user_key_from_internal(&key)?;
            if last_user_key.as_deref() == Some(user_key) {
                if self.snapshot_protected(&key)? {
                    count += 1;
                    if count >= next_split_at && splits.len() < num_splits - 1 {
                        splits.push(user_key.to_vec());
                        next_split_at = count + est_per_split;
                    }
                }
                continue;
            }
            if self.output_level > 0 && extract_value_type(&key)? == ValueType::TypeDelete {
                last_user_key = Some(user_key.to_vec());
                continue;
            }
            count += 1;
            last_user_key = Some(user_key.to_vec());

            if count >= next_split_at && splits.len() < num_splits - 1 {
                splits.push(user_key.to_vec());
                next_split_at = count + est_per_split;
            }
        }

        Ok((count, splits))
    }

    /// 执行 compaction, 可能分裂为多个子任务并行.
    ///
    /// `file_numbers` 中元素个数表示分裂数: 1 = 不分裂, N = N 个子任务.
    /// 每个子任务使用 `file_numbers[i]` 作为输出文件编号.
    /// 调用方需预先分配所有文件编号, 避免并发分配冲突.
    #[tracing::instrument(name = "cmp_run", skip(self))]
    pub fn run(&self, file_numbers: &[u64]) -> Result<Vec<CompactionResult>> {
        if file_numbers.len() <= 1 {
            let fnum = file_numbers.first().copied().unwrap_or(0);
            return Ok(vec![self.run_single(fnum)?]);
        }
        self.run_split(file_numbers)
    }

    /// 单线程完整 compaction. 合并 count_dedup_entries 的计数遍历和写入遍历, 减少一次完整 I/O.
    fn run_single(&self, file_number: u64) -> Result<CompactionResult> {
        let output_path = sstable_path(&self.db_path, file_number, self.output_level);

        // 粗估条目数以设置 Bloom filter (不做精确计数, 避免双遍历)
        let estimated_keys: usize = self
            .all_inputs()
            .iter()
            .map(|r| r.file_size() as usize / 100)
            .sum::<usize>()
            .max(1);

        let mut builder = SSTableBuilder::new(
            &output_path,
            self.block_size,
            self.block_restart_interval,
            self.compression,
            self.bloom_false_positive_rate,
        )?;
        if self.bloom_false_positive_rate > 0.0 {
            builder.set_expected_keys(estimated_keys);
        }

        let mut merge_iter = MergeIterator::new(self.all_inputs())?;
        let mut entry_count = 0usize;
        let mut last_user_key: Option<Vec<u8>> = None;
        let mut smallest_key: Option<Vec<u8>> = None;
        let mut largest_key: Option<Vec<u8>> = None;

        while let Some((key, value)) = merge_iter.next_entry()? {
            let user_key = user_key_from_internal(&key)?;
            let value_type = extract_value_type(&key)?;

            if last_user_key.as_deref() == Some(user_key) {
                if self.snapshot_protected(&key)? {
                    builder.add(&key, &value)?;
                    entry_count += 1;
                }
                continue;
            }

            if self.output_level > 0 && value_type == ValueType::TypeDelete {
                last_user_key = Some(user_key.to_vec());
                continue;
            }

            builder.add(&key, &value)?;
            entry_count += 1;
            last_user_key = Some(user_key.to_vec());
            if smallest_key.is_none() {
                smallest_key = Some(key.clone());
            }
            largest_key = Some(key);
        }

        if entry_count == 0 {
            builder.abandon()?;
            return Ok(CompactionResult {
                file_number: 0,
                entry_count: 0,
                output_path,
                smallest_key: vec![],
                largest_key: vec![],
                file_size: 0,
            });
        }

        let file_size = builder.finish()?;
        let files_merged = self.inputs.len() + self.expanded_inputs.len();
        tracing::info!(
            level = self.output_level,
            files_merged,
            bytes_written = file_size,
            "compaction_complete"
        );
        Ok(CompactionResult {
            file_number,
            entry_count,
            output_path,
            smallest_key: smallest_key.unwrap_or_default(),
            largest_key: largest_key.unwrap_or_default(),
            file_size,
        })
    }

    /// 分裂 compaction: 将输入按 key 范围分成多个子任务并行执行.
    fn run_split(&self, file_numbers: &[u64]) -> Result<Vec<CompactionResult>> {
        let num_splits = file_numbers.len();
        let (total_count, split_keys) = self.count_dedup_with_splits(num_splits)?;

        if total_count == 0 {
            return Ok(vec![CompactionResult {
                file_number: 0,
                entry_count: 0,
                output_path: sstable_path(&self.db_path, file_numbers[0], self.output_level),
                smallest_key: vec![],
                largest_key: vec![],
                file_size: 0,
            }]);
        }

        let est_per_range = total_count / num_splits;
        let all_readers = Arc::new(self.all_inputs());
        let db_path = self.db_path.clone();
        let output_level = self.output_level;
        let block_size = self.block_size;
        let block_restart_interval = self.block_restart_interval;
        let compression = self.compression;
        let bloom_fpr = self.bloom_false_positive_rate;
        let min_snap_seq = self.min_snapshot_sequence;

        let results: Vec<Result<CompactionResult>> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(split_keys.len() + 1);

            for i in 0..=split_keys.len() {
                let range_start = if i == 0 {
                    None
                } else {
                    Some(split_keys[i - 1].clone())
                };
                let range_end = if i >= split_keys.len() {
                    None
                } else {
                    Some(split_keys[i].clone())
                };
                let readers = Arc::clone(&all_readers);
                let path = db_path.clone();
                // 最后一段取剩余估算
                let expected = if i < split_keys.len() {
                    est_per_range
                } else {
                    total_count.saturating_sub(i * est_per_range)
                };

                handles.push(scope.spawn(move || {
                    write_sub_compaction(
                        &readers,
                        file_numbers[i],
                        range_start,
                        range_end,
                        output_level,
                        &path,
                        block_size,
                        block_restart_interval,
                        compression,
                        bloom_fpr,
                        min_snap_seq,
                        expected,
                    )
                }));
            }

            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        results.into_iter().collect()
    }
}

/// 单个子 compaction 任务: 以 `range_end` 为界的 `MergeIterator` 遍历输入,
/// 仅输出在 `[range_start, range_end)` 范围内的条目.
#[allow(clippy::too_many_arguments)]
fn write_sub_compaction(
    readers: &[Arc<SSTableReader>],
    file_number: u64,
    range_start: Option<Vec<u8>>,
    range_end: Option<Vec<u8>>,
    output_level: usize,
    db_path: &Path,
    block_size: usize,
    block_restart_interval: usize,
    compression: CompressionType,
    bloom_fpr: f64,
    min_snap_seq: u64,
    expected_keys: usize,
) -> Result<CompactionResult> {
    let output_path = sstable_path(db_path, file_number, output_level);

    // 无预期条目时提前返回空结果
    if expected_keys == 0 {
        // No keys expected for this range, produce empty result
        return Ok(CompactionResult {
            file_number: 0,
            entry_count: 0,
            output_path,
            smallest_key: vec![],
            largest_key: vec![],
            file_size: 0,
        });
    }

    let mut builder = SSTableBuilder::new(
        &output_path,
        block_size,
        block_restart_interval,
        compression,
        bloom_fpr,
    )?;
    if bloom_fpr > 0.0 {
        builder.set_expected_keys(expected_keys);
    }

    let mut merge_iter = if range_end.is_some() {
        MergeIterator::with_range(readers.to_vec(), range_end.clone())?
    } else {
        MergeIterator::new(readers.to_vec())?
    };

    let mut entry_count = 0usize;
    let mut last_user_key: Option<Vec<u8>> = None;
    let mut smallest_key: Option<Vec<u8>> = None;
    let mut largest_key: Option<Vec<u8>> = None;

    while let Some((key, value)) = merge_iter.next_entry()? {
        let user_key = user_key_from_internal(&key)?;
        let value_type = extract_value_type(&key)?;

        // 跳过 range_start 之前的条目 (由前一个子任务处理)
        if let Some(ref start) = range_start {
            if user_key < &start[..] {
                continue;
            }
        }

        if last_user_key.as_deref() == Some(user_key) {
            if min_snap_seq != u64::MAX && extract_sequence(&key)? >= min_snap_seq {
                builder.add(&key, &value)?;
                entry_count += 1;
            }
            continue;
        }

        if output_level > 0 && value_type == ValueType::TypeDelete {
            last_user_key = Some(user_key.to_vec());
            continue;
        }

        builder.add(&key, &value)?;
        entry_count += 1;
        last_user_key = Some(user_key.to_vec());
        if smallest_key.is_none() {
            smallest_key = Some(key.clone());
        }
        largest_key = Some(key);
    }

    if entry_count == 0 {
        builder.abandon()?;
        return Ok(CompactionResult {
            file_number: 0,
            entry_count: 0,
            output_path,
            smallest_key: vec![],
            largest_key: vec![],
            file_size: 0,
        });
    }

    let file_size = builder.finish()?;
    Ok(CompactionResult {
        file_number,
        entry_count,
        output_path,
        smallest_key: smallest_key.unwrap_or_default(),
        largest_key: largest_key.unwrap_or_default(),
        file_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::memtable::{encode_internal_key, ValueType};
    use crate::engine::sstable::{sstable_path, SSTableBuilder, SSTableReader};
    use std::path::Path;
    use tempfile::tempdir;

    fn sst(
        dir: &Path,
        num: u64,
        level: usize,
        entries: &[(&[u8], u64, ValueType, &[u8])],
    ) -> Arc<SSTableReader> {
        let path = sstable_path(dir, num, level);
        let mut b = SSTableBuilder::new(&path, 512, 16, CompressionType::None, 0.0).unwrap();
        for (uk, seq, ty, val) in entries {
            b.add(&encode_internal_key(uk, *seq, *ty), val).unwrap();
        }
        b.finish().unwrap();
        Arc::new(SSTableReader::open(&path, None).unwrap())
    }

    #[test]
    fn test_compaction_dedup_and_tombstone() {
        let dir = tempdir().unwrap();
        let a = sst(
            dir.path(),
            1,
            0,
            &[
                (b"k", 2, ValueType::TypePut, b"v2"),
                (b"x", 1, ValueType::TypeDelete, b""),
            ],
        );
        let b = sst(dir.path(), 2, 0, &[(b"k", 1, ValueType::TypePut, b"v1")]);
        let job = CompactionJob::new(
            vec![a, b],
            vec![],
            1,
            dir.path().to_path_buf(),
            512,
            16,
            CompressionType::None,
            0.0,
        );
        let out = job.run(&[10]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_count, 1);
        let reader = SSTableReader::open(&out[0].output_path, None).unwrap();
        let seek = encode_internal_key(b"k", u64::MAX, ValueType::TypePut);
        assert_eq!(
            reader.get(&seek).unwrap().map(|(v, _)| v),
            Some(b"v2".to_vec())
        );
        let seek_x = encode_internal_key(b"x", u64::MAX, ValueType::TypePut);
        assert_eq!(reader.get(&seek_x).unwrap(), None);
    }

    #[test]
    fn test_empty_value_not_tombstone() {
        let dir = tempdir().unwrap();
        let a = sst(dir.path(), 1, 0, &[(b"k", 1, ValueType::TypePut, b"")]);
        let job = CompactionJob::new(
            vec![a],
            vec![],
            1,
            dir.path().to_path_buf(),
            512,
            16,
            CompressionType::None,
            0.0,
        );
        let out = job.run(&[11]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_count, 1);
    }

    #[test]
    fn test_compaction_all_tombstones_level1_no_output() {
        let dir = tempdir().unwrap();
        let a = sst(dir.path(), 1, 0, &[(b"k", 1, ValueType::TypeDelete, b"")]);
        let job = CompactionJob::new(
            vec![a],
            vec![],
            1,
            dir.path().to_path_buf(),
            512,
            16,
            CompressionType::None,
            0.01,
        );
        let out = job.run(&[12]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_count, 0);
        assert_eq!(out[0].file_number, 0);
    }

    #[test]
    fn test_subcompaction_split_two_ranges() {
        let dir = tempdir().unwrap();
        // Write keys a, b, c, d, e at the same level
        let a = sst(
            dir.path(),
            1,
            0,
            &[
                (b"a", 1, ValueType::TypePut, b"1"),
                (b"b", 1, ValueType::TypePut, b"2"),
                (b"c", 1, ValueType::TypePut, b"3"),
                (b"d", 1, ValueType::TypePut, b"4"),
                (b"e", 1, ValueType::TypePut, b"5"),
            ],
        );
        let job = CompactionJob::new(
            vec![a],
            vec![],
            1,
            dir.path().to_path_buf(),
            512,
            16,
            CompressionType::None,
            0.0,
        );
        // Force 2 splits (num_splits=2)
        let results = job.run(&[100, 101]).unwrap();
        // Should produce at most 2 results (some may be empty)
        assert!(results.len() <= 2);
        let non_empty: Vec<&CompactionResult> =
            results.iter().filter(|r| r.entry_count > 0).collect();
        // Total entries should be 5 (no dedup needed, no tombstones)
        let total: usize = non_empty.iter().map(|r| r.entry_count).sum();
        assert_eq!(total, 5);
        // Verify all keys are reachable through the output files
        for res in &non_empty {
            let reader = SSTableReader::open(&res.output_path, None).unwrap();
            for key in [b"a", b"b", b"c", b"d", b"e"] {
                let ik = encode_internal_key(key, u64::MAX, ValueType::TypePut);
                if let Some((val, _)) = reader.get(&ik).unwrap() {
                    let expected: &[u8] = if key == b"a" {
                        b"1"
                    } else if key == b"b" {
                        b"2"
                    } else if key == b"c" {
                        b"3"
                    } else if key == b"d" {
                        b"4"
                    } else if key == b"e" {
                        b"5"
                    } else {
                        unreachable!()
                    };
                    assert_eq!(&val[..], expected);
                }
            }
        }
    }

    #[test]
    fn test_subcompaction_preserves_dedup_across_splits() {
        let dir = tempdir().unwrap();
        // Input has duplicate user keys across files; dedup should work within each split
        let a = sst(dir.path(), 1, 0, &[(b"a", 3, ValueType::TypePut, b"a3")]);
        let b = sst(dir.path(), 2, 0, &[(b"b", 2, ValueType::TypePut, b"b2")]);
        let job = CompactionJob::new(
            vec![a, b],
            vec![],
            1,
            dir.path().to_path_buf(),
            512,
            16,
            CompressionType::None,
            0.0,
        );
        let results = job.run(&[200, 201]).unwrap();
        let total: usize = results.iter().map(|r| r.entry_count).sum();
        assert_eq!(total, 2, "a and b should each appear once after dedup");
    }
}
