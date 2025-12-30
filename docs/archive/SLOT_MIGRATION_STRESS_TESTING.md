# Slot Migration Stress Testing Guide

This guide explains how to run stress tests for the online slot migration feature (Phase 5).

## Overview

Slot migration stress tests are designed to validate the robustness and performance of the migration system under heavy load, including:
- High-frequency concurrent migrations
- Large data volumes
- Network failures
- Node crashes during migration
- Memory pressure scenarios

## Running Stress Tests

### Option 1: Manual Trigger via GitHub Actions

The easiest way to run slot migration stress tests is through the GitHub Actions workflow:

1. Go to the **Actions** tab in the GitHub repository
2. Select **"Stress Tests"** workflow
3. Click **"Run workflow"**
4. Choose parameters:
   - **Duration**: 10, 30, 60, or 120 minutes
   - **Test Type**: Select "all" to include migration tests
5. Click **"Run workflow"**

The workflow will automatically run all stress tests including migration-specific scenarios.

### Option 2: Local Execution

To run slot migration stress tests locally:

```bash
# Run all migration stress tests (marked as #[ignore])
cargo test --release --features raft-cluster -- --ignored slot_migration

# Run specific migration stress tests
cargo test --release --features raft-cluster -- --ignored stress_concurrent_migrations
cargo test --release --features raft-cluster -- --ignored stress_large_slot_migration
cargo test --release --features raft-cluster -- --ignored stress_migration_network_failures
```

### Option 3: Custom Stress Test

Create a custom stress test in `tests/stress_tests.rs`:

```rust
#[tokio::test]
#[ignore] // Only run when explicitly requested
async fn stress_custom_migration_scenario() {
    // Your custom test here
}
```

## Migration Stress Test Scenarios

### 1. Concurrent Migrations

**Purpose**: Test multiple slots migrating simultaneously  
**Load**: 10+ concurrent migrations  
**Duration**: 5-10 minutes  
**Success Criteria**: All migrations complete, no data loss

```rust
#[tokio::test]
#[ignore]
async fn stress_concurrent_migrations() {
    // Test concurrent migration of multiple slots
}
```

### 2. Large Data Volume

**Purpose**: Test migration with millions of keys  
**Load**: 1M+ keys per slot  
**Duration**: 15-30 minutes  
**Success Criteria**: All data migrated, acceptable throughput

```rust
#[tokio::test]
#[ignore]
async fn stress_large_slot_migration() {
    // Insert 1M keys, then migrate
}
```

### 3. Network Failures

**Purpose**: Test resilience to network issues  
**Load**: Random network delays and failures  
**Duration**: 10-15 minutes  
**Success Criteria**: Migration completes despite failures, retry logic works

```rust
#[tokio::test]
#[ignore]
async fn stress_migration_network_failures() {
    // Inject network failures during migration
}
```

### 4. Memory Pressure

**Purpose**: Test migration under memory constraints  
**Load**: Limited memory budget  
**Duration**: 10-15 minutes  
**Success Criteria**: No OOM, graceful handling

```rust
#[tokio::test]
#[ignore]
async fn stress_migration_memory_pressure() {
    // Configure small batch sizes and buffers
}
```

### 5. High Write Rate

**Purpose**: Test migration with ongoing writes  
**Load**: 10k+ writes/sec during migration  
**Duration**: 10-15 minutes  
**Success Criteria**: Dual-write works, no data loss

```rust
#[tokio::test]
#[ignore]
async fn stress_migration_with_writes() {
    // Start migration, then bombard with writes
}
```

## Performance Benchmarks

Expected performance metrics for slot migration:

### Small Slots (< 10k keys)
- **Throughput**: 5k-10k keys/sec
- **Duration**: < 5 seconds
- **Memory**: < 50MB
- **CPU**: < 20%

### Medium Slots (10k-100k keys)
- **Throughput**: 3k-5k keys/sec
- **Duration**: 10-30 seconds
- **Memory**: 50-200MB
- **CPU**: 20-40%

### Large Slots (100k-1M keys)
- **Throughput**: 1k-3k keys/sec
- **Duration**: 5-15 minutes
- **Memory**: 200MB-1GB
- **CPU**: 40-60%

