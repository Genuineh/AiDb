//! LSM DB 引擎总协调器 (Phase5 + Phase6 Compaction).

use super::iterator::DbIterGuard;
use super::numbers::scan_next_wal_file_number;
use super::replay::replay_entries;
use super::snapshot::{Snapshot, SnapshotList};
use super::write_batch::{WriteBatch, WriteOp};
use crate::config::Options;
use crate::engine::cache::{BlockCache, CacheStats};
use crate::engine::compaction::{
  current_exists, load_sstables_from_version, remove_orphan_sstables, scan_version_edits_from_dir,
  CompactionJob, CompactionPicker, CompactionTask, VersionEdit, VersionSet,
};
use crate::engine::memtable::{
  encode_internal_key, extract_sequence, ImmutableMemTable, MemTable, ValueType, SEQUENCE_LIMIT,
};
use crate::engine::sstable::{sstable_path, SSTableBuilder, SSTableReader};
use crate::engine::wal::manager::WALManager;
use crate::engine::wal::record::{OpType, WalEntry};
use crate::error::{Error, Result};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

#[expect(dead_code)]
pub const DEFAULT_MAX_LEVELS: usize = 7;
/// LSM-Tree 存储引擎.
pub struct DB {
  path: PathBuf,
  options: Arc<Options>,
  memtable: RwLock<MemTable>,
  immutable_memtables: RwLock<Vec<ImmutableMemTable>>,
  wal: RwLock<WALManager>,
  sstables: RwLock<Vec<Vec<Arc<SSTableReader>>>>,
  version_set: RwLock<VersionSet>,
  compaction_picker: Arc<CompactionPicker>,
  sequence: AtomicU64,
  write_lock: Mutex<()>,
  flush_lock: Mutex<()>,
  flush_shutdown: Arc<AtomicBool>,
  flush_handle: Mutex<Option<JoinHandle<()>>>,
  compaction_shutdown: Arc<AtomicBool>,
  compaction_signals: Vec<Sender<()>>,
  compaction_handles: Mutex<Vec<JoinHandle<()>>>,
  compacting: Mutex<HashSet<u64>>,
  closed: AtomicBool,
  checkpoint_in_progress: AtomicBool,
  total_key_count: AtomicUsize,
  block_cache: Arc<BlockCache>,
  pub(crate) snapshots: Arc<SnapshotList>,
}

impl DB {
  #[tracing::instrument(name = "db_open", skip(path, options), fields(path = %path.as_ref().display()))]
  pub fn open(path: impl AsRef<Path>, options: Options) -> Result<Arc<Self>> {
    options.validate()?;
    let path = path.as_ref().to_path_buf();
    let options = Arc::new(options);

    let exists = path.exists();
    if !exists {
      if options.create_if_missing {
        std::fs::create_dir_all(&path)?;
      } else {
        return Err(Error::NotFound);
      }
    } else if options.error_if_exists {
      return Err(Error::InvalidArgument(
        "database directory already exists".into(),
      ));
    }

    let next_wal = scan_next_wal_file_number(&path);
    let max_levels = options.max_levels;
    let block_cache = Arc::new(BlockCache::new(options.block_cache_size));
    let cache_for_open = Some(Arc::clone(&block_cache));

    let recovery = WALManager::recover(&path, Arc::clone(&options))?;
    let memtable = MemTable::new();
    replay_entries(&memtable, &recovery.entries)?;
    let mut last_sequence = recovery.max_sequence;
    last_sequence = last_sequence.max(max_sequence_in_memtable(&memtable));

    let version_set = if current_exists(&path) {
      VersionSet::recover(&path, max_levels, options.max_manifest_size)?
    } else {
      let edits = scan_version_edits_from_dir(&path, max_levels, cache_for_open.clone())?;
      if edits.is_empty() {
        VersionSet::open_new(&path, max_levels, options.max_manifest_size)?
      } else {
        VersionSet::bootstrap_from_scan(&path, max_levels, options.max_manifest_size, edits)?
      }
    };
    let sstables = load_sstables_from_version(&path, version_set.current(), cache_for_open)?;
    remove_orphan_sstables(&path, version_set.current())?;
    last_sequence = last_sequence.max(max_sequence_in_sstables(&sstables));

    let next_sequence = last_sequence.saturating_add(1).max(1);
    let wal = WALManager::open(&path, next_wal, next_sequence, Arc::clone(&options))?;

    let compaction_picker = Arc::new(CompactionPicker::from_options(&options));
    let background_compaction = options.background_compaction;

    let num_threads = if background_compaction {
      options.compaction_threads.clamp(1, 4)
    } else {
      0
    };
    let (compaction_signals, compaction_receivers): (Vec<Sender<()>>, Vec<Receiver<()>>) = (0
      ..num_threads)
      .map(|_| crossbeam_channel::bounded(options.compaction_channel_size))
      .unzip();

    let db = Arc::new(DB {
      path,
      options,
      memtable: RwLock::new(memtable),
      immutable_memtables: RwLock::new(Vec::new()),
      wal: RwLock::new(wal),
      sstables: RwLock::new(sstables),
      version_set: RwLock::new(version_set),
      compaction_picker,
      sequence: AtomicU64::new(last_sequence),
      write_lock: Mutex::new(()),
      flush_lock: Mutex::new(()),
      flush_shutdown: Arc::new(AtomicBool::new(false)),
      flush_handle: Mutex::new(None),
      compaction_shutdown: Arc::new(AtomicBool::new(false)),
      compaction_signals,
      compaction_handles: Mutex::new(Vec::new()),
      compacting: Mutex::new(HashSet::new()),
      closed: AtomicBool::new(false),
      checkpoint_in_progress: AtomicBool::new(false),
      total_key_count: AtomicUsize::new(0),
      block_cache,
      snapshots: SnapshotList::new(),
    });

    #[cfg(feature = "monitoring")]
    {
      crate::metrics::init();
      crate::metrics::set_sequence(last_sequence);
    }

    db.start_flush_thread();
    if background_compaction {
      db.start_compaction_threads(compaction_receivers);
    }
    tracing::info!(target: "db", "db.open.complete");
    Ok(db)
  }

