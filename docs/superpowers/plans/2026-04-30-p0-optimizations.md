# P0 Optimizations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three independent P0 optimizations for AiDb/AiKv — TTL embedded in StoredValue, SSTable filename level encoding, and cluster-mode WAL disable.

**Architecture:** All three changes are self-contained and can be implemented in any order. TTL optimization is entirely in AiKv's `aidb_adapter.rs`. Compaction metadata change is entirely in AiDb's `lib.rs`, `sstable/builder.rs`, and `compaction/mod.rs`. Cluster WAL disable touches AiDb's `sharded_state_machine.rs` and AiKv's `server/mod.rs`.

**Tech Stack:** Rust, AiDb (LSM-Tree engine), AiKv (Redis protocol layer)

---

### Task 1: TTL Embedded in StoredValue — AiKv aidb_adapter.rs

**Files:**
- Modify: `AiKv/src/storage/aidb_adapter.rs`
- Test: `AiKv/tests/` (existing TTL tests)

**Problem:** TTL uses `__exp__:<key>` metadata keys, doubling reads/writes.

**Solution:** Set `expires_at` on `StoredValue` before serialization. Remove `__exp__:` writes entirely. Add dual-read fallback for old data.

- [ ] **Step 1: Identify all `__exp__:` and `expiration_key()` call sites**

```
sites in aidb_adapter.rs:
  expiration_key()    — helper (line 146)
  is_expired()        — helper (line 121)
  get_value()         — calls is_expired() + deletes exp key on expiry (line 205-213)
  set_value()         — writes __exp__:key on TTL (line 268-270)
  delete_and_get()    — deletes __exp__:key (line 368-369)
  write_batch()       — deletes __exp__:key in batch (line 432-433)
  set_expire_at_in_db() — writes __exp__:key (line 572-573)
  persist_in_db()     — deletes __exp__:key (line 601-606)
```

- [ ] **Step 2: Modify `get_value()` — inline TTL check from blob, add fallback**

Replace:
```rust
// Old: separate __exp__:key read
Some(serialized) => {
    if self.is_expired(db, key_bytes)? {
        db.delete(key_bytes)?;
        let expire_key = Self::expiration_key(key_bytes);
        db.delete(&expire_key)?;
        return Ok(None);
    }
    let serializable: SerializableStoredValue = bincode::deserialize(&serialized)?;
    let value = StoredValue {
        value: serializable.into(),
        expires_at: None,  // TTL was separate
    };
    Ok(Some(value))
}
```

With:
```rust
Some(serialized) => {
    let serializable: SerializableStoredValue = bincode::deserialize(&serialized)?;
    let expires_at = serializable.expires_at;  // now embedded
    let value = StoredValue { value: serializable.into(), expires_at };

    if let Some(exp) = expires_at {
        if current_time_ms() >= exp {
            db.delete(key_bytes)?;
            return Ok(None);
        }
    } else {
        // Dual-read fallback: check legacy __exp__:key
        let expire_key = Self::expiration_key(key_bytes);
        if let Some(expire_bytes) = db.get(&expire_key)? {
            if expire_bytes.len() == 8 {
                let expire_at = u64::from_le_bytes(/*...*/);
                if current_time_ms() >= expire_at {
                    db.delete(key_bytes)?;
                    db.delete(&expire_key)?;
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(value))
}
```

- [ ] **Step 3: Modify `set_value()` — embed expires_at in blob, remove `__exp__:` write**

In `set_value()` (around line 255-275), find:
```rust
if let Some(expires_at) = value.expires_at() {
    let expire_key = Self::expiration_key(key_bytes);
    db.put(&expire_key, &expires_at.to_le_bytes())?;
}
```
Delete these lines. The `expires_at` is already in the `SerializableStoredValue` and will be written in the `bincode::serialize(&serializable)` call earlier in the function.

- [ ] **Step 4: Modify `delete_and_get()` — remove `__exp__:` deletion**

Find (around line 365-370):
```rust
let expire_key = Self::expiration_key(key_bytes);
let _ = db.delete(&expire_key);
```
Delete these lines.

- [ ] **Step 5: Modify `write_batch()` — remove `__exp__:` deletion**

Find (around line 428-435):
```rust
let expire_key = Self::expiration_key(key_bytes);
batch.delete(&expire_key);
```
Delete these lines.

- [ ] **Step 6: Modify `set_expire_at_in_db()` — embed expires_at, remove `__exp__:` write**

