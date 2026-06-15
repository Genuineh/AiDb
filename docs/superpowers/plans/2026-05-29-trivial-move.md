# Trivial Move Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When compaction picks a level-N SST that has zero overlap with level-(N+1), skip the merge-rewrite and simply promote the file by renaming it and updating metadata.

**Architecture:** Three changes: (1) `picker.rs` detects the no-overlap case and marks the task, (2) `inner.rs` short-circuits the merge for trivial-move tasks, (3) tests verify both unit-level picker behavior and end-to-end integration. No new files needed.

**Tech Stack:** Rust, same crate. Dependencies: `std::fs::rename`.

---

### Task 1: Add `is_trivial_move` to CompactionTask + picker detection

**Files:**
- Modify: `src/engine/compaction/picker.rs:9-14` — add field
- Modify: `src/engine/compaction/picker.rs:81-112` — detect in `pick_level_n()`
- Modify: `src/engine/compaction/picker.rs:66-79` — detect in `pick_level0()`
- Test: inline `#[cfg(test)]` in `picker.rs`

- [ ] **Step 1: Write the failing tests for trivial move detection**

Add these tests inside the existing `mod tests` block in `picker.rs`:

```rust
#[test]
fn test_pick_level_n_trivial_move() {
  let dir = tempdir().unwrap();
  let mut opts = Options::for_testing();
  opts.max_bytes_for_level_base = 200;
  let picker = CompactionPicker::from_options(&opts);
  let mut levels = empty_levels(7);
  // Level 1 has one file with key "a", level 2 has NO overlapping files
  levels[1].push(big_file(dir.path(), 1, 1, b"a", 300));
  let task = picker.pick_compaction(&levels).expect("level1 overflow");
  assert!(task.is_trivial_move, "should be trivial move when no overlap");
  assert_eq!(task.inputs.len(), 1);
  assert!(task.expanded_inputs.is_empty());
}

#[test]
fn test_pick_level_n_no_trivial_move_when_overlap() {
  let dir = tempdir().unwrap();
  let mut opts = Options::for_testing();
  opts.max_bytes_for_level_base = 200;
  let picker = CompactionPicker::from_options(&opts);
  let mut levels = empty_levels(7);
  levels[1].push(big_file(dir.path(), 1, 1, b"a", 300));
  levels[2].push(file(dir.path(), 2, 2, b"a"));  // overlaps with seed "a"
  let task = picker.pick_compaction(&levels).expect("level1 overflow");
  assert!(!task.is_trivial_move, "should NOT be trivial move when overlap exists");
}

#[test]
fn test_pick_level0_trivial_move_single_file() {
  let dir = tempdir().unwrap();
  let mut opts = Options::for_testing();
  opts.level0_compaction_trigger = 2;
  let picker = CompactionPicker::from_options(&opts);
  let mut levels = empty_levels(7);
  levels[0].push(file(dir.path(), 1, 0, b"a"));
  levels[0].push(file(dir.path(), 2, 0, b"z"));
  // L1 empty — no overlap
  let task = picker.pick_compaction(&levels).unwrap();
  assert!(task.is_trivial_move, "L0 with single-file-range no overlap should be trivial");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd aidb && cargo test --lib engine::compaction::picker::tests -- --test-threads=1 2>&1 | head -30
```

Expected: compilation fails because `is_trivial_move` field doesn't exist yet.

- [ ] **Step 3: Add `is_trivial_move` field to `CompactionTask`**

Change:
```rust
#[derive(Clone)]
pub struct CompactionTask {
  pub inputs: Vec<Arc<SSTableReader>>,
  pub level: usize,
  pub output_level: usize,
  pub expanded_inputs: Vec<Arc<SSTableReader>>,
  pub is_trivial_move: bool,
}
```

- [ ] **Step 4: Update `pick_level_n()` to detect trivial move**

In `pick_level_n()` (around line 91-104), after the existing `if !expanded.is_empty()` block, add the trivial move path:

```rust
fn pick_level_n(&self, levels: &[Vec<Arc<SSTableReader>>], level: usize) -> Option<CompactionTask> {
  if level + 1 >= levels.len() || levels[level].is_empty() {
    return None;
  }
  let seed = levels[level][0].clone();
  let mut inputs = vec![seed.clone()];
  let mut expanded = overlap_with_reader(levels, level + 1, &seed);

  if expanded.is_empty() {
    // Trivial move: no overlap with target level, promote without rewriting
    return Some(CompactionTask {
      inputs,
      level,
      output_level: level + 1,
      expanded_inputs: Vec::new(),
      is_trivial_move: true,
    });
  }

  // existing merge path
  let (ex_start, ex_end) = combined_range(&expanded);
  for f in &levels[level] {
    if f.file_number() != seed.file_number()
      && key_ranges_overlap_by_meta_raw(ex_start, ex_end, f.smallest_key(), f.largest_key())
    {
      inputs.push(f.clone());
    }
  }
  let (in_start, in_end) = combined_range(&inputs);
  expanded = overlap_in_level(levels, level + 1, in_start, in_end);

  Some(CompactionTask {
    inputs,
    level,
    output_level: level + 1,
    expanded_inputs: expanded,
    is_trivial_move: false,
  })
}
```

- [ ] **Step 5: Update `pick_level0()` to detect trivial move**

When all L0 files together have no overlap with L1, mark as trivial move:

```rust
fn pick_level0(&self, levels: &[Vec<Arc<SSTableReader>>]) -> Option<CompactionTask> {
  if levels[0].is_empty() {
    return None;
  }
  let inputs = levels[0].clone();
  let (in_start, in_end) = combined_range(&inputs);
  let expanded = overlap_in_level(levels, 1, in_start, in_end);
  let is_trivial = expanded.is_empty();
  Some(CompactionTask {
    inputs,
    level: 0,
    output_level: 1,
    expanded_inputs: expanded,
    is_trivial_move: is_trivial,
  })
}
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cd aidb && cargo test --lib engine::compaction::picker::tests -- --test-threads=1
```

