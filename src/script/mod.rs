//! Lua scripting support with automatic rollback.
//!
//! This module provides Lua scripting capabilities for the database with
//! built-in transaction support via WriteBatch. All operations within a script
//! are accumulated in a WriteBatch and only committed if the script completes
//! successfully, providing automatic rollback on failure.
//!
//! # Architecture
//!
//! The scripting system consists of two main components:
//!
//! - **ScriptContext**: Accumulates database operations in a WriteBatch
//! - **LuaExecutor**: Executes Lua scripts with database access
//!
//! # Features
//!
//! - **Automatic Rollback**: Failed scripts discard all changes
//! - **Atomic Commits**: All operations succeed or fail together via WriteBatch
//! - **Timeout Control**: Scripts can be time-limited
//! - **Sandboxed Execution**: Scripts run in isolated Lua environments
//!
//! # Limitations
//!
//! **Read-Your-Writes**: The current implementation does NOT support read-your-writes
//! semantics. This means that `db.get(key)` will not see values written by `db.put(key, value)`
//! within the same script until after the script commits. This limitation exists because
//! WriteBatch does not support querying uncommitted operations.
//!
//! For memory-based storage implementations, read-your-writes can be supported by
//! maintaining a temporary read cache alongside the WriteBatch.
//!
//! # Example
//!
//! ```rust,no_run
//! use aidb::{DB, Options};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), aidb::Error> {
//! let db = Arc::new(DB::open("./data", Options::default())?);
//!
//! // Execute a script
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
//!     db.put("account:2:balance", tostring(tonumber(db.get("account:2:balance")) + 100))
//!     
//!     return "Transfer successful"
//! "#;
//!
//! db.execute_script(script)?;
//! # Ok(())
//! # }
//! ```

pub mod context;
pub mod lua_executor;

pub use context::ScriptContext;
pub use lua_executor::LuaExecutor;
