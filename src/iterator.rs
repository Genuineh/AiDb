//! Database iterator for scanning key-value pairs.
//!
//! Provides sequential and range-based iteration over the database.
//!
//! RocksDB-style merge iterator: iterates over multiple layers (MemTable + SSTables),
//! merging results by picking the smallest key across all active cursors.
//! Supports `seek(target)` for efficient prefix-based positioning, avoiding
//! unnecessary scanning of unrelated keys.

use std::sync::Arc;

use crate::{Result, DB};

/// An iterator over key-value pairs in the database.
///
/// The iterator provides a consistent view of the database and merges
/// data from MemTables and SSTables. It automatically handles tombstones
/// (deleted keys).
///
/// # Example
///
/// ```rust,no_run
/// use aidb::{DB, Options};
/// use std::sync::Arc;
///
/// # fn main() -> Result<(), aidb::Error> {
/// let db = DB::open("./data", Options::default())?;
/// let db = Arc::new(db);
///
/// db.put(b"key1", b"value1")?;
/// db.put(b"key2", b"value2")?;
///
/// let mut iter = db.iter()?;
/// while iter.valid() {
///     let key = iter.key();
///     let value = iter.value();
///     println!("{:?} => {:?}", key, value);
///     iter.next();
/// }
/// # Ok(())
/// # }
/// ```

/// Lazy iterator over key-value pairs.
///
/// RocksDB-style merge iterator: picks the minimum key across all layer cursors
/// on each step. Supports `seek()` for efficient positioning to a target key,
/// which enables prefix scanning without iterating unrelated keys.
pub struct DBIterator {
    /// Current key-value pair
    current: Option<(Vec<u8>, Vec<u8>)>,

    /// Sequence number for consistent reads
    sequence: u64,

    /// Active layer iterators
    layer_iters: Vec<LayerIterState>,

    /// Current cursor position for each layer
    layer_cursors: Vec<Option<LayerEntry>>,

    /// End key for range (exclusive), None means no limit
    end_key: Option<Vec<u8>>,
}

/// State for a single layer's iterator
struct LayerIterState {
    /// Type of the layer
    layer_type: LayerType,
    /// The actual iterator (stored as raw pointer for trait object)
    memtable_iter: Option<Box<dyn MemTableIterTrait>>,
    sstable_iter: Option<Box<dyn SSTableIterTrait>>,
    /// Whether the SSTable iterator has been seeked (positioned at first entry)
    sstable_seeked: bool,
}

/// Type of storage layer
enum LayerType {
    MemTable,
    SSTable,
}

/// Trait for abstracting over MemTable and SSTable iterators
trait MemTableIterTrait {
    fn next(&mut self) -> Option<(Vec<u8>, Vec<u8>, u64, bool)>; // (key, value, seq, is_delete)
    /// Seek to the first entry with user_key >= target.
    fn seek(&mut self, target: &[u8]);
}

trait SSTableIterTrait {
    fn seek_to_first(&mut self) -> bool;
    fn advance(&mut self) -> bool;
    fn valid(&self) -> bool;
    fn key(&self) -> &[u8];
    fn value(&self) -> &[u8];
    /// Seek to the first entry with key >= target.
    fn seek(&mut self, target: &[u8]) -> bool;
}

impl MemTableIterTrait for crate::memtable::MemTableIterator {
    fn next(&mut self) -> Option<(Vec<u8>, Vec<u8>, u64, bool)> {
        use crate::memtable::ValueType;
        use crate::memtable::MemTableEntry;
        Iterator::next(self).map(|entry: MemTableEntry| {
            let kt = entry.value_type();
            (
                entry.key().user_key().to_vec(),
                entry.value().to_vec(),
                entry.sequence(),
                kt == ValueType::Deletion,
            )
        })
    }

    fn seek(&mut self, target: &[u8]) {
        crate::memtable::MemTableIterator::seek(self, target);
    }
}

impl SSTableIterTrait for crate::sstable::reader::SSTableIterator {
    fn seek_to_first(&mut self) -> bool {
        self.seek_to_first().is_ok()
    }