  fn start_compaction_threads(self: &Arc<Self>, receivers: Vec<Receiver<()>>) {
    let mut handles = self.compaction_handles.lock();
    for (i, rx) in receivers.into_iter().enumerate() {
      let weak = Arc::downgrade(self);
      let shutdown = Arc::clone(&self.compaction_shutdown);
      let handle = std::thread::Builder::new()
        .name(format!("aidb-compaction-{i}"))
        .spawn(move || compaction_background_loop(weak, shutdown, rx))
        .expect("spawn compaction thread");
      handles.push(handle);
    }
  }

  fn start_flush_thread(self: &Arc<Self>) {
    let weak = Arc::downgrade(self);
    let shutdown = Arc::clone(&self.flush_shutdown);
    let flush_poll_ms = self.options.flush_poll_ms;
    let handle = std::thread::Builder::new()
      .name("aidb-flush".into())
      .spawn(move || loop {
        if shutdown.load(AtomicOrdering::Acquire) {
          break;
        }
        std::thread::sleep(Duration::from_millis(flush_poll_ms));
        if shutdown.load(AtomicOrdering::Acquire) {
          break;
        }
        let Some(db) = weak.upgrade() else {
          break;
        };
        let _ = db.flush_pending();
      })
      .expect("spawn flush thread");
    *self.flush_handle.lock() = Some(handle);
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  pub fn use_wal(&self) -> bool {
    self.options.use_wal
  }

  pub(crate) fn enter_checkpoint(&self) {
    self
      .checkpoint_in_progress
      .store(true, AtomicOrdering::Release);
  }

  pub(crate) fn leave_checkpoint(&self) {
    self
      .checkpoint_in_progress
      .store(false, AtomicOrdering::Release);
  }

  pub(crate) fn pin_sstables(&self) -> Vec<Vec<Arc<SSTableReader>>> {
    self.sstables.read().iter().cloned().collect()
  }

  pub(crate) fn collect_checkpoint_file_paths(&self) -> Result<Vec<PathBuf>> {
    use crate::engine::compaction::current_exists;
    use crate::engine::wal::manager::WALManager;

    const CURRENT_FILE: &str = "CURRENT";
    let mut paths = Vec::new();
    let db_path = &self.path;

    if current_exists(db_path) {
      paths.push(db_path.join(CURRENT_FILE));
    }

    let manifest = self.version_set.read().manifest_path().to_path_buf();
    if manifest.exists() {
      paths.push(manifest);
    }

    paths.extend(WALManager::scan_wal_file_paths(db_path));

    let version = self.version_set.read().current().clone();
    for (level, files) in version.levels.iter().enumerate() {
      for meta in files {
        let p = sstable_path(db_path, meta.file_number, level);
        if p.exists() {
          paths.push(p);
        }
      }
    }

    Ok(paths)
  }

  /// Level 0 SSTable 文件数量 (测试/诊断).
  pub fn level0_sstable_count(&self) -> usize {
    self.sstables.read()[0].len()
  }

  /// Block cache 统计快照.
  pub fn cache_stats(&self) -> CacheStats {
    self.block_cache.stats()
  }

  /// 清零 block cache 统计计数器.
  pub fn reset_cache_stats(&self) {
    self.block_cache.reset_stats()
  }

  /// 清空 block cache 条目 (不重置统计).
  pub fn clear_cache(&self) {
    self.block_cache.clear()
  }

  /// 当前 block cache 占用字节.
  pub fn block_cache_size(&self) -> u64 {
    self.block_cache.size()
  }

  /// 配置的 block cache 容量上限.
  pub fn block_cache_capacity(&self) -> usize {
    self.block_cache.capacity()
  }

  /// 进程内近似热数据内存: active/frozen MemTable + Block Cache.
  pub fn approximate_memory_bytes(&self) -> u64 {
    let active = self.memtable.read().approximate_size() as u64;
    let frozen: u64 = self
      .immutable_memtables
      .read()
      .iter()
      .map(|t| t.inner().approximate_size() as u64)
      .sum();
    self.block_cache_size() + active + frozen
  }

  pub fn current_sequence(&self) -> u64 {
    self.sequence.load(AtomicOrdering::SeqCst)
  }

  /// 等待 flush 的 Immutable MemTable 数量 (测试/诊断).
  pub fn immutable_memtable_count(&self) -> usize {
    self.immutable_memtables.read().len()
  }

  pub(crate) fn iter_at_sequence(
    &self,
    sequence: u64,
    start: Option<&[u8]>,
    end: Option<&[u8]>,
  ) -> Result<DbIterGuard> {
    self.check_not_closed()?;
    let mem = self.memtable.read();
    let imm = self.immutable_memtables.read();
    let sst = self.sstables.read();
    Ok(super::iterator::DBIterator::new(
      &mem, &imm, &sst, sequence, start, end,
    ))
  }

  fn check_not_closed(&self) -> Result<()> {
    if self.closed.load(AtomicOrdering::Acquire) {
      return Err(Error::InvalidState("database is closed".into()));
    }
    Ok(())
  }

  fn validate_user_key(key: &[u8]) -> Result<()> {
    if key.is_empty() {
      return Err(Error::InvalidArgument("empty key not allowed".into()));
    }
    Ok(())
  }

  fn alloc_sequence(&self, count: u64) -> Result<u64> {
    let base = self.sequence.fetch_add(count, AtomicOrdering::SeqCst) + 1;
    let last = base.saturating_add(count.saturating_sub(1));
    if last >= SEQUENCE_LIMIT {
      return Err(Error::InvalidState("sequence overflow".into()));
    }
    #[cfg(feature = "monitoring")]
    crate::metrics::set_sequence(last + 1);
    Ok(base)
  }

  /// Level 0 文件过多时 stall 写入, 等待 compaction 消化.
  /// 仅在 `background_compaction = true` 时生效 (测试模式手动触发 compaction).
  fn check_write_stall(&self) {
    if !self.options.background_compaction {
      return;
    }
    let l0_count = self.sstables.read()[0].len();
    let opts = &self.options;

    // stop: 轮询等待 until L0 回到 slowdown 阈值以下
    if l0_count >= opts.level0_stop_writes_trigger {
      #[cfg(feature = "monitoring")]
      crate::metrics::record_operation("stall_stop");
      while self.sstables.read()[0].len() >= opts.level0_slowdown_writes_trigger {
        std::thread::sleep(std::time::Duration::from_millis(opts.write_stall_poll_ms));
        self.maybe_trigger_compaction();
      }
      return;
    }

    // slowdown: 按超出比例 sleep
    if l0_count > opts.level0_slowdown_writes_trigger {
      #[cfg(feature = "monitoring")]
      crate::metrics::record_operation("stall_slowdown");
      let excess = l0_count - opts.level0_slowdown_writes_trigger;
      let cap = opts.level0_stop_writes_trigger - opts.level0_slowdown_writes_trigger;
      let sleep_ms = (excess as f64 / cap as f64 * opts.write_stall_slowdown_max_ms as f64) as u64;
      std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
    }
  }

  #[tracing::instrument(name = "db_put", skip(self, key, value))]
  pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
    #[cfg(feature = "monitoring")]
    let op_start = std::time::Instant::now();
    self.check_not_closed()?;
    Self::validate_user_key(key)?;
    self.check_write_stall();
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation("put");

    let existed = self.get(key)?.is_some();
    let _guard = self.write_lock.lock();
    let seq = self.alloc_sequence(1)?;
    self.write_put_to_wal(seq, key, value)?;
    self.memtable.read().put(key, value, seq)?;
    drop(_guard);

    if !existed {
      self.total_key_count.fetch_add(1, AtomicOrdering::Relaxed);
      #[cfg(feature = "monitoring")]
      crate::metrics::set_total_key_count(self.total_key_count.load(AtomicOrdering::Relaxed));
    }
    self.maybe_freeze()?;
    tracing::debug!(target: "db", "db.put");
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation_duration("put", op_start.elapsed().as_secs_f64());
    Ok(())
  }

