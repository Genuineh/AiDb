//! SSTable footer implementation.
//!
//! The footer is a fixed-size (48 bytes) structure at the end of an SSTable file
//! that contains pointers to the index block and meta index block.

use crate::error::{Error, Result};
use crate::sstable::MAGIC_NUMBER;
use std::io::{Read, Write};

/// BlockHandle represents a pointer to a block in the SSTable file.
///
/// It contains the offset and size of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockHandle {
    /// Offset of the block in the file
    pub offset: u64,
    /// Size of the block in bytes
    pub size: u64,
}

impl BlockHandle {
    /// Create a new BlockHandle
    pub fn new(offset: u64, size: u64) -> Self {
        Self { offset, size }
    }

    /// Encode the BlockHandle to bytes (16 bytes: 8 for offset + 8 for size)
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&self.offset.to_le_bytes());
        buf.extend_from_slice(&self.size.to_le_bytes());
        buf
    }

    /// Decode a BlockHandle from bytes
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 16 {
            return Err(Error::corruption("BlockHandle too short"));
        }

        let offset = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let size = u64::from_le_bytes(data[8..16].try_into().unwrap());

        Ok(Self { offset, size })
    }

    /// Get the end offset of this block
    pub fn end_offset(&self) -> u64 {
        self.offset + self.size
    }
}

/// Footer is the last 48 bytes of an SSTable file.
///
/// Format:
/// ```text
/// [meta_index_handle: 16 bytes]
/// [index_handle: 16 bytes]
/// [padding: 8 bytes]
/// [magic: 8 bytes]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    /// Handle to the meta index block
    pub meta_index_handle: BlockHandle,
    /// Handle to the index block
    pub index_handle: BlockHandle,
}

impl Footer {
    /// Create a new Footer
    pub fn new(meta_index_handle: BlockHandle, index_handle: BlockHandle) -> Self {
        Self { meta_index_handle, index_handle }
    }

    /// Encode the footer to bytes (48 bytes)
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(48);

        // Meta index handle (16 bytes)
        buf.extend_from_slice(&self.meta_index_handle.encode());

        // Index handle (16 bytes)
        buf.extend_from_slice(&self.index_handle.encode());

        // Padding (8 bytes) - reserved for future use
        buf.extend_from_slice(&[0u8; 8]);

        // Magic number (8 bytes)
        buf.extend_from_slice(&MAGIC_NUMBER.to_le_bytes());

        assert_eq!(buf.len(), 48);
        buf
    }

    /// Decode a footer from bytes
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() != 48 {
            return Err(Error::corruption(format!(
                "Footer size mismatch: expected 48, got {}",
                data.len()
            )));
        }

        // Verify magic number
        let magic = u64::from_le_bytes(data[40..48].try_into().unwrap());
        if magic != MAGIC_NUMBER {
            return Err(Error::corruption(format!(
                "Invalid SSTable magic number: expected {:#x}, got {:#x}",
                MAGIC_NUMBER, magic
            )));
        }

        // Decode handles
        let meta_index_handle = BlockHandle::decode(&data[0..16])?;
        let index_handle = BlockHandle::decode(&data[16..32])?;

        Ok(Self { meta_index_handle, index_handle })
    }

    /// Write the footer to a writer
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<()> {
        let encoded = self.encode();
        writer.write_all(&encoded)?;
        Ok(())
    }

    /// Read the footer from a reader
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; 48];
        reader.read_exact(&mut buf)?;
        Self::decode(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_block_handle_encode_decode() {
        let handle = BlockHandle::new(1234, 5678);
        let encoded = handle.encode();
        assert_eq!(encoded.len(), 16);

        let decoded = BlockHandle::decode(&encoded).unwrap();
        assert_eq!(decoded, handle);
    }

    #[test]
    fn test_block_handle_end_offset() {
        let handle = BlockHandle::new(100, 50);
        assert_eq!(handle.end_offset(), 150);
    }

    #[test]
    fn test_footer_encode_decode() {
        let meta_handle = BlockHandle::new(1000, 100);
        let index_handle = BlockHandle::new(2000, 200);
        let footer = Footer::new(meta_handle, index_handle);

        let encoded = footer.encode();
        assert_eq!(encoded.len(), 48);

        let decoded = Footer::decode(&encoded).unwrap();
        assert_eq!(decoded, footer);
    }

    #[test]
    fn test_footer_magic_number() {
        let footer = Footer::new(BlockHandle::new(0, 0), BlockHandle::new(0, 0));
        let encoded = footer.encode();

        // Verify magic number is at the end
        let magic = u64::from_le_bytes(encoded[40..48].try_into().unwrap());
        assert_eq!(magic, MAGIC_NUMBER);
    }

    #[test]
    fn test_footer_invalid_magic() {
        let mut data = vec![0u8; 48];
        // Write wrong magic number
        data[40..48].copy_from_slice(&0x1234567890abcdefu64.to_le_bytes());

        let result = Footer::decode(&data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Corruption(_)));
    }

    #[test]
    fn test_footer_write_read() {
        let footer = Footer::new(BlockHandle::new(1000, 100), BlockHandle::new(2000, 200));

        let mut buffer = Vec::new();
        footer.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let read_footer = Footer::read_from(&mut cursor).unwrap();

        assert_eq!(read_footer, footer);
    }
}