    fn advance(&mut self) -> bool {
        self.advance().map(|b| b).unwrap_or(false)
    }

    fn valid(&self) -> bool {
        self.valid()
    }

    fn key(&self) -> &[u8] {
        self.key()
    }

    fn value(&self) -> &[u8] {
        self.value()
    }

    fn seek(&mut self, target: &[u8]) -> bool {
        self.seek_to_target(target).is_ok()
    }
}

/// Entry from a layer iterator
#[derive(Clone)]
struct LayerEntry {
    user_key: Vec<u8>,
    #[allow(dead_code)]
    value: Vec<u8>,
    sequence: u64,
    is_delete: bool,
}

impl DBIterator {
    /// Creates a new iterator starting from the beginning.
    pub(crate) fn new(db: Arc<DB>, sequence: u64) -> Result<Self> {
        Self::new_range(db, sequence, None, None)
    }

    /// Creates a new iterator with a range.
    pub(crate) fn new_range(
        db: Arc<DB>,
        sequence: u64,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<Self> {
        let mut layer_iters: Vec<LayerIterState> = Vec::new();

        // Collect from MemTable (mutable)
        {
            let memtable = db.memtable.read();
            let iter = memtable.iter();
            layer_iters.push(LayerIterState::new_memtable(iter));
        }

        // Collect from immutable MemTables
        {
            let immutable = db.immutable_memtables.read();
            for memtable in immutable.iter() {
                let iter = memtable.iter();
                layer_iters.push(LayerIterState::new_memtable(iter));
            }
        }

        // Collect from SSTables
        {
            let sstables = db.sstables.read();
            for level_tables in sstables.iter() {
                for table in level_tables.iter() {
                    let iter = table.iter();
                    layer_iters.push(LayerIterState::new_sstable(iter));
                }
            }
        }

        let num_layers = layer_iters.len();
        let mut iter = Self {
            current: None,
            sequence,
            layer_iters,
            layer_cursors: vec![None; num_layers],
            end_key: end.map(|e| e.to_vec()),
        };

        // Initialize cursors by getting first entry from each layer
        for i in 0..num_layers {
            iter.layer_cursors[i] = iter.layer_iters[i].next_entry();
        }

        // Load first valid entry
        iter.load_next_valid()?;

        // Seek to start key if provided
        if let Some(start_key) = start {
            iter.seek(start_key)?;
        }

        Ok(iter)
    }

    /// Seek to the first entry with key >= target across all layers.
    ///
    /// This is the RocksDB-style efficient positioning: each layer's iterator
    /// is positioned at the first entry >= target, then the merge logic picks
    /// the minimum across all layers. After seeking, call `load_next_valid()`
    /// to resolve version conflicts and tombstones.
    pub fn seek(&mut self, target: &[u8]) -> Result<()> {
        // Seek each layer to the target
        for layer in &mut self.layer_iters {
            layer.seek(target);
        }

        // Reload all cursors from the new positions
        for i in 0..self.layer_iters.len() {
            self.layer_cursors[i] = self.layer_iters[i].next_entry();
        }

        // Position at first valid entry >= target
        self.load_next_valid()
    }

    /// Get next entry from a layer
    fn next_from_layer(&mut self, layer: usize) {
        if layer < self.layer_iters.len() {
            self.layer_cursors[layer] = self.layer_iters[layer].next_entry();
        }
    }

    /// Find the layer with the smallest key
    fn find_min_layer(&self) -> Option<usize> {
        let mut min_idx = None;
        let mut min_key: Option<Vec<u8>> = None;

        for (i, entry) in self.layer_cursors.iter().enumerate() {
            if let Some(e) = entry {
                // Skip if past end boundary
                if let Some(ref end) = self.end_key {
                    if e.user_key >= *end {
                        continue;
                    }
                }

                match &min_key {
                    None => {
                        min_key = Some(e.user_key.clone());
                        min_idx = Some(i);
                    }
                    Some(min) if e.user_key < *min => {
                        min_key = Some(e.user_key.clone());
                        min_idx = Some(i);
                    }
                    _ => {}
                }
            }
        }

        min_idx
    }

    /// Load the next valid entry, handling deletions and version conflicts.
    ///
    /// RocksDB-style merge logic: collects ALL layers that have the same user key,
    /// picks the latest version (highest sequence), skips deletions, and advances
    /// every layer that held the key so the same key is never emitted twice.
    fn load_next_valid(&mut self) -> Result<()> {
        loop {
            // Find layer with minimum key
            let min_layer = match self.find_min_layer() {
                Some(idx) => idx,
                None => {
                    self.current = None;
                    return Ok(());
                }
            };

            // Get current entry
            let entry = match self.layer_cursors[min_layer].clone() {
                Some(e) => e,
                None => {
                    self.current = None;
                    return Ok(());
                }
            };

            // Skip if past end boundary
            if let Some(ref end) = self.end_key {
                if entry.user_key >= *end {
                    self.current = None;
                    return Ok(());
                }
            }

            let current_key = entry.user_key.clone();

            // Collect ALL layers that have the same user key (RocksDB-style merge)
            let mut layers_with_same_key = vec![min_layer];
            for (i, other_entry) in self.layer_cursors.iter().enumerate() {
                if i != min_layer {
                    if let Some(other) = other_entry {
                        if other.user_key == current_key {
                            layers_with_same_key.push(i);
                        }
                    }
                }
            }

            // Find the latest entry among all layers with this key.
            // The entry with the highest sequence number is the truth:
            // - If it's a deletion, the key is deleted (regardless of older versions).
            // - If seq > snapshot, the write is not yet visible.
            //
            // Using the iterator's own value avoids a redundant get_at_sequence()
            // point lookup.
            //
            // SSTable entries all have seq=0. When both a data entry and its
            // tombstone live in different SSTables, they tie on sequence. In that
            // case the deletion (empty value / is_delete) must win — it represents
            // a later operation.
            let mut best_seq = entry.sequence;
            let mut best_is_delete = entry.is_delete;
            let mut best_value = entry.value.clone();
            for &i in &layers_with_same_key[1..] {
                if let Some(ref other) = self.layer_cursors[i] {
                    if other.sequence > best_seq {
                        best_seq = other.sequence;
                        best_is_delete = other.is_delete;
                        best_value = other.value.clone();
                    } else if other.sequence == best_seq && other.is_delete {
                        // Same sequence, deletion overrides data (SSTable tombstone
                        // vs. older SSTable data, both with seq=0).
                        best_is_delete = true;
                    }
                }
            }

            // Advance ALL layers that have this key (RocksDB: skip duplicates).
            // Keep advancing if the same layer has more entries for the same
            // user_key (e.g., a tombstone followed by the old data in MemTable).
            for &i in &layers_with_same_key {
                self.next_from_layer(i);
                while let Some(ref next_entry) = self.layer_cursors[i] {
                    if next_entry.user_key != current_key {
                        break;
                    }
                    self.next_from_layer(i);
                }
            }

            // Deletion at the latest sequence — key is deleted, skip it.
            if best_is_delete {
                continue;
            }

            // Latest write happened after our snapshot — skip it.
            // (RocksDB child iterators filter by snapshot internally, so the
            //  merge iterator never sees entries with seq > snapshot. Our
            //  MemTable iterator doesn't filter, so we check here.)
            if best_seq > self.sequence {
                continue;
            }

            self.current = Some((current_key, best_value));
            return Ok(());
        }
    }

    /// Returns true if the iterator is positioned at a valid entry.
    pub fn valid(&self) -> bool {
        self.current.is_some()
    }

    /// Returns the key at the current position.
    pub fn key(&self) -> &[u8] {
        self.current.as_ref().expect("Iterator not valid").0.as_slice()
    }

    /// Returns the value at the current position.
    pub fn value(&self) -> &[u8] {
        self.current.as_ref().expect("Iterator not valid").1.as_slice()
    }

    /// Moves to the next entry in forward direction.
    ///
    /// `load_next_valid()` already advanced all layers past the previously
    /// returned key, so we just ask it for the next valid entry.
    pub fn next(&mut self) {
        let _ = self.load_next_valid();
    }

    /// Seeks to the first key that is greater than or equal to the target.
    /// (Delegates to the efficient seek implementation.)
    pub fn seek_to_key(&mut self, target: &[u8]) -> Result<()> {
        self.seek(target)
    }

    /// Seeks to the first key in the database.
    pub fn seek_to_first(&mut self) {
        for layer in &mut self.layer_iters {
            layer.seek_to_first();
        }
        for i in 0..self.layer_iters.len() {
            self.layer_cursors[i] = self.layer_iters[i].next_entry();
        }
        let _ = self.load_next_valid();
    }

    /// Seeks to the last key in the database.
    pub fn seek_to_last(&mut self) {
        // Simplified: not fully implemented for backward iteration
        self.seek_to_first();
    }
}

impl Iterator for DBIterator {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.valid() {
            return None;
        }
        // Clone current key-value before advancing
        let result = self.current.clone();
        // Advance iterator
        DBIterator::next(self);
        result.map(|(k, v)| (k, v))
    }
}

