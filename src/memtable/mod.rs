//! # MemTable - In-Memory Sorted Table
//!
//! The MemTable is an in-memory data structure that stores recent writes.
//! It uses a SkipList for efficient concurrent reads and writes.
//!
//! ## Design
//!
//! - Based on crossbeam-skiplist for lock-free concurrent access
//! - Supports Put, Get, and Delete (via tombstone) operations
//! - Tracks size to determine when to flush to disk
//! - Provides an iterator for ordered traversal
//!
//! ## Thread Safety
//!
//! MemTable is designed to be thread-safe with multiple concurrent readers
//! and writers (crossbeam-skiplist provides this guarantee).

mod internal_key;

pub use internal_key::{InternalKey, ValueType};

use crossbeam_skiplist::SkipMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Default size limit for MemTable (4MB)
pub const DEFAULT_MEMTABLE_SIZE_LIMIT: usize = 4 * 1024 * 1024;

/// MemTable stores recent writes in memory using a SkipList.
///
/// # Design
///
/// - Uses `InternalKey` for sorting (user_key + sequence + type)
/// - Sequence numbers provide MVCC semantics
/// - Delete operations are represented as tombstones
/// - Size is tracked to trigger flushes when full
///
/// # Example
///
/// ```rust,no_run
/// use aidb::memtable::{MemTable, ValueType};
///
/// let memtable = MemTable::new(1); // Start with sequence 1
/// memtable.put(b"key1", b"value1", 1);
/// assert_eq!(memtable.get(b"key1", 2), Some(b"value1".to_vec()));
/// ```
pub struct MemTable {
    /// The underlying SkipList storing InternalKey -> Value
    data: Arc<SkipMap<InternalKey, Vec<u8>>>,

    /// Approximate size in bytes (keys + values)
    size: AtomicUsize,

    /// The starting sequence number for this MemTable
    start_sequence: u64,
}

impl MemTable {
    /// Creates a new empty MemTable.
    ///
    /// # Arguments
    ///
    /// * `start_sequence` - The starting sequence number for this MemTable
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(100);
    /// ```
    pub fn new(start_sequence: u64) -> Self {
        Self { data: Arc::new(SkipMap::new()), size: AtomicUsize::new(0), start_sequence }
    }

    /// Inserts a key-value pair into the MemTable.
    ///
    /// # Arguments
    ///
    /// * `key` - The user key
    /// * `value` - The value to store
    /// * `sequence` - The sequence number for this operation
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key", b"value", 1);
    /// ```
    pub fn put(&self, key: &[u8], value: &[u8], sequence: u64) {
        let internal_key = InternalKey::new(key.to_vec(), sequence, ValueType::Value);
        let value_vec = value.to_vec();

        // Calculate the size of this entry
        let entry_size = internal_key.user_key().len() + value_vec.len() + 16; // 16 bytes overhead

        self.data.insert(internal_key, value_vec);
        self.size.fetch_add(entry_size, Ordering::Relaxed);
    }

    /// Retrieves the value for a key.
    ///
    /// Returns the value if found and not deleted, `None` otherwise.
    /// The lookup will find the entry with the highest sequence number <= max_sequence.
    ///
    /// # Arguments
    ///
    /// * `key` - The user key to look up
    /// * `max_sequence` - The maximum sequence number to consider (for MVCC)
    ///
    /// # Returns
    ///
    /// - `Some(value)` if the key exists and is not deleted
    /// - `None` if the key doesn't exist or is deleted
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key", b"value", 1);
    /// assert_eq!(memtable.get(b"key", 100), Some(b"value".to_vec()));
    /// ```
    pub fn get(&self, key: &[u8], max_sequence: u64) -> Option<Vec<u8>> {
        // Create range bounds for the user key
        // Lower bound: key with max possible sequence (u64::MAX)
        // Upper bound: next key with max sequence
        let lower_bound = InternalKey::new(key.to_vec(), u64::MAX, ValueType::Value);

        // Create an upper bound by appending a byte to the key
        let mut upper_key = key.to_vec();
        upper_key.push(0);
        let upper_bound = InternalKey::new(upper_key, u64::MAX, ValueType::Value);

        // Iterate through entries with matching user key
        let range = self.data.range(lower_bound..upper_bound);

        // Find the most recent entry with sequence <= max_sequence
        for entry in range {
            let internal_key = entry.key();
            let value = entry.value();

            // Double-check the user key matches (it should, given our range)
            if internal_key.user_key() == key && internal_key.sequence() <= max_sequence {
                match internal_key.value_type() {
                    ValueType::Value => return Some(value.clone()),
                    ValueType::Deletion => return None,
                }
            }
        }

        None
    }

