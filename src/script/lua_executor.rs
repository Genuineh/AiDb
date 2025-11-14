//! LuaExecutor executes Lua scripts with automatic rollback support.
//!
//! This module provides the integration between Lua scripting and the database.
//! All database operations within a script are buffered and only committed
//! if the script completes successfully.

use crate::script::context::ScriptContext;
use crate::{Error, Result, DB};
use mlua::Lua;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// LuaExecutor manages the execution of Lua scripts with database access.
///
/// The executor provides a sandboxed Lua environment where scripts can
/// interact with the database through a limited API. All operations are
/// buffered and only committed if the script succeeds.
///
/// # Features
///
/// - **Automatic Rollback**: Failed scripts automatically discard all changes
/// - **Timeout Control**: Scripts can be limited to a maximum execution time
/// - **Read-Your-Writes**: Scripts can read their own uncommitted writes
/// - **Atomic Commits**: All operations succeed or fail together
///
/// # Example
///
/// ```rust,no_run
/// use aidb::{DB, Options};
/// use aidb::script::LuaExecutor;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// # fn main() -> Result<(), aidb::Error> {
/// let db = Arc::new(DB::open("./data", Options::default())?);
/// let executor = LuaExecutor::new(Arc::clone(&db), Some(Duration::from_secs(5)));
///
/// let script = r#"
///     db.put("user:1", "Alice")
///     db.put("user:2", "Bob")
///     return "Created 2 users"
/// "#;
///
/// executor.execute(script)?;
/// # Ok(())
/// # }
/// ```
pub struct LuaExecutor {
    /// Reference to the database
    db: Arc<DB>,
    
    /// Maximum script execution time
    timeout: Option<Duration>,
}

/// Database API exposed to Lua scripts
#[allow(dead_code)]
struct LuaDbApi {
    context: Arc<Mutex<ScriptContext>>,
}

impl LuaDbApi {
    fn create_api_table<'lua>(lua: &'lua Lua, context: Arc<Mutex<ScriptContext>>) -> mlua::Result<mlua::Table<'lua>> {
        let table = lua.create_table()?;
        
        let ctx_put = Arc::clone(&context);
        let put_fn = lua.create_function(move |_, (key, value): (mlua::String<'_>, mlua::String<'_>)| {
            let mut ctx = ctx_put.lock().unwrap();
            ctx.put(key.as_bytes(), value.as_bytes());
            Ok(())
        })?;
        table.set("put", put_fn)?;
        
        let ctx_get = Arc::clone(&context);
        let get_fn = lua.create_function(move |lua, key: mlua::String<'_>| {
            let ctx = ctx_get.lock().unwrap();
            match ctx.get(key.as_bytes()) {
                Ok(Some(value)) => Ok(mlua::Value::String(lua.create_string(&value)?)),
                Ok(None) => Ok(mlua::Value::Nil),
                Err(e) => Err(mlua::Error::external(e)),
            }
        })?;
        table.set("get", get_fn)?;
        
        let ctx_delete = Arc::clone(&context);
        let delete_fn = lua.create_function(move |_, key: mlua::String<'_>| {
            let mut ctx = ctx_delete.lock().unwrap();
            ctx.delete(key.as_bytes());
            Ok(())
        })?;
        table.set("delete", delete_fn)?;
        
        Ok(table)
    }
}

impl LuaExecutor {
    /// Creates a new LuaExecutor.
    ///
    /// # Arguments
    ///
    /// * `db` - Reference to the database
    /// * `timeout` - Optional maximum execution time for scripts
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    /// use aidb::script::LuaExecutor;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// let db = Arc::new(DB::open("./data", Options::default())?);
    /// let executor = LuaExecutor::new(Arc::clone(&db), Some(Duration::from_secs(30)));
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(db: Arc<DB>, timeout: Option<Duration>) -> Self {
        Self { db, timeout }
    }

