# P0 Performance Optimizations for AiDb & AiKv

Date: 2026-04-30

## Overview

Three P0 optimizations to eliminate redundant I/O and ensure predictable read performance after restart.

---

## 1. TTL Embedded in StoredValue (AiKv)

### Problem

TTL expiration is implemented as a separate metadata key (`__exp__:<key>`) per data key. Every `get_value()` performs:

1. `AiDb.get(key)` — read value blob
2. `AiDb.get(__exp__:key)` — read expiration timestamp
3. Comparison + conditional delete if expired

This doubles read traffic and write traffic (each `SET EX/PX` writes two keys).

### Solution

The `StoredValue` struct already carries `expires_at: Option<u64>`. Serialize it together with the value type in one bincode blob. The `__exp__:*` keys and `is_expired()` / `expiration_key()` helpers are deleted.

### Changes

| File | Change |
|------|--------|
| `src/storage/aidb_adapter.rs` | Remove `expiration_key()`, `is_expired()`, `set_expiration()`; inline TTL check into `get_value()` from deserialized `StoredValue.expires_at` |
| `src/storage/memory_adapter.rs` | Already stores `expires_at` in `StoredValue` — no adapter change needed, but verify TTL path in memory mode |
| Tests | Remove __exp__-specific tests; verify TTL still enforced correctly |

### Compatibility — Dual-Read Migration

Old data has `expires_at: None` in the serialized blob (TTL enforced solely by `__exp__:*` keys).
After the change, the `__exp__:` read path is kept as a **fallback** during transition:

```
get_value(key):
  blob = AiDb.get(key)
  if blob:
    value = deserialize(blob)
    if value.expires_at is Some:
      check expiry normally
    else:
      # Fallback: check legacy __exp__:key
      expire_bytes = AiDb.get(__exp__:key)
      if expire_bytes and expired:
        delete(key) and return None
  return value
```

New writes only embed `expires_at` in the blob — `__exp__:` is no longer written.
The fallback can be removed once all old entries are naturally overwritten or expired.

### Risk

Low. Dual-read ensures zero data loss during transition. Single-write means old `__exp__:` entries are cleaned up lazily on access (same as current lazy expiry).

---

## 2. Compaction Metadata Persistence — File Name Level Encoding (AiDb)

### Problem

On restart, `DB::open()` loads every `.sst` file into Level 0:

```rust
sstables[0].push(Arc::new(reader));
```

This means:
- All compaction effort is lost
- Read amplification spikes until compaction re-runs
- Under heavy write load, the L0→L1 compaction storm can stall writes

### Solution

Encode the compaction level into the SSTable file name: `<filenum>_L<N>.sst` (e.g. `000123_L5.sst`). Recovery parses the level from the filename and places each SSTable into its correct level in one pass.

#### File Name Format

Before: `000123.sst`
After:  `000123_L5.sst`  (means file number 123, Level 5)

Files from a fresh flush (no compaction yet) go to `L0`.

### Changes

| File | Change |
|------|--------|
| `src/sstable/builder.rs` (or a shared util) | Add `sstable_path(dir, filenum, level)` function that produces the new format |
| `src/lib.rs` — `DB::open()` | Replace flat load into `sstables[0]` with level-aware dispatch |
| `src/compaction/merge.rs` / `CompactionJob` | Pass target level to the builder so output files get the correct name |
| `src/sstable/reader.rs` | Export a `parse_level_from_filename()` helper |

### Backward Compatibility

Accept both `NNNNN_L<N>.sst` and bare `NNNNN.sst` (legacy). Legacy files land in L0 and will be re-compacted normally.

### Risk

Low. Deterministic filename parsing; no state machine or crash-recovery concern.

---

## 3. Disable AiDb WAL in Cluster Mode (AiKv)

### Problem

Cluster-mode write path:

```
Client → Raft Log (persisted across quorum)
       → Apply to ShardedStateMachine
       → AiDb.put()
       → AiDb WAL.append() + optional sync()   ← REDUNDANT
```

Raft consensus already guarantees durability. AiDb's WAL is redundant and adds an extra serialization + disk write.

### Solution

When the storage engine is `StorageEngine::ClusterRaft`, open AiDb instances with `use_wal(false)`.

### Safety Argument

Raft replays un-applied log entries after a crash; the state machine (AiDb) is treated as ephemeral. As long as the Raft log is intact, no data is lost. This matches Production Practice used by TiKV, CockroachDB, and etcd.

### Changes

| File | Change |
|------|--------|
| `src/storage/cluster_raft.rs` or `ShardedStateMachine::new()` | Accept a `use_wal: bool` parameter; pass `Options::default().use_wal(false)` to each AiDb instance |
| `src/server/mod.rs` | In `initialize_cluster()`, pass `use_wal = false` to the cluster storage |

### Risk

Medium. Requires careful validation: if `ShardedStateMachine` is also used in non-cluster tests, the default must remain `use_wal(true)`. Add a test that verifies the WAL is truly off in cluster mode (check file count).

---

## Implementation Order

| Step | Item | Depends On | Est. Effort |
|------|------|------------|-------------|
| 1 | TTL embedded in StoredValue | — | 4h |
| 2 | Compaction file name level encoding | — | 4h |
| 3 | Cluster mode disable WAL | — | 2h |

All three are independent and can be implemented in any order.
