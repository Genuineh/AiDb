//! InternalKey 编码与比较 — MemTable / SSTable / MergeIterator 共用.
//!
//! # 8B 尾部布局
//!
//! ```text
//! InternalKey = [user_key][~seq_hi:7B][value_type:1B]
//!               sequence 56 位大端写入高 7 字节, 逐位取反
//! ```
//!
//! 比较规则 `compare_internal_key`: (user_key asc, sequence desc, value_type asc) — 同 key 下
//! 新版本 (seq 大) 排前, seek 到首个 `seq <= max_seq` 即最新可见版本.
//!
//! # Invariant
//!
//! - `sequence < 2^56`; 超界 → `Error::InvalidState` (`check_sequence`).
//! - `K_TYPE_SEEK = 0` (TypePut) 为全类型最小值, 用作 seek 目标 key.

use crate::error::{Error, Result};
use std::cmp::Ordering;
use std::sync::Arc;

/// 合法 sequence 上界 (不含): InternalKey 仅编码低 56 位.
pub const SEQUENCE_LIMIT: u64 = 1 << 56;

/// 非 snapshot 读使用的最大 sequence (2^56 - 1).
pub const K_MAX_SEQUENCE: u64 = SEQUENCE_LIMIT - 1;

/// 拒绝 `sequence >= 2^56` (DB/WAL/MemTable 共用).
pub fn check_sequence(sequence: u64) -> Result<()> {
    if sequence >= SEQUENCE_LIMIT {
        return Err(Error::InvalidState(format!(
            "sequence overflow: {sequence} >= 2^56"
        )));
    }
    Ok(())
}

/// seek 使用的 ValueType (TypePut = 0, 全类型最小值).
pub const K_TYPE_SEEK: u8 = 0;

/// ValueType (1 Byte)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueType {
    TypePut = 0,
    TypeDelete = 1,
    TypeRangeDelete = 2,
}

impl TryFrom<u8> for ValueType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(ValueType::TypePut),
            1 => Ok(ValueType::TypeDelete),
            2 => Ok(ValueType::TypeRangeDelete),
            _ => Err(Error::Corruption(format!("unknown ValueType: {value}"))),
        }
    }
}

/// 编码 InternalKey: user_key + 7B 位取反 sequence + 1B type.
pub fn encode_internal_key(user_key: &[u8], sequence: u64, value_type: ValueType) -> Vec<u8> {
    let mut buf = Vec::with_capacity(user_key.len() + 8);
    buf.extend_from_slice(user_key);
    let seq_bytes = (sequence << 8).to_be_bytes();
    for b in &seq_bytes[..7] {
        buf.push(!b);
    }
    buf.push(value_type as u8);
    buf
}

/// 零堆分配: 将 InternalKey 写入栈缓冲区 `[u8; 256]` 并在闭包中传递切片 (user_key 长度 <= 248).
pub fn encode_internal_key_buffered<F, R>(
    user_key: &[u8],
    sequence: u64,
    value_type: ValueType,
    f: F,
) -> R
where
    F: FnOnce(&[u8]) -> R,
{
    let target_len = user_key.len() + 8;
    if target_len <= 256 {
        let mut buf = [0u8; 256];
        buf[..user_key.len()].copy_from_slice(user_key);
        let seq_bytes = (sequence << 8).to_be_bytes();
        for (i, &b) in seq_bytes[..7].iter().enumerate() {
            buf[user_key.len() + i] = !b;
        }
        buf[user_key.len() + 7] = value_type as u8;
        f(&buf[..target_len])
    } else {
        let encoded = encode_internal_key(user_key, sequence, value_type);
        f(&encoded)
    }
}

/// encode_internal_key 的 `Arc<[u8]>` 变体。通过 `Vec::into()` 一步到位, 免去
/// 调用方手动 `from_slice` 产生的中间变量和重复解引用 (F-019)。
pub fn encode_internal_key_arc(user_key: &[u8], sequence: u64, value_type: ValueType) -> Arc<[u8]> {
    encode_internal_key(user_key, sequence, value_type).into()
}

