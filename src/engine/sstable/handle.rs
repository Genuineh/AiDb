//! BlockHandle — SSTable 内 block 位置.

use crate::error::{Error, Result};

/// block 在文件中的偏移与总大小 (含 5B trailer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockHandle {
    pub offset: u64,
    pub size: u64,
}

impl BlockHandle {
    pub fn encode(self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&self.offset.to_le_bytes());
        buf[8..].copy_from_slice(&self.size.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < 16 {
            return Err(Error::Corruption("BlockHandle too short".into()));
        }
        Ok(Self {
            offset: u64::from_le_bytes(buf[..8].try_into().unwrap()),
            size: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
        })
    }
}
