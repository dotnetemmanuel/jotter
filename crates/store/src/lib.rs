#![warn(clippy::pedantic)]
//! The machine-level task store: projects, tasks, and subtasks.
//!
//! Unlike `jotter-index`, this database holds work that exists nowhere else. It
//! lives once per machine in the data directory (see `jotter-paths`), never inside
//! a vault, never synced, and never rebuilt from source material. Losing it loses
//! the tasks. It is deliberately synchronous, like `jotter-index`: callers own their
//! own threading.

use std::path::Path;

use rusqlite::Connection;
use thiserror::Error;

mod model;
pub use model::{Project, Subtask, Task, TaskState};

pub mod command;
pub mod date;
pub mod query;

/// Migrations embedded at compile time, applied in numbered order on open.
///
/// Each entry is `(number, sql)`. Adding `002_*.sql` later means appending one line
/// here; `run_migrations` applies only those whose number exceeds `user_version`.
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/001_init.sql"))];

/// Errors surfaced by the task store. Typed so callers can react without string matching.
#[derive(Debug, Error)]
pub enum StoreError {
    /// A `SQLite` operation failed (open, migrate, query, or write).
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database's `user_version` is higher than any migration this build knows,
    /// meaning a newer version of jotter wrote it. Opening stops here untouched:
    /// no migration, no delete, no read-only fallback.
    #[error(
        "this database is at version {found}, newer than the highest version ({highest}) this build knows; upgrade jotter before opening it"
    )]
    TooNew {
        /// The `user_version` found in the database.
        found: i64,
        /// The highest migration number this build knows how to apply.
        highest: i64,
    },
    /// A `tasks.state` value did not match any known [`TaskState`] variant.
    #[error("unknown task state {0:?}")]
    UnknownTaskState(String),
    /// A stored due date was not valid `YYYY-MM-DD` text.
    #[error("invalid due date {0:?}")]
    InvalidDueDate(String),
}

/// A handle to the task store. Wraps one synchronous `SQLite` connection.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if missing) the store at `path`, enabling foreign keys and
    /// running any pending migrations.
    ///
    /// # Errors
    /// Returns [`StoreError::Sqlite`] if the file cannot be opened or migrated, or
    /// [`StoreError::TooNew`] if the database was written by a newer version of jotter.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Opens an in-memory store. Used by tests and never touches disk.
    ///
    /// # Errors
    /// Returns [`StoreError::Sqlite`] if the connection or migrations fail.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    /// Shared setup: enable foreign keys, WAL mode, and a busy timeout, then migrate.
    fn init(conn: Connection) -> Result<Self, StoreError> {
        // rusqlite does not enable foreign keys by default, so ON DELETE CASCADE and
        // ON DELETE SET NULL both need this.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // journal_mode and busy_timeout both return their new value as a row,
        // so pragma_update cannot set either; pragma_update_and_check reads it back.
        let _: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        let _: i64 = conn.pragma_update_and_check(None, "busy_timeout", 5000, |row| row.get(0))?;
        let store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    /// Applies each migration whose number exceeds `user_version`, in order, in a
    /// transaction, then bumps `user_version`. Idempotent across reopens.
    ///
    /// Refuses outright, before touching anything, if `user_version` already exceeds
    /// the highest migration this build knows: that means a newer jotter wrote this
    /// database, and this build must not migrate, delete, or read it.
    fn run_migrations(&self) -> Result<(), StoreError> {
        let current: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let highest = MIGRATIONS
            .iter()
            .map(|(number, _)| *number)
            .max()
            .unwrap_or(0);
        if current > highest {
            return Err(StoreError::TooNew {
                found: current,
                highest,
            });
        }
        for (number, sql) in MIGRATIONS {
            if *number <= current {
                continue;
            }
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            // user_version does not accept a bound parameter, so format the trusted integer in.
            tx.pragma_update(None, "user_version", number)?;
            tx.commit()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn a_fresh_store_is_at_the_current_version() {
        let store = Store::open_in_memory().unwrap();
        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn reopening_applies_nothing_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.db");
        drop(Store::open(&path).unwrap());
        let store = Store::open(&path).unwrap();
        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn an_on_disk_store_is_in_wal_mode_with_a_busy_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("tasks.db")).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let timeout: i64 = store
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
    }
}
