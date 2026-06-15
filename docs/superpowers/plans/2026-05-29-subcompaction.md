# Subcompaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** Split large compaction jobs into N parallel sub-jobs by key range, reducing wall-clock time.

**Architecture:** `CompactionJob::run()` detects large inputs, records split points during the existing count pass, then uses `std::thread::scope` to run N parallel merges. Returns `Vec<CompactionResult>`. Caller applies all results.

**Tech Stack:** Rust, `std::thread::scope` (available since 1.63, already used in tests).

---

### Task 1: Add subcompaction_min_size config + MergeIterator::with_range

**Files:**
- Modify: `src/config.rs` — add option
- Modify: `src/engine/compaction/merge.rs` — add ranged constructor

- [ ] **Step 1: Add config option**

In `src/config.rs`, add to the `Options` struct and both `Default`/`for_testing`:

```rust
/// Subcompaction 分裂阈值 (bytes, 0=禁用, 默认 64MB)
pub subcompaction_min_size: u64,
```

```rust
// Default impl: subcompaction_min_size: 64 * 1024 * 1024,
// for_testing: subcompaction_min_size: 0,  // disabled in tests
```

Add validation in `Options::validate()`:
```rust
if self.subcompaction_min_size > 0 && self.subcompaction_min_size < 4096 {
    return Err("subcompaction_min_size must be 0 or >= 4096");
}
```

- [ ] **Step 2: Add `MergeIterator::with_range()`**

In `merge.rs`, add a constructor that accepts key range boundaries:

```rust
pub fn with_range(readers: Vec<Arc<SSTableReader>>, range_start: Option<Vec<u8>>, range_end: Option<Vec<u8>>, block_cache: Option<&Arc<BlockCache>>) -> Self {
    let mut iters = Vec::with_capacity(readers.len());
    for reader in &readers {
        let mut iter = reader.iter(block_cache.cloned());
        if let Some(ref start) = range_start {
            iter.seek_to_target(start);
        }
        iters.push(iter);
    }
    let mut heap = BinaryHeap::with_capacity(iters.len());
    for (i, iter) in iters.iter_mut().enumerate() {
        if iter.valid() {
            heap.push(MergeEntry { idx: i, key: iter.key().to_vec() });
        }
    }
    MergeIterator { heap, iters, range_start, range_end }
}
```

Add `range_start: Option<Vec<u8>>` and `range_end: Option<Vec<u8>>` fields to `MergeIterator`. In `next_entry()`, after popping from the heap, check if the key >= range_end:

```rust
fn next_entry(&mut self) -> Option<Vec<u8>> {
    while let Some(entry) = self.heap.pop() {
        let key = entry.key.clone();
        if let Some(ref end) = self.range_end {
            if key.as_slice() >= end.as_slice() {
                continue;  // past range end, skip all remaining entries from this iter
            }
        }
        if let Some(iter) = self.iters.get_mut(entry.idx) {
            iter.advance();
            if iter.valid() {
                self.heap.push(MergeEntry {
                    idx: entry.idx,
                    key: iter.key().to_vec(),
                });
            }
        }
        return Some(key);
    }
    None
}
```

No wait, if we've exceeded range_end, we should NOT continue adding from that iterator. The `continue` above would re-push, which is wrong. Let me fix:

```rust
fn next_entry(&mut self) -> Option<Vec<u8>> {
    while let Some(entry) = self.heap.pop() {
        let key = entry.key.clone();
        if let Some(ref end) = self.range_end {
            if key.as_slice() >= end.as_slice() {
                continue;  // this entry is past the range, skip it
            }
        }
        if let Some(iter) = self.iters.get_mut(entry.idx) {
            iter.advance();
            if iter.valid() {
                let next_key = iter.key().to_vec();
                if let Some(ref end) = self.range_end {
                    if next_key.as_slice() < end.as_slice() {
                        self.heap.push(MergeEntry {
                            idx: entry.idx,
                            key: next_key,
                        });
                    }
                } else {
                    self.heap.push(MergeEntry {
                        idx: entry.idx,
                        key: next_key,
                    });
                }
            }
        }
        return Some(key);
    }
    None
}
```

Actually this is getting complex. A simpler approach: just check in `next_entry()` and filter entries past range_end. But the issue is that once the iterator is past the end, all subsequent entries from it are also past the end, so we can stop reading from it entirely.

Simplest: just don't push entries that are past range_end. That's what the check above does.

- [ ] **Step 3: Build and test**

```bash
cd aidb && cargo build 2>&1
```

---

### Task 2: Subcompaction splitting in CompactionJob

**Files:**
- Modify: `src/engine/compaction/job.rs` — split logic + parallel execution
- Modify: `src/engine/compaction/mod.rs` — update re-exports if needed

- [ ] **Step 1: Refactor CompactionJob::run to support split**

The most practical approach: keep existing `run()` but add a `should_split()` check. If splitting is beneficial, delegate to a new `run_split()` method.

```rust
pub fn run(&self, file_number: u64) -> Result<Vec<CompactionResult>> {
    if self.should_split() {
        self.run_split(file_number)
    } else {
        Ok(vec![self.run_single(file_number)?])
    }
}
```

`should_split()`: check `self.subcompaction_min_size > 0` and total input size > threshold.

`run_split(file_number)`:
1. Call `count_dedup_entries()` with split point sampling. Since we need bloom filter pre-allocation per split, calculate during this pass:
   - Record split points (user keys at approximately evenly spaced intervals)
   - Count entries per sub-range (for bloom filter pre-allocation)

