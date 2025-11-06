//! Version and Manifest management.
//!
//! This module manages SSTable file metadata and version history.
//! The Manifest file records all version changes (file additions/deletions).

use crate::error::{Error, Result};
use crate::sstable::SSTableReader;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A version edit describes changes to the database version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionEdit {
    /// Add a new SSTable file
    AddFile {
        /// Level where the file is added
        level: usize,
        /// File number identifier
        file_number: u64,
        /// Size of the file in bytes
        file_size: u64,
        /// Smallest key in the file
        smallest_key: Vec<u8>,
        /// Largest key in the file
        largest_key: Vec<u8>,
    },
    /// Delete an SSTable file
    DeleteFile {
        /// Level where the file is located
        level: usize,
        /// File number to delete
        file_number: u64,
    },
    /// Set the next file number
    SetNextFileNumber(u64),
    /// Set the sequence number
    SetSequenceNumber(u64),
}

/// A version represents the set of SSTables at a point in time
#[derive(Debug, Clone)]
pub struct Version {
    /// SSTables organized by level
    pub levels: Vec<Vec<FileMetaData>>,
}

/// Metadata for an SSTable file
#[derive(Debug, Clone)]
pub struct FileMetaData {
    /// File number identifier
    pub file_number: u64,
    /// Size of the file in bytes
    pub file_size: u64,
    /// Smallest key in the file
    pub smallest_key: Vec<u8>,
    /// Largest key in the file
    pub largest_key: Vec<u8>,
}

impl Version {
    /// Create a new empty version
    pub fn new(max_levels: usize) -> Self {
        Self { levels: vec![Vec::new(); max_levels] }
    }

    /// Apply a version edit to create a new version
    pub fn apply(&self, edit: &VersionEdit) -> Self {
        let mut new_version = self.clone();

        match edit {
            VersionEdit::AddFile { level, file_number, file_size, smallest_key, largest_key } => {
                new_version.levels[*level].push(FileMetaData {
                    file_number: *file_number,
                    file_size: *file_size,
                    smallest_key: smallest_key.clone(),
                    largest_key: largest_key.clone(),
                });
            }
            VersionEdit::DeleteFile { level, file_number } => {
                new_version.levels[*level].retain(|f| f.file_number != *file_number);
            }
            _ => {
                // SetNextFileNumber and SetSequenceNumber are handled by VersionSet
            }
        }

        new_version
    }

    /// Get the total number of files
    pub fn num_files(&self) -> usize {
        self.levels.iter().map(|level| level.len()).sum()
    }

    /// Get the total size of all files
    pub fn total_size(&self) -> u64 {
        self.levels
            .iter()
            .flat_map(|level| level.iter())
            .map(|file| file.file_size)
            .sum()
    }
}

/// Manages versions and the manifest file
pub struct VersionSet {
    /// Current version
    current: Version,
    /// Path to the manifest file
    manifest_path: PathBuf,
    /// Manifest file handle
    manifest_file: Option<File>,
    /// Maximum number of levels
    max_levels: usize,
    /// Next file number
    next_file_number: u64,
}

impl VersionSet {
    /// Create a new version set
    pub fn new<P: AsRef<Path>>(db_path: P, max_levels: usize) -> Result<Self> {
        let db_path = db_path.as_ref();
        let manifest_path = db_path.join("MANIFEST");

        let mut version_set = Self {
            current: Version::new(max_levels),
            manifest_path: manifest_path.clone(),
            manifest_file: None,
            max_levels,
            next_file_number: 1,
        };

        // Try to recover from existing manifest
        if manifest_path.exists() {
            version_set.recover()?;
        } else {
            // Create new manifest file
            version_set.create_manifest()?;
        }

        Ok(version_set)
    }

    /// Recover from an existing manifest file
    fn recover(&mut self) -> Result<()> {
        log::info!("Recovering from manifest: {:?}", self.manifest_path);

        let file = File::open(&self.manifest_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let edit: VersionEdit = serde_json::from_str(&line)
                .map_err(|e| Error::corruption(format!("Failed to parse manifest entry: {}", e)))?;

            self.apply_edit(&edit)?;
        }

        // Reopen manifest for appending
        self.manifest_file =
            Some(OpenOptions::new().create(true).append(true).open(&self.manifest_path)?);

        log::info!("Recovered {} files from manifest", self.current.num_files());

        Ok(())
    }

    /// Create a new manifest file
    fn create_manifest(&mut self) -> Result<()> {
        log::info!("Creating new manifest: {:?}", self.manifest_path);

        self.manifest_file = Some(
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.manifest_path)?,
        );

