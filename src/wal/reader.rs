//! WAL reader implementation for recovery.

use super::record::{Record, RecordType, HEADER_SIZE};
use crate::error::{Error, Result};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// WAL reader for reading and recovering from log files
pub struct WALReader {
    /// Buffered reader for efficient I/O
    reader: BufReader<File>,
    /// Current read position
    position: u64,
}

impl WALReader {
    /// Open a WAL file for reading
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path).map_err(Error::Io)?;
        let reader = BufReader::new(file);

        Ok(Self { reader, position: 0 })
    }

    /// Read the next complete entry from the WAL
    ///
    /// Returns None if EOF is reached.
    /// Handles fragmented records by reassembling them.
    pub fn read_next(&mut self) -> Result<Option<Vec<u8>>> {
        let mut assembled_data = Vec::new();
        let mut expecting_continuation = false;

        loop {
            // Try to read the header
            let record = match self.read_record() {
                Ok(Some(r)) => r,
                Ok(None) => {
                    // EOF reached
                    if expecting_continuation {
                        return Err(Error::Corruption(
                            "EOF while expecting continuation record".to_string(),
                        ));
                    }
                    return Ok(None);
                }
                Err(e) => return Err(e),
            };

            match record.record_type {
                RecordType::Full => {
                    if expecting_continuation {
                        return Err(Error::Corruption(
                            "Unexpected Full record while expecting continuation".to_string(),
                        ));
                    }
                    return Ok(Some(record.data));
                }
                RecordType::First => {
                    if expecting_continuation {
                        return Err(Error::Corruption(
                            "Unexpected First record while expecting continuation".to_string(),
                        ));
                    }
                    assembled_data = record.data;
                    expecting_continuation = true;
                }
                RecordType::Middle => {
                    if !expecting_continuation {
                        return Err(Error::Corruption(
                            "Unexpected Middle record without First".to_string(),
                        ));
                    }
                    assembled_data.extend_from_slice(&record.data);
                }
                RecordType::Last => {
                    if !expecting_continuation {
                        return Err(Error::Corruption(
                            "Unexpected Last record without First".to_string(),
                        ));
                    }
                    assembled_data.extend_from_slice(&record.data);
                    return Ok(Some(assembled_data));
                }
            }
        }
    }

    /// Read a single record from the WAL
    fn read_record(&mut self) -> Result<Option<Record>> {
        // Read header
        let mut header = [0u8; HEADER_SIZE];
        match self.reader.read_exact(&mut header) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => return Err(Error::Io(e)),
        }

        // Parse length from header
        let length = u16::from_le_bytes([header[4], header[5]]) as usize;

        // Read complete record (header + data)
        let total_size = HEADER_SIZE + length;
        let mut buffer = vec![0u8; total_size];
        buffer[..HEADER_SIZE].copy_from_slice(&header);

        if length > 0 {
            self.reader.read_exact(&mut buffer[HEADER_SIZE..]).map_err(Error::Io)?;
        }

        self.position += total_size as u64;

        // Decode record
        Record::decode(&buffer).map(Some)
    }

    /// Get the current read position
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Seek to a specific position in the WAL
    pub fn seek(&mut self, pos: u64) -> Result<()> {
        self.reader.seek(SeekFrom::Start(pos)).map_err(Error::Io)?;
        self.position = pos;
        Ok(())
    }

    /// Recover all entries from the WAL
    ///
    /// Returns a vector of all valid entries. Stops on first corruption.
    pub fn recover_all(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut entries = Vec::new();

        loop {
            match self.read_next() {
                Ok(Some(data)) => entries.push(data),
                Ok(None) => break, // EOF
                Err(Error::Corruption(msg)) => {
                    // Log corruption but continue recovery up to this point
                    log::warn!("WAL corruption at position {}: {}", self.position, msg);
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::writer::WALWriter;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_single_record() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write a record
        {
            let mut writer = WALWriter::new(path).unwrap();
            writer.append(b"hello world").unwrap();
            writer.sync().unwrap();
        }

        // Read it back
        let mut reader = WALReader::new(path).unwrap();
        let data = reader.read_next().unwrap();
        assert_eq!(data, Some(b"hello world".to_vec()));

        // EOF
        assert_eq!(reader.read_next().unwrap(), None);
    }

    #[test]
    fn test_read_multiple_records() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let test_data = vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()];

        // Write records
        {
            let mut writer = WALWriter::new(path).unwrap();
            for data in &test_data {
                writer.append(data).unwrap();
            }
            writer.sync().unwrap();
        }

        // Read them back
        let mut reader = WALReader::new(path).unwrap();
        for expected in &test_data {
            let data = reader.read_next().unwrap();
            assert_eq!(data, Some(expected.clone()));
        }

        assert_eq!(reader.read_next().unwrap(), None);
    }

    #[test]
    fn test_read_large_record() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create large data (will be fragmented)
        let large_data = vec![0xAB; 100_000];

        // Write it
        {
            let mut writer = WALWriter::new(path).unwrap();
            writer.append(&large_data).unwrap();
            writer.sync().unwrap();
        }

        // Read it back
        let mut reader = WALReader::new(path).unwrap();
        let data = reader.read_next().unwrap();
        assert_eq!(data, Some(large_data));
    }

    #[test]
    fn test_recover_all() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let test_data = vec![b"entry1".to_vec(), b"entry2".to_vec(), b"entry3".to_vec()];

        // Write records
        {
            let mut writer = WALWriter::new(path).unwrap();
            for data in &test_data {
                writer.append(data).unwrap();
            }
            writer.sync().unwrap();
        }

        // Recover all
        let mut reader = WALReader::new(path).unwrap();
        let recovered = reader.recover_all().unwrap();
        assert_eq!(recovered, test_data);
    }

    #[test]
    fn test_empty_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut reader = WALReader::new(temp_file.path()).unwrap();
        assert_eq!(reader.read_next().unwrap(), None);
    }

    #[test]
    fn test_position_tracking() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = WALWriter::new(path).unwrap();
            writer.append(b"data1").unwrap();
            writer.append(b"data2").unwrap();
            writer.sync().unwrap();
        }

        let mut reader = WALReader::new(path).unwrap();
        assert_eq!(reader.position(), 0);

        reader.read_next().unwrap();
        let pos1 = reader.position();
        assert!(pos1 > 0);

        reader.read_next().unwrap();
        let pos2 = reader.position();
        assert!(pos2 > pos1);
    }
}
