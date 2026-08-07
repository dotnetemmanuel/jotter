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