Actually, the `count_dedup_entries()` already does a full pass. Let me modify it to also record split points:

```rust
fn count_with_splits(&self, num_splits: usize) -> Result<(usize, Vec<Vec<u8>>)> {
    // ... same as existing count_dedup_entries ...
    // but also:
    let split_interval = max(1, total_count / num_splits);
    let mut splits: Vec<Vec<u8>> = Vec::new();
    let mut entry_counter = 0;
    
    // In the dedup loop:
    entry_counter += 1;
    if splits.len() < num_splits - 1 && entry_counter % split_interval == 0 {
        splits.push(current_user_key.to_vec());
    }
    
    Ok((total_count, splits))
}
```

2. Allocate N output file numbers (sequential from `file_number`)
3. Create ranges: for each adjacent pair of split points, create a (start, end) range
4. Spawn `num_splits` threads via `std::thread::scope`, each processing one range
5. Each thread:
   - Creates MergeIterator with the range bounds
   - Creates SSTableBuilder (bloom set_expected_keys from per-range count)
   - Iterates, dedup, writes
   - Returns CompactionResult
6. Collect all results

```rust
fn run_split(&self, file_number: u64) -> Result<Vec<CompactionResult>> {
    let num_splits = self.compute_num_splits();
    let (total_count, splits) = self.count_with_splits(num_splits)?;
    
    if total_count == 0 {
        return Ok(vec![CompactionResult::empty(file_number)]);
    }
    
    // Build ranges from split points
    let all_readers = self.all_inputs();
    let readers = Arc::new(all_readers);
    
    let results: Vec<CompactionResult> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for i in 0..num_splits {
            let range_start = if i == 0 { None } else { Some(splits[i-1].clone()) };
            let range_end = if i == num_splits - 1 { None } else { Some(splits[i].clone()) };
            let r = Arc::clone(&readers);
            let fnum = file_number + i as u64;
            
            handles.push(scope.spawn(move || {
                let iter = MergeIterator::with_range(
                    r.iter().cloned().collect(),
                    range_start,
                    range_end,
                    None,  // no block cache for compaction
                );
                // ... dedup loop ... builder.add() ... finish ...
                CompactionResult { file_number: fnum, ... }
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    
    Ok(results)
}
```

- [ ] **Step 2: Adapt inner.rs for Vec<CompactionResult>**

In `inner.rs`, `run_compaction_once()` currently:

```rust
let result = CompactionJob::new(...).run(file_number)?;
let new_reader = if result.entry_count > 0 { ... };

// apply:
if let Some(reader) = new_reader { sst_guard[output_level].push(reader); ... }
for input in &task.inputs { sst_guard[level].retain(...); }
for expanded in &task.expanded_inputs { sst_guard[output_level].retain(...); }
// delete old files
```

Change to:

```rust
let results = CompactionJob::new(...).run(file_number)?;

{
    let mut sst_guard = self.sstables.write();
    let mut vs_guard = self.version_set.write();
    
    for result in &results {
        if result.entry_count > 0 {
            let reader = Arc::new(SSTableReader::open(&result.output_path, ...)?);
            sst_guard[task.output_level].push(reader);
            vs_guard.apply_edit(&VersionEdit::AddFile {
                level: task.output_level,
                file_number: result.file_number,
                file_size: result.file_size,
                smallest_key: result.smallest_key.clone(),
                largest_key: result.largest_key.clone(),
            })?;
        }
    }
    
    // delete old input files (same as before)
    for input in &task.inputs { ... }
    for expanded in &task.expanded_inputs { ... }
}
```

Also, the old file deletion at lines ~750-760 needs to work with multiple output files — but the old input SSTs are deleted by the same file numbers, so the deletion loop doesn't change.

- [ ] **Step 3: Add integration test**

In `tests/engine/compaction.rs`:

```rust
#[test]
fn test_subcompaction_large_job() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.background_compaction = true;
    opts.compaction_threads = 2;
    opts.subcompaction_min_size = 1024;  // small threshold to trigger splits
    opts.level0_compaction_trigger = 2;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());
    
    for batch in 0..20 {
        for i in 0..20u64 {
            db.put(format!("k{batch:04}_{i:04}").as_bytes(), b"val").unwrap();
        }
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();
    
    // Verify data
    for batch in 0..20 {
        for i in 0..20u64 {
            let key = format!("k{batch:04}_{i:04}");
            assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"val".to_vec()));
        }
    }
    db.close().unwrap();
}
```

- [ ] **Step 4: Build, test, iterate**

```bash
cd aidb && cargo build 2>&1 | tail -10
cargo test --test engine compaction -- --test-threads=1 2>&1 | tail -20
```

- [ ] **Step 5: Clippy + fmt + full test**

```bash
cd aidb && RUSTFLAGS='-D warnings' cargo clippy --all-targets && cargo fmt --check
cargo test --test engine --test db --test sstable --test cache -- --test-threads=1 2>&1 | tail -5
```

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/engine/compaction/ src/engine/db/inner.rs tests/engine/compaction.rs && git commit -m "feat: subcompaction — split large compaction jobs into parallel sub-jobs

When total input size exceeds subcompaction_min_size (default 64MB),
CompactionJob splits the key range into N sub-ranges during the existing
count-dedup pass and processes each range in parallel via
std::thread::scope.

Each sub-job creates its own MergeIterator (seeking to sub-range start)
and SSTableBuilder. The caller applies Vec<CompactionResult> with
multiple VersionEdit::AddFile entries.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```
