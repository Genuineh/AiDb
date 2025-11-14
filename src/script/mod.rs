//! Lua scripting support with automatic rollback.
//!
//! This module provides Lua scripting capabilities for the database with
//! built-in transaction support via WriteBatch. All operations within a script
//! are accumulated in a WriteBatch and only committed if the script completes
//! successfully, providing automatic rollback on failure.
//!
//! # Architecture
//!
//! The scripting system consists of three main components:
//!
//! - **ScriptContext**: Accumulates database operations in a WriteBatch (no read-your-writes)
//! - **MemoryScriptContext**: Like ScriptContext but with read-your-writes support via in-memory cache
//! - **LuaExecutor**: Executes Lua scripts with database access
//!
//! # Features
//!
//! - **Automatic Rollback**: Failed scripts discard all changes
//! - **Atomic Commits**: All operations succeed or fail together via WriteBatch
//! - **Timeout Control**: Scripts can be time-limited
//! - **Sandboxed Execution**: Scripts run in isolated Lua environments
//!
//! # Read-Your-Writes Support
//!
//! The module provides two context types:
//!
//! ## ScriptContext (Default)
//!
//! Does **not** support read-your-writes. `db.get(key)` reads directly from the database
//! and will not see values written by `db.put(key, value)` within the same script until
//! after commit. This is simpler and more efficient for scripts that don't need to read
//! their own writes.
//!
//! ## MemoryScriptContext
//!
//! **Supports** read-your-writes by maintaining an in-memory cache alongside the WriteBatch.
//! Scripts can read their own uncommitted changes. Use this for complex business logic
//! that needs to read and modify data within the same transaction.
//!
//! # Example - Basic Usage (No Read-Your-Writes)
//!
//! ```rust,no_run
//! use aidb::{DB, Options};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), aidb::Error> {
//! let db = Arc::new(DB::open("./data", Options::default())?);
//!
//! // Execute a script (uses ScriptContext by default)
//! let script = r#"
//!     -- Note: db.get() only sees committed data
//!     local balance = db.get("account:1:balance")
//!     
//!     if tonumber(balance) < 100 then
//!         error("Insufficient balance")
//!     end
//!     
//!     -- These writes are buffered and committed atomically
//!     db.put("account:1:balance", tostring(tonumber(balance) - 100))
//!     db.put("account:2:balance", tostring(tonumber(balance) + 100))
//!     
//!     return "Transfer successful"
//! "#;
//!
//! db.execute_script(script)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example - With Read-Your-Writes
//!
//! ```rust,no_run
//! use aidb::{DB, Options};
//! use aidb::script::MemoryScriptContext;
//! use std::sync::Arc;
//!
//! # fn main() -> Result<(), aidb::Error> {
//! let db = Arc::new(DB::open("./data", Options::default())?);
//! let mut ctx = MemoryScriptContext::new(Arc::clone(&db));
//!
//! // Write a value
//! ctx.put(b"counter", b"1");
//!
//! // Read it back immediately (read-your-writes works!)
//! let value = ctx.get(b"counter")?;
//! assert_eq!(value, Some(b"1".to_vec()));
//!
//! // Increment and write again
//! let count: i32 = String::from_utf8_lossy(&value.unwrap()).parse().unwrap();
//! ctx.put(b"counter", (count + 1).to_string().as_bytes());
//!
//! // Read the updated value
//! let new_value = ctx.get(b"counter")?;
//! assert_eq!(new_value, Some(b"2".to_vec()));
//!
//! // Commit atomically
//! ctx.commit()?;
//! # Ok(())
//! # }
//! ```

pub mod context;
pub mod lua_executor;

pub use context::{MemoryScriptContext, ScriptContext};
pub use lua_executor::LuaExecutor;