    /// Marks a key as deleted by inserting a tombstone.
    ///
    /// # Arguments
    ///
    /// * `key` - The user key to delete
    /// * `sequence` - The sequence number for this deletion
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key", b"value", 1);
    /// memtable.delete(b"key", 2);
    /// assert_eq!(memtable.get(b"key", 100), None);
    /// ```
    pub fn delete(&self, key: &[u8], sequence: u64) {
        let internal_key = InternalKey::new(key.to_vec(), sequence, ValueType::Deletion);

        // Tombstone has no value
        let entry_size = internal_key.user_key().len() + 16; // 16 bytes overhead

        self.data.insert(internal_key, Vec::new());
        self.size.fetch_add(entry_size, Ordering::Relaxed);
    }

    /// Returns the approximate size of the MemTable in bytes.
    ///
    /// This includes the size of keys and values, plus some overhead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key", b"value", 1);
    /// assert!(memtable.approximate_size() > 0);
    /// ```
    pub fn approximate_size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// Returns the number of entries in the MemTable.
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key1", b"value1", 1);
    /// memtable.put(b"key2", b"value2", 2);
    /// assert_eq!(memtable.len(), 2);
    /// ```
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the MemTable contains no entries.
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// assert!(memtable.is_empty());
    /// memtable.put(b"key", b"value", 1);
    /// assert!(!memtable.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Returns an iterator over the MemTable entries.
    ///
    /// The iterator yields entries in sorted order by InternalKey.
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key1", b"value1", 1);
    /// memtable.put(b"key2", b"value2", 2);
    ///
    /// for entry in memtable.iter() {
    ///     println!("Key: {:?}", entry.key().user_key());
    /// }
    /// ```
    pub fn iter(&self) -> MemTableIterator {
        MemTableIterator::new(self.data.clone())
    }

    /// Returns the starting sequence number for this MemTable.
    pub fn start_sequence(&self) -> u64 {
        self.start_sequence
    }
}

/// Iterator over MemTable entries in sorted order.
pub struct MemTableIterator {
    _data: Arc<SkipMap<InternalKey, Vec<u8>>>,
    iter: crossbeam_skiplist::map::Iter<'static, InternalKey, Vec<u8>>,
}

impl MemTableIterator {
    fn new(data: Arc<SkipMap<InternalKey, Vec<u8>>>) -> Self {
        // SAFETY: We're using Arc to keep the SkipMap alive for the lifetime of the iterator
        let iter = unsafe {
            std::mem::transmute::<
                crossbeam_skiplist::map::Iter<'_, InternalKey, Vec<u8>>,
                crossbeam_skiplist::map::Iter<'static, InternalKey, Vec<u8>>,
            >(data.iter())
        };

        Self { _data: data, iter }
    }

    /// Returns the current entry without advancing the iterator.
    pub fn peek(&self) -> Option<(&InternalKey, &Vec<u8>)> {
        // This is a simplified implementation
        // A full implementation would need to track the current position
        None
    }
}

impl Iterator for MemTableIterator {
    type Item = MemTableEntry;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|entry| MemTableEntry { key: entry.key().clone(), value: entry.value().clone() })
    }
}

/// A single entry in the MemTable.
#[derive(Debug, Clone)]
pub struct MemTableEntry {
    key: InternalKey,
    value: Vec<u8>,
}

impl MemTableEntry {
    /// Returns the internal key of this entry.
    pub fn key(&self) -> &InternalKey {
        &self.key
    }

    /// Returns the value of this entry.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Returns the user key (without sequence number and type).
    pub fn user_key(&self) -> &[u8] {
        self.key.user_key()
    }

    /// Returns the sequence number of this entry.
    pub fn sequence(&self) -> u64 {
        self.key.sequence()
    }

