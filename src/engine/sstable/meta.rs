//! Meta Block 辅助: Bloom 等裸字节块.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::error::Result;

use super::handle::BlockHandle;
use super::index::{IndexBlock, IndexEntry};

pub const BLOOM_META_NAME: &[u8] = b"bloom";
pub const PROPERTIES_META_NAME: &[u8] = b"properties";

pub fn find_meta_block_handle(meta_index: &IndexBlock, name: &[u8]) -> Result<Option<BlockHandle>> {
    for entry in meta_index.entries()? {
        if entry.key == name {
            return Ok(Some(entry.handle));
        }
    }
    Ok(None)
}

pub fn read_raw_bytes(file: &File, offset: u64, size: u64) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; size as usize];
    let mut f = file.try_clone()?;
    f.seek(SeekFrom::Start(offset))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn write_raw_block(
    writer: &mut impl std::io::Write,
    data_block_offset: &mut u64,
    data: &[u8],
) -> Result<BlockHandle> {
    let offset = *data_block_offset;
    writer.write_all(data)?;
    let handle = BlockHandle {
        offset,
        size: data.len() as u64,
    };
    *data_block_offset += handle.size;
    Ok(handle)
}

pub fn index_entry_for_bloom(handle: BlockHandle) -> IndexEntry {
    IndexEntry {
        key: BLOOM_META_NAME.to_vec(),
        handle,
    }
}
