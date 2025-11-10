//! Database iterator for scanning key-value pairs.
//!
//! Provides sequential and range-based iteration over the database.

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
/// // Insert some data
/// db.put(b"key1", b"value1")?;
/// db.put(b"key2", b"value2")?;
/// db.put(b"key3", b"value3")?;
///
/// // Create an iterator
/// let mut iter = db.iter();
///
/// // Iterate through all keys
/// while iter.valid() {
///     let key = iter.key();
///     let value = iter.value();
///     println!("{:?} => {:?}", key, value);
///     iter.next();
/// }
/// # Ok(())
/// # }
/// ```
pub struct DBIterator {
    /// Reference to the database
    db: Arc<DB>,
    
    /// Current key-value pair
    current: Option<(Vec<u8>, Vec<u8>)>,
    
    /// Sequence number for consistent reads
    sequence: u64,
    
    /// All keys in sorted order (cached for simplicity)
    keys: Vec<Vec<u8>>,
    
    /// Current position in the keys vector
    position: usize,
}

impl DBIterator {
    /// Creates a new iterator starting from the beginning.
    pub(crate) fn new(db: Arc<DB>, sequence: u64) -> Result<Self> {
        let mut iter = Self {
            db,
            current: None,
            sequence,
            keys: Vec::new(),
            position: 0,
        };
        
        // Collect all keys from the database
        iter.collect_keys(None, None)?;
        
        // Position at the first key
        if !iter.keys.is_empty() {
            iter.position = 0;
            iter.load_current()?;
        }
        
        Ok(iter)
    }

    /// Creates a new iterator with a range.
    pub(crate) fn new_range(
        db: Arc<DB>,
        sequence: u64,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<Self> {
        let mut iter = Self {
            db,
            current: None,
            sequence,
            keys: Vec::new(),
            position: 0,
        };
        
        // Collect keys in the specified range
        iter.collect_keys(start.map(|s| s.to_vec()), end.map(|e| e.to_vec()))?;
        
        // Position at the first key
        if !iter.keys.is_empty() {
            iter.position = 0;
            iter.load_current()?;
        }
        
        Ok(iter)
    }

    /// Collects all keys from the database that fall within the specified range.
    fn collect_keys(&mut self, start: Option<Vec<u8>>, end: Option<Vec<u8>>) -> Result<()> {
        use std::collections::BTreeSet;
        
        let mut all_keys = BTreeSet::new();
        
        // Collect from current MemTable
        {
            let memtable = self.db.memtable.read();
            all_keys.extend(memtable.keys());
        }
        
        // Collect from immutable MemTables
        {
            let immutable = self.db.immutable_memtables.read();
            for memtable in immutable.iter() {
                all_keys.extend(memtable.keys());
            }
        }
        
        // Collect from SSTables
        {
            let sstables = self.db.sstables.read();
            for level_tables in sstables.iter() {
                for table in level_tables.iter() {
                    all_keys.extend(table.keys()?);
                }
            }
        }
        
        // Filter by range and convert to Vec
        self.keys = all_keys
            .into_iter()
            .filter(|key| {
                let after_start = start.as_ref().map_or(true, |s| key >= s);
                let before_end = end.as_ref().map_or(true, |e| key < e);
                after_start && before_end
            })
            .collect();
        
        Ok(())
    }

    /// Loads the current key-value pair from the database.
    fn load_current(&mut self) -> Result<()> {
        if self.position >= self.keys.len() {
            self.current = None;
            return Ok(());
        }
        
        let key = &self.keys[self.position];
        
        // Get the value using the snapshot sequence
        if let Some(value) = self.db.get_at_sequence(key, self.sequence)? {
            self.current = Some((key.clone(), value));
        } else {
            // Key was deleted or doesn't exist at this sequence, skip it
            self.next();
        }
        
        Ok(())
    }

    /// Returns true if the iterator is positioned at a valid entry.
    pub fn valid(&self) -> bool {
        self.current.is_some()
    }

    /// Returns the key at the current position.
    ///
    /// # Panics
    ///
    /// Panics if the iterator is not valid. Call `valid()` first to check.
    pub fn key(&self) -> &[u8] {
        self.current.as_ref().expect("Iterator not valid").0.as_slice()
    }

    /// Returns the value at the current position.
    ///
    /// # Panics
    ///
    /// Panics if the iterator is not valid. Call `valid()` first to check.
    pub fn value(&self) -> &[u8] {
        self.current.as_ref().expect("Iterator not valid").1.as_slice()
    }

    /// Moves to the next entry in forward direction.
    pub fn next(&mut self) {
        self.position += 1;
        let _ = self.load_current();
    }

    /// Moves to the previous entry in backward direction.
    pub fn prev(&mut self) {
        if self.position > 0 {
            self.position -= 1;
            let _ = self.load_current();
        } else {
            self.current = None;
        }
    }

    /// Seeks to the first key that is greater than or equal to the target.
    pub fn seek(&mut self, target: &[u8]) {
        // Binary search for the target key
        match self.keys.binary_search_by(|k| k.as_slice().cmp(target)) {
            Ok(pos) => {
                self.position = pos;
            }
            Err(pos) => {
                self.position = pos;
            }
        }
        let _ = self.load_current();
    }

    /// Seeks to the first key in the database.
    pub fn seek_to_first(&mut self) {
        self.position = 0;
        let _ = self.load_current();
    }

    /// Seeks to the last key in the database.
    pub fn seek_to_last(&mut self) {
        if !self.keys.is_empty() {
            self.position = self.keys.len() - 1;
            let _ = self.load_current();
        } else {
            self.current = None;
        }
    }
}

impl DB {
    /// Creates an iterator over all key-value pairs.
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
    /// let mut iter = db.iter();
    /// while iter.valid() {
    ///     println!("{:?} => {:?}", iter.key(), iter.value());
    ///     iter.next();
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn iter(self: &Arc<Self>) -> DBIterator {
        let seq = self.sequence.load(std::sync::atomic::Ordering::SeqCst);
        DBIterator::new(Arc::clone(self), seq).unwrap()
    }