Expected: all existing tests pass + 3 new tests pass.

- [ ] **Step 7: Commit**

```bash
cd aidb && git add src/engine/compaction/picker.rs docs/superpowers/specs/2026-05-29-trivial-move-design.md && git commit -m "feat: add trivial move detection to CompactionPicker

Add is_trivial_move field to CompactionTask. pick_level_n() returns
trivial move when the seed file has no overlapping files in the target
level. pick_level0() also detects the all-L0-no-overlap case.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: Implement trivial move fast path in run_compaction_once

**Files:**
- Modify: `src/engine/db/inner.rs:666-751`

- [ ] **Step 1: Write the failing integration test**

Add to `tests/engine/compaction.rs`:

```rust
#[test]
fn test_trivial_move_promotes_file_without_rewrite() {
  let dir = tempdir().unwrap();
  let mut opts = Options::for_testing();
  opts.background_compaction = true;
  opts.max_bytes_for_level_base = 1000;   // small target so L1 triggers compaction
  opts.max_bytes_for_level_multiplier = 10;
  opts.level0_compaction_trigger = 2;
  let db = Arc::new(DB::open(dir.path(), opts).unwrap());

  // Write one range of keys so L1 files don't overlap with L2 seed
  for i in 0..5 {
    let key = format!("k{i:04}");
    db.put(key.as_bytes(), b"v").unwrap();
    db.flush().unwrap();
  }
  db.drain_compactions().unwrap();

  // Now write more data to push into L1 again (different key range, no overlap with first)
  for i in 100..105 {
    let key = format!("k{i:04}");
    db.put(key.as_bytes(), b"v").unwrap();
    db.flush().unwrap();
  }
  db.drain_compactions().unwrap();

  // Verify data is still correct after trivial moves
  for i in 0..5 {
    let key = format!("k{i:04}");
    assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"v".to_vec()), "missing key {key}");
  }
  for i in 100..105 {
    let key = format!("k{i:04}");
    assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"v".to_vec()), "missing key {key}");
  }
  db.close().unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd aidb && cargo test --test engine compaction::test_trivial_move_promotes_file_without_rewrite -- --test-threads=1 2>&1 | head -30
```

Expected: fails (probably with assertion error or panic from the picker now returning trival-move tasks that `run_compaction_once` doesn't handle).

- [ ] **Step 3: Implement trivial move fast path in `run_compaction_once`**

In `inner.rs`, after the picker returns a task and before the `CompactionJob::new(...).run(...)` call, add the trivial move bypass:

```rust
// existing pick code...
let task = match self.compaction_picker.pick_compaction(&levels) {
  Some(t) => t,
  None => return Ok(false),
};

// --- TRIVIAL MOVE FAST PATH ---
if task.is_trivial_move {
  return self.run_trivial_move(task);
}
// --- END TRIVIAL MOVE ---

// existing CompactionJob code...
```

And add the `run_trivial_move` method to `DB`:

```rust
/// Handle a trivial move: promote SST(s) to the next level without rewriting.
fn run_trivial_move(&self, task: CompactionTask) -> Result<bool> {
  let file_number = task.inputs[0].file_number();
  let old_path = sstable_path(&self.path, file_number, task.level);
  let new_path = sstable_path(&self.path, file_number, task.output_level);

  // Rename the SST file to reflect the new level
  std::fs::rename(&old_path, &new_path).map_err(|e| {
    Error::Io(std::io::Error::new(e.kind(), format!(
      "trivial move rename failed: {old_path:?} -> {new_path:?}: {e}"
    )))
  })?;

  // Re-open at the new path (file content unchanged, just new level in name)
  let reader = Arc::new(SSTableReader::open(
    &new_path,
    Some(Arc::clone(&self.block_cache)),
  )?);

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
  Ok(true)
}
```

Then in the existing compaction path, the old file deletion loop at lines 744-751 should NOT delete the moved file. Add a check:

```rust
// (existing code, no change needed here for merge path — trivial moves
//  return early via run_trivial_move and never reach this deletion)
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd aidb && cargo test --test engine compaction::test_trivial_move_promotes_file_without_rewrite -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Run full compaction test suite**

```bash
cd aidb && cargo test --test engine compaction -- --test-threads=1
```

Expected: all existing compaction tests still pass + new test passes.

- [ ] **Step 6: Commit**

```bash
cd aidb && git add src/engine/db/inner.rs tests/engine/compaction.rs && git commit -m "feat: implement trivial move fast path in compaction

When CompactionTask.is_trivial_move is true, skip the full merge-dedup-
rewrite pipeline. Instead, rename the SST file to the target level and
update metadata (VersionSet + in-memory sstables list). This saves CPU
and I/O when the source level's file has no key-range overlap with the
target level.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 3: Verify full test suite + clippy + fmt

- [ ] **Step 1: Run all tests**

```bash
cd aidb && cargo test --test engine -- --test-threads=1 && cargo test --lib -- --test-threads=1
```

Expected: all passing.

- [ ] **Step 2: Run clippy and fmt**

```bash
cd aidb && RUSTFLAGS='-D warnings' cargo clippy --all-targets 2>&1 && cargo fmt --check
```

Expected: clean.

- [ ] **Step 3: Commit any fixes**

```bash
cd aidb && git add -u && git commit -m "chore: fix clippy/fmt after trivial move" 2>/dev/null || echo "nothing to fix"
```

- [ ] **Step 4: Final sign-off**

```bash
cd aidb && git log --oneline -5
```
