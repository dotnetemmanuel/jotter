#![warn(clippy::pedantic)]
//! Synchronous `SQLite` index for a jotter vault: notes, tags, links, and `FTS5` search.
//!
//! The index is a plain on-disk `SQLite` file (`<vault>/.jotter/index.db`). It is
//! deliberately synchronous: the vault reindex runs on a background thread owned by
//! the caller, not via async in this crate. All public methods return `Result` and
//! the crate never panics in library code.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

/// Migrations embedded at compile time, applied in numbered order on open.
///
/// Each entry is `(number, sql)`. Adding `002_*.sql` later means appending one line
/// here; `run_migrations` applies only those whose number exceeds `user_version`.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/001_init.sql")),
    (2, include_str!("../migrations/002_link_target.sql")),
];

/// Errors surfaced by the index. Typed so callers can react without string matching.
#[derive(Debug, Error)]
pub enum IndexError {
    /// A `SQLite` operation failed (open, migrate, query, or write).
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// One note row plus the body used only to populate the contentless FTS table.
///
/// `body` is not stored in `notes`; it feeds `notes_fts` so search can match on it.
#[derive(Debug, Clone)]
pub struct NoteRecord {
    /// Path relative to the vault root. Unique key for a note.
    pub path: String,
    /// Display title: frontmatter title, else first H1, else filename.
    pub title: String,
    /// Modification time in unix seconds, used for incremental reindex.
    pub mtime: i64,
    /// File size in bytes.
    pub size: i64,
    /// Raw serialized frontmatter, or `None` when the note has none.
    pub frontmatter: Option<String>,
    /// Plain-text body indexed into `FTS5`. Not persisted in the `notes` row.
    pub body: String,
}

/// A note as stored in the `notes` table. No body: that lives only in FTS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Stable row id (also the FTS rowid).
    pub id: i64,
    /// Path relative to the vault root.
    pub path: String,
    /// Display title.
    pub title: String,
    /// Modification time in unix seconds.
    pub mtime: i64,
    /// File size in bytes.
    pub size: i64,
    /// Raw serialized frontmatter, or `None`.
    pub frontmatter: Option<String>,
}

/// One outbound link from a note to a target.
#[derive(Debug, Clone)]
pub struct LinkRecord {
    /// The target exactly as written in the note, never rewritten.
    pub target: String,
    /// Resolved relative path, or the raw target when unresolved.
    pub dst_path: String,
    /// Whether the link resolved to an existing note.
    pub resolved: bool,
}

impl LinkRecord {
    /// A link to `target` already known to point at `dst_path`.
    #[must_use]
    pub fn resolved(target: impl Into<String>, dst_path: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            dst_path: dst_path.into(),
            resolved: true,
        }
    }

    /// An unresolved link to `target`, for [`Index::reresolve_links`] to settle.
    #[must_use]
    pub fn unresolved(target: impl Into<String>) -> Self {
        let target = target.into();
        Self {
            dst_path: target.clone(),
            target,
            resolved: false,
        }
    }
}

/// A handle to the vault index. Wraps one synchronous `SQLite` connection.
pub struct Index {
    conn: Connection,
}

impl Index {
    /// Opens (creating if missing) the index at `path`, enabling foreign keys and
    /// running any pending migrations.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] if the file cannot be opened or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, IndexError> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Opens an in-memory index. Used by tests and never touches disk.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] if the connection or migrations fail.
    pub fn open_in_memory() -> Result<Self, IndexError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    /// Shared setup: enable foreign keys, WAL mode, and a busy timeout, then migrate.
    fn init(conn: Connection) -> Result<Self, IndexError> {
        // rusqlite does not enable foreign keys by default, so ON DELETE CASCADE needs this.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // journal_mode and busy_timeout both return their new value as a row,
        // so pragma_update cannot set either; pragma_update_and_check reads it back.
        let _: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        let _: i64 = conn.pragma_update_and_check(None, "busy_timeout", 5000, |row| row.get(0))?;
        let index = Self { conn };
        index.run_migrations()?;
        Ok(index)
    }

