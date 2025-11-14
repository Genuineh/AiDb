//! ScriptContext provides transaction support for Lua script operations.
//!
//! This module implements the core rollback mechanism by accumulating all
//! write operations in a WriteBatch. If the script completes successfully,
//! the batch is committed atomically. If the script fails, the batch is
//! discarded, achieving automatic rollback.

use crate::write_batch::WriteBatch;
use crate::{Result, DB};
use std::collections::HashMap;
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
/// For scenarios requiring read-your-writes semantics, use `MemoryScriptContext` instead.
pub struct ScriptContext {
    /// Reference to the database
    db: Arc<DB>,
    
    /// WriteBatch accumulating all write operations
    batch: WriteBatch,
}

/// MemoryScriptContext provides transaction support with read-your-writes semantics.
///
/// This variant maintains an in-memory cache of pending writes alongside the WriteBatch,
/// enabling scripts to read their own uncommitted changes. This is useful for complex
/// business logic that needs to read and modify data within the same transaction.
///
/// # Architecture
///
/// ```text
/// Script Operation Flow:
///
/// 1. db.put(key, value)  →  Update cache + Add to WriteBatch
/// 2. db.get(key)         →  Check cache first, then DB
/// 3. db.delete(key)      →  Mark in cache + Add to WriteBatch
///
/// On success:  WriteBatch → DB (atomic commit via db.write())
/// On failure:  Cache + WriteBatch discarded (automatic rollback)
/// ```
///
/// # Example
///
/// ```rust,no_run
/// use aidb::{DB, Options};
/// use aidb::script::MemoryScriptContext;
/// use std::sync::Arc;
///
/// # fn main() -> Result<(), aidb::Error> {
/// let db = Arc::new(DB::open("./data", Options::default())?);
/// let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
///
/// // Write a value
/// ctx.put(b"counter", b"1");
///
/// // Read it back immediately (read-your-writes)
/// let value = ctx.get(b"counter")?;
/// assert_eq!(value, Some(b"1".to_vec()));
///
/// // Commit atomically
/// ctx.commit()?;
/// # Ok(())
/// # }
/// ```
pub struct MemoryScriptContext {
    /// Reference to the database
    db: Arc<DB>,
    
    /// WriteBatch accumulating all write operations
    batch: WriteBatch,
    
    /// In-memory cache for read-your-writes support
    cache: HashMap<Vec<u8>, CachedValue>,
}

/// Represents a cached value in MemoryScriptContext
#[derive(Debug, Clone)]
enum CachedValue {
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

// ============================================================================
// MemoryScriptContext Implementation (with read-your-writes support)
// ============================================================================

impl MemoryScriptContext {
    /// Creates a new MemoryScriptContext for the given database.
    ///
    /// This variant supports read-your-writes semantics by maintaining an
    /// in-memory cache alongside the WriteBatch.
    ///
    /// # Arguments
    ///
    /// * `db` - Reference to the database instance
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    /// use aidb::script::MemoryScriptContext;
    /// use std::sync::Arc;
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// let db = Arc::new(DB::open("./data", Options::default())?);
    /// let ctx = MemoryScriptContext::new(Arc::clone(&db));
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(db: Arc<DB>) -> Self {
        Self {
            db,
            batch: WriteBatch::new(),
            cache: HashMap::new(),
        }
    }

    /// Adds a put operation to the WriteBatch and updates the cache.
    ///
    /// The operation is added to both the in-memory cache (for read-your-writes)
    /// and the WriteBatch (for atomic commit).
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
    /// # use aidb::script::MemoryScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
    /// ctx.put(b"key", b"value");
    /// # Ok(())
    /// # }
    /// ```
    pub fn put(&mut self, key: &[u8], value: &[u8]) {
        // Update cache for read-your-writes
        self.cache.insert(key.to_vec(), CachedValue::Put(value.to_vec()));
        
        // Add to WriteBatch for atomic commit
        self.batch.put(key, value);
    }

    /// Retrieves a value, checking the cache first before querying the database.
    ///
    /// This method implements read-your-writes semantics: if a value was
    /// written earlier in the same script, it will be returned from the cache
    /// even though it hasn't been committed to the database yet.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// - `Ok(Some(value))` if the key exists (in cache or DB)
    /// - `Ok(None)` if the key doesn't exist or was deleted
    /// - `Err(_)` if there's an I/O error reading from the database
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::MemoryScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
    /// 
    /// ctx.put(b"key", b"value");
    /// 
    /// // Read-your-writes: can see the uncommitted value
    /// let value = ctx.get(b"key")?;
    /// assert_eq!(value, Some(b"value".to_vec()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // First, check the cache
        if let Some(cached) = self.cache.get(key) {
            return match cached {
                CachedValue::Put(value) => Ok(Some(value.clone())),
                CachedValue::Delete => Ok(None),
            };
        }

