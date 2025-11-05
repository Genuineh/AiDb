//! WAL writer implementation.

use super::record::{Record, RecordType, MAX_RECORD_SIZE};
use crate::error::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// WAL writer for appending records to the log file
pub struct WALWriter {
    /// Path to the WAL file
    path: PathBuf,
    /// Buffered writer for efficient I/O
    writer: BufWriter<File>,
    /// Current file size
    file_size: u64,
}

impl WALWriter {
    /// Create a new WAL writer
    ///
    /// Opens the WAL file in append mode, creating it if it doesn't exist.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::Io(e))?;

        let file_size = file.metadata().map_err(|e| Error::Io(e))?.len();
        let writer = BufWriter::new(file);

        Ok(Self {
            path,
            writer,
            file_size,
        })
    }

    /// Append a record to the WAL
    ///
    /// Large records are automatically split into multiple fragments.
    pub fn append(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // Split large data into chunks
        let mut offset = 0;
        let data_len = data.len();

        while offset < data_len {
            let remaining = data_len - offset;
            let chunk_size = remaining.min(MAX_RECORD_SIZE);
            let chunk = &data[offset..offset + chunk_size];

            // Determine record type
            let record_type = if data_len <= MAX_RECORD_SIZE {
                // Single record
                RecordType::Full
            } else if offset == 0 {
                // First fragment
                RecordType::First
            } else if offset + chunk_size >= data_len {
                // Last fragment
                RecordType::Last
            } else {
                // Middle fragment
                RecordType::Middle
            };

            // Create and encode record
            let record = Record::new(record_type, chunk.to_vec());
            let encoded = record.encode();

            // Write to file
            self.writer
                .write_all(&encoded)
                .map_err(|e| Error::Io(e))?;

            self.file_size += encoded.len() as u64;
            offset += chunk_size;
        }

        Ok(())
    }

    /// Sync the WAL to disk
    ///
    /// Ensures all buffered data is written and fsync'd to persistent storage.
    pub fn sync(&mut self) -> Result<()> {
        self.writer.flush().map_err(|e| Error::Io(e))?;
        self.writer
            .get_ref()
            .sync_all()
            .map_err(|e| Error::Io(e))?;
        Ok(())
    }

    /// Get the current file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Get the path to the WAL file
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Close the writer, flushing all data
    pub fn close(mut self) -> Result<()> {
        self.sync()
    }
}

impl Drop for WALWriter {
    fn drop(&mut self) {
        // Best effort flush on drop
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_writer_create() {
        let temp_file = NamedTempFile::new().unwrap();
        let writer = WALWriter::new(temp_file.path());
        assert!(writer.is_ok());
    }

    #[test]
    fn test_append_small_record() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut writer = WALWriter::new(temp_file.path()).unwrap();

        let data = b"hello world";
        writer.append(data).unwrap();
        writer.sync().unwrap();

        assert!(writer.file_size() > 0);
    }

    #[test]
    fn test_append_large_record() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut writer = WALWriter::new(temp_file.path()).unwrap();

        // Create data larger than MAX_RECORD_SIZE
        let data = vec![0xAB; MAX_RECORD_SIZE * 2 + 100];
        writer.append(&data).unwrap();
        writer.sync().unwrap();

        // Should have written multiple fragments
        assert!(writer.file_size() > data.len() as u64);
    }

    #[test]
    fn test_multiple_appends() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut writer = WALWriter::new(temp_file.path()).unwrap();

        for i in 0..10 {
            let data = format!("record {}", i);
            writer.append(data.as_bytes()).unwrap();
        }

        writer.sync().unwrap();
        assert!(writer.file_size() > 0);
    }

    #[test]
    fn test_empty_append() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut writer = WALWriter::new(temp_file.path()).unwrap();

        writer.append(&[]).unwrap();
        assert_eq!(writer.file_size(), 0);
    }

    #[test]
    fn test_writer_reopen() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        {
            let mut writer = WALWriter::new(&path).unwrap();
            writer.append(b"first write").unwrap();
            writer.sync().unwrap();
        }

        // Reopen and append more
        let mut writer = WALWriter::new(&path).unwrap();
        let initial_size = writer.file_size();
        writer.append(b"second write").unwrap();
        writer.sync().unwrap();

        assert!(writer.file_size() > initial_size);
    }
}
