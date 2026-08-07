use std::fs;

use jotter_store::{Store, StoreError};

#[test]
fn a_database_from_a_newer_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }

    let err = Store::open(&path).unwrap_err();

    assert!(matches!(
        err,
        StoreError::TooNew {
            found: 999,
            highest: 1
        }
    ));
}

#[test]
fn a_refused_open_leaves_the_file_byte_identical_and_creates_no_sidecar_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }
    let bytes_before = fs::read(&path).unwrap();

    let err = Store::open(&path).unwrap_err();

    assert!(matches!(err, StoreError::TooNew { .. }));
    assert_eq!(fs::read(&path).unwrap(), bytes_before);
    assert!(!path.with_extension("db-wal").exists());
    assert!(!path.with_extension("db-shm").exists());
}