Replace: write to `__exp__:key` → instead read current blob, update `expires_at`, re-serialize. Lines 556-573 currently do:
```rust
stored_value.set_expiration(Some(timestamp_ms));
// ... reserialize ...
let expire_key = Self::expiration_key(key_bytes);
db.put(&expire_key, &timestamp_ms.to_le_bytes())?;
```
Delete the last two lines (`expire_key` + `db.put`). The `expires_at` is already in the re-serialized blob.

- [ ] **Step 7: Modify `persist_in_db()` — remove `__exp__:` deletion**

Find (around line 595-610):
```rust
let expire_key = Self::expiration_key(key_bytes);
let _ = db.delete(&expire_key);
```
Delete these lines.

- [ ] **Step 8: Remove unused helpers**

Delete `expiration_key()` and `is_expired()` functions entirely.

- [ ] **Step 9: Check `SerializableStoredValue` has `expires_at` field**

In `memory_adapter.rs`, verify `SerializableStoredValue` includes `expires_at`. If not, add it so bincode serialization includes the field:

```rust
#[derive(Serialize, Deserialize)]
struct SerializableStoredValue {
    value_type: SerializableValueType,
    expires_at: Option<u64>,
}
```

- [ ] **Step 10: Run existing TTL tests**

```bash
cd /root/code/wiqun/AiKv && cargo test -- test_ttl test_expire test_persist 2>&1
```
Expected: ALL tests pass. If any fail, debug the fallback path.

- [ ] **Step 11: Run full test suite**

```bash
cd /root/code/wiqun/AiKv && cargo test 2>&1
```
Expected: all tests pass.

- [ ] **Step 12: Commit**

```bash
cd /root/code/wiqun/AiKv
git add src/storage/aidb_adapter.rs src/storage/memory_adapter.rs
git commit -m "perf: embed TTL in StoredValue instead of separate __exp__: keys

- Remove expiration_key() and is_expired() helpers
- Remove all __exp__: metadata key writes and deletes
- Add dual-read fallback for old data with separate TTL keys
- Eliminates 2x read/write amplification for TTL keys"
```

---

### Task 2: SSTable Filename Level Encoding — AiDb

**Files:**
- Modify: `AiDb/src/lib.rs`
- Modify: `AiDb/src/compaction/mod.rs`
- Modify: `AiDb/src/sstable/builder.rs`
- Modify: `AiDb/src/sstable/reader.rs`

**Problem:** All SSTables loaded into L0 on restart, losing compaction state.

**Solution:** Encode level in filename as `NNNNN_L<N>.sst`. Parse on recovery.

- [ ] **Step 1: Add level-encoded path helper in `sstable/builder.rs` or a shared location**

Add at end of `sstable/builder.rs` or as a `pub` function:

```rust
/// Build an SSTable path with level encoding.
/// Example: /data/000123_L5.sst (file 123, level 5)
pub fn sstable_path(dir: &std::path::Path, file_number: u64, level: usize) -> std::path::PathBuf {
    dir.join(format!("{:06}_L{}.sst", file_number, level))
}

/// Parse level from an SSTable filename.
/// Returns level and file number, or None for legacy format.
pub fn parse_sstable_filename(filename: &str) -> Option<(u64, usize)> {
    if let Some(rest) = filename.strip_suffix(".sst") {
        if let Some(lpos) = rest.rfind("_L") {
            let num_part = &rest[..lpos];
            let level_part = &rest[lpos+2..];
            if let (Ok(num), Ok(level)) = (num_part.parse::<u64>(), level_part.parse::<usize>()) {
                return Some((num, level));
            }
        }
        // Legacy: NNNNNN.sst → level 0
        if let Ok(num) = rest.parse::<u64>() {
            return Some((num, 0));
        }
    }
    None
}
```

- [ ] **Step 2: Update `flush` path in `lib.rs` — write to L0 filename**

In `DB::flush_memtable_to_sstable()` (line 996), change:
```rust
let sstable_path = self.path.join(format!("{:06}.sst", file_number));
```
To:
```rust
let sstable_path = crate::sstable::sstable_path(&self.path, file_number, 0);
```

- [ ] **Step 3: Update compaction output in `compaction/mod.rs` — write to target level filename**