  #[tracing::instrument(name = "db_get", skip(self, key))]
  pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
    #[cfg(feature = "monitoring")]
    let op_start = std::time::Instant::now();
    self.check_not_closed()?;
    Self::validate_user_key(key)?;
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation("get");
    let max_seq = self.sequence.load(AtomicOrdering::SeqCst);
    let r = self.get_at_sequence(key, max_seq)?;
    tracing::debug!(target: "db", found = r.is_some(), "db.get.result");
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation_duration("get", op_start.elapsed().as_secs_f64());
    Ok(r)
  }

  pub(crate) fn get_at_sequence(&self, key: &[u8], max_seq: u64) -> Result<Option<Vec<u8>>> {
    let seek_key = encode_internal_key(key, max_seq, ValueType::TypePut);
    if let Some((value, ty)) = self.memtable.read().search(&seek_key)? {
      return Ok(match ty {
        ValueType::TypePut => Some(value),
        ValueType::TypeDelete => None,
      });
    }
    for imm in self.immutable_memtables.read().iter().rev() {
      if let Some((value, ty)) = imm.search(&seek_key)? {
        return Ok(match ty {
          ValueType::TypePut => Some(value),
          ValueType::TypeDelete => None,
        });
      }
    }
    self.get_from_sstables(key, max_seq)
  }

  fn get_from_sstables(&self, key: &[u8], max_seq: u64) -> Result<Option<Vec<u8>>> {
    let seek_key = encode_internal_key(key, max_seq, ValueType::TypePut);
    let tables = self.sstables.read();
    // Level 0: newest SSTable first (insert at head on load / flush).
    for reader in &tables[0] {
      if let Some((value, ty)) = reader.get(&seek_key)? {
        return Ok(match ty {
          ValueType::TypePut => Some(value),
          ValueType::TypeDelete => None,
        });
      }
    }
    for level in tables.iter().skip(1) {
      if let Some(reader) = find_sstable_for_key(level, key) {
        if let Some((value, ty)) = reader.get(&seek_key)? {
          return Ok(match ty {
            ValueType::TypePut => Some(value),
            ValueType::TypeDelete => None,
          });
        }
      }
    }
    Ok(None)
  }

  #[tracing::instrument(name = "db_delete", skip(self, key))]
  pub fn delete(&self, key: &[u8]) -> Result<()> {
    #[cfg(feature = "monitoring")]
    let op_start = std::time::Instant::now();
    self.check_not_closed()?;
    Self::validate_user_key(key)?;
    self.check_write_stall();
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation("delete");

    let existed = self.get(key)?.is_some();
    let _guard = self.write_lock.lock();
    let seq = self.alloc_sequence(1)?;
    self.write_delete_to_wal(seq, key)?;
    self.memtable.read().delete(key, seq)?;
    drop(_guard);

    if existed {
      self
        .total_key_count
        .fetch_update(AtomicOrdering::Relaxed, AtomicOrdering::Relaxed, |c| {
          Some(c.saturating_sub(1))
        })
        .ok();
      #[cfg(feature = "monitoring")]
      crate::metrics::set_total_key_count(self.total_key_count.load(AtomicOrdering::Relaxed));
    }
    self.maybe_freeze()?;
    tracing::debug!(target: "db", "db.delete");
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation_duration("delete", op_start.elapsed().as_secs_f64());
    Ok(())
  }

  #[tracing::instrument(name = "db_write_batch", skip(self, batch))]
  pub fn write(&self, batch: &WriteBatch) -> Result<()> {
    if batch.is_empty() {
      return Ok(());
    }
    #[cfg(feature = "monitoring")]
    let op_start = std::time::Instant::now();
    self.check_not_closed()?;
    self.check_write_stall();
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation("write_batch");

    let n = batch.len() as u64;
    let _guard = self.write_lock.lock();
    let base = self.alloc_sequence(n)?;

    if self.options.use_wal {
      let mut wal = self.wal.write();
      let batch_start = WalEntry {
        sequence: 0,
        op_type: OpType::BatchStart,
        has_value: true,
        key: vec![],
        value: Some((n as u32).to_le_bytes().to_vec()),
      };
      wal.append(&batch_start.encode())?;
      for (i, op) in batch.operations.iter().enumerate() {
        let seq = base + i as u64;
        let encoded = wal_entry_for_op(op, seq)?.encode();
        wal.append(&encoded)?;
        wal.note_appended_sequence(seq);
      }
      if self.options.sync_wal {
        wal.sync()?;
      }
    }

    // Scope the read lock so it is released before maybe_freeze() which
    // may need a write lock on the same memtable.
    {
      let mt = self.memtable.read();
      for (i, op) in batch.operations.iter().enumerate() {
        let seq = base + i as u64;
        match op {
          WriteOp::Put { key, value } => {
            Self::validate_user_key(key)?;
            mt.put(key, value, seq)?;
          }
          WriteOp::Delete { key } => {
            Self::validate_user_key(key)?;
            mt.delete(key, seq)?;
          }
        }
      }
    }
    // Release the write lock before maybe_freeze to prevent holding both
    // locks simultaneously if a freeze is triggered.
    drop(_guard);
    self.maybe_freeze()?;
    tracing::debug!(target: "db", op_count = batch.len(), "db.write_batch");
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation_duration("write_batch", op_start.elapsed().as_secs_f64());
    Ok(())
  }

  /// 删除 `[start, end)` 半开区间内的全部 user key (scan + WriteBatch; 非 RangeTombstone).
  #[tracing::instrument(name = "db_delete_range", skip(self, start, end))]
  pub fn delete_range(&self, start: &[u8], end: &[u8]) -> Result<()> {
    self.check_not_closed()?;
    if start >= end {
      return Ok(());
    }
    let mut keys = Vec::new();
    {
      let iter = self.scan(Some(start), Some(end))?;
      for item in iter {
        let (k, _) = item?;
        keys.push(k);
      }
    }
    if keys.is_empty() {
      return Ok(());
    }
    let mut batch = WriteBatch::new();
    for key in keys {
      batch.delete(key);
    }
    self.write(&batch)
  }

  #[tracing::instrument(name = "db_snapshot", skip(self))]
  pub fn snapshot(self: &Arc<Self>) -> Result<Snapshot> {
    self.check_not_closed()?;
    let _guard = self.write_lock.lock();
    let seq = self.sequence.load(AtomicOrdering::SeqCst);
    drop(_guard);
    let snapshot_id = self.snapshots.register(seq);
    #[cfg(feature = "monitoring")]
    crate::metrics::record_operation("snapshot");
    tracing::Span::current().record("sequence", seq);
    tracing::debug!(target: "db", sequence = seq, id = snapshot_id, "db.snapshot.create");
    Ok(Snapshot::new(Arc::clone(self), seq, snapshot_id))
  }

  pub fn iter(&self) -> Result<DbIterGuard> {
    self.check_not_closed()?;
    // Phase5: 全表扫描使用 K_MAX_SEQUENCE, 与 get_at_sequence 的 MVCC 边界区分.
    let seq = crate::engine::memtable::K_MAX_SEQUENCE;
    let mem = self.memtable.read();
    let imm = self.immutable_memtables.read();
    let sst = self.sstables.read();
    Ok(super::iterator::DBIterator::new(
      &mem, &imm, &sst, seq, None, None,
    ))
  }

  #[tracing::instrument(name = "db_scan", skip(self, start, end))]
  pub fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<DbIterGuard> {
    self.check_not_closed()?;
    let seq = crate::engine::memtable::K_MAX_SEQUENCE;
    let mem = self.memtable.read();
    let imm = self.immutable_memtables.read();
    let sst = self.sstables.read();
    tracing::debug!(target: "db", "db.scan.complete");
    Ok(super::iterator::DBIterator::new(
      &mem, &imm, &sst, seq, start, end,
    ))
  }

  pub fn flush(&self) -> Result<()> {
    self.check_not_closed()?;
    self.do_flush_with_span()
  }

  #[tracing::instrument(name = "db_flush", skip(self))]
  fn do_flush_with_span(&self) -> Result<()> {
    self.do_flush()
  }

  fn do_flush(&self) -> Result<()> {
    self.freeze_active_if_nonempty()?;
    let _flush_guard = self.flush_lock.lock();
    self.flush_immutable_memtables()?;
    self.rotate_wal()?;
    self.try_cleanup_wals()?;
    self.maybe_trigger_compaction();
    Ok(())
  }

  fn flush_pending(&self) -> Result<()> {
    if self.flush_shutdown.load(AtomicOrdering::Acquire) {
      return Ok(());
    }
    let _flush_guard = self.flush_lock.lock();
    if self.flush_shutdown.load(AtomicOrdering::Acquire) {
      return Ok(());
    }
    let flushed = self.flush_immutable_memtables()?;
    if flushed > 0 {
      self.rotate_wal()?;
      self.try_cleanup_wals()?;
      self.maybe_trigger_compaction();
    }
    Ok(())
  }

  #[tracing::instrument(name = "db_close", skip(self))]
  pub fn close(&self) -> Result<()> {
    if self.closed.load(AtomicOrdering::Acquire) {
      return Ok(());
    }
    self
      .compaction_shutdown
      .store(true, AtomicOrdering::Release);
    for s in &self.compaction_signals {
      let _ = s.try_send(());
    }
    for h in self.compaction_handles.lock().drain(..) {
      let _ = h.join();
    }
    self.flush_shutdown.store(true, AtomicOrdering::Release);
    if let Some(h) = self.flush_handle.lock().take() {
      let _ = h.join();
    }
    self.do_flush()?;
    self.closed.store(true, AtomicOrdering::Release);
    self.wal.write().sync()?;
    let _ = self.wal.write().close();
    let _ = self.wal.write().cleanup(u64::MAX);
    tracing::info!(target: "db", "db.close");
    Ok(())
  }

  fn maybe_trigger_compaction(&self) {
    let levels: Vec<Vec<Arc<SSTableReader>>> = self.sstables.read().iter().cloned().collect();
    if self.compaction_picker.pick_compaction(&levels).is_some() {
      for s in &self.compaction_signals {
        let _ = s.try_send(());
      }
    }
  }

  /// Claim all file numbers in the compaction task. Returns false if any file
  /// is already being compacted by another thread.
  fn try_claim_files(&self, task: &CompactionTask) -> bool {
    let mut guard = self.compacting.lock();
    let mut claimed: Vec<u64> = Vec::new();
    for f in task.inputs.iter().chain(task.expanded_inputs.iter()) {
      if !guard.insert(f.file_number()) {
        // Collision: roll back all claims for this task
        for num in &claimed {
          guard.remove(num);
        }
        return false;
      }
      claimed.push(f.file_number());
    }
    true
  }

  /// Release all file numbers claimed by the compaction task.
  fn release_files(&self, task: &CompactionTask) {
    let mut guard = self.compacting.lock();
    for f in task.inputs.iter().chain(task.expanded_inputs.iter()) {
      guard.remove(&f.file_number());
    }
  }

  /// 执行一轮 compaction; 返回 true 表示应链式再 pick.
  pub(crate) fn run_compaction_once(&self) -> Result<bool> {
    if self.checkpoint_in_progress.load(AtomicOrdering::Acquire) {
      return Ok(false);
    }
    #[cfg(feature = "monitoring")]
    let pick_start = std::time::Instant::now();
    let levels: Vec<Vec<Arc<SSTableReader>>> = self.sstables.read().iter().cloned().collect();
    let task = match self.compaction_picker.pick_compaction(&levels) {
      Some(t) => t,
      None => return Ok(false),
    };
    #[cfg(feature = "monitoring")]
    {
      crate::metrics::record_compaction("pick");
      crate::metrics::record_compaction_duration("pick", pick_start.elapsed().as_secs_f64());
    }

    // Claim files to prevent overlapping compactions from different threads
    if !self.try_claim_files(&task) {
      return Ok(true);
    }

    // --- TRIVIAL MOVE FAST PATH ---
    if task.is_trivial_move {
      return self.run_trivial_move(task);
    }
    // --- END TRIVIAL MOVE ---

    #[cfg(feature = "monitoring")]
    let run_start = std::time::Instant::now();
    let min_snap_seq = self.snapshots.min_snapshot_sequence();
    let num_splits = self.compute_subcompaction_splits(&task);
    // Pre-allocate all needed file numbers to avoid concurrent allocation races
    let file_numbers: Vec<u64> = (0..num_splits)
      .map(|_| self.version_set.read().allocate_file_number())
      .collect();
    let results = CompactionJob::new(
      task.inputs.clone(),
      task.expanded_inputs.clone(),
      task.output_level,
      self.path.clone(),
      self.options.block_size,
      self.options.block_restart_interval,
      self.options.compression,
      self.options.bloom_false_positive_rate,
    )
    .with_snapshot_threshold(min_snap_seq)
    .run(&file_numbers)?;

    #[cfg(feature = "monitoring")]
    {
      crate::metrics::record_compaction("run");
      crate::metrics::record_compaction_duration("run", run_start.elapsed().as_secs_f64());
    }

    #[cfg(feature = "monitoring")]
    let apply_start = std::time::Instant::now();

    {
      let mut sst_guard = self.sstables.write();
      let mut vs_guard = self.version_set.write();

      for result in &results {
        if result.entry_count > 0 {
          let reader = Arc::new(SSTableReader::open(
            &result.output_path,
            Some(Arc::clone(&self.block_cache)),
          )?);
          if task.output_level == 0 {
            sst_guard[task.output_level].insert(0, reader);
          } else {
            sst_guard[task.output_level].push(reader);
          }
          vs_guard.apply_edit(&VersionEdit::AddFile {
            level: task.output_level,
            file_number: result.file_number,
            file_size: result.file_size,
            smallest_key: result.smallest_key.clone(),
            largest_key: result.largest_key.clone(),
          })?;
        }
      }

      for input in &task.inputs {
        let num = input.file_number();
        vs_guard.apply_edit(&VersionEdit::DeleteFile {
          level: task.level,
          file_number: num,
        })?;
        sst_guard[task.level].retain(|f| f.file_number() != num);
      }
      for expanded in &task.expanded_inputs {
        let num = expanded.file_number();
        vs_guard.apply_edit(&VersionEdit::DeleteFile {
          level: task.output_level,
          file_number: num,
        })?;
        sst_guard[task.output_level].retain(|f| f.file_number() != num);
      }
      #[cfg(feature = "monitoring")]
      {
        crate::metrics::record_compaction("apply");
        crate::metrics::record_compaction_duration("apply", apply_start.elapsed().as_secs_f64());
      }
    }

    update_sstable_metrics(&self.sstables.read());

    self.try_cleanup_wals()?;

    for input in &task.inputs {
      let path = sstable_path(&self.path, input.file_number(), task.level);
      let _ = std::fs::remove_file(path);
    }
    for expanded in &task.expanded_inputs {
      let path = sstable_path(&self.path, expanded.file_number(), task.output_level);
      let _ = std::fs::remove_file(path);
    }

    self.release_files(&task);
    Ok(true)
  }

  /// 根据 subcompaction_min_size 和 compaction_threads 计算分裂数.
  fn compute_subcompaction_splits(&self, task: &CompactionTask) -> usize {
    let min_size = self.options.subcompaction_min_size;
    if min_size == 0 || self.options.compaction_threads <= 1 {
      return 1;
    }
    let total_size: u64 = task
      .inputs
      .iter()
      .chain(task.expanded_inputs.iter())
      .map(|r| r.file_size())
      .sum();
    if total_size < min_size {
      return 1;
    }
    let n = (total_size / min_size)
      .min(self.options.compaction_threads as u64)
      .min(self.options.max_sub_compactions as u64);
    n.max(self.options.min_sub_compactions as u64) as usize
  }

  /// Handle a trivial move: promote SST(s) to the next level without rewriting.
  fn run_trivial_move(&self, task: CompactionTask) -> Result<bool> {
    debug_assert!(
      !task.inputs.is_empty(),
      "trivial move requires at least one input"
    );
    let file_number = task.inputs[0].file_number();
    let old_path = sstable_path(&self.path, file_number, task.level);
    let new_path = sstable_path(&self.path, file_number, task.output_level);

    // Rename the SST file to reflect the new level
    std::fs::rename(&old_path, &new_path).map_err(|e| {
      Error::Io(std::io::Error::new(
        e.kind(),
        format!("trivial move rename failed: {old_path:?} -> {new_path:?}: {e}"),
      ))
    })?;

    // Re-open at the new path (file content unchanged, just new level in name)
    let reader = match SSTableReader::open(&new_path, Some(Arc::clone(&self.block_cache))) {
      Ok(r) => Arc::new(r),
      Err(e) => {
        // Rollback: move the file back to its original location
        let _ = std::fs::rename(&new_path, &old_path);
        return Err(e);
      }
    };

    {
      let mut sst_guard = self.sstables.write();
      let mut vs_guard = self.version_set.write();

      // Add at new level
      if task.output_level == 0 {
        sst_guard[task.output_level].insert(0, reader.clone());
      } else {
        sst_guard[task.output_level].push(reader.clone());
      }
      let smallest = reader.smallest_key().to_vec();
      let largest = reader.largest_key().to_vec();
      vs_guard.apply_edit(&VersionEdit::AddFile {
        level: task.output_level,
        file_number,
        file_size: reader.file_size(),
        smallest_key: smallest,
        largest_key: largest,
      })?;

      // Delete from old level
      vs_guard.apply_edit(&VersionEdit::DeleteFile {
        level: task.level,
        file_number,
      })?;
      sst_guard[task.level].retain(|f| f.file_number() != file_number);
    }

    update_sstable_metrics(&self.sstables.read());

    self.release_files(&task);
    Ok(true)
  }

  /// 测试/诊断: 排空当前可执行的 compaction 任务.
  pub fn drain_compactions(&self) -> Result<()> {
    while self.run_compaction_once()? {}
    Ok(())
  }

  fn write_put_to_wal(&self, seq: u64, key: &[u8], value: &[u8]) -> Result<()> {
    if !self.options.use_wal {
      return Ok(());
    }
    let entry = WalEntry {
      sequence: seq,
      op_type: OpType::TypePut,
      has_value: true,
      key: key.to_vec(),
      value: Some(value.to_vec()),
    };
    let mut wal = self.wal.write();
    wal.append(&entry.encode())?;
    wal.note_appended_sequence(seq);
    if self.options.sync_wal {
      wal.sync()?;
    }
    Ok(())
  }

  fn write_delete_to_wal(&self, seq: u64, key: &[u8]) -> Result<()> {
    if !self.options.use_wal {
      return Ok(());
    }
    let entry = WalEntry {
      sequence: seq,
      op_type: OpType::TypeDelete,
      has_value: false,
      key: key.to_vec(),
      value: None,
    };
    let mut wal = self.wal.write();
    wal.append(&entry.encode())?;
    wal.note_appended_sequence(seq);
    if self.options.sync_wal {
      wal.sync()?;
    }
    Ok(())
  }

  fn maybe_freeze(&self) -> Result<()> {
    if self.memtable.read().approximate_size() < self.options.memtable_size {
      return Ok(());
    }
    self.wait_for_memtable_slot()?;
    let mut mem = self.memtable.write();
    if mem.approximate_size() < self.options.memtable_size {
      return Ok(());
    }
    let flush_seq = self.sequence.load(AtomicOrdering::SeqCst);
    let old = std::mem::take(&mut *mem);
    let frozen = old.freeze(flush_seq);
    self.immutable_memtables.write().push(frozen);
    Ok(())
  }

  /// 当 Immutable MemTable 数达到 `max_write_buffer_number` 上限时阻塞写入, 并驱动 flush.
  fn wait_for_memtable_slot(&self) -> Result<()> {
    for _ in 0..self.options.memtable_wait_iters {
      if self.immutable_memtables.read().len() + 1 < self.options.max_write_buffer_number {
        return Ok(());
      }
      self.flush_pending()?;
      if self.immutable_memtables.read().len() + 1 < self.options.max_write_buffer_number {
        return Ok(());
      }
      std::thread::sleep(Duration::from_millis(
        self.options.memtable_wait_interval_ms,
      ));
    }
    Err(Error::Busy(
      "too many immutable memtables waiting flush".into(),
    ))
  }

  fn freeze_active_if_nonempty(&self) -> Result<()> {
    let mut mem = self.memtable.write();
    if mem.approximate_size() == 0 {
      return Ok(());
    }
    let flush_seq = self.sequence.load(AtomicOrdering::SeqCst);
    let old = std::mem::take(&mut *mem);
    self.immutable_memtables.write().push(old.freeze(flush_seq));
    Ok(())
  }

  fn flush_immutable_memtables(&self) -> Result<usize> {
    let mut flushed = 0usize;
    loop {
      let has_front = !self.immutable_memtables.read().is_empty();
      if !has_front {
        break;
      }
      {
        let imm = self.immutable_memtables.read();
        self.flush_memtable_to_sstable(imm[0].inner())?;
      }
      self.immutable_memtables.write().remove(0);
      flushed += 1;
    }
    if flushed > 0 {
      #[cfg(feature = "monitoring")]
      crate::metrics::record_flush();
    }
    Ok(flushed)
  }

  #[tracing::instrument(name = "db_flush_sst", skip(self, table))]
  fn flush_memtable_to_sstable(&self, table: &MemTable) -> Result<()> {
    #[cfg(feature = "monitoring")]
    let flush_start = std::time::Instant::now();
    let mut count = 0u64;
    let file_number = self.version_set.read().allocate_file_number();
    let path = sstable_path(&self.path, file_number, 0);
    let key_count = table.map().iter().count();
    let mut builder = SSTableBuilder::new(
      &path,
      self.options.block_size,
      self.options.block_restart_interval,
      self.options.compression,
      self.options.bloom_false_positive_rate,
    )?;
    if self.options.bloom_false_positive_rate > 0.0 {
      builder.set_expected_keys(key_count);
    }
    for entry in table.map().iter() {
      builder.add(entry.key().as_ref(), entry.value().as_ref())?;
      count += 1;
    }
    if count == 0 {
      builder.abandon()?;
      #[cfg(feature = "monitoring")]
      crate::metrics::record_flush_duration(flush_start.elapsed().as_secs_f64());
      return Ok(());
    }
    let file_size = builder.finish()?;
    let reader = Arc::new(SSTableReader::open(
      &path,
      Some(Arc::clone(&self.block_cache)),
    )?);
    {
      let mut tables = self.sstables.write();
      tables[0].insert(0, Arc::clone(&reader));
      let mut vs = self.version_set.write();
      vs.apply_edit(&VersionEdit::AddFile {
        level: 0,
        file_number,
        file_size: reader.file_size(),
        smallest_key: reader.smallest_key().to_vec(),
        largest_key: reader.largest_key().to_vec(),
      })?;
    }
    update_sstable_metrics(&self.sstables.read());
    tracing::info!(target: "db", file_number, file_size, "db.flush.complete");
    #[cfg(feature = "monitoring")]
    crate::metrics::record_flush_duration(flush_start.elapsed().as_secs_f64());
    Ok(())
  }

  fn rotate_wal(&self) -> Result<()> {
    let next = self.sequence.load(AtomicOrdering::SeqCst).saturating_add(1);
    self.wal.write().rotate(next)
  }

  fn try_cleanup_wals(&self) -> Result<()> {
    let watermark = self.wal_gc_watermark();
    let _ = self.wal.write().cleanup(watermark)?;
    Ok(())
  }

  fn wal_gc_watermark(&self) -> u64 {
    let imm = self.immutable_memtables.read();
    if let Some(min_flush) = imm.iter().map(|m| m.flush_seq()).min() {
      return min_flush;
    }
    drop(imm);
    let mem = self.memtable.read();
    if let Some(min_seq) = min_sequence_in_memtable(&mem) {
      return min_seq;
    }
    u64::MAX
  }
}

