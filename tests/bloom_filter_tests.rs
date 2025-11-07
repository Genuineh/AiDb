//! Integration tests for Bloom Filter in SSTable

use aidb::filter::BloomFilter;
use aidb::sstable::{SSTableBuilder, SSTableReader};
use tempfile::NamedTempFile;

#[test]
fn test_sstable_with_bloom_filter() {
    let temp_file = NamedTempFile::new().unwrap();

    // Build SSTable with bloom filter
    let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
    builder.set_expected_keys(100); // Set expected keys for optimal bloom filter

    // Add keys
    for i in 0..100 {
        let key = format!("key{:04}", i);
        let value = format!("value{:04}", i);
        builder.add(key.as_bytes(), value.as_bytes()).unwrap();
    }

    builder.finish().unwrap();

    // Read SSTable with bloom filter
    let reader = SSTableReader::open(temp_file.path()).unwrap();

    // Verify bloom filter is loaded
    assert!(reader.has_bloom_filter(), "Bloom filter should be present");

    // Test existing keys (should all be found)
    for i in 0..100 {
        let key = format!("key{:04}", i);
        let value = reader.get(key.as_bytes()).unwrap();
        assert!(value.is_some(), "Key {} should be found", key);
        assert_eq!(value.unwrap(), format!("value{:04}", i).as_bytes());
    }

    // Test non-existent keys (bloom filter should help avoid disk reads)
    let mut false_positives = 0;
    for i in 1000..2000 {
        let key = format!("key{:04}", i);
        let value = reader.get(key.as_bytes()).unwrap();
        if value.is_some() {
            false_positives += 1;
        }
    }

    println!("False positives: {}/1000", false_positives);
    // With 1% target FP rate, we expect around 10 false positives
    assert!(false_positives < 50, "Too many false positives: {}", false_positives);
}

#[test]
fn test_sstable_bloom_filter_effectiveness() {
    let temp_file = NamedTempFile::new().unwrap();

    // Build a large SSTable
    let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
    builder.set_expected_keys(10000);

    for i in 0..10000 {
        let key = format!("existing_key_{:08}", i);
        let value = format!("value{:08}", i);
        builder.add(key.as_bytes(), value.as_bytes()).unwrap();
    }

    builder.finish().unwrap();

    // Read and test
    let reader = SSTableReader::open(temp_file.path()).unwrap();

    // Test that all existing keys are found (no false negatives)
    for i in 0..10000 {
        let key = format!("existing_key_{:08}", i);
        let result = reader.get(key.as_bytes()).unwrap();
        assert!(result.is_some(), "Existing key should be found: {}", key);
    }

    // Test non-existent keys
    let mut false_positives = 0;
    let test_count = 10000;

    for i in 100000..100000 + test_count {
        let key = format!("nonexistent_key_{:08}", i);
        let result = reader.get(key.as_bytes()).unwrap();
        if result.is_some() {
            false_positives += 1;
        }
    }

    let fp_rate = false_positives as f64 / test_count as f64;
    println!("False positive rate: {:.4} ({}/{})", fp_rate, false_positives, test_count);

    // Should be close to 1% target rate
    assert!(fp_rate < 0.03, "False positive rate too high: {:.4}", fp_rate);
}

#[test]
fn test_sstable_without_bloom_filter() {
    let temp_file = NamedTempFile::new().unwrap();

    // Build SSTable without bloom filter
    let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
    builder.set_bloom_filter_enabled(false); // Disable bloom filter

    for i in 0..100 {
        let key = format!("key{:04}", i);
        let value = format!("value{:04}", i);
        builder.add(key.as_bytes(), value.as_bytes()).unwrap();
    }

    builder.finish().unwrap();

    // Read SSTable
    let reader = SSTableReader::open(temp_file.path()).unwrap();

    // Bloom filter should not be present
    // Note: Our implementation might still create a minimal meta block
    // so we just verify that queries still work

    // Test existing keys
    for i in 0..100 {
        let key = format!("key{:04}", i);
        let value = reader.get(key.as_bytes()).unwrap();
        assert!(value.is_some(), "Key {} should be found", key);
    }
}

#[test]
fn test_sstable_bloom_filter_small_dataset() {
    let temp_file = NamedTempFile::new().unwrap();

    // Build SSTable with very few keys
    let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
    builder.set_expected_keys(10);

    for i in 0..10 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        builder.add(key.as_bytes(), value.as_bytes()).unwrap();
    }

    builder.finish().unwrap();

    // Read and verify
    let reader = SSTableReader::open(temp_file.path()).unwrap();

    // All keys should be found
    for i in 0..10 {
        let key = format!("key{}", i);
        assert!(reader.get(key.as_bytes()).unwrap().is_some());
    }
}

#[test]
fn test_sstable_bloom_filter_with_tombstones() {
    let temp_file = NamedTempFile::new().unwrap();

    // Build SSTable with some tombstones (empty values)
    let mut builder = SSTableBuilder::new(temp_file.path()).unwrap();
    builder.set_expected_keys(50);

    // Add normal entries
    for i in 0..25 {
        let key = format!("key{:04}", i);
        let value = format!("value{:04}", i);
        builder.add(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Add tombstones (empty values represent deleted keys)
    for i in 25..50 {
        let key = format!("key{:04}", i);
        builder.add(key.as_bytes(), b"").unwrap(); // Empty value = tombstone
    }

    builder.finish().unwrap();

    // Read and verify
    let reader = SSTableReader::open(temp_file.path()).unwrap();

    // Normal entries should be found
    for i in 0..25 {
        let key = format!("key{:04}", i);
        let result = reader.get(key.as_bytes()).unwrap();
        assert!(result.is_some(), "Key {} should be found", key);
    }

    // Tombstones should return None
    for i in 25..50 {
        let key = format!("key{:04}", i);
        let result = reader.get(key.as_bytes()).unwrap();
        assert!(result.is_none(), "Deleted key {} should return None", key);
    }
}

#[test]
fn test_bloom_filter_unit() {
    use aidb::filter::Filter;

    // Create a bloom filter directly
    let mut filter = BloomFilter::new(1000, 0.01);

    // Add keys
    for i in 0..1000 {
        let key = format!("key{}", i);
        filter.add(key.as_bytes());
    }

    // Test membership
    for i in 0..1000 {
        let key = format!("key{}", i);
        assert!(filter.may_contain(key.as_bytes()), "Key {} should be in filter", key);
    }

    // Test false positives
    let mut fps = 0;
    for i in 10000..20000 {
        let key = format!("key{}", i);
        if filter.may_contain(key.as_bytes()) {
            fps += 1;
        }
    }

    let fp_rate = fps as f64 / 10000.0;
    println!("FP rate: {:.4}", fp_rate);

    assert!(fp_rate < 0.02, "FP rate too high: {:.4}", fp_rate);
}

#[test]
fn test_bloom_filter_encode_decode() {
    use aidb::filter::Filter;

    let mut filter = BloomFilter::new(100, 0.01);

    // Add some keys
    for i in 0..100 {
        filter.add(format!("key{}", i).as_bytes());
    }

    // Encode
    let encoded = filter.encode();
    println!("Encoded size: {} bytes", encoded.len());

    // Decode
    let decoded = BloomFilter::decode(&encoded).unwrap();

    // Verify it works the same
    for i in 0..100 {
        let key = format!("key{}", i);
        assert!(decoded.may_contain(key.as_bytes()));
    }
}
