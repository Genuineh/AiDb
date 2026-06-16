//! WAL 模块级 dataflow — Writer event 时序

use crate::common::observability::capture_events_under_lock;
use aidb::engine::wal::record::RecordType;
use aidb::engine::wal::writer::Writer;
use tempfile::tempdir;

#[test]
fn test_wal_event_order() {
    let d = tempdir().unwrap();
    let path = d.path().join("order.log");
    let events = capture_events_under_lock(|| {
        let mut w = Writer::open(&path).unwrap();
        w.write_record(RecordType::Full, b"t").unwrap();
        w.sync_all().unwrap();
    });

    let pos = |needle: &str| -> Option<usize> { events.iter().position(|e| e.contains(needle)) };
    for needle in [
        "wal.write.start",
        "wal.write.complete",
        "wal.sync.start",
        "wal.sync.complete",
    ] {
        assert!(
            events.iter().any(|e| e.contains(needle)),
            "missing event {needle:?}, got: {events:?}"
        );
    }
    assert!(
        pos("wal.write.start").unwrap() < pos("wal.write.complete").unwrap(),
        "write.start must precede write.complete, got: {events:?}"
    );
    assert!(
        pos("wal.sync.start").unwrap() < pos("wal.sync.complete").unwrap(),
        "sync.start must precede sync.complete, got: {events:?}"
    );
}
