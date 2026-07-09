//! FIX-0056-A1: 迁移 tombstone/tip 的**值**编码 (键编码见
//! `storage/keys.rs::mig_tombstone_key` / `mig_tip_key`).
//!
//! tombstone 只记录"这个 key 在本 epoch 内最后一次是被 Put 还是 Del,
//! 以及分配到的单调 seq" —— 不存值本身. `PutConditional` / 合并读永远从
//! `sm_key` 读取真实值; tombstone 只用来回答"要不要相信 sm_key 里 (或
//! source 里) 看到的东西".

/// tombstone 记录的操作类型.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigOp {
    Put,
    Del,
}

const OP_TAG_PUT: u8 = 1;
const OP_TAG_DEL: u8 = 2;
/// 1 byte op tag + 8 byte BE seq.
const TOMBSTONE_LEN: usize = 9;
const TIP_LEN: usize = 8;

impl MigOp {
    /// tombstone value 里的操作标记, 也用作 `GetMigrationTombstone` RPC
    /// 传输 tag (FIX-0056-A1 跨节点合并读).
    pub fn tag(self) -> u8 {
        match self {
            MigOp::Put => OP_TAG_PUT,
            MigOp::Del => OP_TAG_DEL,
        }
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            OP_TAG_PUT => Some(MigOp::Put),
            OP_TAG_DEL => Some(MigOp::Del),
            _ => None,
        }
    }
}

/// 编码 tombstone value: `op_tag(1B) || seq(8B BE)`.
pub fn encode_tombstone(op: MigOp, seq: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(TOMBSTONE_LEN);
    out.push(op.tag());
    out.extend_from_slice(&seq.to_be_bytes());
    out
}

/// 解码 tombstone value. 长度或 tag 不合法时返回 `None` (视为"无 tombstone").
pub fn decode_tombstone(bytes: &[u8]) -> Option<(MigOp, u64)> {
    if bytes.len() != TOMBSTONE_LEN {
        return None;
    }
    let op = MigOp::from_tag(bytes[0])?;
    let seq = u64::from_be_bytes(bytes[1..TOMBSTONE_LEN].try_into().ok()?);
    Some((op, seq))
}

/// 编码 tip value: `seq(8B BE)`.
pub fn encode_tip(seq: u64) -> Vec<u8> {
    seq.to_be_bytes().to_vec()
}

/// 解码 tip value. 长度不合法时返回 `None` (视为 tip = 0, 由调用方处理).
pub fn decode_tip(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != TIP_LEN {
        return None;
    }
    Some(u64::from_be_bytes(bytes.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tombstone_roundtrip_put() {
        let bytes = encode_tombstone(MigOp::Put, 42);
        assert_eq!(decode_tombstone(&bytes), Some((MigOp::Put, 42)));
    }

    #[test]
    fn test_tombstone_roundtrip_del() {
        let bytes = encode_tombstone(MigOp::Del, 1);
        assert_eq!(decode_tombstone(&bytes), Some((MigOp::Del, 1)));
    }

    #[test]
    fn test_tombstone_decode_rejects_bad_length() {
        assert_eq!(decode_tombstone(b"short"), None);
        assert_eq!(decode_tombstone(&[0u8; 100]), None);
    }

    #[test]
    fn test_tombstone_decode_rejects_bad_tag() {
        let mut bytes = encode_tombstone(MigOp::Put, 1);
        bytes[0] = 0xff;
        assert_eq!(decode_tombstone(&bytes), None);
    }

    #[test]
    fn test_tip_roundtrip() {
        let bytes = encode_tip(1234);
        assert_eq!(decode_tip(&bytes), Some(1234));
    }

    #[test]
    fn test_tip_decode_rejects_bad_length() {
        assert_eq!(decode_tip(b"nope"), None);
    }
}
