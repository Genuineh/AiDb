# AiKv Integration Notes

## Issue: Key Exists After Delete

### Problem
Tests in AiKv (`TestKeyIsExistsAsync`) fail because deleted keys still appear to exist.

### Root Cause Analysis

AiDb v0.6.3 fixed the tombstone handling:
- `DB::get()` correctly returns `None` for deleted keys
- All AiDb tests pass

The issue is likely in **AiKv's integration layer**:

### Possible Causes in AiKv

1. **Cache Not Invalidated**
   ```rust
   // Bad: Cache not cleared on delete
   fn delete(key: &str) {
       self.db.delete(key)?;
       // Missing: self.cache.remove(key);
   }
   
   fn exists(key: &str) -> bool {
       // Returns stale cached value
       self.cache.contains_key(key) || self.db.get(key)?.is_some()
   }
   ```

2. **Incorrect Exists Implementation**
   ```rust
   // Bad: Checking SSTable directly without going through DB::get()
   fn exists(key: &str) -> bool {
       // This bypasses tombstone logic
       self.internal_check(key)
   }
   
   // Good: Use DB::get()
   fn exists(key: &str) -> bool {
       self.db.get(key)?.is_some()
   }
   ```

3. **Empty Value Confusion**
   ```rust
   // Bad: Treating empty Vec as "exists with empty value"
   fn exists(key: &str) -> bool {
       match self.db.get(key)? {
           Some(value) => true,  // Wrong if value is empty!
           None => false,
       }
   }
   
   // Note: In AiDb 0.6.3, empty values ARE tombstones
   // db.put(key, b"") is equivalent to db.delete(key)
   ```

### Fix for AiKv

**Option 1: Use DB::get() for exists check**
```rust
pub fn exists(&self, key: &[u8]) -> Result<bool> {
    Ok(self.db.get(key)?.is_some())
}
```

**Option 2: Clear cache on delete**
```rust
pub fn delete(&self, key: &[u8]) -> Result<()> {
    self.db.delete(key)?;
    self.cache.remove(key);  // Clear cache
    Ok(())
}
```

**Option 3: Never store empty values**
```rust
pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidArgument("Value cannot be empty"));
    }
    self.db.put(key, value)
}
```

### Testing AiKv Integration

Add this test to AiKv:

```rust
#[test]
async fn test_delete_and_exists() {
    let kv = KvStore::new()?;
    
    // Set key
    kv.set("key", "value").await?;
    assert!(kv.exists("key").await?);
    
    // Delete key
    kv.delete("key").await?;
    
    // Key should not exist
    assert!(!kv.exists("key").await?, "Key should not exist after delete");
}
```

### Important Notes

1. **Empty Values**: In AiDb 0.6.3+, empty byte slices (`b""`) are treated as tombstones (deletions). Do not use empty values for actual data.

2. **Snapshot Isolation**: Snapshots created before a delete will still see the old value (until flush). For consistent reads across deletes, avoid long-lived snapshots.

3. **Cache Consistency**: Any caching layer above AiDb must be invalidated on delete operations.

### Concurrent Clear/Delete Operations

The issue might occur in concurrent scenarios, such as:

**Scenario: Batch Clear (FLUSHDB-like operation)**
```rust
// AiKv clears all keys concurrently
async fn clear_all(&self) -> Result<()> {
    let keys = self.get_all_keys()?;  // Problem: Snapshot of keys
    
    // Parallel delete
    for key in keys {
        self.delete(key)?;
    }
    
    // If cache is checked BEFORE the delete finishes:
    // exists(key) might still return true from cache!
}
```

**Fix: Ensure atomic clear or proper cache invalidation**
```rust
async fn clear_all(&self) -> Result<()> {
    // Option 1: Clear cache first
    self.cache.clear();
    
    let keys = self.get_all_keys()?;
    for key in keys {
        self.db.delete(key)?;
    }
    
    // Option 2: Use WriteBatch for atomicity
    let mut batch = WriteBatch::new();
    for key in keys {
        batch.delete(key);
    }
    self.db.write(batch)?;
}
```

### Verified Concurrent Scenarios

AiDb has been tested for:
- ✅ Concurrent delete of same key by multiple threads
- ✅ Concurrent delete and exists checks
- ✅ Batch clear with concurrent reads
- ✅ Delete with concurrent flush operations
- ✅ Reopen after concurrent clear
- ✅ Redis FLUSHDB-like patterns (parallel delete ranges)

All tests pass - AiDb handles concurrent deletes correctly.

### Verification

Run these tests in AiDb to confirm delete works:
```bash
cd /path/to/AiDb
cargo test --test key_exists_test
cargo test --test tombstone_concurrent_tests
cargo test --test concurrent_delete_clear_test
```

All tests should pass.

### Next Steps for AiKv Team

1. Check `exists()` implementation - ensure it calls `db.get()` and checks for `None`
2. Clear any caches on `delete()` operations
3. Add integration test similar to `test_delete_and_exists` above
4. Ensure no code path stores empty values intentionally