    /// Executes a Lua script with automatic rollback on failure.
    ///
    /// The script is executed in a sandboxed Lua environment with access
    /// to the `db` API. All operations are buffered and only committed
    /// if the script completes successfully.
    ///
    /// # Arguments
    ///
    /// * `script` - The Lua script code to execute
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the script executed successfully and changes were committed
    /// - `Err(_)` if the script failed or timed out (changes are rolled back)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The script has syntax errors
    /// - The script raises a runtime error
    /// - The script exceeds the timeout limit
    /// - Database operations fail
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    /// use aidb::script::LuaExecutor;
    /// use std::sync::Arc;
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let executor = LuaExecutor::new(Arc::clone(&db), None);
    ///
    /// // Successful script
    /// executor.execute(r#"
    ///     db.put("key1", "value1")
    ///     db.put("key2", "value2")
    /// "#)?;
    ///
    /// // Failed script - changes are rolled back
    /// let result = executor.execute(r#"
    ///     db.put("key3", "value3")
    ///     error("Something went wrong!")
    /// "#);
    /// assert!(result.is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn execute(&self, script: &str) -> Result<()> {
        let start_time = Instant::now();
        
        // Create Lua VM
        let lua = Lua::new();
        
        // Create script context
        let context = Arc::new(Mutex::new(ScriptContext::new(Arc::clone(&self.db))));
        
        // Set up timeout hook if specified
        if let Some(timeout) = self.timeout {
            let timeout_start = start_time;
            lua.set_hook(
                mlua::HookTriggers {
                    every_nth_instruction: Some(1000),
                    ..Default::default()
                },
                move |_lua, _debug| {
                    if timeout_start.elapsed() > timeout {
                        Err(mlua::Error::RuntimeError("Script execution timeout".to_string()))
                    } else {
                        Ok(())
                    }
                },
            );
        }

        // Execute script - create the API as a table
        let result = (|| -> mlua::Result<()> {
            let globals = lua.globals();
            
            // Create db API table with functions
            let db_table = LuaDbApi::create_api_table(&lua, Arc::clone(&context))?;
            
            globals.set("db", db_table)?;
            lua.load(script).exec()
        })();

