//! SHA256 工具函数，供整个 backup 模块共享。

use crate::error::Result;
use std::io::Read;
use std::path::Path;

/// 计算文件的 SHA256 十六进制字符串。
pub fn sha256_file(path: &Path) -> Result<String> {
  let mut file = std::fs::File::open(path)?;
  let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
  let mut buf = [0u8; 8192];
  loop {
    let n = file.read(&mut buf)?;
    if n == 0 {
      break;
    }
    ctx.update(&buf[..n]);
  }
  Ok(hex::encode(ctx.finish().as_ref()))
}

/// 计算字节数据的 SHA256 十六进制字符串。
pub fn sha256_bytes(data: &[u8]) -> String {
  let ctx = ring::digest::Context::new(&ring::digest::SHA256);
  let mut ctx = ctx;
  ctx.update(data);
  hex::encode(ctx.finish().as_ref())
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  #[test]
  fn test_sha256_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, b"hello").unwrap();
    let hash = sha256_file(&path).unwrap();
    assert_eq!(hash.len(), 64);
  }

  #[test]
  fn test_sha256_bytes() {
    let hash = sha256_bytes(b"hello");
    assert_eq!(hash.len(), 64);
  }
}
