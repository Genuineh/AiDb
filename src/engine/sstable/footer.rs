//! SSTable 文件尾 48B Footer.

use crate::error::{Error, Result};

use super::handle::BlockHandle;

pub const FOOTER_SIZE: usize = 48;
pub const MAGIC_NUMBER: u64 = 0x5f454c4241545353;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
  pub meta_index_handle: BlockHandle,
  pub index_handle: BlockHandle,
}

impl Footer {
  pub fn new(meta_index_handle: BlockHandle, index_handle: BlockHandle) -> Self {
    Self {
      meta_index_handle,
      index_handle,
    }
  }

  pub fn encode(self) -> [u8; FOOTER_SIZE] {
    let mut buf = [0u8; FOOTER_SIZE];
    buf[..16].copy_from_slice(&self.meta_index_handle.encode());
    buf[16..32].copy_from_slice(&self.index_handle.encode());
    // padding 32..40 = 0
    buf[40..].copy_from_slice(&MAGIC_NUMBER.to_le_bytes());
    buf
  }

  pub fn decode(buf: &[u8]) -> Result<Self> {
    if buf.len() != FOOTER_SIZE {
      return Err(Error::Corruption(format!(
        "footer size {} != {FOOTER_SIZE}",
        buf.len()
      )));
    }
    let magic = u64::from_le_bytes(buf[40..48].try_into().unwrap());
    if magic != MAGIC_NUMBER {
      return Err(Error::Corruption(format!("bad SSTable magic: {magic:#x}")));
    }
    Ok(Self {
      meta_index_handle: BlockHandle::decode(&buf[..16])?,
      index_handle: BlockHandle::decode(&buf[16..32])?,
    })
  }
}
