//! Snapshot implementation for point-in-time consistent reads.
//!
//! Snapshots allow reading data as it existed at a specific point in time,
//! providing isolation from concurrent writes.

use std::sync::Arc;

use crate::{DB, Result};

/// A snapshot represents a point-in-time view of the database.
///
/// All read operations through a snapshot will see data as it existed
/// at the time the snapshot was created, even if the data is modified
/// or deleted afterwards.
///
/// # Example
///
/// ```rust,no_run
/// use aidb::{DB, Options};
///
/// # fn main() -> Result<(), aidb::Error> {
/// let db = DB::open("./data", Options::default())?;
///
/// db.put(b"key1", b"value1")?;
///
/// // Create a snapshot
/// let snapshot = db.snapshot();
///
/// // Modify the database
/// db.put(b"key1", b"value2")?;
///
/// // Snapshot still sees the old value
/// assert_eq!(snapshot.get(b"key1")?, Some(b"value1".to_vec()));
///
/// // Current DB sees the new value
/// assert_eq!(db.get(b"key1")?, Some(b"value2".to_vec()));
/// # Ok(())
/// # }
/// ```
pub struct Snapshot {
    /// Reference to the database
    db: Arc<DB>,
    
    /// Sequence number at the time of snapshot creation
    /// All reads will be filtered to only see entries with seq <= snapshot_seq
    sequence: u64,
}

impl Snapshot {
    /// Creates a new snapshot with the given sequence number.
    ///
    /// # Arguments
    ///
    /// * `db` - Reference to the database
    /// * `sequence` - The sequence number at snapshot creation time
    pub(crate) fn new(db: Arc<DB>, sequence: u64) -> Self {
        Self { db, sequence }
    }

    /// Retrieves the value associated with a key as it existed at snapshot time.
    ///
    /// Returns `None` if the key did not exist or was deleted at snapshot time.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails due to I/O errors or data corruption.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get_at_sequence(key, self.sequence)
    }

    /// Returns the sequence number of this snapshot.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("sequence", &self.sequence)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    #[test]
    fn test_snapshot_isolation() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        // Write initial value
        db.put(b"key1", b"value1").unwrap();

        // Create snapshot
        let snapshot = db.snapshot();

        // Modify database
        db.put(b"key1", b"value2").unwrap();
        db.put(b"key2", b"value2").unwrap();

        // Snapshot should see old value
        assert_eq!(snapshot.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        
        // Snapshot should not see key2
        assert_eq!(snapshot.get(b"key2").unwrap(), None);

        // Current DB should see new values
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_snapshot_with_deletes() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        // Write values
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();

        // Create snapshot
        let snapshot = db.snapshot();

        // Delete a key
        db.delete(b"key1").unwrap();

        // Snapshot should still see the deleted key
        assert_eq!(snapshot.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(snapshot.get(b"key2").unwrap(), Some(b"value2".to_vec()));

        // Current DB should not see deleted key
        assert_eq!(db.get(b"key1").unwrap(), None);
        assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_multiple_snapshots() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        // State 1
        db.put(b"key", b"v1").unwrap();
        let snapshot1 = db.snapshot();

        // State 2
        db.put(b"key", b"v2").unwrap();
        let snapshot2 = db.snapshot();

        // State 3
        db.put(b"key", b"v3").unwrap();

        // Each snapshot sees its own version
        assert_eq!(snapshot1.get(b"key").unwrap(), Some(b"v1".to_vec()));
        assert_eq!(snapshot2.get(b"key").unwrap(), Some(b"v2".to_vec()));
        assert_eq!(db.get(b"key").unwrap(), Some(b"v3".to_vec()));
    }

    #[test]
    fn test_snapshot_sequence_number() {
        let tmp_dir = TempDir::new().unwrap();
        let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);

        db.put(b"key", b"value").unwrap();
        
        let snapshot = db.snapshot();
        let seq1 = snapshot.sequence();
        
        db.put(b"another", b"value").unwrap();
        
        let snapshot2 = db.snapshot();
        let seq2 = snapshot2.sequence();
        
        // Second snapshot should have higher sequence number
        assert!(seq2 > seq1);
    }
}