fn wal_entry_for_op(op: &WriteOp, seq: u64) -> Result<WalEntry> {
  match op {
    WriteOp::Put { key, value } => Ok(WalEntry {
      sequence: seq,
      op_type: OpType::TypePut,
      has_value: true,
      key: key.clone(),
      value: Some(value.clone()),
    }),
    WriteOp::Delete { key } => Ok(WalEntry {
      sequence: seq,
      op_type: OpType::TypeDelete,
      has_value: false,
      key: key.clone(),
      value: None,
    }),
  }
}

fn max_sequence_in_memtable(mt: &MemTable) -> u64 {
  let mut max = 0u64;
  for entry in mt.map().iter() {
    if let Ok(seq) = extract_sequence(entry.key().as_ref()) {
      max = max.max(seq);
    }
  }
  max
}

fn min_sequence_in_memtable(mt: &MemTable) -> Option<u64> {
  let mut min: Option<u64> = None;
  for entry in mt.map().iter() {
    if let Ok(seq) = extract_sequence(entry.key().as_ref()) {
      min = Some(min.map_or(seq, |m| m.min(seq)));
    }
  }
  min
}

fn max_sequence_in_sstables(sstables: &[Vec<Arc<SSTableReader>>]) -> u64 {
  let mut max = 0u64;
  for level in sstables {
    for reader in level {
      let mut it = reader.iter();
      while it.valid() {
        if let Some(key) = it.key() {
          if let Ok(seq) = extract_sequence(key) {
            max = max.max(seq);
          }
        }
        if !it.advance() {
          break;
        }
      }
    }
  }
  max
}

