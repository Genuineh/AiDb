//! ScriptContext provides transaction support for Lua script operations.
//!
//! This module implements the core rollback mechanism by accumulating all
//! write operations in a WriteBatch. If the script completes successfully,
//! the batch is committed atomically. If the script fails, the batch is
//! discarded, achieving automatic rollback.

use crate::write_batch::WriteBatch;
use crate::{Result, DB};
use std::sync::Arc;

/// ScriptContext maintains a WriteBatch for operations executed within a Lua script.
///
/// All write operations (put/delete) are accumulated in a WriteBatch and only
/// committed to the database when the script completes successfully. This provides:
///
/// - **Atomicity**: All operations succeed or fail together via WriteBatch
/// - **Rollback**: Failed scripts automatically discard all changes
///
/// # Architecture
///
/// ```text
/// Script Operation Flow:
///
/// 1. db.put(key, value)  →  Add to WriteBatch
/// 2. db.get(key)         →  Read from DB directly
/// 3. db.delete(key)      →  Add delete to WriteBatch
///
/// On success:  WriteBatch → DB (atomic commit via db.write())
/// On failure:  WriteBatch discarded (automatic rollback)
/// ```
///
/// # Note on Read-Your-Writes
///
/// The current implementation does **not** support read-your-writes semantics.
/// This means that `db.get(key)` within a script will not see values written
/// by `db.put(key, value)` in the same script until the script completes and
/// commits. This is a known limitation pending WriteBatch query support in AiDb.
///
/// For memory-based storage, read-your-writes can be supported by maintaining
/// a temporary read cache alongside the WriteBatch.
pub struct ScriptContext {
    /// Reference to the database
    db: Arc<DB>,
    
    /// WriteBatch accumulating all write operations
    batch: WriteBatch,
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
            batch: WriteBatch::new(),
        }
    }

    /// Adds a put operation to the WriteBatch.
    ///
    /// The operation is not immediately written to the database.
    /// Instead, it's added to the WriteBatch and will be committed when
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
        self.batch.put(key, value);
    }

    /// Retrieves a value directly from the database.
    ///
    /// **Note**: This method reads directly from the database and does NOT
    /// see uncommitted writes from the current script. Read-your-writes
    /// semantics are not currently supported and are pending AiDb improvements.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// - `Ok(Some(value))` if the key exists in the database
    /// - `Ok(None)` if the key doesn't exist
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
    /// let ctx = ScriptContext::new(Arc::clone(&db));
    /// 
    /// let value = ctx.get(b"key")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Read directly from database
        // Note: Does not see uncommitted writes in the current script
        self.db.get(key)
    }

    /// Adds a delete operation to the WriteBatch.
    ///
    /// The operation is not immediately applied to the database.
    /// Instead, it's added to the WriteBatch and will be committed
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
        self.batch.delete(key);
    }

    /// Commits all operations in the WriteBatch to the database atomically.
    ///
    /// All operations are applied via `db.write(batch)`, which ensures
    /// atomicity through AiDb's WriteBatch implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if the database write fails. In this case,
    /// none of the operations will be applied.
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
        if self.batch.is_empty() {
            return Ok(());
        }

        // Use AiDb's WriteBatch to commit atomically
        self.db.write(self.batch)
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
        // Simply drop the context, discarding the WriteBatch
        drop(self);
    }

    /// Returns the number of operations in the WriteBatch.
    pub fn operation_count(&self) -> usize {
        self.batch.len()
    }

    /// Returns true if there are no operations in the WriteBatch.
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Takes the WriteBatch out of the context for manual commit.
    /// This is useful when the context is held in a Mutex.
    pub(crate) fn take_batch(&mut self) -> WriteBatch {
        std::mem::replace(&mut self.batch, WriteBatch::new())
    }

    /// Returns a reference to the database.
    pub(crate) fn db(&self) -> &Arc<DB> {
        &self.db
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
    fn test_context_put() {
        let (_dir, db) = setup_db();
        let mut ctx = ScriptContext::new(Arc::clone(&db));

        ctx.put(b"key1", b"value1");
        assert_eq!(ctx.operation_count(), 1);
        assert!(!ctx.is_empty());
    }

    #[test]
    fn test_context_read_from_db() {
        let (_dir, db) = setup_db();

        // Write directly to DB
        db.put(b"existing_key", b"existing_value").unwrap();

        let ctx = ScriptContext::new(Arc::clone(&db));

        // Should read from DB
        let value = ctx.get(b"existing_key").unwrap();
        assert_eq!(value, Some(b"existing_value".to_vec()));
    }

    #[test]
    fn test_context_no_read_your_writes() {
        let (_dir, db) = setup_db();
        let mut ctx = ScriptContext::new(Arc::clone(&db));

        // Write to context (not committed yet)
        ctx.put(b"key1", b"new_value");

        // Get should NOT see the uncommitted write
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, None); // Not committed yet, so None
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
    fn test_context_multiple_operations() {
        let (_dir, db) = setup_db();
        let mut ctx = ScriptContext::new(Arc::clone(&db));

        // Multiple operations on same key - all will be applied
        ctx.put(b"key", b"value1");
        ctx.put(b"key", b"value2");
        ctx.put(b"key", b"value3");

        assert_eq!(ctx.operation_count(), 3);

        // Commit
        ctx.commit().unwrap();

        // DB should have the final value
        assert_eq!(db.get(b"key").unwrap(), Some(b"value3".to_vec()));
    }
}