/// 解码 InternalKey.
pub fn decode_internal_key(encoded: &[u8]) -> Result<(Vec<u8>, u64, ValueType)> {
    if encoded.len() < 8 {
        return Err(Error::Corruption("InternalKey too short".into()));
    }
    let value_type = ValueType::try_from(encoded[encoded.len() - 1])?;
    let user_key = encoded[..encoded.len() - 8].to_vec();
    let sequence = extract_sequence(encoded)?;
    Ok((user_key, sequence, value_type))
}

/// 比较两段 InternalKey 编码字节: (user_key asc, sequence desc, value_type asc).
pub fn compare_internal_key(a: &[u8], b: &[u8]) -> Ordering {
    match extract_user_key(a).cmp(extract_user_key(b)) {
        Ordering::Equal => {}
        ord => return ord,
    }
    let sa = extract_sequence(a).unwrap_or(0);
    let sb = extract_sequence(b).unwrap_or(0);
    match sb.cmp(&sa) {
        Ordering::Equal => {}
        ord => return ord,
    }
    extract_value_type(a)
        .unwrap_or(ValueType::TypePut)
        .cmp(&extract_value_type(b).unwrap_or(ValueType::TypePut))
}

/// 从 InternalKey 编码中提取 user_key.
pub fn extract_user_key(internal_key: &[u8]) -> &[u8] {
    &internal_key[..internal_key.len().saturating_sub(8)]
}

/// 从 InternalKey 中提取 value_type.
pub fn extract_value_type(internal_key: &[u8]) -> Result<ValueType> {
    if internal_key.is_empty() {
        return Err(Error::Corruption("empty InternalKey".into()));
    }
    ValueType::try_from(internal_key[internal_key.len() - 1])
}

/// 从 InternalKey 编码中提取 sequence.
pub fn extract_sequence(internal_key: &[u8]) -> Result<u64> {
    if internal_key.len() < 8 {
        return Err(Error::Corruption(
            "InternalKey too short for sequence extraction".into(),
        ));
    }
    let seq_start = internal_key.len() - 8;
    let mut seq_bytes = [0u8; 8];
    seq_bytes[..7].copy_from_slice(&internal_key[seq_start..seq_start + 7]);
    for b in seq_bytes[..7].iter_mut() {
        *b = !*b;
    }
    Ok(u64::from_be_bytes(seq_bytes) >> 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_key_encode_decode() {
        let enc = encode_internal_key(b"foo", 100, ValueType::TypePut);
        let (k, seq, ty) = decode_internal_key(&enc).unwrap();
        assert_eq!(k, b"foo");
        assert_eq!(seq, 100);
        assert_eq!(ty, ValueType::TypePut);
    }

    #[test]
    fn test_internal_key_ordering() {
        let k1 = encode_internal_key(b"k", 1, ValueType::TypePut);
        let k2 = encode_internal_key(b"k", 2, ValueType::TypePut);
        // 新版本 (seq 大) 在 SkipMap 中排更前 → Less
        assert_eq!(compare_internal_key(&k2, &k1), Ordering::Less);
        assert_eq!(compare_internal_key(&k1, &k2), Ordering::Greater);
    }

    #[test]
    fn test_compare_internal_key_user_key_order() {
        let a = encode_internal_key(b"a", 99, ValueType::TypePut);
        let b = encode_internal_key(b"b", 1, ValueType::TypePut);
        assert_eq!(compare_internal_key(&a, &b), Ordering::Less);
    }

    #[test]
    fn test_range_delete_ordering() {
        let put = encode_internal_key(b"k", 100, ValueType::TypePut);
        let del = encode_internal_key(b"k", 100, ValueType::TypeDelete);
        let rng = encode_internal_key(b"k", 100, ValueType::TypeRangeDelete);
        assert_eq!(compare_internal_key(&put, &del), Ordering::Less);
        assert_eq!(compare_internal_key(&del, &rng), Ordering::Less);
        assert_eq!(compare_internal_key(&put, &rng), Ordering::Less);
    }

    #[test]
    fn test_range_delete_decode() {
        let enc = encode_internal_key(b"k", 100, ValueType::TypeRangeDelete);
        let (k, seq, ty) = decode_internal_key(&enc).unwrap();
        assert_eq!(k, b"k");
        assert_eq!(seq, 100);
        assert_eq!(ty, ValueType::TypeRangeDelete);
    }
}
