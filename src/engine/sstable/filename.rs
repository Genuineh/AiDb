//! SSTable 文件命名.

use std::path::{Path, PathBuf};

/// `{dir}/{file_number:06d}_L{level}.sst`
pub fn sstable_path(dir: &Path, file_number: u64, level: usize) -> PathBuf {
  dir.join(format!("{file_number:06}_L{level}.sst"))
}

/// 解析 `000123_L5.sst` 或旧格式 `000001.sst` (level 0).
pub fn parse_sstable_filename(filename: &str) -> Option<(u64, usize)> {
  let name = filename.strip_suffix(".sst")?;
  if let Some((num, level)) = name.split_once("_L") {
    let n: u64 = num.parse().ok()?;
    let l: usize = level.parse().ok()?;
    return Some((n, l));
  }
  let n: u64 = name.parse().ok()?;
  Some((n, 0))
}
