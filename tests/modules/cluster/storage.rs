use aidb::cluster::{OpenRaftStorage, DEFAULT_GROUP_ID};
use aidb::config::Options;
use aidb::DB;
use tempfile::TempDir;

fn test_storage() -> (OpenRaftStorage, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    (
        OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap(),
        dir,
    )
}

#[test]
fn test_storage_state_default() {
    let (storage, _dir) = test_storage();
    assert_eq!(storage.group_id(), DEFAULT_GROUP_ID);
}

#[test]
fn raft_storage_open_new_db() {
    let (_storage, _dir) = test_storage();
}

#[test]
fn raft_storage_sm_read_empty() {
    let (storage, _dir) = test_storage();
    assert!(storage
        .get_state_machine_value(b"missing")
        .unwrap()
        .is_none());
}