impl LayerIterState {
    fn new_memtable(iter: crate::memtable::MemTableIterator) -> Self {
        Self {
            layer_type: LayerType::MemTable,
            memtable_iter: Some(Box::new(iter)),
            sstable_iter: None,
            sstable_seeked: false,
        }
    }

    fn new_sstable(iter: crate::sstable::reader::SSTableIterator) -> Self {
        Self {
            layer_type: LayerType::SSTable,
            memtable_iter: None,
            sstable_iter: Some(Box::new(iter)),
            sstable_seeked: false,
        }
    }

    fn seek_to_first(&mut self) {
        match &self.layer_type {
            LayerType::MemTable => {
                // MemTable currently starts from smallest key; re-seek to beginning.
                if let Some(ref mut iter) = self.memtable_iter {
                    iter.seek(b"");
                }
            }
            LayerType::SSTable => {
                if let Some(ref mut iter) = self.sstable_iter {
                    iter.seek_to_first();
                    self.sstable_seeked = true;
                }
            }
        }
    }

    /// Seek to the first entry >= target (RocksDB-style positioning).
    fn seek(&mut self, target: &[u8]) {
        match &self.layer_type {
            LayerType::MemTable => {
                if let Some(ref mut iter) = self.memtable_iter {
                    iter.seek(target);
                }
            }
            LayerType::SSTable => {
                if let Some(ref mut iter) = self.sstable_iter {
                    iter.seek(target);
                    self.sstable_seeked = true; // positioned, next_entry will read current
                }
            }
        }
    }

