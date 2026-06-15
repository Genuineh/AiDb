# Multi-Threaded Compaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Support configurable `compaction_threads > 1` so non-overlapping compaction tasks run in parallel.

**Architecture:** N independent background threads, each with its own signal `Receiver`. File claiming (`Mutex<HashSet<u64>>`) prevents overlapping compactions. `sstables`+`version_set` write locks serialize the apply phase (which is O(metadata) and fast).

**Tech Stack:** Rust, `crossbeam_channel`, `parking_lot::Mutex`, `std::thread`.

---

### Task 1: Refactor data structures for multi-thread support

**Files:**
- Modify: `src/config.rs:64-65`
- Modify: `src/engine/db/inner.rs` — DB struct, open(), start/stop/maybe_trigger_compaction

- [ ] **Step 1: Update config.rs**

Change the comment on `compaction_threads`:
```rust
/// Compaction 后台线程数 (默认 1, 建议 1-4)
pub compaction_threads: usize,
```

The default values remain unchanged (already `1` in both `Default` and `for_testing`).

- [ ] **Step 2: Update DB struct in inner.rs**

Replace single signal/handle with Vec versions, add `compacting` set:

```rust
pub struct DB {
  // ... existing fields ...
  compaction_shutdown: Arc<AtomicBool>,
  compaction_signals: Vec<Sender<()>>,
  compaction_handles: Mutex<Vec<JoinHandle<()>>>,
  compacting: Mutex<HashSet<u64>>,
  // ... rest unchanged ...
}
```

Remove the old single `compaction_signal: Sender<()>` and `compaction_handle: Mutex<Option<JoinHandle<()>>>`.
Add `use std::collections::HashSet;` to the imports.

- [ ] **Step 3: Update `DB::open()` to create N channels**

Replace the single channel creation with N channels:

```rust
let num_threads = if background_compaction {
    options.compaction_threads.max(1).min(4)
} else {
    0
};
let (compaction_signals, compaction_receivers): (Vec<Sender<()>>, Vec<Receiver<()>>) =
    (0..num_threads).map(|_| crossbeam_channel::bounded(64)).unzip();

// ... construct DB ...
let db = Arc::new(DB {
    compaction_shutdown: Arc::new(AtomicBool::new(false)),
    compaction_signals,
    compaction_handles: Mutex::new(Vec::new()),
    compacting: Mutex::new(HashSet::new()),
    // ... rest ...
});

db.start_flush_thread();
if background_compaction {
    db.start_compaction_threads(compaction_receivers);
}
```

- [ ] **Step 4: Update `start_compaction_threads`**

Rename `start_compaction_thread` → `start_compaction_threads`. Each thread gets a unique name:

```rust
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
```

- [ ] **Step 5: Update `close()` and `drop()`**

Join all handles:

```rust
pub fn close(&self) -> Result<()> {
    if self.closed.load(AtomicOrdering::Acquire) {
        return Ok(());
    }
    self.compaction_shutdown.store(true, AtomicOrdering::Release);
    for s in &self.compaction_signals {
        let _ = s.try_send(());
    }
    if let Some(handles) = self.compaction_handles.lock().take() {
        for h in handles {
            let _ = h.join();
        }
    }
    self.flush_shutdown.store(true, AtomicOrdering::Release);
    if let Some(h) = self.flush_handle.lock().take() {
        let _ = h.join();
    }
    // ... rest unchanged ...
}
```

Same pattern for `drop()`.

- [ ] **Step 6: Update `maybe_trigger_compaction()`**

Broadcast to all threads:

```rust
fn maybe_trigger_compaction(&self) {
    let levels: Vec<Vec<Arc<SSTableReader>>> = self.sstables.read().iter().cloned().collect();
    if self.compaction_picker.pick_compaction(&levels).is_some() {
        for s in &self.compaction_signals {
            let _ = s.try_send(());
        }
    }
}
```

- [ ] **Step 7: Build and verify**

```bash
cd aidb && cargo build 2>&1
```

Fix any compilation errors.

- [ ] **Step 8: Run existing tests (single-threaded baseline)**

```bash
cd aidb && cargo test --test engine compaction -- --test-threads=1
```

Expected: all pass (default `compaction_threads = 1`, no behavior change).

---

### Task 2: Add file claiming (compaction conflict prevention)

**Files:**
- Modify: `src/engine/db/inner.rs` — add claim/release to `run_compaction_once`

- [ ] **Step 1: Add claim/release methods to DB**

```rust
/// Claim all files in the compaction task. Returns false if any file is already claimed.
fn try_claim_files(&self, task: &CompactionTask) -> bool {
    let mut guard = self.compacting.lock();
    let mut claimed = Vec::new();
    for f in task.inputs.iter().chain(task.expanded_inputs.iter()) {
        if !guard.insert(f.file_number()) {
            // Conflict: roll back all claims for this task
            for num in claimed { guard.remove(&num); }
            return false;
        }
        claimed.push(f.file_number());
    }
    true
}

/// Release all files claimed by the compaction task.
fn release_files(&self, task: &CompactionTask) {
    let mut guard = self.compacting.lock();
    for f in task.inputs.iter().chain(task.expanded_inputs.iter()) {
        guard.remove(&f.file_number());
    }
}
```

