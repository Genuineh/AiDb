//! SSTable Properties Block — 统计信息.

use crate::error::{Error, Result};

/// SSTable 统计属性, 24B 大端编码.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SstProperties {
    pub num_entries: u64,
    pub raw_key_size: u64,
    pub raw_value_size: u64,
}

impl SstProperties {
    pub const ENCODED_SIZE: usize = 24;

    pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
        let mut buf = [0u8; Self::ENCODED_SIZE];
        buf[0..8].copy_from_slice(&self.num_entries.to_be_bytes());
        buf[8..16].copy_from_slice(&self.raw_key_size.to_be_bytes());
        buf[16..24].copy_from_slice(&self.raw_value_size.to_be_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::ENCODED_SIZE {
            return Err(Error::Corruption("SSTable properties too short".into()));
        }
        Ok(Self {
            num_entries: u64::from_be_bytes(buf[0..8].try_into().unwrap()),
            raw_key_size: u64::from_be_bytes(buf[8..16].try_into().unwrap()),
            raw_value_size: u64::from_be_bytes(buf[16..24].try_into().unwrap()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_properties_roundtrip() {
        let p = SstProperties {
            num_entries: 42,
            raw_key_size: 1024,
            raw_value_size: 4096,
        };
        let e = p.encode();
        let d = SstProperties::decode(&e).unwrap();
        assert_eq!(d, p);
    }

    #[test]
    fn test_properties_decode_too_short() {
        assert!(SstProperties::decode(&[0u8; 8]).is_err());
    }
}
