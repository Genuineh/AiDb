//! Backup and recovery system for AiDb.
//!
//! This module provides functionality for creating consistent backups of the database
//! and restoring from them. It supports multiple storage backends (local filesystem, S3, etc.)
//! and both full and incremental backups.
//!
//! # Architecture
//!
//! - **BackupStorage trait**: Abstract interface for different storage backends
//! - **BackupManager**: Creates and manages backups (snapshots + WAL archiving)
//! - **RecoveryManager**: Restores database from backups
//! - **BackupMetadata**: Tracks backup information and retention policies
//!
//! # Example
//!
//! ```rust,no_run
//! use aidb::{DB, Options};
//! use aidb::backup::{BackupManager, LocalFileStorage};
//!
//! # fn main() -> Result<(), aidb::Error> {
//! let db = DB::open("./data", Options::default())?;
//! let storage = LocalFileStorage::new("./backups");
//! let backup_manager = BackupManager::new(storage);
//!
//! // Create a backup
//! let backup_id = backup_manager.create_backup(&db)?;
//! println!("Created backup: {}", backup_id);
//!
//! // Restore from backup
//! // RecoveryManager::restore(&db, &backup_manager, &backup_id)?;
//! # Ok(())
//! # }
//! ```

pub mod manager;
pub mod metadata;
pub mod recovery;
pub mod storage;

pub use manager::BackupManager;
pub use metadata::{BackupInfo, BackupMetadata, RetentionPolicy};
pub use recovery::RecoveryManager;
pub use storage::{BackupStorage, LocalFileStorage};