### Concurrent Migrations (10 slots)
- **Throughput**: 10k+ keys/sec total
- **Memory**: < 2GB
- **CPU**: 60-80%

## Monitoring During Stress Tests

### Key Metrics to Watch

1. **Migration Progress**
   ```rust
   if let Some(progress) = manager.get_migration_progress(slot) {
       println!("Progress: {:.2}%", progress.progress_pct());
   }
   ```

2. **Migration Metrics**
   ```rust
   let metrics = manager.metrics();
   println!("Migrated: {}", metrics.keys_migrated.load(Ordering::Relaxed));
   println!("Failed: {}", metrics.keys_failed.load(Ordering::Relaxed));
   println!("Rate: {} keys/sec", metrics.current_rate.load(Ordering::Relaxed));
   println!("Success Rate: {:.2}%", metrics.success_rate());
   ```

3. **System Resources**
   ```bash
   # Monitor memory
   watch -n 1 'free -h'
   
   # Monitor CPU
   top -p $(pgrep -f aidb)
   
   # Monitor disk I/O
   iostat -x 1
   ```

## Failure Injection

To test resilience, you can inject failures:

### 1. Network Delays
```rust
// Add delay before key migration
tokio::time::sleep(Duration::from_millis(100)).await;
```

### 2. Random Failures
```rust
use rand::Rng;
if rand::thread_rng().gen_bool(0.01) { // 1% failure rate
    return Err(Error::Internal("Injected failure".into()));
}
```

### 3. Memory Pressure
```rust
// Allocate large buffers to simulate memory pressure
let _pressure = vec![0u8; 100 * 1024 * 1024]; // 100MB
```

## Analyzing Results

After running stress tests, analyze:

1. **Success Rate**: Should be > 99% for stable network
2. **Throughput**: Should meet performance benchmarks
3. **Memory Usage**: Should not grow unboundedly
4. **Error Logs**: Check for unexpected errors
5. **Data Integrity**: Verify all data migrated correctly

## Example: Complete Stress Test Session

```bash
# 1. Build in release mode
cargo build --release --features raft-cluster

# 2. Run concurrent migration stress test
cargo test --release --features raft-cluster -- --ignored stress_concurrent_migrations --nocapture

# 3. Monitor resources in another terminal
watch -n 1 'ps aux | grep aidb'

# 4. Check results
# - Review test output
# - Check for any panics or errors
# - Verify all migrations completed
```

## Troubleshooting

### High Memory Usage
- Reduce `batch_size` in MigrationConfig
- Increase `batch_delay` to reduce pressure
- Check for memory leaks

### Low Throughput
- Increase `batch_size`
- Decrease `batch_delay`
- Check network latency
- Verify disk I/O is not bottleneck

### High Failure Rate
- Increase `max_retries`
- Increase `key_timeout`
- Check network stability
- Verify target group has capacity

### Slow Progress
- Check `rate_limit` setting
- Monitor CPU usage
- Check for lock contention
- Verify no other heavy operations running

## Best Practices

1. **Always test with realistic data sizes** - Don't just test with toy datasets
2. **Include failure scenarios** - Test network failures, node crashes, etc.
3. **Monitor system resources** - Watch for memory leaks, CPU spikes, etc.
4. **Validate data integrity** - Always verify migrated data is correct
5. **Test concurrent scenarios** - Real production has multiple operations
6. **Use realistic configurations** - Test with production-like configs
7. **Document results** - Keep records of stress test results for comparison

## CI/CD Integration

For automated stress testing in CI/CD:

1. **Nightly Runs**: Schedule stress tests to run nightly
2. **PR Validation**: Run quick stress tests on important PRs
3. **Release Testing**: Full stress test suite before releases
4. **Performance Regression**: Compare against baseline metrics

## Conclusion

Stress testing is crucial for validating the slot migration implementation under real-world conditions. Use the GitHub Actions workflow for convenient automated testing, or run tests locally for faster iteration during development.

For questions or issues, refer to the main documentation or open an issue on GitHub.
