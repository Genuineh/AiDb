//! Range tombstone 辅助函数 (LevelDB 内联风格).

use super::internal_key::{extract_sequence, extract_user_key, extract_value_type, ValueType};
use super::key_bytes::InternalKeyBytes;
use crate::error::Result;
use crossbeam_skiplist::SkipMap;
use std::ops::Bound;
use std::sync::Arc;

/// 半开区间 `[start, end)` 是否覆盖 `user_key`.
pub fn range_covers(start: &[u8], end: &[u8], user_key: &[u8]) -> bool {
    start <= user_key && user_key < end
}

/// 给定 user_key, 返回 strictly greater 的最小 user_key 前缀 (用于 SkipMap 上界).
pub fn user_key_successor(user_key: &[u8]) -> Vec<u8> {
    let mut upper = user_key.to_vec();
    while let Some(last) = upper.last_mut() {
        if *last == 0xff {
            upper.pop();
        } else {
            *last += 1;
            return upper;
        }
    }
    vec![0]
}

/// 在 SkipMap 中查找覆盖 `user_key` 且 `sequence <= max_seq` 的最大 range tombstone sequence.
pub(crate) fn max_covering_range_tombstone_seq(
    table: &SkipMap<InternalKeyBytes, Arc<[u8]>>,
    user_key: &[u8],
    max_seq: u64,
) -> Result<Option<u64>> {
    let upper = user_key_successor(user_key);
    let bound = InternalKeyBytes::from_slice(&super::internal_key::encode_internal_key(
        &upper,
        0,
        ValueType::TypePut,
    ));
    let Some(mut entry) = table.upper_bound(Bound::Excluded(&bound)) else {
        return Ok(None);
    };

    let mut best: Option<u64> = None;
    loop {
        let ik = entry.key().as_ref();
        let start = extract_user_key(ik);
        if start > user_key {
            if entry.prev().is_none() {
                break;
            }
            entry = entry.prev().unwrap();
            continue;
        }

        if extract_value_type(ik)? == ValueType::TypeRangeDelete {
            let seq = extract_sequence(ik)?;
            if seq <= max_seq {
                let end = entry.value().as_ref();
                if range_covers(start, end, user_key) {
                    best = Some(best.map_or(seq, |b| b.max(seq)));
                }
            }
        }

        if entry.prev().is_none() {
            break;
        }
        entry = entry.prev().unwrap();
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::memtable::{encode_internal_key, key_bytes::InternalKeyBytes, ValueType};

    #[test]
    fn test_user_key_successor() {
        assert_eq!(user_key_successor(b"ab"), b"ac".to_vec());
        assert_eq!(user_key_successor(b"ab\xff"), b"ac".to_vec());
    }

    #[test]
    fn test_max_covering_range_tombstone_seq() {
        let table = SkipMap::new();
        let ik = InternalKeyBytes::from_slice(&encode_internal_key(
            b"10",
            5,
            ValueType::TypeRangeDelete,
        ));
        table.insert(ik, Arc::from(b"50".as_slice()));

        assert_eq!(
            max_covering_range_tombstone_seq(&table, b"25", 10).unwrap(),
            Some(5)
        );
        assert_eq!(
            max_covering_range_tombstone_seq(&table, b"60", 10).unwrap(),
            None
        );
        assert_eq!(
            max_covering_range_tombstone_seq(&table, b"25", 4).unwrap(),
            None
        );
    }
}