    /// Applies each migration whose number exceeds `user_version`, in order, in a
    /// transaction, then bumps `user_version`. Idempotent across reopens.
    fn run_migrations(&self) -> Result<(), IndexError> {
        let current: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
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

    /// Inserts a new note or updates the existing row with the same `path`, keeping the
    /// FTS row in sync. Returns the note id, which is stable across updates.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on any write failure.
    pub fn upsert_note(&self, record: &NoteRecord) -> Result<i64, IndexError> {
        let tx = self.conn.unchecked_transaction()?;
        // ON CONFLICT keeps the id stable so tags, links, and the FTS rowid stay valid.
        tx.execute(
            "INSERT INTO notes (path, title, mtime, size, frontmatter)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
               title = excluded.title,
               mtime = excluded.mtime,
               size = excluded.size,
               frontmatter = excluded.frontmatter",
            params![
                record.path,
                record.title,
                record.mtime,
                record.size,
                record.frontmatter,
            ],
        )?;
        let id: i64 = tx.query_row(
            "SELECT id FROM notes WHERE path = ?1",
            params![record.path],
            |row| row.get(0),
        )?;
        // Contentless FTS is managed by rowid: drop the old row, then insert the fresh one.
        Self::fts_delete(&tx, id)?;
        tx.execute(
            "INSERT INTO notes_fts (rowid, title, body) VALUES (?1, ?2, ?3)",
            params![id, record.title, record.body],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Replaces all tags for a note (delete then insert).
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on any write failure.
    pub fn set_tags(&self, note_id: i64, tags: &[String]) -> Result<(), IndexError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM tags WHERE note_id = ?1", params![note_id])?;
        {
            let mut stmt =
                tx.prepare("INSERT OR IGNORE INTO tags (note_id, tag) VALUES (?1, ?2)")?;
            for tag in tags {
                stmt.execute(params![note_id, tag])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Replaces all outbound links for a note (delete then insert).
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on any write failure.
    pub fn set_links(&self, note_id: i64, links: &[LinkRecord]) -> Result<(), IndexError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM links WHERE src_note_id = ?1", params![note_id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO links (src_note_id, target, dst_path, resolved)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for link in links {
                stmt.execute(params![
                    note_id,
                    link.target,
                    link.dst_path,
                    i64::from(link.resolved)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Re-runs `resolve` over every distinct link target and stores the outcome.
    ///
    /// Targets are never rewritten, so this always asks the same question a fresh
    /// scan of the note would: a stem stays a stem even after it once matched a
    /// path. Run it whenever the set of notes changes, and after reindexing a note.
    ///
    /// Returns the number of link rows whose destination or resolved flag changed.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on any read or write failure.
    pub fn reresolve_links<F>(&self, resolve: F) -> Result<usize, IndexError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let current: Vec<(String, String, bool)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT DISTINCT target, dst_path, resolved FROM links")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })?;
            rows.collect::<Result<_, _>>()?
        };

        let tx = self.conn.unchecked_transaction()?;
        let mut changed = 0;
        {
            let mut stmt =
                tx.prepare("UPDATE links SET dst_path = ?1, resolved = ?2 WHERE target = ?3")?;
            for (target, was_dst, was_resolved) in current {
                let (dst, resolved) = match resolve(&target) {
                    Some(path) => (path, true),
                    None => (target.clone(), false),
                };
                if dst == was_dst && resolved == was_resolved {
                    continue;
                }
                changed += stmt.execute(params![dst, i64::from(resolved), target])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Removes a note by path. Tags and links cascade; the FTS row is dropped too.
    /// Returns whether a note existed at that path.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on any failure.
    pub fn remove_note_by_path(&self, path: &str) -> Result<bool, IndexError> {
        let tx = self.conn.unchecked_transaction()?;
        let id: Option<i64> = tx
            .query_row(
                "SELECT id FROM notes WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;
        let Some(id) = id else {
            return Ok(false);
        };
        // Contentless FTS has no cascade, so its row must be removed explicitly.
        Self::fts_delete(&tx, id)?;
        tx.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(true)
    }

    /// Looks up a single note by its unique path.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn note_by_path(&self, path: &str) -> Result<Option<Note>, IndexError> {
        let note = self
            .conn
            .query_row(
                "SELECT id, path, title, mtime, size, frontmatter FROM notes WHERE path = ?1",
                params![path],
                Self::map_note,
            )
            .optional()?;
        Ok(note)
    }

    /// Returns every note, ordered by path, for the tree and reindex comparison.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn all_notes(&self) -> Result<Vec<Note>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, title, mtime, size, frontmatter FROM notes ORDER BY path")?;
        let rows = stmt.query_map([], Self::map_note)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    /// Returns the number of indexed notes, for the status bar count.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn count_notes(&self) -> Result<i64, IndexError> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the stored mtime for a path, for skipping unchanged files on reindex.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn mtime_by_path(&self, path: &str) -> Result<Option<i64>, IndexError> {
        let mtime = self
            .conn
            .query_row(
                "SELECT mtime FROM notes WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(mtime)
    }

    /// Returns the ids of notes that link, resolved, to `dst_path`.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn backlinks(&self, dst_path: &str) -> Result<Vec<i64>, IndexError> {
        let mut stmt = self
            .conn
            // DISTINCT: one note can reach the same target by stem and by path.
            .prepare(
                "SELECT DISTINCT src_note_id FROM links WHERE dst_path = ?1 AND resolved = 1",
            )?;
        let rows = stmt.query_map(params![dst_path], |row| row.get(0))?;
        let mut ids = Vec::new();
        for id in rows {
            ids.push(id?);
        }
        Ok(ids)
    }

    /// Every tag with how many notes carry it, in alphabetical order.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn tag_counts(&self) -> Result<Vec<(String, i64)>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag, count(*) FROM tags GROUP BY tag ORDER BY tag")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut counts = Vec::new();
        for row in rows {
            counts.push(row?);
        }
        Ok(counts)
    }

    /// Notes carrying `tag`, whole and sorted by path.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn tagged_notes(&self, tag: &str) -> Result<Vec<Note>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.path, n.title, n.mtime, n.size, n.frontmatter
             FROM tags t JOIN notes n ON n.id = t.note_id
             WHERE t.tag = ?1 ORDER BY n.path",
        )?;
        let rows = stmt.query_map(params![tag], Self::map_note)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    /// Notes whose links resolve to `dst_path`, whole and sorted by path.
    ///
    /// The companion to [`Index::backlinks`] for callers that want the notes
    /// rather than their row ids. A note reaching the target by both stem and
    /// path is listed once.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn linking_notes(&self, dst_path: &str) -> Result<Vec<Note>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT n.id, n.path, n.title, n.mtime, n.size, n.frontmatter
             FROM links l JOIN notes n ON n.id = l.src_note_id
             WHERE l.dst_path = ?1 AND l.resolved = 1
             ORDER BY n.path",
        )?;
        let rows = stmt.query_map(params![dst_path], Self::map_note)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    /// Returns each unresolved link target with the count of notes pointing at it,
    /// in alphabetical order.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn broken_links(&self) -> Result<Vec<(String, i64)>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT dst_path, count(DISTINCT src_note_id) FROM links
             WHERE resolved = 0 GROUP BY dst_path ORDER BY dst_path",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Notes holding an unresolved link to `dst_path`, whole and sorted by path.
    ///
    /// The companion to [`Index::broken_links`], which names the targets but not
    /// who points at them. A note pointing twice is listed once.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn broken_link_notes(&self, dst_path: &str) -> Result<Vec<Note>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT n.id, n.path, n.title, n.mtime, n.size, n.frontmatter
             FROM links l JOIN notes n ON n.id = l.src_note_id
             WHERE l.dst_path = ?1 AND l.resolved = 0
             ORDER BY n.path",
        )?;
        let rows = stmt.query_map(params![dst_path], Self::map_note)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    /// Runs an `FTS5` `MATCH` and returns matching note ids (FTS rowids).
    ///
    /// Raw user input can be invalid `FTS5` syntax. Rather than surfacing that as an
    /// error or panic, a syntax error yields an empty result so a half-typed query in
    /// the search box is simply "no matches yet".
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] only for failures unrelated to query syntax.
    pub fn search(&self, query: &str) -> Result<Vec<i64>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?1 ORDER BY rank")?;
        let rows = match stmt.query_map(params![query], |row| row.get(0)) {
            Ok(rows) => rows,
            Err(err) if is_fts_syntax_error(&err) => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut ids = Vec::new();
        for id in rows {
            match id {
                Ok(id) => ids.push(id),
                Err(err) if is_fts_syntax_error(&err) => return Ok(Vec::new()),
                Err(err) => return Err(err.into()),
            }
        }
        Ok(ids)
    }

    /// Full-text search returning whole notes, best match first, capped at `limit`.
    ///
    /// Ranking is `FTS5` bm25. As with [`Index::search`], invalid query syntax
    /// yields no matches rather than an error.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] only for failures unrelated to query syntax.
    pub fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.path, n.title, n.mtime, n.size, n.frontmatter
             FROM notes_fts f JOIN notes n ON n.id = f.rowid
             WHERE notes_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = match stmt.query_map(params![query, limit], Self::map_note) {
            Ok(rows) => rows,
            Err(err) if is_fts_syntax_error(&err) => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut notes = Vec::new();
        for note in rows {
            match note {
                Ok(note) => notes.push(note),
                Err(err) if is_fts_syntax_error(&err) => return Ok(Vec::new()),
                Err(err) => return Err(err.into()),
            }
        }
        Ok(notes)
    }

    /// Returns the tags for a note, ordered.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn tags_for(&self, note_id: i64) -> Result<Vec<String>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM tags WHERE note_id = ?1 ORDER BY tag")?;
        let rows = stmt.query_map(params![note_id], |row| row.get(0))?;
        let mut tags = Vec::new();
        for tag in rows {
            tags.push(tag?);
        }
        Ok(tags)
    }

    /// Returns the ids of notes carrying `tag`.
    ///
    /// # Errors
    /// Returns [`IndexError::Sqlite`] on query failure.
    pub fn notes_with_tag(&self, tag: &str) -> Result<Vec<i64>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT note_id FROM tags WHERE tag = ?1 ORDER BY note_id")?;
        let rows = stmt.query_map(params![tag], |row| row.get(0))?;
        let mut ids = Vec::new();
        for id in rows {
            ids.push(id?);
        }
        Ok(ids)
    }