        // Handle script execution result
        match result {
            Ok(_) => {
                // Script succeeded - commit changes
                ScriptContext::commit_from_mutex(&context)?;
                
                log::info!(
                    "Lua script executed successfully in {:?}",
                    start_time.elapsed()
                );
                
                Ok(())
            }
            Err(e) => {
                // Script failed - rollback changes
                log::warn!("Lua script failed: {}", e);
                
                // Context is dropped here, automatically rolling back
                
                Err(Error::ScriptError(format!("Lua script failed: {}", e)))
            }
        }
    }

    /// Executes a Lua script and returns its result value.
    ///
    /// Similar to `execute()`, but captures and returns the script's return value.
    ///
    /// # Arguments
    ///
    /// * `script` - The Lua script code to execute
    ///
    /// # Returns
    ///
    /// - `Ok(Some(String))` if the script returned a string value
    /// - `Ok(None)` if the script returned nil or nothing
    /// - `Err(_)` if the script failed (changes are rolled back)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use aidb::{DB, Options};
    /// use aidb::script::LuaExecutor;
    /// use std::sync::Arc;
    ///
    /// # fn main() -> Result<(), aidb::Error> {
    /// # let db = Arc::new(DB::open("./data", Options::default())?);
    /// let executor = LuaExecutor::new(Arc::clone(&db), None);
    ///
    /// let result = executor.execute_with_result(r#"
    ///     db.put("counter", "1")
    ///     return "Operation completed"
    /// "#)?;
    ///
    /// assert_eq!(result, Some("Operation completed".to_string()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn execute_with_result(&self, script: &str) -> Result<Option<String>> {
        let start_time = Instant::now();
        
        let lua = Lua::new();
        let context = Arc::new(Mutex::new(ScriptContext::new(Arc::clone(&self.db))));
        
        // Set up timeout hook
        if let Some(timeout) = self.timeout {
            let timeout_start = start_time;
            lua.set_hook(
                mlua::HookTriggers {
                    every_nth_instruction: Some(1000),
                    ..Default::default()
                },
                move |_lua, _debug| {
                    if timeout_start.elapsed() > timeout {
                        Err(mlua::Error::RuntimeError("Script execution timeout".to_string()))
                    } else {
                        Ok(())
                    }
                },
            );
        }

        // Execute script and capture return value - create API as table
        let result = (|| -> mlua::Result<mlua::Value<'_>> {
            let globals = lua.globals();
            
            let db_table = LuaDbApi::create_api_table(&lua, Arc::clone(&context))?;
            
            globals.set("db", db_table)?;
            lua.load(script).eval::<mlua::Value<'_>>()
        })();

        // Handle script execution result
        match result {
            Ok(value) => {
                // Extract return value
                let return_value = match value {
                    mlua::Value::String(s) => Some(s.to_str()?.to_string()),
                    mlua::Value::Nil => None,
                    other => Some(format!("{:?}", other)),
                };

                // Commit changes using the helper method
                ScriptContext::commit_from_mutex(&context)?;
                
                log::info!(
                    "Lua script executed successfully in {:?}",
                    start_time.elapsed()
                );
                
                Ok(return_value)
            }
            Err(e) => {
                // Script failed - rollback changes
                log::warn!("Lua script failed: {}", e);
                
                // Context is dropped here, automatically rolling back
                
                Err(Error::ScriptError(format!("Lua script failed: {}", e)))
            }
        }
    }

    /// Sets the timeout for script execution.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum execution time, or None for no limit
    pub fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    /// Gets the current timeout setting.
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    fn setup_executor() -> (TempDir, Arc<DB>, LuaExecutor) {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(DB::open(temp_dir.path(), Options::default()).unwrap());
        let executor = LuaExecutor::new(Arc::clone(&db), Some(Duration::from_secs(5)));
        (temp_dir, db, executor)
    }

    #[test]
    fn test_executor_simple_put() {
        let (_dir, _db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1")
        "#;

        executor.execute(script).unwrap();
    }

    #[test]
    fn test_executor_put_and_get() {
        let (_dir, _db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1")
            local value = db.get("key1")
            if value ~= "value1" then
                error("Value mismatch")
            end
        "#;

        executor.execute(script).unwrap();
    }

    #[test]
    fn test_executor_delete() {
        let (_dir, _db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1")
            db.delete("key1")
            local value = db.get("key1")
            if value ~= nil then
                error("Key should be deleted")
            end
        "#;

        executor.execute(script).unwrap();
    }

    #[test]
    fn test_executor_rollback_on_error() {
        let (dir, db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1")
            db.put("key2", "value2")
            error("Intentional error")
        "#;

        let result = executor.execute(script);
        assert!(result.is_err());

        // Verify changes were rolled back
        assert_eq!(db.get(b"key1").unwrap(), None);
        assert_eq!(db.get(b"key2").unwrap(), None);
    }

    #[test]
    fn test_executor_commit_on_success() {
        let (dir, db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1")
            db.put("key2", "value2")
        "#;

        executor.execute(script).unwrap();

        // Verify changes were committed
        assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_executor_with_result() {
        let (_dir, _db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1")
            return "Success"
        "#;

        let result = executor.execute_with_result(script).unwrap();
        assert_eq!(result, Some("Success".to_string()));
    }

    #[test]
    fn test_executor_read_existing_data() {
        let (dir, db, executor) = setup_executor();

        // Pre-populate database
        db.put(b"existing", b"data").unwrap();

        let script = r#"
            local value = db.get("existing")
            if value ~= "data" then
                error("Cannot read existing data")
            end
            db.put("new_key", value)
        "#;

        executor.execute(script).unwrap();

        // Verify new key was created
        assert_eq!(db.get(b"new_key").unwrap(), Some(b"data".to_vec()));
    }

    #[test]
    fn test_executor_timeout() {
        let (_, _db, mut executor) = setup_executor();
        executor.set_timeout(Some(Duration::from_millis(100)));

        let script = r#"
            local i = 0
            while true do
                i = i + 1
            end
        "#;

        let result = executor.execute(script);
        assert!(result.is_err());
    }

    #[test]
    fn test_executor_multiple_operations() {
        let (dir, db, executor) = setup_executor();

        let script = r#"
            for i = 1, 100 do
                db.put("key" .. i, "value" .. i)
            end
        "#;

        executor.execute(script).unwrap();

        // Verify all keys were created
        for i in 1..=100 {
            let key = format!("key{}", i);
            let expected = format!("value{}", i);
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                Some(expected.as_bytes().to_vec())
            );
        }
    }

    #[test]
    fn test_executor_syntax_error() {
        let (_dir, _db, executor) = setup_executor();

        let script = r#"
            db.put("key1", "value1"
            -- Missing closing parenthesis
        "#;

        let result = executor.execute(script);
        assert!(result.is_err());
    }
}