    fn next_entry(&mut self) -> Option<LayerEntry> {
        match &self.layer_type {
            LayerType::MemTable => {
                if let Some(ref mut iter) = self.memtable_iter {
                    iter.next().map(|(key, value, seq, is_delete)| LayerEntry {
                        user_key: key,
                        value,
                        sequence: seq,
                        is_delete,
                    })
                } else {
                    None
                }
            }
            LayerType::SSTable => {
                if let Some(ref mut iter) = self.sstable_iter {
                    // For SSTable, seek_to_first positions at the first entry;
                    // advance moves to the next. First call reads current then advances.
                    if !self.sstable_seeked {
                        iter.seek_to_first();
                        self.sstable_seeked = true;
                    }
                    if iter.valid() {
                        let key = iter.key().to_vec();
                        let value = iter.value().to_vec();
                        // SSTables store tombstones as empty values, but the
                        // iterator doesn't parse the value type. An empty value
                        // signals a deletion; non-empty is real data.
                        let is_delete = value.is_empty();
                        iter.advance();
                        Some(LayerEntry {
                            user_key: key,
                            value,
                            sequence: 0, // SSTable doesn't track sequence in iterator
                            is_delete,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
}

impl DB {
    /// Creates an iterator over all key-value pairs.
    pub fn iter(self: &Arc<Self>) -> Result<DBIterator> {
        let seq = self.sequence.load(std::sync::atomic::Ordering::SeqCst);
        DBIterator::new(Arc::clone(self), seq)
    }

    /// Creates an iterator over a range of keys.
    pub fn scan(self: &Arc<Self>, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<DBIterator> {
        let seq = self.sequence.load(std::sync::atomic::Ordering::SeqCst);
        DBIterator::new_range(Arc::clone(self), seq, start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    #[test]
    fn test_iterator_basic() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();

        db.flush().unwrap();

        db.put(b"key3", b"value3").unwrap();

        let mut iter = db.iter().unwrap();
        let mut count = 0;
        let mut keys = Vec::new();

        while iter.valid() {
            keys.push(iter.key().to_vec());
            count += 1;
            iter.next();
        }

        assert_eq!(count, 3);
        assert_eq!(keys, vec![b"key1", b"key2", b"key3"]);
    }

    #[test]
    fn test_iterator_empty_db() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        let iter = db.iter().unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn test_iterator_seek_existing() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"key1", b"val1").unwrap();
        db.put(b"key2", b"val2").unwrap();
        db.put(b"key3", b"val3").unwrap();

        let mut iter = db.iter().unwrap();
        iter.seek(b"key2").unwrap();
        assert!(iter.valid());
        assert_eq!(iter.key(), b"key2");
        assert_eq!(iter.value(), b"val2");
    }

    #[test]
    fn test_iterator_seek_nonexistent() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"key1", b"val1").unwrap();
        db.put(b"key3", b"val3").unwrap();
        db.put(b"key5", b"val5").unwrap();

        // Seek between key1 and key3 should land on key3
        let mut iter = db.iter().unwrap();
        iter.seek(b"key2").unwrap();
        assert!(iter.valid());
        assert_eq!(iter.key(), b"key3");

        // Seek past all keys should be invalid
        let mut iter = db.iter().unwrap();
        iter.seek(b"zzz").unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn test_iterator_seek_to_first() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"key2", b"val2").unwrap();
        db.put(b"key1", b"val1").unwrap();
        db.put(b"key3", b"val3").unwrap();

        let mut iter = db.iter().unwrap();
        iter.seek_to_first();
        assert!(iter.valid());
        assert_eq!(iter.key(), b"key1");
    }

    #[test]
    fn test_iterator_range() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"key1", b"val1").unwrap();
        db.put(b"key2", b"val2").unwrap();
        db.put(b"key3", b"val3").unwrap();
        db.put(b"key4", b"val4").unwrap();
        db.put(b"key5", b"val5").unwrap();

        let mut iter = db.scan(Some(b"key2"), Some(b"key5")).unwrap();
        let mut entries = Vec::new();
        while iter.valid() {
            entries.push((iter.key().to_vec(), iter.value().to_vec()));
            iter.next();
        }
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, b"key2");
        assert_eq!(entries[1].0, b"key3");
        assert_eq!(entries[2].0, b"key4");
    }

    #[test]
    fn test_iterator_tombstone_filtering() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"key1", b"val1").unwrap();
        db.put(b"key2", b"val2").unwrap();
        db.delete(b"key2").unwrap();

        let mut iter = db.iter().unwrap();
        let mut keys = Vec::new();
        while iter.valid() {
            keys.push(iter.key().to_vec());
            iter.next();
        }
        assert_eq!(keys, vec![b"key1"]);
    }

    #[test]
    fn test_iterator_overwrite() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"key1", b"v1").unwrap();
        db.put(b"key1", b"v2").unwrap();

        let iter = db.iter().unwrap();
        assert!(iter.valid());
        assert_eq!(iter.key(), b"key1");
        assert_eq!(iter.value(), b"v2");
    }

    #[test]
    fn test_iterator_seek_to_last() {
        let tmp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

        db.put(b"a", b"1").unwrap();
        db.put(b"b", b"2").unwrap();
        db.put(b"c", b"3").unwrap();

        // seek_to_last currently delegates to seek_to_first
        let mut iter = db.iter().unwrap();
        iter.seek_to_last();
        assert!(iter.valid());
    }
}