    /// Removes the FTS row for `rowid`, if present.
    ///
    /// The FTS table is contentless with `contentless_delete=1`, so a plain DELETE by
    /// rowid removes the indexed row without needing the original column values.
    fn fts_delete(tx: &rusqlite::Transaction<'_>, rowid: i64) -> Result<(), IndexError> {
        tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", params![rowid])?;
        Ok(())
    }

    /// Maps a `notes` row to a [`Note`]. Column order matches the SELECTs above.
    fn map_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
        Ok(Note {
            id: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            mtime: row.get(3)?,
            size: row.get(4)?,
            frontmatter: row.get(5)?,
        })
    }
}

/// Detects an `FTS5` query-syntax error so `search` can treat it as "no matches".
///
/// Our search SQL is fixed and valid; the only variable is the user MATCH string, so
/// a generic `SQLITE_ERROR` (code 1) here means the query text was malformed. `FTS5`
/// reports these with messages like "fts5: syntax error near ..." or
/// "unterminated string", none of which should surface as a hard error to the caller.
fn is_fts_syntax_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::Unknown
    )
}

#[cfg(test)]
mod tests {
    use super::{Index, LinkRecord, NoteRecord};

    fn note(path: &str, title: &str, body: &str) -> NoteRecord {
        NoteRecord {
            path: path.to_string(),
            title: title.to_string(),
            mtime: 100,
            size: 10,
            frontmatter: None,
            body: body.to_string(),
        }
    }