In `CompactionJob::run()` (line 76), change:
```rust
let output_path = self.db_path.join(format!("{:06}.sst", file_number));
```
To:
```rust
let output_path = crate::sstable::sstable_path(&self.db_path, file_number, self.output_level);
```

- [ ] **Step 4: Make `parse_sstable_filename` accessible from `lib.rs`**

Either make `sstable_path` and `parse_sstable_filename` public on the `sstable` module, or create a small `util.rs` that both can import.

Update `sstable/mod.rs`:
```rust
pub use builder::{SSTableBuilder, sstable_path, parse_sstable_filename};
```

- [ ] **Step 5: Update `DB::open()` in `lib.rs` — level-aware SSTable loading**

Replace the current flat load (lines 326-356):
```rust
// Scan directory for SSTable files (*.sst)
if path.exists() {
    if let Ok(entries) = std::fs::read_dir(&path) {
        let mut sst_files: Vec<(u64, usize, std::path::PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            if let Some(filename) = entry.file_name().to_str() {
                if let Some((num, level)) = parse_sstable_filename(filename) {
                    sst_files.push((num, level, entry.path()));
                }
            }
        }
        sst_files.sort_by_key(|&(num, _, _)| num);
        for (_, level, sst_path) in sst_files {
            match SSTableReader::open_with_cache(&sst_path, Some(Arc::clone(&block_cache))) {
                Ok(reader) => {
                    if level < sstables.len() {
                        sstables[level].push(Arc::new(reader));
                    } else {
                        sstables[0].push(Arc::new(reader)); // fallback
                    }
                }
                Err(e) => log::warn!("Failed to load SSTable {:?}: {}", sst_path, e),
            }
        }
    }
}
```

Remove the old `log::info!("Loaded {} SSTables at Level 0", ...)` and replace with:
```rust
for (i, level_ssts) in sstables.iter().enumerate() {
    if !level_ssts.is_empty() {
        log::info!("Loaded {} SSTables at Level {}", level_ssts.len(), i);
    }
}
```

- [ ] **Step 6: Update the `sstable_path` usage in tests**

In `compaction/merge.rs` tests (line 133):
```rust
let path = dir.path().join(format!("{:06}.sst", file_num));
```
This is test code. It creates files for test — can keep legacy format since `parse_sstable_filename` handles both formats. But update to use the helper for consistency:
```rust
let path = crate::sstable::sstable_path(dir.path(), file_num, 0);
```

- [ ] **Step 7: Run AiDb tests**

```bash
cd /root/code/wiqun/AiDb && cargo test 2>&1
```
Expected: all tests pass.

- [ ] **Step 8: Verify recovery loads correct levels**

The existing recovery tests should now load files at their proper levels. If there are tests that check `sstables.len()` or verify L0 counts, they may need adjustment.

- [ ] **Step 9: Commit**

```bash
cd /root/code/wiqun/AiDb
git add src/sstable/builder.rs src/sstable/mod.rs src/lib.rs src/compaction/mod.rs
git commit -m "perf: encode compaction level in SSTable filename

- Add sstable_path() and parse_sstable_filename() helpers
- Flush writes as _L0.sst; compaction writes as _L<N>.sst
- Recovery parses level from filename, avoiding full L0 reload
- Legacy format (bare NNNNNN.sst) falls back to level 0"
```

---

### Task 3: Disable AiDb WAL in Cluster Mode

**Files:**
- Modify: `AiDb/src/cluster/sharded_state_machine.rs`
- Modify: `AiKv/src/server/mod.rs`

**Problem:** Cluster mode writes go through Raft log (persisted) AND AiDb WAL (redundant).

**Solution:** Pass `use_wal(false)` when opening AiDb instances in cluster mode.

- [ ] **Step 1: Read current ShardedStateMachine options flow**

In `AiDb/src/cluster/sharded_state_machine.rs`, verify that `ShardedStateMachine` stores `Options` and passes them to `DB::open()` at lines 119-122:
```rust
let db = DB::open(&db_path, self.options.clone())?;
```

- [ ] **Step 2: Verify AiKv initialization flow**

In `AiKv/src/server/mod.rs`, around line 164, check the flow:
```rust
let multi_raft = MultiRaftNode::new(...)
// ...
multi_raft.init_router()?;
// ...
self.storage = StorageEngine::new_cluster_raft(multi_arc, storage_databases);
```

The `ShardedStateMachine` is created inside `MultiRaftNode::new()`. The `Options` defaults are used.

