use aidb::memtable::MemTable;

#[test]
fn test_memtable_snapshot_isolation() {
    let memtable = MemTable::new(1);

    // Write at sequence 1
    memtable.put(b"key1", b"value1", 1);

    // Read with max_seq=1 should see value1
    assert_eq!(memtable.get(b"key1", 1), Some(b"value1".to_vec()));

    // Delete at sequence 2
    memtable.delete(b"key1", 2);

    // Read with max_seq=1 should STILL see value1 (before delete)
    let result = memtable.get(b"key1", 1);
    println!("Result for max_seq=1: {:?}", result);
    assert_eq!(result, Some(b"value1".to_vec()));

    // Read with max_seq=2 should see tombstone (empty vec)
    let result = memtable.get(b"key1", 2);
    println!("Result for max_seq=2: {:?}", result);
    assert_eq!(result, Some(Vec::new()));

    // Read with max_seq=100 should see tombstone
    assert_eq!(memtable.get(b"key1", 100), Some(Vec::new()));
}