    /// Creates an iterator over a range of keys.
    ///
    /// # Arguments
    ///
    /// * `start` - Optional start key (inclusive). If None, starts from the beginning.
    /// * `end` - Optional end key (exclusive). If None, continues to the end.
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
    /// // Scan from "key1" to "key9" (exclusive)
    /// let mut iter = db.scan(Some(b"key1"), Some(b"key9"))?;
    /// while iter.valid() {
    ///     println!("{:?} => {:?}", iter.key(), iter.value());
    ///     iter.next();
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn scan(
        self: &Arc<Self>,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<DBIterator> {
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

        // Insert test data
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.put(b"key3", b"value3").unwrap();

        // Test iteration
        let mut iter = db.iter();
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
    fn test_iterator_seek() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        db.put(b"a", b"1").unwrap();
        db.put(b"c", b"3").unwrap();
        db.put(b"e", b"5").unwrap();

        let mut iter = db.iter();
        
        // Seek to "c"
        iter.seek(b"c");
        assert!(iter.valid());
        assert_eq!(iter.key(), b"c");
        assert_eq!(iter.value(), b"3");

        // Seek to "b" should position at "c"
        iter.seek(b"b");
        assert!(iter.valid());
        assert_eq!(iter.key(), b"c");
    }

    #[test]
    fn test_iterator_prev() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.put(b"key3", b"value3").unwrap();

        let mut iter = db.iter();
        iter.seek_to_last();
        
        assert!(iter.valid());
        assert_eq!(iter.key(), b"key3");
        
        iter.prev();
        assert_eq!(iter.key(), b"key2");
        
        iter.prev();
        assert_eq!(iter.key(), b"key1");
    }

    #[test]
    fn test_scan_range() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        db.put(b"a", b"1").unwrap();
        db.put(b"b", b"2").unwrap();
        db.put(b"c", b"3").unwrap();
        db.put(b"d", b"4").unwrap();
        db.put(b"e", b"5").unwrap();

        // Scan from "b" to "d" (exclusive)
        let mut iter = db.scan(Some(b"b"), Some(b"d")).unwrap();
        let mut keys = Vec::new();
        
        while iter.valid() {
            keys.push(iter.key().to_vec());
            iter.next();
        }

        assert_eq!(keys, vec![b"b", b"c"]);
    }

    #[test]
    fn test_iterator_with_deletes() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.put(b"key3", b"value3").unwrap();
        
        // Delete key2
        db.delete(b"key2").unwrap();

        // Iterator should skip deleted keys
        let mut iter = db.iter();
        let mut keys = Vec::new();
        
        while iter.valid() {
            keys.push(iter.key().to_vec());
            iter.next();
        }

        assert_eq!(keys, vec![b"key1", b"key3"]);
    }

    #[test]
    fn test_empty_iterator() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        let mut iter = db.iter();
        assert!(!iter.valid());
    }
}
