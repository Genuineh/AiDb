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
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
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
/// - Key count is tracked for accurate DBSIZE without full scan
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

    /// Count of unique visible keys (user_key -> (latest sequence, latest value_type))
    /// Used for O(1) DBSIZE without full scan
    /// ValueType is tracked to distinguish between actual values and tombstones
    key_map: Mutex<HashMap<Vec<u8>, (u64, ValueType)>>,

    /// Total unique visible keys count
    unique_key_count: AtomicUsize,
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
        Self {
            data: Arc::new(SkipMap::new()),
            size: AtomicUsize::new(0),
            start_sequence,
            key_map: Mutex::new(HashMap::new()),
            unique_key_count: AtomicUsize::new(0),
        }
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

        // Track unique key count
        {
            let mut key_map = self.key_map.lock().unwrap();
            let key_vec = key.to_vec();
            let old_entry = key_map.get(&key_vec).cloned();

            match old_entry {
                None => {
                    // New key - increment count
                    self.unique_key_count.fetch_add(1, Ordering::SeqCst);
                }
                Some((_old_seq, old_value_type)) => {
                    // Key exists - only increment if old entry was a tombstone (deletion)
                    // This handles the case: put -> delete -> put (the second put should increment)
                    if old_value_type == ValueType::Deletion {
                        self.unique_key_count.fetch_add(1, Ordering::SeqCst);
                    }
                    // If old entry was also a Value, we don't increment (updating existing key)
                }
            }
            // Store the new sequence and value type
            key_map.insert(key_vec, (sequence, ValueType::Value));
        }

        self.data.insert(internal_key, value_vec);
        self.size.fetch_add(entry_size, Ordering::Relaxed);
    }

    /// Retrieves the value for a key.
    ///
    /// Returns the value if found. For deleted keys (tombstones), returns an empty Vec.
    /// The lookup will find the entry with the highest sequence number <= max_sequence.
    ///
    /// # Arguments
    ///
    /// * `key` - The user key to look up
    /// * `max_sequence` - The maximum sequence number to consider (for MVCC)
    ///
    /// # Returns
    ///
    /// - `Some(value)` if the key exists (non-empty for values, empty for tombstones)
    /// - `None` if the key doesn't exist
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
                    ValueType::Deletion => return Some(Vec::new()), // Return empty Vec for tombstone
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
    /// // Tombstone returns empty Vec, DB layer converts to None
    /// assert_eq!(memtable.get(b"key", 100), Some(Vec::new()));
    /// ```
    pub fn delete(&self, key: &[u8], sequence: u64) {
        let internal_key = InternalKey::new(key.to_vec(), sequence, ValueType::Deletion);

        // Tombstone has no value
        let entry_size = internal_key.user_key().len() + 16; // 16 bytes overhead

        // Track unique key count - only decrement if old entry was a Value (not a tombstone)
        {
            let mut key_map = self.key_map.lock().unwrap();
            let key_vec = key.to_vec();
            if let Some((old_seq, old_value_type)) = key_map.get(&key_vec).cloned() {
                // Only decrement if the old entry was a visible value (not a tombstone)
                // and if this delete has a higher sequence (meaning it actually makes it invisible)
                if old_value_type == ValueType::Value && old_seq < sequence {
                    self.unique_key_count.fetch_sub(1, Ordering::SeqCst);
                }
                // If old entry was a tombstone, we don't decrement (key was already invisible)
            }
            // Store the new tombstone with its sequence
            key_map.insert(key_vec, (sequence, ValueType::Deletion));
        }

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

    /// Returns the approximate number of unique keys in the MemTable.
    ///
    /// This counts user keys that have at least one Value (non-tombstone) entry
    /// visible at the latest sequence. The count is maintained incrementally
    /// during put/delete operations for O(1) lookup.
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::MemTable;
    ///
    /// let memtable = MemTable::new(1);
    /// memtable.put(b"key1", b"value1", 1);
    /// memtable.put(b"key2", b"value2", 2);
    /// assert_eq!(memtable.approximate_unique_key_count(), 2);
    ///
    /// // Update key1
    /// memtable.put(b"key1", b"value1_updated", 3);
    /// assert_eq!(memtable.approximate_unique_key_count(), 2); // Still 2
    ///
    /// // Delete key1
    /// memtable.delete(b"key1", 4);
    /// assert_eq!(memtable.approximate_unique_key_count(), 1); // Now 1
    /// ```
    pub fn approximate_unique_key_count(&self) -> usize {
        self.unique_key_count.load(Ordering::SeqCst)
    }

    /// Returns `true` if the key exists in the MemTable's key_map.
    ///
    /// Unlike `get`, this returns `true` even if the key is marked as deleted (tombstone).
    /// This is useful for checking if a key has ever been written, regardless of visibility.
    pub fn key_map_contains(&self, key: &[u8]) -> bool {
        let key_map = self.key_map.lock().unwrap();
        key_map.contains_key(key)
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

    /// Returns all unique user keys in the MemTable.
    ///
    /// This collects all user keys, removing duplicates (keeping only latest version).
    /// Note: this does not take visibility (sequence) into account — callers that need
    /// snapshot-aware keys should use `keys_at_sequence` instead.
    pub fn keys(&self) -> Vec<Vec<u8>> {
        use std::collections::BTreeSet;

        let mut keys = BTreeSet::new();
        for entry in self.data.iter() {
            keys.insert(entry.key().user_key().to_vec());
        }
        keys.into_iter().collect()
    }

    /// Returns all unique user keys visible at `max_sequence`.
    ///
    /// For each user key, we consider only entries with sequence <= max_sequence
    /// and include the key only if the latest such entry is a Value (not Deletion).
    /// Returned keys are in key-sorted order.
    pub fn keys_at_sequence(&self, max_sequence: u64) -> Vec<Vec<u8>> {
        use std::collections::BTreeMap;

        // Map from user_key -> (sequence, ValueType) for entries with seq <= max_sequence
        let mut latest = BTreeMap::<Vec<u8>, (u64, ValueType)>::new();

        for entry in self.data.iter() {
            let user = entry.key().user_key().to_vec();
            let seq = entry.key().sequence();
            let vtype = entry.key().value_type();

            if seq > max_sequence {
                continue;
            }

            match latest.get(&user) {
                Some(&(existing_seq, _)) => {
                    if seq > existing_seq {
                        latest.insert(user, (seq, vtype));
                    }
                }
                None => {
                    latest.insert(user, (seq, vtype));
                }
            }
        }

        latest
            .into_iter()
            .filter(|(_, (_, t))| *t == ValueType::Value)
            .map(|(k, _)| k)
            .collect()
    }
}

/// Type alias for a full-range skiplist range iterator.
type MemTableRangeIter = crossbeam_skiplist::map::Range<
    'static,
    InternalKey,
    std::ops::RangeFrom<InternalKey>,
    InternalKey,
    Vec<u8>,
>;

/// Iterator over MemTable entries in sorted order.
///
/// Uses a `Range` internally to support efficient seeking (RocksDB-style positioning).
/// The initial range covers all entries; `seek(target)` narrows to a prefix.
pub struct MemTableIterator {
    data: Arc<SkipMap<InternalKey, Vec<u8>>>,
    range: MemTableRangeIter,
}

impl MemTableIterator {
    fn new(data: Arc<SkipMap<InternalKey, Vec<u8>>>) -> Self {
        // Start from the smallest possible InternalKey to cover all entries
        let lower = InternalKey::new(vec![], u64::MAX, ValueType::Value);
        // SAFETY: We're using Arc to keep the SkipMap alive for the lifetime of the iterator
        let range = unsafe {
            std::mem::transmute::<
                crossbeam_skiplist::map::Range<'_, InternalKey, std::ops::RangeFrom<InternalKey>, InternalKey, Vec<u8>>,
                MemTableRangeIter,
            >(data.range(lower..))
        };

        Self { data, range }
    }

    /// Seek to the first entry whose user_key >= target.
    ///
    /// Creates a new Range starting from the target key with u64::MAX sequence
    /// and Value type (the smallest InternalKey for that user_key, since higher
    /// sequences come first in the descending order).
    pub fn seek(&mut self, target: &[u8]) {
        let lower = InternalKey::new(target.to_vec(), u64::MAX, ValueType::Value);
        // SAFETY: The Arc keeps SkipMap alive for the lifetime of the range.
        let range = unsafe {
            std::mem::transmute::<
                crossbeam_skiplist::map::Range<'_, InternalKey, std::ops::RangeFrom<InternalKey>, InternalKey, Vec<u8>>,
                MemTableRangeIter,
            >(self.data.range(lower..))
        };
        self.range = range;
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
        self.range
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
        // After delete, get returns empty Vec (tombstone marker)
        assert_eq!(memtable.get(b"key1", 100), Some(Vec::new()));

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
    fn test_memtable_keys_excludes_tombstones() {
        let memtable = MemTable::new(1);

        memtable.put(b"key1", b"value1", 1);
        memtable.put(b"key2", b"value2", 2);

        let keys = memtable.keys();
        assert!(keys.contains(&b"key1".to_vec()));
        assert!(keys.contains(&b"key2".to_vec()));

        // Delete key1
        memtable.delete(b"key1", 3);
        // keys() should still include the user key (non-snapshot-aware)
        let keys = memtable.keys();
        assert!(keys.contains(&b"key1".to_vec()));

        // But keys_at_sequence with a snapshot before delete should include it,
        // and keys_at_sequence after delete should not.
        let keys_before = memtable.keys_at_sequence(2);
        assert!(keys_before.contains(&b"key1".to_vec()));

        let keys_after = memtable.keys_at_sequence(3);
        assert!(!keys_after.contains(&b"key1".to_vec()));

        // Re-put key1 at higher sequence -> should reappear for later sequences
        memtable.put(b"key1", b"value3", 4);
        let keys_late = memtable.keys_at_sequence(5);
        assert!(keys_late.contains(&b"key1".to_vec()));
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
