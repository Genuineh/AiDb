//! Multi-way merge iterator for compaction.
//!
//! This module provides an iterator that merges multiple SSTable iterators
//! into a single sorted stream.

use crate::error::Result;
use crate::sstable::SSTableReader;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;

/// Entry in the merge heap
struct MergeEntry {
    key: Vec<u8>,
    value: Vec<u8>,
    iterator_index: usize,
}

impl PartialEq for MergeEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for MergeEntry {}

impl PartialOrd for MergeEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MergeEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (smallest key first)
        other.key.cmp(&self.key).then_with(|| {
            // For equal keys, prefer smaller iterator index (newer data)
            other.iterator_index.cmp(&self.iterator_index)
        })
    }
}

/// Multi-way merge iterator over multiple SSTables
///
/// This iterator merges entries from multiple SSTables in sorted order.
/// For duplicate keys, it keeps the entry from the iterator with the smallest index
/// (which typically corresponds to the newest data).
pub struct MergeIterator {
    heap: BinaryHeap<MergeEntry>,
    iterators: Vec<Box<crate::sstable::reader::SSTableIterator>>,
}

impl MergeIterator {
    /// Create a new merge iterator from multiple SSTable readers
    pub fn new(readers: Vec<Arc<SSTableReader>>) -> Result<Self> {
        let mut iterators = Vec::new();
        let mut heap = BinaryHeap::new();

        for (idx, reader) in readers.into_iter().enumerate() {
            let mut iter = reader.iter();
            iter.seek_to_first()?;

            // Add the first entry from this iterator to the heap
            if iter.advance()? && iter.valid() {
                heap.push(MergeEntry {
                    key: iter.key().to_vec(),
                    value: iter.value().to_vec(),
                    iterator_index: idx,
                });
            }

            iterators.push(Box::new(iter));
        }

        Ok(Self { heap, iterators })
    }

    /// Advance the iterator at the given index and add its next entry to the heap
    fn advance_iterator(&mut self, index: usize) -> Result<()> {
        if index >= self.iterators.len() {
            return Ok(());
        }

        let iter = &mut self.iterators[index];
        if iter.advance()? && iter.valid() {
            self.heap.push(MergeEntry {
                key: iter.key().to_vec(),
                value: iter.value().to_vec(),
                iterator_index: index,
            });
        }

        Ok(())
    }
}

impl Iterator for MergeIterator {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        // Pop the smallest entry from the heap
        let entry = self.heap.pop()?;

        // Advance the iterator that provided this entry
        if let Err(e) = self.advance_iterator(entry.iterator_index) {
            log::error!("Error advancing iterator: {}", e);
            return None;
        }

