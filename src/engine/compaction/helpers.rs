//! Compaction 用 InternalKey 辅助 (带长度校验).

use crate::engine::memtable::extract_user_key;
use crate::error::{Error, Result};

/// 从 InternalKey 取 user_key; 长度 < 8 返回 Corruption.
pub fn user_key_from_internal(key: &[u8]) -> Result<&[u8]> {
    if key.len() < 8 {
        return Err(Error::Corruption("InternalKey too short".into()));
    }
    Ok(extract_user_key(key))
}

/// key range 重叠检测 (按 user_key, 参数为完整 InternalKey).
pub fn key_ranges_overlap_by_meta_raw(s1: &[u8], l1: &[u8], s2: &[u8], l2: &[u8]) -> bool {
    if s1.len() < 8 || l1.len() < 8 || s2.len() < 8 || l2.len() < 8 {
        tracing::warn!(
          target: "cmp",
          "InternalKey shorter than 8 bytes; assuming overlap"
        );
        return true;
    }
    let u1_start = &s1[..s1.len() - 8];
    let u1_end = &l1[..l1.len() - 8];
    let u2_start = &s2[..s2.len() - 8];
    let u2_end = &l2[..l2.len() - 8];
    u1_start <= u2_end && u2_start <= u1_end
}
