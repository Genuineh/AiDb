//! 数据目录内 WAL / SSTable 文件编号扫描.

use crate::engine::sstable::parse_sstable_filename;
use std::path::Path;

fn parse_wal_filename(filename: &str) -> Option<u64> {
    let name = filename.strip_suffix(".log")?;
    let num = name.strip_prefix("wal_")?;
    num.parse::<u64>().ok()
}

pub(crate) fn scan_next_wal_file_number(path: &Path) -> u64 {
    let mut max = 0u64;
    if let Ok(dir) = std::fs::read_dir(path) {
        for entry in dir.flatten() {
            let name = entry.file_name();
            if let Some(n) = parse_wal_filename(&name.to_string_lossy()) {
                max = max.max(n);
            }
        }
    }
    max.saturating_add(1)
}

#[allow(dead_code)]
pub(crate) fn scan_next_sst_file_number(path: &Path) -> u64 {
    let mut max = 0u64;
    if let Ok(dir) = std::fs::read_dir(path) {
        for entry in dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some((n, _)) = parse_sstable_filename(&name_str) {
                max = max.max(n);
            }
        }
    }
    max.saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scan_empty_dir_starts_at_one() {
        let dir = tempdir().unwrap();
        assert_eq!(scan_next_wal_file_number(dir.path()), 1);
        assert_eq!(scan_next_sst_file_number(dir.path()), 1);
    }
}