    #[test]
    fn open_in_memory_runs_migrations() {
        let index = Index::open_in_memory().unwrap();
        let version: i64 = index
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let table_count: i64 = index
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('notes', 'tags', 'links', 'notes_fts')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 4);
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let index = Index::open_in_memory().unwrap();
        let on: i64 = index
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn an_on_disk_index_is_in_wal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path().join("index.db")).unwrap();
        let mode: String = index
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn wal_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        drop(Index::open(&path).unwrap());
        let index = Index::open(&path).unwrap();
        let mode: String = index
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    // Already 5000 by rusqlite own default; this pins the contract value, not proof our call sets it.
    #[test]
    fn every_connection_gets_a_busy_timeout() {
        let index = Index::open_in_memory().unwrap();
        let timeout: i64 = index
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn upsert_inserts_then_updates_same_path() {
        let index = Index::open_in_memory().unwrap();
        let id1 = index.upsert_note(&note("a.md", "First", "one")).unwrap();
        let mut updated = note("a.md", "Second", "two");
        updated.mtime = 200;
        let id2 = index.upsert_note(&updated).unwrap();
        assert_eq!(id1, id2, "id stays stable across update");

        let count: i64 = index
            .conn
            .query_row("SELECT count(*) FROM notes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "no duplicate row for the same path");

        let stored = index.note_by_path("a.md").unwrap().unwrap();
        assert_eq!(stored.title, "Second");
        assert_eq!(stored.mtime, 200);
    }

    #[test]
    fn count_notes_tracks_inserts_and_removes() {
        let index = Index::open_in_memory().unwrap();
        assert_eq!(index.count_notes().unwrap(), 0);
        index.upsert_note(&note("a.md", "A", "")).unwrap();
        index.upsert_note(&note("b.md", "B", "")).unwrap();
        assert_eq!(index.count_notes().unwrap(), 2);
        // Re-upserting the same path does not double-count.
        index.upsert_note(&note("a.md", "A2", "")).unwrap();
        assert_eq!(index.count_notes().unwrap(), 2);
        index.remove_note_by_path("a.md").unwrap();
        assert_eq!(index.count_notes().unwrap(), 1);
    }

    #[test]
    fn set_tags_replaces() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index.set_tags(id, &["one".into(), "two".into()]).unwrap();
        assert_eq!(index.tags_for(id).unwrap(), vec!["one", "two"]);
        index.set_tags(id, &["three".into()]).unwrap();
        assert_eq!(index.tags_for(id).unwrap(), vec!["three"]);
        assert_eq!(index.notes_with_tag("three").unwrap(), vec![id]);
        assert!(index.notes_with_tag("one").unwrap().is_empty());
    }

    #[test]
    fn set_links_replaces() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(
                id,
                &[
                    LinkRecord::unresolved("b.md"),
                    LinkRecord::unresolved("c.md"),
                ],
            )
            .unwrap();
        let count: i64 = index
            .conn
            .query_row(
                "SELECT count(*) FROM links WHERE src_note_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        index
            .set_links(id, &[LinkRecord::unresolved("d.md")])
            .unwrap();
        let count: i64 = index
            .conn
            .query_row(
                "SELECT count(*) FROM links WHERE src_note_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// Every link row as (`dst_path`, resolved), ordered for stable assertions.
    fn link_rows(index: &Index) -> Vec<(String, bool)> {
        let mut stmt = index
            .conn
            .prepare("SELECT dst_path, resolved FROM links ORDER BY dst_path")
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    #[test]
    fn reresolve_promotes_a_target_that_now_exists() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(id, &[LinkRecord::unresolved("standup")])
            .unwrap();

        let changed = index
            .reresolve_links(|t| (t == "standup").then(|| "work/standup.md".to_owned()))
            .unwrap();

        assert_eq!(changed, 1);
        assert_eq!(link_rows(&index), [("work/standup.md".to_owned(), true)]);
    }

    #[test]
    fn reresolve_demotes_a_target_that_disappeared() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(
                id,
                &[LinkRecord {
                    target: "work/standup.md".into(),
                    dst_path: "work/standup.md".into(),
                    resolved: true,
                }],
            )
            .unwrap();

        index.reresolve_links(|_| None).unwrap();

        assert_eq!(link_rows(&index), [("work/standup.md".to_owned(), false)]);
    }

    #[test]
    fn reresolve_is_idempotent() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(id, &[LinkRecord::unresolved("standup")])
            .unwrap();
        let resolve = |t: &str| {
            matches!(t, "standup" | "work/standup.md").then(|| "work/standup.md".to_owned())
        };

        index.reresolve_links(resolve).unwrap();
        assert_eq!(index.reresolve_links(resolve).unwrap(), 0);
        assert_eq!(link_rows(&index), [("work/standup.md".to_owned(), true)]);
    }

    #[test]
    fn reresolve_keeps_asking_about_the_written_target() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(id, &[LinkRecord::unresolved("standup")])
            .unwrap();

        // First the stem matches the personal note, then that note is deleted.
        index
            .reresolve_links(|_| Some("personal/standup.md".to_owned()))
            .unwrap();
        index
            .reresolve_links(|target| (target == "standup").then(|| "work/standup.md".to_owned()))
            .unwrap();

        // The stem must fall through to the surviving note, not go broken.
        assert_eq!(link_rows(&index), [("work/standup.md".to_owned(), true)]);
    }

    #[test]
    fn two_targets_may_point_at_one_note() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(
                id,
                &[
                    LinkRecord::unresolved("standup"),
                    LinkRecord::unresolved("Standup"),
                ],
            )
            .unwrap();

        index
            .reresolve_links(|_| Some("work/standup.md".to_owned()))
            .unwrap();

        // Both rows survive under their own targets; backlinks dedups them.
        assert_eq!(
            link_rows(&index),
            [
                ("work/standup.md".to_owned(), true),
                ("work/standup.md".to_owned(), true)
            ]
        );
        assert_eq!(index.backlinks("work/standup.md").unwrap(), vec![id]);
    }

    #[test]
    fn backlinks_returns_only_resolved() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        let b = index.upsert_note(&note("b.md", "B", "")).unwrap();
        index
            .set_links(
                a,
                &[LinkRecord {
                    target: "target.md".into(),
                    dst_path: "target.md".into(),
                    resolved: true,
                }],
            )
            .unwrap();
        index
            .set_links(b, &[LinkRecord::unresolved("target.md")])
            .unwrap();
        assert_eq!(index.backlinks("target.md").unwrap(), vec![a]);
    }

    #[test]
    fn linking_notes_come_back_whole() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index.upsert_note(&note("b.md", "B", "")).unwrap();
        index
            .set_links(a, &[LinkRecord::resolved("b", "b.md")])
            .unwrap();
        let found = index.linking_notes("b.md").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "a.md");
        assert_eq!(found[0].title, "A");
    }

    #[test]
    fn linking_notes_are_sorted_by_path() {
        let index = Index::open_in_memory().unwrap();
        index.upsert_note(&note("target.md", "T", "")).unwrap();
        for name in ["z.md", "a.md"] {
            let id = index.upsert_note(&note(name, name, "")).unwrap();
            index
                .set_links(id, &[LinkRecord::resolved("target", "target.md")])
                .unwrap();
        }
        let paths: Vec<String> = index
            .linking_notes("target.md")
            .unwrap()
            .into_iter()
            .map(|note| note.path)
            .collect();
        assert_eq!(paths, ["a.md", "z.md"]);
    }

    #[test]
    fn an_unresolved_link_is_not_a_backlink() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(a, &[LinkRecord::unresolved("ghost")])
            .unwrap();
        assert!(index.linking_notes("ghost").unwrap().is_empty());
    }

    #[test]
    fn a_note_reaching_a_target_twice_is_listed_once() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index.upsert_note(&note("sub/b.md", "B", "")).unwrap();
        index
            .set_links(
                a,
                &[
                    LinkRecord::resolved("b", "sub/b.md"),
                    LinkRecord::resolved("sub/b", "sub/b.md"),
                ],
            )
            .unwrap();
        assert_eq!(index.linking_notes("sub/b.md").unwrap().len(), 1);
    }

    #[test]
    fn broken_links_groups_with_counts() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        let b = index.upsert_note(&note("b.md", "B", "")).unwrap();
        index
            .set_links(a, &[LinkRecord::unresolved("missing.md")])
            .unwrap();
        index
            .set_links(
                b,
                &[
                    LinkRecord::unresolved("missing.md"),
                    LinkRecord::unresolved("other.md"),
                ],
            )
            .unwrap();
        assert_eq!(
            index.broken_links().unwrap(),
            vec![("missing.md".to_string(), 2), ("other.md".to_string(), 1)]
        );
    }

    #[test]
    fn a_note_pointing_twice_at_a_missing_target_counts_once() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index
            .set_links(
                a,
                &[
                    LinkRecord::unresolved("ghost"),
                    LinkRecord::unresolved("ghost"),
                ],
            )
            .unwrap();
        assert_eq!(
            index.broken_links().unwrap(),
            vec![("ghost".to_string(), 1)]
        );
    }

    #[test]
    fn broken_link_notes_names_who_points_at_a_missing_target() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        let b = index.upsert_note(&note("b.md", "B", "")).unwrap();
        index
            .set_links(
                a,
                &[
                    LinkRecord::unresolved("ghost"),
                    LinkRecord::unresolved("ghost"),
                ],
            )
            .unwrap();
        index
            .set_links(b, &[LinkRecord::resolved("a", "a.md")])
            .unwrap();

        let linkers = index.broken_link_notes("ghost").unwrap();
        assert_eq!(
            linkers.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(),
            ["a.md"]
        );
        assert!(index.broken_link_notes("a").unwrap().is_empty());
    }

    #[test]
    fn search_finds_then_misses_after_remove() {
        let index = Index::open_in_memory().unwrap();
        let id = index
            .upsert_note(&note("a.md", "Rustacean notes", "the quick brown fox"))
            .unwrap();
        assert_eq!(index.search("brown").unwrap(), vec![id]);
        assert_eq!(index.search("Rustacean").unwrap(), vec![id]);
        assert!(index.remove_note_by_path("a.md").unwrap());
        assert!(index.search("brown").unwrap().is_empty());
        assert!(index.search("Rustacean").unwrap().is_empty());
    }

    #[test]
    fn search_notes_returns_matches_ranked() {
        let index = Index::open_in_memory().unwrap();
        index
            .upsert_note(&note("a.md", "Alpha", "webkit rendering notes"))
            .unwrap();
        index
            .upsert_note(&note("b.md", "Beta", "nothing to see"))
            .unwrap();
        let found = index.search_notes("webkit", 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "a.md");
        assert_eq!(found[0].title, "Alpha");
    }

    #[test]
    fn search_notes_respects_the_limit() {
        let index = Index::open_in_memory().unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            index
                .upsert_note(&note(name, name, "shared webkit body"))
                .unwrap();
        }
        assert_eq!(index.search_notes("webkit", 2).unwrap().len(), 2);
    }

    #[test]
    fn search_notes_matches_the_title_too() {
        let index = Index::open_in_memory().unwrap();
        index
            .upsert_note(&note("a.md", "Webkit rendering", "unrelated body"))
            .unwrap();
        assert_eq!(index.search_notes("webkit", 10).unwrap().len(), 1);
    }

    #[test]
    fn search_notes_survives_invalid_query() {
        let index = Index::open_in_memory().unwrap();
        index.upsert_note(&note("a.md", "Alpha", "body")).unwrap();
        assert!(index.search_notes("\"unclosed", 10).unwrap().is_empty());
    }

    #[test]
    fn search_survives_invalid_query() {
        let index = Index::open_in_memory().unwrap();
        index
            .upsert_note(&note("a.md", "A", "hello world"))
            .unwrap();
        // Unbalanced quote is invalid FTS5 syntax; expect an empty result, not an error.
        assert!(index.search("\"unterminated").unwrap().is_empty());
    }

    #[test]
    fn tag_counts_are_alphabetical() {
        let index = Index::open_in_memory().unwrap();
        let a = index.upsert_note(&note("a.md", "A", "")).unwrap();
        let b = index.upsert_note(&note("b.md", "B", "")).unwrap();
        index
            .set_tags(a, &["project".into(), "demo".into()])
            .unwrap();
        index
            .set_tags(b, &["project".into(), "alpha".into()])
            .unwrap();
        assert_eq!(
            index.tag_counts().unwrap(),
            [
                ("alpha".to_string(), 1),
                ("demo".to_string(), 1),
                ("project".to_string(), 2)
            ]
        );
    }

    #[test]
    fn a_vault_without_tags_counts_nothing() {
        let index = Index::open_in_memory().unwrap();
        index.upsert_note(&note("a.md", "A", "")).unwrap();
        assert!(index.tag_counts().unwrap().is_empty());
    }

    #[test]
    fn tagged_notes_come_back_whole_and_sorted() {
        let index = Index::open_in_memory().unwrap();
        for name in ["z.md", "a.md"] {
            let id = index.upsert_note(&note(name, name, "")).unwrap();
            index.set_tags(id, &["project".into()]).unwrap();
        }
        let paths: Vec<String> = index
            .tagged_notes("project")
            .unwrap()
            .into_iter()
            .map(|note| note.path)
            .collect();
        assert_eq!(paths, ["a.md", "z.md"]);
    }

    #[test]
    fn an_unknown_tag_has_no_notes() {
        let index = Index::open_in_memory().unwrap();
        assert!(index.tagged_notes("nothing").unwrap().is_empty());
    }

    #[test]
    fn removing_note_cascades_tags_and_links() {
        let index = Index::open_in_memory().unwrap();
        let id = index.upsert_note(&note("a.md", "A", "")).unwrap();
        index.set_tags(id, &["t".into()]).unwrap();
        index
            .set_links(id, &[LinkRecord::unresolved("b.md")])
            .unwrap();
        assert!(index.remove_note_by_path("a.md").unwrap());
        let tag_count: i64 = index
            .conn
            .query_row("SELECT count(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        let link_count: i64 = index
            .conn
            .query_row("SELECT count(*) FROM links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tag_count, 0, "tags cascade on note delete");
        assert_eq!(link_count, 0, "links cascade on note delete");
    }

    #[test]
    fn remove_missing_path_is_false() {
        let index = Index::open_in_memory().unwrap();
        assert!(!index.remove_note_by_path("nope.md").unwrap());
    }

    #[test]
    fn reopen_on_disk_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        let id = {
            let index = Index::open(&path).unwrap();
            index.upsert_note(&note("a.md", "A", "body")).unwrap()
        };
        // Reopening must not re-run migrations or lose data.
        let index = Index::open(&path).unwrap();
        let version: i64 = index
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let stored = index.note_by_path("a.md").unwrap().unwrap();
        assert_eq!(stored.id, id);
        assert_eq!(index.all_notes().unwrap().len(), 1);
        assert_eq!(index.mtime_by_path("a.md").unwrap(), Some(100));
    }
}
