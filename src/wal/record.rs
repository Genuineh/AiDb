//! WAL record format implementation.
//!
//! Each record consists of:
//! - Checksum (4 bytes): CRC32 of type and data
//! - Length (2 bytes): Length of the data
//! - Type (1 byte): Record type (Full, First, Middle, Last)
//! - Data (variable): Actual user data

use crate::error::{Error, Result};
use bytes::{Buf, BufMut, BytesMut};
use crc32fast::Hasher;

/// Maximum size of a single record's data portion
pub const MAX_RECORD_SIZE: usize = 32 * 1024; // 32KB

/// Size of the record header (checksum + length + type)
pub const HEADER_SIZE: usize = 7;

/// Record types for handling large entries that span multiple blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordType {
    /// Complete record contained in a single block
    Full = 1,
    /// First fragment of a multi-block record
    First = 2,
    /// Middle fragment of a multi-block record
    Middle = 3,
    /// Last fragment of a multi-block record
    Last = 4,
}

impl RecordType {
    /// Convert from u8 to RecordType
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            1 => Ok(RecordType::Full),
            2 => Ok(RecordType::First),
            3 => Ok(RecordType::Middle),
            4 => Ok(RecordType::Last),
            _ => Err(Error::Corruption(format!("Invalid record type: {}", value))),
        }
    }
}

/// A WAL record
#[derive(Debug, Clone)]
pub struct Record {
    /// Type of the record
    pub record_type: RecordType,
    /// Data payload
    pub data: Vec<u8>,
}

impl Record {
    /// Create a new record
    pub fn new(record_type: RecordType, data: Vec<u8>) -> Self {
        Self { record_type, data }
    }

    /// Encode the record into bytes
    ///
    /// Format: [checksum: u32][length: u16][type: u8][data: bytes]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE + self.data.len());

        // Reserve space for checksum (will be filled later)
        buf.put_u32_le(0);

        // Write length
        buf.put_u16_le(self.data.len() as u16);

        // Write type
        buf.put_u8(self.record_type as u8);

        // Write data
        buf.put_slice(&self.data);

        // Calculate and write checksum
        let checksum = Self::calculate_checksum(self.record_type, &self.data);
        let mut result = buf.to_vec();
        result[0..4].copy_from_slice(&checksum.to_le_bytes());

        result
    }

    /// Decode a record from bytes
    pub fn decode(mut data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Corruption(format!("Record too short: {} bytes", data.len())));
        }

        // Read checksum
        let checksum = data.get_u32_le();

        // Read length
        let length = data.get_u16_le() as usize;

        // Read type
        let record_type = RecordType::from_u8(data.get_u8())?;

        // Validate remaining data length
        if data.len() < length {
            return Err(Error::Corruption(format!(
                "Incomplete record: expected {} bytes, got {}",
                length,
                data.len()
            )));
        }

        // Read data
        let record_data = data[..length].to_vec();

        // Verify checksum
        let expected_checksum = Self::calculate_checksum(record_type, &record_data);
        if checksum != expected_checksum {
            return Err(Error::Corruption(format!(
                "Checksum mismatch: expected {:#x}, got {:#x}",
                expected_checksum, checksum
            )));
        }

        Ok(Record { record_type, data: record_data })
    }

    /// Calculate CRC32 checksum for record type and data
    fn calculate_checksum(record_type: RecordType, data: &[u8]) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(&[record_type as u8]);
        hasher.update(data);
        hasher.finalize()
    }

    /// Get the total size of the encoded record
    pub fn encoded_size(&self) -> usize {
        HEADER_SIZE + self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_encode_decode() {
        let data = b"hello world".to_vec();
        let record = Record::new(RecordType::Full, data.clone());

        let encoded = record.encode();
        let decoded = Record::decode(&encoded).unwrap();

        assert_eq!(decoded.record_type, RecordType::Full);
        assert_eq!(decoded.data, data);
    }

    #[test]
    fn test_record_types() {
        let types = vec![RecordType::Full, RecordType::First, RecordType::Middle, RecordType::Last];

        for record_type in types {
            let data = b"test".to_vec();
            let record = Record::new(record_type, data);
            let encoded = record.encode();
            let decoded = Record::decode(&encoded).unwrap();
            assert_eq!(decoded.record_type, record_type);
        }
    }

    #[test]
    fn test_checksum_validation() {
        let record = Record::new(RecordType::Full, b"test data".to_vec());
        let mut encoded = record.encode();

        // Corrupt the data
        encoded[HEADER_SIZE] ^= 0xFF;

        let result = Record::decode(&encoded);
        assert!(result.is_err());
        match result {
            Err(Error::Corruption(_)) => {}
            _ => panic!("Expected corruption error"),
        }
    }

    #[test]
    fn test_empty_data() {
        let record = Record::new(RecordType::Full, vec![]);
        let encoded = record.encode();
        let decoded = Record::decode(&encoded).unwrap();
        assert_eq!(decoded.data.len(), 0);
    }

    #[test]
    fn test_large_data() {
        let data = vec![0xAB; MAX_RECORD_SIZE];
        let record = Record::new(RecordType::Full, data.clone());
        let encoded = record.encode();
        let decoded = Record::decode(&encoded).unwrap();
        assert_eq!(decoded.data, data);
    }

    #[test]
    fn test_record_size() {
        let data = b"test".to_vec();
        let record = Record::new(RecordType::Full, data);
        assert_eq!(record.encoded_size(), HEADER_SIZE + 4);
    }
}
