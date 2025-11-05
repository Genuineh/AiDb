//! Write-Ahead Log (WAL) implementation.
//!
//! The WAL ensures durability by persisting all writes before they are applied
//! to the MemTable. This allows recovery from crashes by replaying the log.
//!
//! ## Architecture
//!
//! - **Record Format**: Each entry is encoded as a record with CRC32 checksum
//! - **Fragmentation**: Large entries are split into multiple records
//! - **Recovery**: On startup, the WAL is replayed to restore the MemTable
//!
//! ## Usage
//!
//! ```rust,no_run
//! use aidb::wal::{WALWriter, WALReader};
//!
//! # fn main() -> Result<(), aidb::Error> {
//! // Writing to WAL
//! let mut writer = WALWriter::new("data.wal")?;
//! writer.append(b"key1:value1")?;
//! writer.append(b"key2:value2")?;
//! writer.sync()?;
//!
//! // Reading from WAL
//! let mut reader = WALReader::new("data.wal")?;
//! while let Some(entry) = reader.read_next()? {
//!     println!("Recovered: {:?}", entry);
//! }
//! # Ok(())
//! # }
//! ```

pub mod reader;
pub mod record;
pub mod writer;

pub use reader::WALReader;
pub use record::{Record, RecordType};
pub use writer::WALWriter;

use crate::error::Result;
use std::path::Path;

/// WAL manager that coordinates reading and writing
pub struct WAL {
    writer: WALWriter,
}

impl WAL {
    /// Open or create a WAL file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let writer = WALWriter::new(path)?;
        Ok(Self { writer })
    }

    /// Append an entry to the WAL
    pub fn append(&mut self, data: &[u8]) -> Result<()> {
        self.writer.append(data)
    }

    /// Sync the WAL to disk
    pub fn sync(&mut self) -> Result<()> {
        self.writer.sync()
    }

    /// Get the current file size
    pub fn size(&self) -> u64 {
        self.writer.file_size()
    }

    /// Get the path to the WAL file
    pub fn path(&self) -> &Path {
        self.writer.path()
    }

    /// Close the WAL
    pub fn close(self) -> Result<()> {
        self.writer.close()
    }

    /// Recover entries from a WAL file
    pub fn recover<P: AsRef<Path>>(path: P) -> Result<Vec<Vec<u8>>> {
        let mut reader = WALReader::new(path)?;
        reader.recover_all()
    }
}

/// Generate a WAL filename for a given sequence number
pub fn wal_filename(seq: u64) -> String {
    format!("{:06}.log", seq)
}

/// Parse a WAL filename to extract the sequence number
pub fn parse_wal_filename(filename: &str) -> Option<u64> {
    if !filename.ends_with(".log") {
        return None;
    }

    let name = filename.trim_end_matches(".log");
    name.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_wal_write_and_recover() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let test_data = vec![b"entry1".to_vec(), b"entry2".to_vec(), b"entry3".to_vec()];

        // Write entries
        {
            let mut wal = WAL::open(path).unwrap();
            for data in &test_data {
                wal.append(data).unwrap();
            }
            wal.sync().unwrap();
        }

        // Recover entries
        let recovered = WAL::recover(path).unwrap();
        assert_eq!(recovered, test_data);
    }

    #[test]
    fn test_wal_filename() {
        assert_eq!(wal_filename(1), "000001.log");
        assert_eq!(wal_filename(123), "000123.log");
        assert_eq!(wal_filename(999999), "999999.log");
    }

    #[test]
    fn test_parse_wal_filename() {
        assert_eq!(parse_wal_filename("000001.log"), Some(1));
        assert_eq!(parse_wal_filename("000123.log"), Some(123));
        assert_eq!(parse_wal_filename("999999.log"), Some(999999));
        assert_eq!(parse_wal_filename("invalid"), None);
        assert_eq!(parse_wal_filename("123.txt"), None);
    }

    #[test]
    fn test_wal_multiple_operations() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut wal = WAL::open(path).unwrap();
            assert_eq!(wal.size(), 0);

            wal.append(b"first").unwrap();
            assert!(wal.size() > 0);

            let size_after_first = wal.size();

            wal.append(b"second").unwrap();
            assert!(wal.size() > size_after_first);

            wal.sync().unwrap();
        }

        let recovered = WAL::recover(path).unwrap();
        assert_eq!(recovered.len(), 2);
    }

    #[test]
    fn test_wal_empty_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut wal = WAL::open(path).unwrap();
            wal.append(&[]).unwrap();
            wal.sync().unwrap();
        }

        let recovered = WAL::recover(path).unwrap();
        assert_eq!(recovered.len(), 0);
    }
}
