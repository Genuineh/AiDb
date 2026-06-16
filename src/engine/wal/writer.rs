//! WAL Writer — 追加写入 Record.
//!
//! 处理 Record 分片、block padding、CRC32 校验、sync。
//! 每个 Record 的磁盘格式:
//! ┌───────────────────────────────────────┐
//! │ CRC32 (4B) │ Length (2B) │ Type (1B) │
//! │ Data (length B)                       │
//! └───────────────────────────────────────┘

use super::record::{RecordType, HEADER_SIZE};
use crate::error::Result;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// WAL 默认 block 大小
pub const BLOCK_SIZE: usize = 32768;

pub struct Writer {
    file: std::fs::File,
    block_offset: usize,
    sync_wal: bool,
}

impl Writer {
    /// 创建 Writer, 打开或创建 WAL 文件
    #[tracing::instrument(name = "wal_writer_open", skip(path))]
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_sync(path, false)
    }

    /// 创建 Writer, 指定是否每次写入后 sync
    #[tracing::instrument(name = "wal_writer_open_with_sync", skip(path))]
    pub fn open_with_sync(path: &Path, sync_wal: bool) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        let block_offset = (file.metadata()?.len() % BLOCK_SIZE as u64) as usize;
        Ok(Writer {
            file,
            block_offset,
            sync_wal,
        })
    }

    /// 单条 Record 的 data 最大长度 (Length 字段为 u16)
    const MAX_RECORD_DATA: usize = 65535;

    /// 追加一条 Record.
    ///
    /// `data` 是已编码的 WalEntry 字节.
    /// 如果 data 长度超过 block 容量或 65535, 自动分片为 First/Middle/Last.
    /// 每片写入前检查 block 剩余空间, 不足时填充 0x00 padding 后切到新 block.
    #[tracing::instrument(skip(self, data))]
    pub fn write_record(&mut self, record_type: RecordType, data: &[u8]) -> Result<()> {
        let max_payload = BLOCK_SIZE - HEADER_SIZE;
        let total = data.len();
        let mut offset = 0;
        let mut fragment_index = 0;

        while offset < total {
            // 当前 block 剩余空间不足以放下最小 HEADER 时, 填充 padding
            if self.block_offset + HEADER_SIZE >= BLOCK_SIZE {
                let remaining = BLOCK_SIZE - self.block_offset;
                if remaining > 0 {
                    let padding = vec![0u8; remaining];
                    self.file.write_all(&padding)?;
                }
                self.block_offset = 0;
            }

            // 当前 block 剩余容量
            let block_avail = BLOCK_SIZE - self.block_offset - HEADER_SIZE;
            // 单片最大长度: 受 block 剩余空间和 u16 Length 字段双重约束
            let max_chunk = max_payload.min(Self::MAX_RECORD_DATA);
            let chunk_size = (total - offset).min(max_chunk).min(block_avail);

            let end = offset + chunk_size;
            let chunk = &data[offset..end];
            offset = end;

            // 确定 RecordType: 根据分片位置
            let is_first_fragment = fragment_index == 0;
            let is_last_fragment = offset >= total;

            let frag_type = if is_first_fragment && is_last_fragment {
                record_type // 整个数据一次性写入, 使用调用方指定的类型
            } else if is_first_fragment {
                RecordType::First
            } else if is_last_fragment {
                RecordType::Last
            } else {
                RecordType::Middle
            };

            self.write_one_record(frag_type, chunk)?;
            fragment_index += 1;
        }

        Ok(())
    }

    /// 写一条完整的单块 Record (自动处理 block padding).
    /// data 长度必须 ≤ BLOCK_SIZE - HEADER_SIZE.
    fn write_one_record(&mut self, record_type: RecordType, data: &[u8]) -> Result<()> {
        tracing::debug!(target: "wal", "wal.write.start: record_type={:?} record_size={}", record_type, data.len() + HEADER_SIZE);
        let data_len = data.len();
        let needed = HEADER_SIZE + data_len;

        // 数据必须能放进一个 block
        assert!(
            needed <= BLOCK_SIZE,
            "record {} bytes > block size {}",
            needed,
            BLOCK_SIZE
        );

        // 当前 block 剩余空间不足时, 填充 padding 并重置 block_offset
        if BLOCK_SIZE - self.block_offset < needed {
            let remaining = BLOCK_SIZE - self.block_offset;
            if remaining > 0 {
                let padding = vec![0u8; remaining];
                self.file.write_all(&padding)?;
            }
            self.block_offset = 0;
        }

        // 计算 CRC32: 覆盖 Length(2B) + Type(1B) + Data
        let crc = crc32fast::hash(
            &[
                &(data_len as u16).to_le_bytes()[..],
                &[record_type as u8],
                data,
            ]
            .concat(),
        );

        self.file.write_all(&crc.to_le_bytes())?; // 4B CRC32
        self.file.write_all(&(data_len as u16).to_le_bytes())?; // 2B Length
        self.file.write_all(&[record_type as u8])?; // 1B Type
        self.file.write_all(data)?; // Data

        self.block_offset += HEADER_SIZE + data_len;

        if self.sync_wal {
            self.file.sync_all()?;
        }

        tracing::debug!(
          target: "wal",
          "wal.write.complete: record_type={:?} record_size={}",
          record_type,
          data_len + HEADER_SIZE,
        );

        Ok(())
    }

    /// Flush 文件缓冲区
    #[tracing::instrument(skip(self))]
    pub fn flush(&mut self) -> Result<()> {
        tracing::debug!(target: "wal", "wal.flush.start");
        self.file.flush()?;
        tracing::debug!(target: "wal", "wal.flush.complete");
        Ok(())
    }

    /// fsync
    #[tracing::instrument(skip(self))]
    pub fn sync_all(&mut self) -> Result<()> {
        tracing::debug!(target: "wal", "wal.sync.start");
        self.file.sync_all()?;
        tracing::debug!(target: "wal", "wal.sync.complete");
        Ok(())
    }

    /// 当前 block 偏移
    pub fn block_offset(&self) -> usize {
        self.block_offset
    }

    /// 获取文件大小
    pub fn file_size(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// 获取底层文件句柄 (用于 Reader)
    pub fn file(&self) -> &std::fs::File {
        &self.file
    }

    /// 重建 Reader 用
    pub fn try_clone_file(&self) -> Result<std::fs::File> {
        // 需要读权限, 以 read-write 方式打开
        // 但这里我们只是尝试副本
        Ok(self.file.try_clone()?)
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Seek for Writer {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = self.file.seek(pos)?;
        // 重新计算 block_offset (仅在确定性 seek 后有效)
        self.block_offset = (new_pos as usize) % BLOCK_SIZE;
        Ok(new_pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::wal::record::RecordType;
    use tempfile::tempdir;

    #[test]
    fn test_writer_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal_test.log");
        let mut writer = Writer::open(&path).unwrap();
        assert!(path.exists());
        writer.write_record(RecordType::Full, b"hello").unwrap();
        writer.sync_all().unwrap();
        assert!(writer.file_size().unwrap() > 0);
    }

    #[test]
    fn test_writer_write_and_file_size() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_write.log");
        let mut writer = Writer::open(&path).unwrap();
        // 一个 Full record: 7B header + 5B data = 12 bytes
        writer.write_record(RecordType::Full, b"hello").unwrap();
        assert_eq!(writer.file_size().unwrap(), (HEADER_SIZE + 5) as u64);
    }

    #[test]
    fn test_writer_block_padding() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_padding.log");
        let mut writer = Writer::open(&path).unwrap();
        // 填满当前 block
        let data = vec![0u8; BLOCK_SIZE - HEADER_SIZE]; // 刚好塞满一个 block
        writer.write_record(RecordType::Full, &data).unwrap();
        // block 已满, block_offset 应指向 block 末尾
        assert!(writer.block_offset >= BLOCK_SIZE);
        // 再写一条, 应产生 padding (32768 - 32768 = 0, 即填充一个完整的 block trailer)
        writer.write_record(RecordType::Full, b"x").unwrap();
        // 第二条记录的 HEADER + 1 byte data 在新 block 中
        // 文件大小 = 32768 (第一个block, 含 padding) + HEADER_SIZE + 1
        assert_eq!(
            writer.file_size().unwrap(),
            (BLOCK_SIZE + HEADER_SIZE + 1) as u64
        );
    }
}