fn find_sstable_for_key<'a>(
  level: &'a [Arc<SSTableReader>],
  user_key: &[u8],
) -> Option<&'a Arc<SSTableReader>> {
  level
    .iter()
    .find(|reader| user_key_in_sstable_range(user_key, reader.smallest_key(), reader.largest_key()))
}

/// Level 1+ 文件范围检测: 仅比较 user_key (不用 seek InternalKey, 避免 sequence 干扰).
fn user_key_in_sstable_range(user_key: &[u8], smallest: &[u8], largest: &[u8]) -> bool {
  if smallest.len() < 8 || largest.len() < 8 {
    return true;
  }
  let s_user = &smallest[..smallest.len() - 8];
  let l_user = &largest[..largest.len() - 8];
  user_key >= s_user && user_key <= l_user
}

fn compaction_background_loop(weak: Weak<DB>, shutdown: Arc<AtomicBool>, signal: Receiver<()>) {
  loop {
    if shutdown.load(AtomicOrdering::Acquire) {
      break;
    }
    let Some(db) = weak.upgrade() else {
      break;
    };
    let pending = db
      .compaction_picker
      .pick_compaction(&db.sstables.read().iter().cloned().collect::<Vec<_>>())
      .is_some();
    let _bg = tracing::debug_span!("cmp_background", pending).entered();
    match db.run_compaction_once() {
      Ok(true) => continue,
      Ok(false) => {
        let _ = signal.recv_timeout(Duration::from_millis(db.options.compaction_poll_ms));
      }
      Err(e) => {
        tracing::error!(target: "cmp", error = %e, "compaction round failed");
        let _ = signal.recv_timeout(Duration::from_millis(db.options.compaction_poll_ms));
      }
    }
  }
}

