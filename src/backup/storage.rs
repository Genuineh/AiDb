//! Storage backends for backup and recovery.
//!
//! This module defines the `BackupStorage` trait and provides implementations
//! for different storage backends (local filesystem, S3, etc.).

use crate::Result;
use std::path::{Path, PathBuf};

/// Trait for backup storage backends.
///
/// This trait abstracts the storage layer, allowing backups to be stored
/// in different locations (local filesystem, S3, etc.).
pub trait BackupStorage: Send + Sync {
    /// Write data to the specified path in the backup storage.
    fn write(&self, path: &str, data: &[u8]) -> Result<()>;

    /// Read data from the specified path in the backup storage.
    fn read(&self, path: &str) -> Result<Vec<u8>>;

    /// Check if a file exists at the specified path.
    fn exists(&self, path: &str) -> Result<bool>;

    /// List all files with the given prefix.
    fn list(&self, prefix: &str) -> Result<Vec<String>>;

    /// Delete a file at the specified path.
    fn delete(&self, path: &str) -> Result<()>;

    /// Copy a local file to the backup storage.
    fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()>;

    /// Copy a file from backup storage to local filesystem.
    fn download_file(&self, remote_path: &str, local_path: &Path) -> Result<()>;
}

/// Local filesystem storage backend.
///
/// Stores backups in a local directory.
pub struct LocalFileStorage {
    /// Root directory for backups
    root_dir: PathBuf,
}

impl LocalFileStorage {
    /// Create a new local file storage backend.
    ///
    /// # Arguments
    ///
    /// * `root_dir` - Root directory for storing backups
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Self {
        Self { root_dir: root_dir.as_ref().to_path_buf() }
    }

    /// Get the full path for a given relative path.
    fn full_path(&self, path: &str) -> PathBuf {
        self.root_dir.join(path)
    }
}

impl BackupStorage for LocalFileStorage {
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        let full_path = self.full_path(path);

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&full_path, data)?;
        Ok(())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let full_path = self.full_path(path);
        std::fs::read(&full_path).map_err(|e| e.into())
    }

    fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.full_path(path).exists())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let prefix_path = self.full_path(prefix);
        let mut results = Vec::new();

        // If prefix doesn't exist, return empty list
        if !prefix_path.exists() {
            return Ok(results);
        }

        // Walk the directory tree
        for entry in walkdir::WalkDir::new(&prefix_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if let Ok(relative) = entry.path().strip_prefix(&self.root_dir) {
                if let Some(path_str) = relative.to_str() {
                    results.push(path_str.to_string());
                }
            }
        }

        Ok(results)
    }

    fn delete(&self, path: &str) -> Result<()> {
        let full_path = self.full_path(path);
        if full_path.is_file() {
            std::fs::remove_file(&full_path)?;
        } else if full_path.is_dir() {
            std::fs::remove_dir_all(&full_path)?;
        }
        Ok(())
    }

    fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let full_path = self.full_path(remote_path);

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::copy(local_path, &full_path)?;
        Ok(())
    }

    fn download_file(&self, remote_path: &str, local_path: &Path) -> Result<()> {
        let full_path = self.full_path(remote_path);

        // Create parent directories if they don't exist
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::copy(&full_path, local_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_local_storage_write_read() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(tmp_dir.path());

        let data = b"test data";
        storage.write("test/file.txt", data).unwrap();

        let read_data = storage.read("test/file.txt").unwrap();
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_local_storage_exists() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(tmp_dir.path());

        assert!(!storage.exists("test/file.txt").unwrap());

        storage.write("test/file.txt", b"data").unwrap();
        assert!(storage.exists("test/file.txt").unwrap());
    }

    #[test]
    fn test_local_storage_list() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(tmp_dir.path());

        storage.write("test/file1.txt", b"data1").unwrap();
        storage.write("test/file2.txt", b"data2").unwrap();
        storage.write("test/subdir/file3.txt", b"data3").unwrap();

        let files = storage.list("test").unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"test/file1.txt".to_string()));
        assert!(files.contains(&"test/file2.txt".to_string()));
        assert!(files.contains(&"test/subdir/file3.txt".to_string()));
    }

    #[test]
    fn test_local_storage_delete() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(tmp_dir.path());

        storage.write("test/file.txt", b"data").unwrap();
        assert!(storage.exists("test/file.txt").unwrap());

        storage.delete("test/file.txt").unwrap();
        assert!(!storage.exists("test/file.txt").unwrap());
    }

    #[test]
    fn test_local_storage_upload_download() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(tmp_dir.path());

        // Create a source file
        let source_dir = TempDir::new().unwrap();
        let source_path = source_dir.path().join("source.txt");
        std::fs::write(&source_path, b"test data").unwrap();

        // Upload file
        storage.upload_file(&source_path, "backup/source.txt").unwrap();
        assert!(storage.exists("backup/source.txt").unwrap());

        // Download file
        let dest_dir = TempDir::new().unwrap();
        let dest_path = dest_dir.path().join("dest.txt");
        storage.download_file("backup/source.txt", &dest_path).unwrap();

        // Verify content
        let content = std::fs::read(&dest_path).unwrap();
        assert_eq!(content, b"test data");
    }
}
