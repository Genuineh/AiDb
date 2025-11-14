//! Lua scripting support with automatic rollback.
//!
//! This module provides Lua scripting capabilities for the database with
//! built-in transaction support. All operations within a script are buffered
//! and only committed if the script completes successfully, providing
//! automatic rollback on failure.
//!
//! # Architecture
//!
//! The scripting system consists of two main components:
//!
//! - **ScriptContext**: Buffers database operations in memory
//! - **LuaExecutor**: Executes Lua scripts with database access
//!
//! # Features
//!
//! - **Automatic Rollback**: Failed scripts discard all changes
//! - **Read-Your-Writes**: Scripts can read their own uncommitted changes
//! - **Atomic Commits**: All operations succeed or fail together
//! - **Timeout Control**: Scripts can be time-limited
//! - **Sandboxed Execution**: Scripts run in isolated Lua environments
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
//!     -- Transfer credits between accounts
//!     local balance1 = db.get("account:1:balance")
//!     local balance2 = db.get("account:2:balance")
//!     
//!     if tonumber(balance1) < 100 then
//!         error("Insufficient balance")
//!     end
//!     
//!     db.put("account:1:balance", tostring(tonumber(balance1) - 100))
//!     db.put("account:2:balance", tostring(tonumber(balance2) + 100))
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