        // If not in cache, query the database
        self.db.get(key)
    }

    /// Adds a delete operation to the WriteBatch and updates the cache.
    ///
    /// The operation is added to both the in-memory cache (marking as deleted)
    /// and the WriteBatch (for atomic commit).
    ///
    /// # Arguments
    ///
    /// * `key` - The key to delete
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::MemoryScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
    /// ctx.delete(b"key");
    /// # Ok(())
    /// # }
    /// ```
    pub fn delete(&mut self, key: &[u8]) {
        // Update cache to mark as deleted
        self.cache.insert(key.to_vec(), CachedValue::Delete);
        
        // Add to WriteBatch for atomic commit
        self.batch.delete(key);
    }

    /// Commits all operations in the WriteBatch to the database atomically.
    ///
    /// All operations are applied via `db.write(batch)`, which ensures
    /// atomicity through AiDb's WriteBatch implementation. The in-memory
    /// cache is discarded after successful commit.
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
    /// # use aidb::script::MemoryScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
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

    /// Discards all buffered operations and cache without committing.
    ///
    /// This is called automatically when the script fails or when the
    /// MemoryScriptContext is dropped without calling commit().
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use aidb::{DB, Options};
    /// # use aidb::script::MemoryScriptContext;
    /// # use std::sync::Arc;
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
    /// 
    /// ctx.put(b"key", b"value");
    /// ctx.rollback(); // Discard the operation and cache
    /// # Ok(())
    /// # }
    /// ```
    pub fn rollback(self) {
        // Simply drop the context, discarding both cache and WriteBatch
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

    // =========================================================================
    // MemoryScriptContext Tests (with read-your-writes support)
    // =========================================================================

    #[test]
    fn test_memory_context_new() {
        let (_dir, db) = setup_db();
        let ctx = MemoryScriptContext::new(Arc::clone(&db));
        assert!(ctx.is_empty());
        assert_eq!(ctx.operation_count(), 0);
    }

    #[test]
    fn test_memory_context_read_your_writes() {
        let (_dir, db) = setup_db();
        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Write to context (not committed yet)
        ctx.put(b"key1", b"new_value");

        // Get SHOULD see the uncommitted write (read-your-writes)
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, Some(b"new_value".to_vec()));
    }

    #[test]
    fn test_memory_context_cache_overrides_db() {
        let (_dir, db) = setup_db();

        // Write to DB first
        db.put(b"key1", b"old_value").unwrap();

        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Override in cache
        ctx.put(b"key1", b"new_value");

        // Should read new value from cache
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, Some(b"new_value".to_vec()));

        // DB should still have old value
        let db_value = db.get(b"key1").unwrap();
        assert_eq!(db_value, Some(b"old_value".to_vec()));
    }

    #[test]
    fn test_memory_context_delete_in_cache() {
        let (_dir, db) = setup_db();

        // Write to DB
        db.put(b"key1", b"value1").unwrap();

        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Delete in cache
        ctx.delete(b"key1");

        // Should return None (marked as deleted in cache)
        let value = ctx.get(b"key1").unwrap();
        assert_eq!(value, None);

        // DB should still have the value
        let db_value = db.get(b"key1").unwrap();
        assert_eq!(db_value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_memory_context_commit() {
        let (_dir, db) = setup_db();

        {
            let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
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
    fn test_memory_context_rollback() {
        let (_dir, db) = setup_db();

        {
            let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
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
    fn test_memory_context_complex_scenario() {
        let (_dir, db) = setup_db();

        // Pre-populate DB
        db.put(b"balance:1", b"1000").unwrap();
        db.put(b"balance:2", b"500").unwrap();

        let mut ctx = MemoryScriptContext::new(Arc::clone(&db));

        // Read current balances
        let balance1 = ctx.get(b"balance:1").unwrap().unwrap();
        let balance2 = ctx.get(b"balance:2").unwrap().unwrap();

        assert_eq!(balance1, b"1000");
        assert_eq!(balance2, b"500");

        // Transfer 200 from account 1 to account 2
        ctx.put(b"balance:1", b"800");
        ctx.put(b"balance:2", b"700");

        // Read-your-writes: see updated values before commit
        assert_eq!(ctx.get(b"balance:1").unwrap(), Some(b"800".to_vec()));
        assert_eq!(ctx.get(b"balance:2").unwrap(), Some(b"700".to_vec()));

        // Commit
        ctx.commit().unwrap();

        // Verify in DB
        assert_eq!(db.get(b"balance:1").unwrap(), Some(b"800".to_vec()));
        assert_eq!(db.get(b"balance:2").unwrap(), Some(b"700".to_vec()));
    }
}

