//! SSTable 读取: Footer → Index → Data Block.

use std::cmp::Ordering;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;

use crate::engine::cache::BlockCache;
use crate::engine::filter::{bloom::record_bloom_false_positive, BloomFilter, Filter};
use crate::engine::memtable::{extract_sequence, extract_user_key, extract_value_type, ValueType};
use crate::error::{Error, Result};

use super::block::Block;
use super::block_io::{read_block_cached, read_block_from_file};
use super::footer::{Footer, FOOTER_SIZE};
use super::handle::BlockHandle;
use super::index::{find_block_handle, load_index_entries, IndexBlock};
use super::iterator::SSTableIterator;
use super::meta::{find_meta_block_handle, read_raw_bytes, BLOOM_META_NAME};

pub struct SSTableReader {
  file: Arc<File>,
  file_number: u64,
  level: usize,
  index_entries: Vec<(Vec<u8>, BlockHandle)>,
  file_size: u64,
  smallest_key: Vec<u8>,
  largest_key: Vec<u8>,
  bloom_filter: Option<BloomFilter>,
  block_cache: Option<Arc<BlockCache>>,
}

impl SSTableReader {
  pub fn open(path: &Path, block_cache: Option<Arc<BlockCache>>) -> Result<Self> {
    let file = Arc::new(File::open(path)?);
    let file_size = file.metadata()?.len();
    if file_size < FOOTER_SIZE as u64 {
      return Err(Error::Corruption("SSTable file too small".into()));
    }

    let (file_number, level) = path
      .file_name()
      .and_then(|n| n.to_str())
      .and_then(super::filename::parse_sstable_filename)
      .unwrap_or((0, 0));

    let footer = read_footer(&file, file_size)?;
    let index_bytes = read_block_from_file(&file, &footer.index_handle)?;
    let index_block = Block::new(Bytes::from(index_bytes))?;
    let index_entries = load_index_entries(&index_block)?;

    let meta_index_bytes = read_block_from_file(&file, &footer.meta_index_handle)?;
    let meta_index = IndexBlock::new(Bytes::from(meta_index_bytes))?;
    let bloom_filter = load_bloom_filter(&file, &meta_index);

    let (smallest_key, largest_key) =
      read_key_range(&file, file_number, &index_entries, block_cache.as_ref())?;

    Ok(Self {
      file,
      file_number,
      level,
      index_entries,
      file_size,
      smallest_key,
      largest_key,
      bloom_filter,
      block_cache,
    })
  }

  pub fn file_number(&self) -> u64 {
    self.file_number
  }

  pub fn level(&self) -> usize {
    self.level
  }

  pub fn file_size(&self) -> u64 {
    self.file_size
  }

  pub fn smallest_key(&self) -> &[u8] {
    &self.smallest_key
  }

  pub fn largest_key(&self) -> &[u8] {
    &self.largest_key
  }

  pub fn has_bloom_filter(&self) -> bool {
    self.bloom_filter.is_some()
  }

  #[tracing::instrument(
    name = "sst_seek",
    skip(self, seek_key),
    fields(file_number = self.file_number, level = self.level)
  )]
  pub fn get(&self, seek_key: &[u8]) -> Result<Option<(Vec<u8>, ValueType)>> {
    let target_user = extract_user_key(seek_key);

    let bloom_passed = if let Some(ref filter) = self.bloom_filter {
      let hit = filter.may_contain(target_user);
      tracing::debug!(
        target: "sst",
        file_number = self.file_number,
        hit,
        "bloom_check"
      );
      let _span =
        tracing::trace_span!("bloom_check", file_number = self.file_number, hit).entered();
      if !hit {
        tracing::debug!(target: "sst", found = false, "sst.seek.result");
        return Ok(None);
      }
      true
    } else {
      false
    };

    let handle = find_block_handle(&self.index_entries, seek_key)?;
    let block_data = read_block_cached(
      &self.file,
      self.file_number,
      &handle,
      self.block_cache.as_ref(),
    )?;
    let block = Block::new(block_data)?;
    let mut it = block.iter();
    let seek_seq = extract_sequence(seek_key)?;

    while it.valid() {
      let key = it.key();
      match extract_user_key(key).cmp(target_user) {
        Ordering::Greater => break,
        Ordering::Less => {
          if !it.advance() {
            break;
          }
          continue;
        }
        Ordering::Equal => {
          let seq = extract_sequence(key)?;
          if seq <= seek_seq {
            let ty = extract_value_type(key)?;
            let val = it.value().to_vec();
            tracing::debug!(target: "sst", found = true, "sst.seek.result");
            return Ok(Some((val, ty)));
          }
        }
      }
      if !it.advance() {
        break;
      }
    }
    tracing::debug!(target: "sst", found = false, "sst.seek.result");
    if bloom_passed {
      record_bloom_false_positive();
    }
    Ok(None)
  }

  pub fn iter(&self) -> SSTableIterator {
    SSTableIterator::new(
      Arc::clone(&self.file),
      self.file_number,
      self.index_entries.clone(),
      self.block_cache.clone(),
    )
  }
}

fn load_bloom_filter(file: &File, meta_index: &IndexBlock) -> Option<BloomFilter> {
  let handle = match find_meta_block_handle(meta_index, BLOOM_META_NAME) {
    Ok(Some(h)) => h,
    Ok(None) => return None,
    Err(e) => {
      tracing::warn!(error = %e, "bloom meta index read failed, degraded to no-filter");
      return None;
    }
  };
  let raw = match read_raw_bytes(file, handle.offset, handle.size) {
    Ok(b) => b,
    Err(e) => {
      tracing::warn!(error = %e, "bloom meta block read failed, degraded to no-filter");
      return None;
    }
  };
  match BloomFilter::decode(&raw) {
    Ok(f) => Some(f),
    Err(e) => {
      tracing::warn!(error = %e, "bloom decode failed, degraded to no-filter");
      None
    }
  }
}

fn read_footer(file: &File, file_size: u64) -> Result<Footer> {
  let mut buf = [0u8; FOOTER_SIZE];
  let mut f = file.try_clone()?;
  f.seek(SeekFrom::Start(file_size - FOOTER_SIZE as u64))?;
  f.read_exact(&mut buf)?;
  Footer::decode(&buf)
}

fn read_key_range(
  file: &File,
  file_number: u64,
  entries: &[(Vec<u8>, BlockHandle)],
  block_cache: Option<&Arc<BlockCache>>,
) -> Result<(Vec<u8>, Vec<u8>)> {
  if entries.is_empty() {
    return Ok((Vec::new(), Vec::new()));
  }
  let first = read_block_cached(file, file_number, &entries[0].1, block_cache)?;
  let first_it = Block::new(first)?.iter();
  let smallest = if first_it.valid() {
    first_it.key().to_vec()
  } else {
    Vec::new()
  };

  let last = read_block_cached(
    file,
    file_number,
    &entries[entries.len() - 1].1,
    block_cache,
  )?;
  let block = Block::new(last)?;
  let mut it = block.iter();
  let mut largest = Vec::new();
  while it.valid() {
    largest = it.key().to_vec();
    if !it.advance() {
      break;
    }
  }
  Ok((smallest, largest))
}
