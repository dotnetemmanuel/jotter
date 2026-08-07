//! Two binaries open the same on-disk index, so every connection must be in WAL
//! mode and wait on a lock rather than error immediately.

use jotter_index::Index;

#[test]
fn an_on_disk_index_is_in_wal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let index = Index::open(dir.path().join("index.db")).unwrap();
    assert_eq!(index.journal_mode().unwrap(), "wal");
}

#[test]
fn wal_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    drop(Index::open(&path).unwrap());
    let index = Index::open(&path).unwrap();
    assert_eq!(index.journal_mode().unwrap(), "wal");
}

#[test]
fn every_connection_gets_a_busy_timeout() {
    let index = Index::open_in_memory().unwrap();
    assert_eq!(index.busy_timeout_ms().unwrap(), 5000);
}
