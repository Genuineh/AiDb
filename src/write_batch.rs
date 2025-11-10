//! WriteBatch provides atomic batch write operations.
//!
//! WriteBatch allows multiple write operations (put, delete) to be grouped together
//! and applied atomically to the database. This improves performance and ensures
//! consistency when multiple related writes need to be made together.
//!
//! # Example
//!
//! ```rust,no_run
//! use aidb::{DB, Options, WriteBatch};
//!
//! # fn main() -> Result<(), aidb::Error> {
//! let db = DB::open("./data", Options::default())?;
//! let mut batch = WriteBatch::new();
//!
//! // Add multiple operations to the batch
//! batch.put(b"key1", b"value1");
//! batch.put(b"key2", b"value2");
//! batch.delete(b"key3");
//!
//! // Apply all operations atomically
//! db.write(batch)?;
//! # Ok(())
//! # }
//! ```

use std::collections::VecDeque;

/// Type of write operation in a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    /// Put operation with key and value
    Put {
        /// Key to insert
        key: Vec<u8>,
        /// Value to associate with the key
        value: Vec<u8>,
    },
    /// Delete operation with key
    Delete {
        /// Key to delete
        key: Vec<u8>,
    },
}

/// WriteBatch accumulates a sequence of write operations to be applied atomically.
///
/// Operations are buffered in memory and applied to the database together when
/// `DB::write()` is called. This provides better performance than individual writes
/// and ensures all operations succeed or fail together.
#[derive(Debug, Default)]
pub struct WriteBatch {
    operations: VecDeque<WriteOp>,
    approximate_size: usize,
}

impl WriteBatch {
    /// Creates a new empty WriteBatch.
    ///
    /// # Example
    ///
    /// ```
    /// use aidb::WriteBatch;
    ///
    /// let batch = WriteBatch::new();
    /// assert!(batch.is_empty());
    /// ```
    pub fn new() -> Self {
        Self { operations: VecDeque::new(), approximate_size: 0 }
    }

    /// Adds a Put operation to the batch.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to insert
    /// * `value` - The value to associate with the key
    ///
    /// # Example
    ///
    /// ```
    /// use aidb::WriteBatch;
    ///
    /// let mut batch = WriteBatch::new();
    /// batch.put(b"key", b"value");
    /// assert_eq!(batch.len(), 1);
    /// ```
    pub fn put(&mut self, key: &[u8], value: &[u8]) {
        let op_size = key.len() + value.len() + 8; // Approximate overhead
        self.approximate_size += op_size;
        self.operations
            .push_back(WriteOp::Put { key: key.to_vec(), value: value.to_vec() });
    }

    /// Adds a Delete operation to the batch.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to delete
    ///
    /// # Example
    ///
    /// ```
    /// use aidb::WriteBatch;
    ///
    /// let mut batch = WriteBatch::new();
    /// batch.delete(b"key");
    /// assert_eq!(batch.len(), 1);
    /// ```
    pub fn delete(&mut self, key: &[u8]) {
        let op_size = key.len() + 4; // Approximate overhead
        self.approximate_size += op_size;
        self.operations.push_back(WriteOp::Delete { key: key.to_vec() });
    }

    /// Clears all operations from the batch.
    ///
    /// # Example
    ///
    /// ```
    /// use aidb::WriteBatch;
    ///
    /// let mut batch = WriteBatch::new();
    /// batch.put(b"key", b"value");
    /// batch.clear();
    /// assert!(batch.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.operations.clear();
        self.approximate_size = 0;
    }

    /// Returns the number of operations in the batch.
    ///
    /// # Example
    ///
    /// ```
    /// use aidb::WriteBatch;
    ///
    /// let mut batch = WriteBatch::new();
    /// batch.put(b"key1", b"value1");
    /// batch.put(b"key2", b"value2");
    /// assert_eq!(batch.len(), 2);
    /// ```
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Returns true if the batch contains no operations.
    ///
    /// # Example
    ///
    /// ```
    /// use aidb::WriteBatch;
    ///
    /// let batch = WriteBatch::new();
    /// assert!(batch.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Returns the approximate size of the batch in bytes.
    ///
    /// This is an estimate and may not reflect the exact memory usage.
    pub fn approximate_size(&self) -> usize {
        self.approximate_size
    }

    /// Returns an iterator over the operations in the batch.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &WriteOp> {
        self.operations.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_batch_new() {
        let batch = WriteBatch::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }

    #[test]
    fn test_write_batch_put() {
        let mut batch = WriteBatch::new();
        batch.put(b"key1", b"value1");
        batch.put(b"key2", b"value2");

        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
        assert!(batch.approximate_size() > 0);
    }

    #[test]
    fn test_write_batch_delete() {
        let mut batch = WriteBatch::new();
        batch.delete(b"key1");
        batch.delete(b"key2");

        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_write_batch_mixed_operations() {
        let mut batch = WriteBatch::new();
        batch.put(b"key1", b"value1");
        batch.delete(b"key2");
        batch.put(b"key3", b"value3");

        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn test_write_batch_clear() {
        let mut batch = WriteBatch::new();
        batch.put(b"key1", b"value1");
        batch.put(b"key2", b"value2");

        assert_eq!(batch.len(), 2);

        batch.clear();

        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.approximate_size(), 0);
    }

    #[test]
    fn test_write_batch_approximate_size() {
        let mut batch = WriteBatch::new();
        let initial_size = batch.approximate_size();

        batch.put(b"key", b"value");
        let after_put_size = batch.approximate_size();

        assert!(after_put_size > initial_size);
    }

    #[test]
    fn test_write_batch_iter() {
        let mut batch = WriteBatch::new();
        batch.put(b"key1", b"value1");
        batch.delete(b"key2");
        batch.put(b"key3", b"value3");

        let ops: Vec<_> = batch.iter().collect();
        assert_eq!(ops.len(), 3);

        match &ops[0] {
            WriteOp::Put { key, value } => {
                assert_eq!(key, b"key1");
                assert_eq!(value, b"value1");
            }
            _ => panic!("Expected Put operation"),
        }

        match &ops[1] {
            WriteOp::Delete { key } => {
                assert_eq!(key, b"key2");
            }
            _ => panic!("Expected Delete operation"),
        }
    }
}
