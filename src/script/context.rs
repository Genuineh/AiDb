//! ScriptContext provides a buffering layer for Lua script operations.
//!
//! This module implements the core rollback mechanism by buffering all
//! write operations in memory until the script completes successfully.
//! If the script fails, the buffer is discarded, achieving automatic rollback.

use crate::write_batch::{WriteOp, WriteBatch};
use crate::{Result, DB};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// ScriptContext maintains a temporary buffer for operations executed within a Lua script.
///
/// All write operations (put/delete) are buffered in memory and only committed
/// to the database when the script completes successfully. This provides:
///
/// - **Atomicity**: All operations succeed or fail together
/// - **Isolation**: Uncommitted changes are visible within the script
/// - **Rollback**: Failed scripts automatically discard all changes
///
/// # Architecture
///
/// ```text
/// Script Operation Flow:
///
/// 1. db.put(key, value)  →  Write to buffer
/// 2. db.get(key)         →  Check buffer first, then DB
/// 3. db.delete(key)      →  Write tombstone to buffer
///
/// On success:  Buffer → WriteBatch → DB (atomic commit)
/// On failure:  Buffer discarded (automatic rollback)
/// ```
pub struct ScriptContext {
    /// Reference to the database
    db: Arc<DB>,
    
    /// Buffered write operations (put/delete)
    write_buffer: HashMap<Vec<u8>, BufferedValue>,
    
    /// Ordered list of operations for WriteBatch conversion
    operations: Vec<WriteOp>,
}

/// Represents a buffered value in the script context
#[derive(Debug, Clone)]
enum BufferedValue {
    /// A value to be written
    Put(Vec<u8>),
    /// A tombstone (deletion marker)
    Delete,
}

impl ScriptContext {
    /// Creates a new ScriptContext for the given database.
    ///
    /// # Arguments
    ///
    /// * `db` - Reference to the database instance
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    /// use aidb::script::ScriptContext;
    /// use std::sync::Arc;
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// let db = Arc::new(DB::open("./data", Options::default())?);
    /// let ctx = ScriptContext::new(Arc::clone(&db));
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(db: Arc<DB>) -> Self {
        Self {
            db,
            write_buffer: HashMap::new(),
            operations: Vec::new(),
        }
    }

    /// Buffers a put operation.
    ///
    /// The operation is not immediately written to the database.
    /// Instead, it's stored in the buffer and will be committed when
    /// the script completes successfully.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to insert
    /// * `value` - The value to associate with the key
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::ScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = ScriptContext::new(Arc::clone(&db));
    /// ctx.put(b"key", b"value");
    /// # Ok(())
    /// # }
    /// ```
    pub fn put(&mut self, key: &[u8], value: &[u8]) {
        let key_vec = key.to_vec();
        let value_vec = value.to_vec();
        
        self.write_buffer.insert(key_vec.clone(), BufferedValue::Put(value_vec.clone()));
        self.operations.push(WriteOp::Put {
            key: key_vec,
            value: value_vec,
        });
    }

    /// Retrieves a value, checking the buffer first before querying the database.
    ///
    /// This method implements read-your-writes semantics: if a value was
    /// written earlier in the same script, it will be returned even though
    /// it hasn't been committed to the database yet.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// - `Ok(Some(value))` if the key exists (in buffer or DB)
    /// - `Ok(None)` if the key doesn't exist or was deleted
    /// - `Err(_)` if there's an I/O error reading from the database
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::ScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = ScriptContext::new(Arc::clone(&db));
    /// 
    /// ctx.put(b"key", b"value");
    /// let value = ctx.get(b"key")?;
    /// assert_eq!(value, Some(b"value".to_vec()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // First, check the write buffer
        if let Some(buffered) = self.write_buffer.get(key) {
            return match buffered {
                BufferedValue::Put(value) => Ok(Some(value.clone())),
                BufferedValue::Delete => Ok(None),
            };
        }