    /// Returns the value type (Value or Deletion).
    pub fn value_type(&self) -> ValueType {
        self.key.value_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memtable_new() {
        let memtable = MemTable::new(100);
        assert_eq!(memtable.start_sequence(), 100);
        assert!(memtable.is_empty());
        assert_eq!(memtable.len(), 0);
    }

    #[test]
    fn test_memtable_put_and_get() {
        let memtable = MemTable::new(1);

        memtable.put(b"key1", b"value1", 1);
        memtable.put(b"key2", b"value2", 2);

        assert_eq!(memtable.get(b"key1", 100), Some(b"value1".to_vec()));
        assert_eq!(memtable.get(b"key2", 100), Some(b"value2".to_vec()));
        assert_eq!(memtable.get(b"key3", 100), None);

        assert_eq!(memtable.len(), 2);
        assert!(!memtable.is_empty());
    }

    #[test]
    fn test_memtable_delete() {
        let memtable = MemTable::new(1);

        memtable.put(b"key1", b"value1", 1);
        assert_eq!(memtable.get(b"key1", 100), Some(b"value1".to_vec()));

        memtable.delete(b"key1", 2);
        assert_eq!(memtable.get(b"key1", 100), None);

        // Entry still exists (as tombstone)
        assert_eq!(memtable.len(), 2);
    }

    #[test]
    fn test_memtable_mvcc() {
        let memtable = MemTable::new(1);

        memtable.put(b"key1", b"value1", 1);
        memtable.put(b"key1", b"value2", 2);
        memtable.put(b"key1", b"value3", 3);

        // Should get the version at sequence 1
        assert_eq!(memtable.get(b"key1", 1), Some(b"value1".to_vec()));

        // Should get the version at sequence 2
        assert_eq!(memtable.get(b"key1", 2), Some(b"value2".to_vec()));

        // Should get the latest version
        assert_eq!(memtable.get(b"key1", 100), Some(b"value3".to_vec()));
    }

    #[test]
    fn test_memtable_size() {
        let memtable = MemTable::new(1);

        let initial_size = memtable.approximate_size();
        assert_eq!(initial_size, 0);

        memtable.put(b"key1", b"value1", 1);
        assert!(memtable.approximate_size() > initial_size);

        let size_after_first = memtable.approximate_size();
        memtable.put(b"key2", b"value2", 2);
        assert!(memtable.approximate_size() > size_after_first);
    }

    #[test]
    fn test_memtable_iterator() {
        let memtable = MemTable::new(1);

        memtable.put(b"key1", b"value1", 1);
        memtable.put(b"key2", b"value2", 2);
        memtable.put(b"key3", b"value3", 3);

        let entries: Vec<_> = memtable.iter().collect();
        assert_eq!(entries.len(), 3);

        // Verify keys are in sorted order
        assert_eq!(entries[0].user_key(), b"key1");
        assert_eq!(entries[1].user_key(), b"key2");
        assert_eq!(entries[2].user_key(), b"key3");
    }

    #[test]
    fn test_memtable_overwrite() {
        let memtable = MemTable::new(1);

        memtable.put(b"key1", b"value1", 1);
        memtable.put(b"key1", b"value2", 2);

        // Should return the latest value
        assert_eq!(memtable.get(b"key1", 100), Some(b"value2".to_vec()));

        // But both entries exist in the table
        assert_eq!(memtable.len(), 2);
    }

    #[test]
    fn test_memtable_concurrent_access() {
        use std::thread;

        let memtable = Arc::new(MemTable::new(1));
        let mut handles = vec![];

        // Spawn multiple writer threads
        for i in 0..10 {
            let mt = memtable.clone();
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let key = format!("key{}", i * 100 + j);
                    let value = format!("value{}", i * 100 + j);
                    mt.put(key.as_bytes(), value.as_bytes(), (i * 100 + j) as u64);
                }
            });
            handles.push(handle);
        }

        // Wait for all writers to finish
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all entries were written
        assert_eq!(memtable.len(), 1000);

        // Spawn multiple reader threads
        let mut handles = vec![];
        for i in 0..10 {
            let mt = memtable.clone();
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let key = format!("key{}", i * 100 + j);
                    let expected = format!("value{}", i * 100 + j);
                    assert_eq!(
                        mt.get(key.as_bytes(), u64::MAX),
                        Some(expected.as_bytes().to_vec())
                    );
                }
            });
            handles.push(handle);
        }

        // Wait for all readers to finish
        for handle in handles {
            handle.join().unwrap();
        }
    }
}