- [ ] **Step 3: Add `use_wal` parameter to `ShardedStateMachine`**

In `AiDb/src/cluster/sharded_state_machine.rs`, add field and constructor param:

```rust
pub struct ShardedStateMachine {
    // ... existing fields ...
    use_wal: bool,
}

// In new() and with_router(), add use_wal: bool parameter:
pub fn new<P: Into<PathBuf>>(base_dir: P, options: Options, use_wal: bool) -> Self {
    Self {
        // ... existing ...
        use_wal,
    }
}
```

In `get_or_create_db()` around line 119-122, apply the flag before opening:
```rust
let opts = if !self.use_wal {
    Options { use_wal: false, ..self.options.clone() }
} else {
    self.options.clone()
};
let db = DB::open(&db_path, opts)?;
```

- [ ] **Step 4: Find and update callers of `ShardedStateMachine`**

In `AiDb/src/cluster/multi_raft_node.rs`, find where `ShardedStateMachine` is constructed and pass `use_wal: true` for default, or `false` for cluster mode:

```rust
// In MultiRaftNode::new()
let state_machine = ShardedStateMachine::new(
    base_dir.join("groups"),
    options.clone(),
    true,  // use_wal = true for non-cluster
);
```

In `AiKv/src/server/mod.rs` or wherever cluster mode path does `new_cluster_raft()`, the `ShardedStateMachine` needs to be constructed with `use_wal: false`. This may require threading through `MultiRaftNode`.

- [ ] **Step 4b (alternative): Simpler approach — modify `Options` before passing**

If the call path is complex, a simpler approach is to modify the `Options` object before it reaches `ShardedStateMachine`:

In `AiKv/src/server/mod.rs`, after `multi_raft = MultiRaftNode::new(...)`:

```rust
// In cluster mode, AiDb WAL is redundant (Raft log ensures durability)
multi_raft.set_wal_enabled(false);
```

This requires adding a `set_wal_enabled(bool)` method on `MultiRaftNode` that updates the options before state machine creation. This is the least invasive approach.

- [ ] **Step 5: Add a test verifying WAL files are not created in cluster mode**

In `ShardedStateMachine` tests, after creating with `use_wal: false`, verify no WAL files exist in the group directories:

```rust
#[test]
fn test_cluster_mode_no_wal() {
    let dir = TempDir::new().unwrap();
    let sm = ShardedStateMachine::new(dir.path(), Options::default(), false);
    let db = sm.get_or_create_db(1).unwrap();
    let wal_dir = dir.path().join("1");
    let has_wal = std::fs::read_dir(&wal_dir).unwrap().any(|e| {
        e.unwrap().file_name().to_str().unwrap_or("").contains(".wal")
    });
    assert!(!has_wal, "WAL files should not exist in cluster mode");
}
```

Run:
```bash
cd /root/code/wiqun/AiDb && cargo test -- cluster_mode_no_wal 2>&1
```
Expected: PASS.

- [ ] **Step 6: Run full AiDb and AiKv test suites**

```bash
cd /root/code/wiqun/AiDb && cargo test 2>&1
cd /root/code/wiqun/AiKv && cargo test 2>&1
```
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /root/code/wiqun/AiDb
git add src/cluster/sharded_state_machine.rs
git commit -m "perf: disable AiDb WAL in cluster mode (Raft log is authoritative)

- Add use_wal parameter to ShardedStateMachine
- Cluster-mode AiDb instances skip WAL writes
- Raft log ensures durability; AiDb WAL is redundant"

cd /root/code/wiqun/AiKv
git add src/server/mod.rs
git commit -m "perf: disable AiDb WAL in cluster mode via ShardedStateMachine"
```

---

## Spec Coverage Check

| Spec Requirement | Task |
|-----------------|------|
| TTL embedded in StoredValue blob | Task 1, Steps 2-8 |
| Dual-read fallback for old __exp__: keys | Task 1, Step 2 (fallback in get_value) |
| SSTable filename level encoding | Task 2, Step 1 (helpers) |
| Flush writes _L0.sst | Task 2, Step 2 |
| Compaction writes _L<N>.sst | Task 2, Step 3 |
| Recovery loads correct levels | Task 2, Step 5 |
| Legacy filename backward compat | Task 2, Step 1 (None return → level 0 fallback) |
| Cluster WAL disable | Task 3, Steps 3-4 |
| Cluster mode safety verification | Task 3, Step 5 |