        Ok(())
    }

    /// Apply a version edit
    pub fn apply_edit(&mut self, edit: &VersionEdit) -> Result<()> {
        // Update internal state
        match edit {
            VersionEdit::SetNextFileNumber(num) => {
                self.next_file_number = *num;
            }
            VersionEdit::SetSequenceNumber(_) => {
                // Handled by DB
            }
            _ => {
                // Apply to current version
                self.current = self.current.apply(edit);
            }
        }

        Ok(())
    }

    /// Log a version edit to the manifest
    pub fn log_edit(&mut self, edit: &VersionEdit) -> Result<()> {
        // Apply the edit
        self.apply_edit(edit)?;

        // Write to manifest file
        if let Some(ref mut file) = self.manifest_file {
            let json = serde_json::to_string(edit)
                .map_err(|e| Error::internal(format!("Failed to serialize edit: {}", e)))?;
            writeln!(file, "{}", json)?;
            file.flush()?;
        }

        Ok(())
    }

    /// Get the current version
    pub fn current(&self) -> &Version {
        &self.current
    }

    /// Get the next file number
    pub fn next_file_number(&self) -> u64 {
        self.next_file_number
    }

    /// Allocate a new file number
    pub fn allocate_file_number(&mut self) -> u64 {
        let num = self.next_file_number;
        self.next_file_number += 1;
        num
    }

    /// Load SSTable readers for the current version
    pub fn load_sstables(&self, db_path: &Path) -> Result<Vec<Vec<Arc<SSTableReader>>>> {
        let mut levels = vec![Vec::new(); self.max_levels];

        for (level_idx, level) in self.current.levels.iter().enumerate() {
            for file_meta in level {
                let path = db_path.join(format!("{:06}.sst", file_meta.file_number));
                if path.exists() {
                    match SSTableReader::open(&path) {
                        Ok(reader) => {
                            levels[level_idx].push(Arc::new(reader));
                        }
                        Err(e) => {
                            log::warn!("Failed to load SSTable {:?}: {}", path, e);
                        }
                    }
                }
            }
        }

        Ok(levels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_version_apply_add_file() {
        let version = Version::new(7);

        let edit = VersionEdit::AddFile {
            level: 0,
            file_number: 1,
            file_size: 1024,
            smallest_key: b"a".to_vec(),
            largest_key: b"z".to_vec(),
        };

        let new_version = version.apply(&edit);

        assert_eq!(new_version.levels[0].len(), 1);
        assert_eq!(new_version.levels[0][0].file_number, 1);
        assert_eq!(new_version.levels[0][0].file_size, 1024);
    }

    #[test]
    fn test_version_apply_delete_file() {
        let version = Version::new(7);

        // Add a file
        let add_edit = VersionEdit::AddFile {
            level: 0,
            file_number: 1,
            file_size: 1024,
            smallest_key: b"a".to_vec(),
            largest_key: b"z".to_vec(),
        };
        let version = version.apply(&add_edit);

        // Delete the file
        let delete_edit = VersionEdit::DeleteFile { level: 0, file_number: 1 };
        let version = version.apply(&delete_edit);

        assert_eq!(version.levels[0].len(), 0);
    }

    #[test]
    fn test_version_set_create() {
        let temp_dir = TempDir::new().unwrap();
        let version_set = VersionSet::new(temp_dir.path(), 7).unwrap();

        assert_eq!(version_set.current().num_files(), 0);
        assert!(version_set.manifest_path.exists());
    }

    #[test]
    fn test_version_set_log_edit() {
        let temp_dir = TempDir::new().unwrap();
        let mut version_set = VersionSet::new(temp_dir.path(), 7).unwrap();

        let edit = VersionEdit::AddFile {
            level: 0,
            file_number: 1,
            file_size: 1024,
            smallest_key: b"a".to_vec(),
            largest_key: b"z".to_vec(),
        };

        version_set.log_edit(&edit).unwrap();

        assert_eq!(version_set.current().num_files(), 1);
        assert_eq!(version_set.current().levels[0].len(), 1);
    }

    #[test]
    fn test_version_set_recover() {
        let temp_dir = TempDir::new().unwrap();

        // Create and populate a version set
        {
            let mut version_set = VersionSet::new(temp_dir.path(), 7).unwrap();

            for i in 1..=5 {
                let edit = VersionEdit::AddFile {
                    level: 0,
                    file_number: i,
                    file_size: 1024,
                    smallest_key: b"a".to_vec(),
                    largest_key: b"z".to_vec(),
                };
                version_set.log_edit(&edit).unwrap();
            }
        }

        // Recover from manifest
        let version_set = VersionSet::new(temp_dir.path(), 7).unwrap();
        assert_eq!(version_set.current().num_files(), 5);
        assert_eq!(version_set.current().levels[0].len(), 5);
    }

    #[test]
    fn test_version_set_allocate_file_number() {
        let temp_dir = TempDir::new().unwrap();
        let mut version_set = VersionSet::new(temp_dir.path(), 7).unwrap();

        let num1 = version_set.allocate_file_number();
        let num2 = version_set.allocate_file_number();
        let num3 = version_set.allocate_file_number();

        assert_eq!(num1, 1);
        assert_eq!(num2, 2);
        assert_eq!(num3, 3);
    }

    #[test]
    fn test_version_total_size() {
        let version = Version::new(7);

        let edit1 = VersionEdit::AddFile {
            level: 0,
            file_number: 1,
            file_size: 1024,
            smallest_key: b"a".to_vec(),
            largest_key: b"m".to_vec(),
        };

        let edit2 = VersionEdit::AddFile {
            level: 0,
            file_number: 2,
            file_size: 2048,
            smallest_key: b"n".to_vec(),
            largest_key: b"z".to_vec(),
        };

        let version = version.apply(&edit1).apply(&edit2);

        assert_eq!(version.total_size(), 3072);
    }
}