fn update_sstable_metrics(_sstables: &[Vec<Arc<SSTableReader>>]) {
  #[cfg(feature = "monitoring")]
  for (level, readers) in _sstables.iter().enumerate() {
    let label = level.to_string();
    crate::metrics::SSTABLE_COUNT
      .with_label_values(&[&label])
      .set(readers.len() as i64);
    let total: u64 = readers.iter().map(|r| r.file_size()).sum();
    crate::metrics::SSTABLE_SIZE_BYTES
      .with_label_values(&[&label])
      .set(total as i64);
  }
}

impl Drop for DB {
  fn drop(&mut self) {
    if self.closed.load(AtomicOrdering::Acquire) {
      return;
    }
    self
      .compaction_shutdown
      .store(true, AtomicOrdering::Release);
    for s in &self.compaction_signals {
      let _ = s.try_send(());
    }
    for h in self.compaction_handles.lock().drain(..) {
      let _ = h.join();
    }
    self.flush_shutdown.store(true, AtomicOrdering::Release);
    if let Some(h) = self.flush_handle.lock().take() {
      let _ = h.join();
    }
    let _ = self.wal.write().sync();
  }
}

#[cfg(test)]
mod sequence_tests {
  use super::*;
  use crate::engine::memtable::SEQUENCE_LIMIT;
  use crate::error::Error;
  use tempfile::tempdir;

  #[test]
  fn test_sequence_overflow_on_put() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    db.sequence
      .store(SEQUENCE_LIMIT - 1, AtomicOrdering::SeqCst);
    assert!(matches!(
      db.put(b"overflow", b"x"),
      Err(Error::InvalidState(_))
    ));
    db.close().unwrap();
  }
}
