//! BackupStorage trait + LocalFileStorage 实现.

use std::fs;
use std::path::{Path, PathBuf};

use crate::backup::util;
use crate::error::Result;

/// 备份存储抽象, 支持多种后端 (本地文件系统、对象存储等).
pub trait BackupStorage: Send + Sync {
    fn store(&self, src: &Path, dest_path: &Path) -> Result<String>;
    fn store_bytes(&self, dest_path: &Path, data: &[u8]) -> Result<String>;
    fn load(&self, src_path: &Path, dest: &Path) -> Result<()>;
    fn read_to_string(&self, path: &Path) -> Result<String>;
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
    fn backup_path(&self, backup_id: u64) -> PathBuf;
    fn delete(&self, path: &Path) -> Result<()>;
}

pub struct LocalFileStorage {
    root: PathBuf,
}

impl LocalFileStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl BackupStorage for LocalFileStorage {
    fn store(&self, src: &Path, dest_path: &Path) -> Result<String> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dest_path)?;
        util::sha256_file(dest_path)
    }

    fn store_bytes(&self, dest_path: &Path, data: &[u8]) -> Result<String> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest_path, data)?;
        Ok(util::sha256_bytes(data))
    }

    fn load(&self, src_path: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src_path, dest)?;
        Ok(())
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut entries = Vec::new();
        if !self.root.exists() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) {
                entries.push(name);
            }
        }
        entries.sort();
        Ok(entries)
    }

    fn backup_path(&self, backup_id: u64) -> PathBuf {
        self.root.join(format!("backup_{backup_id}"))
    }

    fn delete(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_local_storage_store_load() {
        let root = tempdir().unwrap();
        let storage = LocalFileStorage::new(root.path().to_path_buf());
        let src = root.path().join("source.txt");
        std::fs::write(&src, b"hello world").unwrap();
        let backup_id = 12345u64;
        let dest = storage.backup_path(backup_id).join("source.txt");
        let checksum = storage.store(&src, &dest).unwrap();
        assert!(!checksum.is_empty());
        let loaded = root.path().join("loaded.txt");
        storage.load(&dest, &loaded).unwrap();
        assert_eq!(std::fs::read(&loaded).unwrap(), b"hello world");
    }

    #[test]
    fn test_local_storage_store_bytes() {
        let root = tempdir().unwrap();
        let storage = LocalFileStorage::new(root.path().to_path_buf());
        let path = storage.backup_path(12345).join("manifest.json");
        storage.store_bytes(&path, b"{\"key\": \"value\"}").unwrap();
        assert!(path.exists());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"key\": \"value\"}"
        );
    }

    #[test]
    fn test_local_storage_list() {
        let root = tempdir().unwrap();
        let storage = LocalFileStorage::new(root.path().to_path_buf());
        storage
            .store_bytes(&storage.backup_path(1).join("f1"), b"a")
            .unwrap();
        storage
            .store_bytes(&storage.backup_path(2).join("f2"), b"b")
            .unwrap();
        let entries = storage.list("backup_").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_local_storage_delete() {
        let root = tempdir().unwrap();
        let storage = LocalFileStorage::new(root.path().to_path_buf());
        let path = storage.backup_path(99).join("x.txt");
        storage.store_bytes(&path, b"data").unwrap();
        storage.delete(&storage.backup_path(99)).unwrap();
        assert!(!storage.backup_path(99).exists());
    }
}
