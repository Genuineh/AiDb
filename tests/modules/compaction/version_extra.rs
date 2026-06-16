use aidb::engine::compaction::{user_key_from_internal, VersionEdit, VersionSet};
use aidb::error::Error;
use tempfile::tempdir;

#[test]
fn test_user_key_from_internal_short() {
    let err = user_key_from_internal(&[1, 2, 3]).unwrap_err();
    assert!(matches!(err, Error::Corruption(_)));
}

#[test]
fn test_version_total_size() {
    let dir = tempdir().unwrap();
    let mut vs = VersionSet::open_new(dir.path(), 7, 1024 * 1024).unwrap();
    vs.apply_edit(&VersionEdit::AddFile {
        level: 0,
        file_number: 1,
        file_size: 100,
        smallest_key: vec![0; 16],
        largest_key: vec![1; 16],
    })
    .unwrap();
    assert_eq!(vs.current().total_size(), 100);
}
