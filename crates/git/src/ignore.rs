//! Keeping jotter's own state out of the user's repository.
//!
//! The index database is derived, per-machine, and binary: committed, it would
//! churn every reindex and produce the one conflict nobody can resolve by hand.
//! The trash holds notes the user deleted, which have no business syncing.

use std::io;
use std::path::Path;

/// The jotter state directory inside a vault.
const JOTTER_DIR: &str = ".jotter";
/// Where deleted notes go.
const TRASH_DIR: &str = ".trash";

/// Ignores the database and its journals, but nothing else: per-vault settings
/// may later live beside it and want committing.
const JOTTER_IGNORE: &str = "\
# Written by jotter: the index is a derived cache, never worth committing.
index.db
index.db-journal
index.db-wal
index.db-shm
# And this file, so jotter never adds anything to a vault's history. It is
# rewritten wherever jotter opens the vault, so a fresh clone is covered too.
.gitignore
";

/// Deleted notes, all of them.
const TRASH_IGNORE: &str = "*\n";

/// Writes the ignore files for jotter's own state under `vault_root`.
///
/// The files live inside the directories they cover, so the vault's own
/// `.gitignore` is never touched. An existing file is left exactly as it is,
/// edits included. The trash is only handled once it exists, so a vault that has
/// never deleted a note does not grow an empty directory.
///
/// # Errors
/// Returns the underlying [`io::Error`] if a directory or file cannot be written.
pub fn write_ignores(vault_root: &Path) -> io::Result<()> {
    let jotter = vault_root.join(JOTTER_DIR);
    std::fs::create_dir_all(&jotter)?;
    write_if_missing(&jotter.join(".gitignore"), JOTTER_IGNORE)?;

    let trash = vault_root.join(TRASH_DIR);
    if trash.is_dir() {
        write_if_missing(&trash.join(".gitignore"), TRASH_IGNORE)?;
    }
    Ok(())
}

/// Writes `text` to `path` unless something is already there.
fn write_if_missing(path: &Path, text: &str) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, text)
}