        Some((entry.key, entry.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sstable::SSTableBuilder;
    use tempfile::TempDir;

    fn create_sstable(
        dir: &TempDir,
        file_num: u64,
        entries: &[(&[u8], &[u8])],
    ) -> Arc<SSTableReader> {
        let path = dir.path().join(format!("{:06}.sst", file_num));
        let mut builder = SSTableBuilder::new(&path).unwrap();

        for (key, value) in entries {
            builder.add(key, value).unwrap();
        }
        builder.finish().unwrap();

        Arc::new(SSTableReader::open(&path).unwrap())
    }

    #[test]
    fn test_merge_iterator_two_tables() {
        let temp_dir = TempDir::new().unwrap();

        // Create two SSTables with non-overlapping keys
        let table1 = create_sstable(
            &temp_dir,
            1,
            &[(b"a", b"1"), (b"c", b"3"), (b"e", b"5")],
        );

        let table2 = create_sstable(
            &temp_dir,
            2,
            &[(b"b", b"2"), (b"d", b"4"), (b"f", b"6")],
        );

        let merge_iter = MergeIterator::new(vec![table1, table2]).unwrap();
        let result: Vec<_> = merge_iter.collect();

        assert_eq!(result.len(), 6);
        assert_eq!(result[0], (b"a".to_vec(), b"1".to_vec()));
        assert_eq!(result[1], (b"b".to_vec(), b"2".to_vec()));
        assert_eq!(result[2], (b"c".to_vec(), b"3".to_vec()));
        assert_eq!(result[3], (b"d".to_vec(), b"4".to_vec()));
        assert_eq!(result[4], (b"e".to_vec(), b"5".to_vec()));
        assert_eq!(result[5], (b"f".to_vec(), b"6".to_vec()));
    }

    #[test]
    fn test_merge_iterator_overlapping_keys() {
        let temp_dir = TempDir::new().unwrap();

        // Create two SSTables with overlapping keys
        // Table 1 (newer) should take precedence
        let table1 = create_sstable(
            &temp_dir,
            1,
            &[(b"a", b"new_a"), (b"c", b"new_c")],
        );

        let table2 = create_sstable(
            &temp_dir,
            2,
            &[(b"a", b"old_a"), (b"b", b"old_b"), (b"c", b"old_c")],
        );

        let merge_iter = MergeIterator::new(vec![table1, table2]).unwrap();
        let result: Vec<_> = merge_iter.collect();

        // Should have all keys, but duplicates come from table1 (newer)
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], (b"a".to_vec(), b"new_a".to_vec()));
        assert_eq!(result[1], (b"a".to_vec(), b"old_a".to_vec()));
        assert_eq!(result[2], (b"b".to_vec(), b"old_b".to_vec()));
        assert_eq!(result[3], (b"c".to_vec(), b"new_c".to_vec()));
        assert_eq!(result[4], (b"c".to_vec(), b"old_c".to_vec()));
    }

    #[test]
    fn test_merge_iterator_empty_table() {
        let temp_dir = TempDir::new().unwrap();

        let table1 = create_sstable(&temp_dir, 1, &[(b"a", b"1"), (b"b", b"2")]);
        let table2 = create_sstable(&temp_dir, 2, &[]); // Empty table

        let merge_iter = MergeIterator::new(vec![table1, table2]).unwrap();
        let result: Vec<_> = merge_iter.collect();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (b"a".to_vec(), b"1".to_vec()));
        assert_eq!(result[1], (b"b".to_vec(), b"2".to_vec()));
    }

    #[test]
    fn test_merge_iterator_single_table() {
        let temp_dir = TempDir::new().unwrap();

        let table = create_sstable(
            &temp_dir,
            1,
            &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")],
        );

        let merge_iter = MergeIterator::new(vec![table]).unwrap();
        let result: Vec<_> = merge_iter.collect();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], (b"a".to_vec(), b"1".to_vec()));
        assert_eq!(result[1], (b"b".to_vec(), b"2".to_vec()));
        assert_eq!(result[2], (b"c".to_vec(), b"3".to_vec()));
    }

    #[test]
    fn test_merge_iterator_multiple_tables() {
        let temp_dir = TempDir::new().unwrap();

        let table1 = create_sstable(&temp_dir, 1, &[(b"a", b"1"), (b"d", b"4")]);
        let table2 = create_sstable(&temp_dir, 2, &[(b"b", b"2"), (b"e", b"5")]);
        let table3 = create_sstable(&temp_dir, 3, &[(b"c", b"3"), (b"f", b"6")]);

        let merge_iter = MergeIterator::new(vec![table1, table2, table3]).unwrap();
        let result: Vec<_> = merge_iter.collect();

        assert_eq!(result.len(), 6);
        assert_eq!(result[0], (b"a".to_vec(), b"1".to_vec()));
        assert_eq!(result[1], (b"b".to_vec(), b"2".to_vec()));
        assert_eq!(result[2], (b"c".to_vec(), b"3".to_vec()));
        assert_eq!(result[3], (b"d".to_vec(), b"4".to_vec()));
        assert_eq!(result[4], (b"e".to_vec(), b"5".to_vec()));
        assert_eq!(result[5], (b"f".to_vec(), b"6".to_vec()));
    }
}