        // If not in buffer, query the database
        self.db.get(key)
    }

    /// Buffers a delete operation.
    ///
    /// The operation is not immediately applied to the database.
    /// Instead, a tombstone is stored in the buffer and will be committed
    /// when the script completes successfully.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to delete
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::ScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = ScriptContext::new(Arc::clone(&db));
    /// ctx.delete(b"key");
    /// # Ok(())
    /// # }
    /// ```
    pub fn delete(&mut self, key: &[u8]) {
        let key_vec = key.to_vec();
        
        self.write_buffer.insert(key_vec.clone(), BufferedValue::Delete);
        self.operations.push(WriteOp::Delete { key: key_vec });
    }

    /// Commits all buffered operations to the database atomically.
    ///
    /// All operations in the buffer are converted to a WriteBatch and
    /// applied to the database as a single atomic transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if the database write fails. In this case,
    /// none of the buffered operations will be applied.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::ScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = ScriptContext::new(Arc::clone(&db));
    /// 
    /// ctx.put(b"key1", b"value1");
    /// ctx.put(b"key2", b"value2");
    /// ctx.delete(b"key3");
    /// 
    /// // Commit all operations atomically
    /// ctx.commit()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn commit(self) -> Result<()> {
        if self.operations.is_empty() {
            return Ok(());
        }

        // Convert buffered operations to WriteBatch
        let mut batch = WriteBatch::new();
        for op in self.operations {
            match op {
                WriteOp::Put { key, value } => {
                    batch.put(&key, &value);
                }
                WriteOp::Delete { key } => {
                    batch.delete(&key);
                }
            }
        }

        // Atomically apply all operations
        self.db.write(batch)
    }

    /// Commits buffered operations from within a Mutex.
    ///
    /// This is a helper method for use with Arc<Mutex<ScriptContext>>.
    pub(crate) fn commit_from_mutex(context: &Arc<Mutex<ScriptContext>>) -> Result<()> {
        let operations = {
            let ctx = context.lock().unwrap();
            ctx.operations.clone()
        };
        
        if operations.is_empty() {
            return Ok(());
        }

        let db: Arc<DB> = {
            let ctx = context.lock().unwrap();
            Arc::clone(&ctx.db)
        };

        let mut batch = WriteBatch::new();
        for op in operations {
            match op {
                WriteOp::Put { key, value } => {
                    batch.put(&key, &value);
                }
                WriteOp::Delete { key } => {
                    batch.delete(&key);
                }
            }
        }

        db.write(batch)
    }

    /// Discards all buffered operations without committing them to the database.
    ///
    /// This is called automatically when the script fails or when the
    /// ScriptContext is dropped without calling commit().
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::ScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = ScriptContext::new(Arc::clone(&db));
    /// 
    /// ctx.put(b"key", b"value");
    /// ctx.rollback(); // Discard the operation
    /// # Ok(())
    /// # }
    /// ```
    pub fn rollback(self) {
        // Simply drop the context, discarding all buffered operations
        drop(self);
    }

    /// Returns the number of buffered operations.
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    /// Returns true if there are no buffered operations.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    fn setup_db() -> (TempDir, Arc<DB>) {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        (temp_dir, Arc::new(db))
    }

    #[test]
    fn test_context_new() {
        let (_dir, db) = setup_db();
        let ctx = ScriptContext::new(Arc::clone(&db));
        assert!(ctx.is_empty());
        assert_eq!(ctx.operation_count(), 0);
    }

    #[test]
    fn test_context_put_and_get() {
        let (_dir, db) = setup_db();
        let mut ctx = ScriptContext::new(Arc::clone(&db));

        ctx.put(b"key1", b"value1");
        assert_eq!(ctx.operation_count(), 1);

        // Should be able to read from buffer
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_context_delete() {
        let (_dir, db) = setup_db();
        let mut ctx = ScriptContext::new(Arc::clone(&db));

        // Put then delete
        ctx.put(b"key1", b"value1");
        ctx.delete(b"key1");

        // Should return None
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_context_read_through_to_db() {
        let (_dir, db) = setup_db();

        // Write directly to DB
        db.put(b"existing_key", b"existing_value").unwrap();

        let ctx = ScriptContext::new(Arc::clone(&db));

        // Should read from DB
        let value = ctx.get(b"existing_key").unwrap();
        assert_eq!(value, Some(b"existing_value".to_vec()));
    }

    #[test]
    fn test_context_buffer_overrides_db() {
        let (_dir, db) = setup_db();

        // Write to DB
        db.put(b"key1", b"old_value").unwrap();

        let mut ctx = ScriptContext::new(Arc::clone(&db));

        // Override in buffer
        ctx.put(b"key1", b"new_value");

        // Should read new value from buffer
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, Some(b"new_value".to_vec()));

        // DB should still have old value
        let db_value = db.get(b"key1").unwrap();
        assert_eq!(db_value, Some(b"old_value".to_vec()));
    }

    #[test]
    fn test_context_commit() {
        let (_dir, db) = setup_db();

        {
            let mut ctx = ScriptContext::new(Arc::clone(&db));
            ctx.put(b"key1", b"value1");
            ctx.put(b"key2", b"value2");
            ctx.delete(b"key3");

            // Commit operations
            ctx.commit().unwrap();
        }

        // Verify data was committed to DB
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(db.get(b"key3").unwrap(), None);
    }

    #[test]
    fn test_context_rollback() {
        let (_dir, db) = setup_db();

        {
            let mut ctx = ScriptContext::new(Arc::clone(&db));
            ctx.put(b"key1", b"value1");
            ctx.put(b"key2", b"value2");

            // Explicit rollback
            ctx.rollback();
        }

        // Verify data was NOT committed to DB
        assert_eq!(db.get(b"key1").unwrap(), None);
        assert_eq!(db.get(b"key2").unwrap(), None);
    }

    #[test]
    fn test_context_automatic_rollback_on_drop() {
        let (_dir, db) = setup_db();

        {
            let mut ctx = ScriptContext::new(Arc::clone(&db));
            ctx.put(b"key1", b"value1");
            // Context dropped without commit = automatic rollback
        }

        // Verify data was NOT committed to DB
        assert_eq!(db.get(b"key1").unwrap(), None);
    }

    #[test]
    fn test_context_empty_commit() {
        let (_dir, db) = setup_db();
        let ctx = ScriptContext::new(Arc::clone(&db));

        // Should succeed with no operations
        ctx.commit().unwrap();
    }

    #[test]
    fn test_context_multiple_operations_same_key() {
        let (_dir, db) = setup_db();
        let mut ctx = ScriptContext::new(Arc::clone(&db));

        // Multiple operations on same key
        ctx.put(b"key", b"value1");
        ctx.put(b"key", b"value2");
        ctx.put(b"key", b"value3");

        // Should return latest value
        let value = ctx.get(b"key").unwrap();
        assert_eq!(value, Some(b"value3".to_vec()));

        // Commit
        ctx.commit().unwrap();

        // DB should have all operations applied
        assert_eq!(db.get(b"key").unwrap(), Some(b"value3".to_vec()));
    }
}