- [ ] **Step 2: Integrate claim into `run_compaction_once()`**

After the picker returns a task (line 654-657), add the claim attempt:

```rust
let task = match self.compaction_picker.pick_compaction(&levels) {
    Some(t) => t,
    None => return Ok(false),
};

// Claim files to prevent overlapping compactions
if !self.try_claim_files(&task) {
    return Ok(true);  // another thread got it, retry immediately
}
```

After the apply section (before `Ok(true)` at the end), release files:

```rust
update_sstable_metrics(&self.sstables.read());
self.release_files(&task);
// ... existing cleanup ...
Ok(true)
```

Also handle early returns (trivial move, error):

For the trivial move fast path, add release before return:
```rust
if task.is_trivial_move {
    let result = self.run_trivial_move(task);
    // files already released inside run_trivial_move (or we release here)
    return result;
}
```

Actually, for the trivial move, the files are applied in `run_trivial_move` which updates sstables/version_set. The claim should be released after the apply. Let me handle it differently: release at the very end before the `Ok(true)` return, and also on error paths.

Better structure: wrap the compaction body in a block that always releases:

```rust
pub(crate) fn run_compaction_once(&self) -> Result<bool> {
    // ... checkpoint check ...
    // ... pick ...
    
    let task = match self.compaction_picker.pick_compaction(&levels) {
        Some(t) => t,
        None => return Ok(false),
    };
    
    if !self.try_claim_files(&task) {
        return Ok(true);  // another thread claimed overlapping files
    }
    
    let result = self.do_compaction(task);
    
    // result propagates up; claim is released inside do_compaction
    result
}

fn do_compaction(&self, task: CompactionTask) -> Result<bool> {
    // use a scope to ensure release:
    let _guard = CompactionGuard { db: self, task: &task };
    
    if task.is_trivial_move {
        return self.run_trivial_move(task);
    }
    
    // ... full merge path ...
    
    // cleanup old files...
    // _guard drops here, releasing claim
    Ok(true)
}

struct CompactionGuard<'a> {
    db: &'a DB,
    task: &'a CompactionTask,
}

impl Drop for CompactionGuard<'_> {
    fn drop(&mut self) {
        self.db.release_files(self.task);
    }
}
```

This is the cleanest approach — the release always happens, even on panic or error. But since Rust doesn't guarantee Drop on panic in all cases, and we're using `Result` returns anyway, a simpler approach is fine:

```rust
// At the end of run_compaction_once, just before Ok(true):
self.release_files(&task);

// On early returns:
// - For trivial move, release inside run_trivial_move or after it
// - For errors, release before propagating

// Actually, simplest: release right before each return point.
```

Or even simpler: add the release at the end, and for early returns (trivial move), release after calling run_trivial_move.

Actually, let me just keep it simple and explicit:

```rust
// After pick + claim:
if !self.try_claim_files(&task) {
    return Ok(true);
}

// ... existing body of run_compaction_once ...

// Before each return:
// 1. Trivial move early return → release_files before Ok(true)
// 2. Error → release_files before Err(...)
// 3. Normal return → release_files before Ok(true)
```

The most concise approach: keep the existing `run_compaction_once` structure, just add `try_claim_files` after pick and `release_files` before each return. Since there are only ~3 return paths in the function, this is manageable.

- [ ] **Step 3: Verify tests still pass**

```bash
cd aidb && cargo test --test engine compaction -- --test-threads=1
```

Expected: all pass.

- [ ] **Step 4: Add integration test with compaction_threads=2**

Add to `tests/engine/compaction.rs`:

```rust
#[test]
fn test_compaction_threads_2() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.background_compaction = true;
    opts.compaction_threads = 2;
    opts.level0_compaction_trigger = 2;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());

    // Write enough data to trigger multiple compactions
    for batch in 0..10 {
        for i in 0..10u64 {
            db.put(format!("k{batch}_{i}").as_bytes(), b"val").unwrap();
        }
        db.flush().unwrap();
    }

    // Drain all compactions
    db.drain_compactions().unwrap();

    // Verify data
    for batch in 0..10 {
        for i in 0..10u64 {
            let key = format!("k{batch}_{i}");
            assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"val".to_vec()));
        }
    }
    db.close().unwrap();
}
```

Run:
```bash
cd aidb && cargo test --test engine compaction::test_compaction_threads_2 -- --test-threads=1
```

- [ ] **Step 5: Run full compaction test suite**

```bash
cd aidb && cargo test --test engine compaction -- --test-threads=1
```

- [ ] **Step 6: Clippy + fmt + commit**

```bash
cd aidb && RUSTFLAGS='-D warnings' cargo clippy --all-targets && cargo fmt --check
```

Commit:
```bash
git add src/config.rs src/engine/db/inner.rs tests/engine/compaction.rs docs/superpowers/specs/2026-05-29-multi-threaded-compaction-design.md && git commit -m "feat: multi-threaded compaction support

Extend background compaction from single-thread to N threads
(configurable via Options.compaction_threads, default 1, max 4).

- N independent signal channels for waking idle threads
- File claiming (Mutex<HashSet<u64>>) prevents overlapping compactions
- sstables + version_set write locks serialize the apply phase
- Existing behavior unchanged when compaction_threads = 1

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```
