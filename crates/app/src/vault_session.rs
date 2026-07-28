//! Vault opening and indexing helpers used by the app.
//!
//! Opening a vault means locating its `.jotter/index.db`, opening a synchronous
//! [`Index`], and (separately, on a background thread) reconciling the index with
//! what is on disk. Incremental reindex of a single note is also here so the
//! watcher and the startup pass share one code path.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jotter_index::{Index, IndexError, LinkRecord, NoteRecord};
use jotter_vault::Vault;

use crate::links::Resolver;
use crate::title::extract_title;

/// The per-vault directory that holds the index database.
const JOTTER_DIR: &str = ".jotter";
/// The index database file name inside [`JOTTER_DIR`].
const INDEX_DB: &str = "index.db";

/// Progress sent from the background indexing thread to the UI thread.
#[derive(Debug, Clone)]
pub enum IndexProgress {
    /// Notes indexed so far out of the total to consider.
    Working {
        /// Notes processed (upserted or skipped) so far.
        done: usize,
        /// Total notes enumerated on disk.
        total: usize,
    },
    /// Indexing finished with the final count of notes present.
    Done {
        /// Total notes now present in the vault.
        total: usize,
    },
    /// Indexing failed; the message is for the status bar and stderr.
    Failed(String),
}

/// The absolute path to a vault's index database (`<root>/.jotter/index.db`).
#[must_use]
pub fn index_db_path(root: &Path) -> PathBuf {
    root.join(JOTTER_DIR).join(INDEX_DB)
}

/// Opens the vault index, creating the `.jotter` directory first.
///
/// # Errors
/// Returns an [`IndexError`] if the directory cannot be created or the database
/// cannot be opened or migrated.
pub fn open_index(root: &Path) -> Result<Index, IndexError> {
    let dir = root.join(JOTTER_DIR);
    // A failure to create the dir surfaces as an open error below, but log it plainly.
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "jotter: could not create index dir {}: {err}",
            dir.display()
        );
    }
    Index::open(index_db_path(root))
}

/// Reindexes a single note against `index` (create/modify path).
///
/// Reads the note, computes its title and record, and upserts it. Used by the
/// watcher and by explicit saves.
///
/// # Errors
/// Returns an error if the read or the upsert fails.
pub fn reindex_note(vault: &Vault, index: &Index, rel: &Path) -> Result<()> {
    let text = vault
        .read_note(rel)
        .with_context(|| format!("read note {}", rel.display()))?;
    let record = build_record(vault, rel, &text)?;
    let note_id = index.upsert_note(&record)?;
    index.set_links(note_id, &outbound_links(&text))?;
    index.set_tags(note_id, &note_tags(&text))?;
    Ok(())
}

/// Reindexes one note and settles its link resolution.
///
/// The incremental counterpart to [`reindex_note`], which leaves links
/// unresolved because a full index resolves once at the end instead of per note.
///
/// # Errors
/// Returns an error if the reindex or the resolve pass fails.
pub fn reindex_note_resolved(vault: &Vault, index: &Index, rel: &Path) -> Result<()> {
    reindex_note(vault, index, rel)?;
    resolve_links(index)
}

/// Every tag of a note: the frontmatter list first, then inline `#tag` uses.
///
/// Both sources are lowercased and deduplicated, so a note declaring a tag it
/// also uses inline counts once.
fn note_tags(text: &str) -> Vec<String> {
    let mut tags: Vec<String> = jotter_parser::frontmatter::parse(text)
        .tags
        .iter()
        .map(|tag| tag.to_lowercase())
        .collect();
    for tag in jotter_parser::tags::scan(text) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// Outbound wikilinks of a note, stored under their raw targets.
///
/// Resolution is deliberately deferred to [`resolve_links`]: during a full index
/// a note can link to one that has not been indexed yet.
fn outbound_links(text: &str) -> Vec<LinkRecord> {
    jotter_parser::wikilink::scan(text)
        .into_iter()
        .map(|link| LinkRecord::unresolved(link.target))
        .collect()
}

/// Resolves every link target in `index` against the notes it now contains.
///
/// # Errors
/// Returns an error if the index cannot be read or updated.
pub fn resolve_links(index: &Index) -> Result<()> {
    let paths = index.all_notes()?.into_iter().map(|note| note.path);
    let resolver = Resolver::new(paths);
    index.reresolve_links(|target| resolver.lookup(target))?;
    Ok(())
}

/// Removes a note from `index` by its vault-relative path. A missing row is fine.
///
/// # Errors
/// Returns an error if the delete fails.
pub fn deindex_note(index: &Index, rel: &Path) -> Result<()> {
    index.remove_note_by_path(&rel_to_key(rel))?;
    Ok(())
}

/// Builds a [`NoteRecord`] for `rel` from its on-disk text and metadata.
///
/// The body fed to FTS is the raw file text. Tags and links are left empty for
/// now (later phases populate them).
fn build_record(vault: &Vault, rel: &Path, text: &str) -> Result<NoteRecord> {
    let abs = vault.root().join(rel);
    let meta = std::fs::metadata(&abs)
        .with_context(|| format!("read metadata {}", abs.display()))?;
    let mtime = mtime_seconds(&meta);
    let size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
    Ok(NoteRecord {
        path: rel_to_key(rel),
        title: extract_title(text, rel),
        mtime,
        size,
        frontmatter: jotter_parser::frontmatter::parse(text).raw,
        body: text.to_owned(),
    })
}

/// A note's stable index key: its vault-relative path with forward slashes.
#[must_use]
pub fn rel_to_key(rel: &Path) -> String {
    // Index keys are stored as forward-slash paths so they match across platforms.
    rel.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// Reads a file's mtime as unix seconds, clamping pre-epoch times to zero.
fn mtime_seconds(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// Runs a full incremental reconcile of `index` against the vault at `root`.
///
/// Opens its own index connection (`SQLite` connections are not `Send`), so this is
/// safe to call from a spawned thread. New and changed notes (by mtime) are
/// upserted; index rows whose path no longer exists on disk are removed. Progress
/// is reported through `report`.
///
/// # Errors
/// Returns an error if the vault cannot be enumerated or the index cannot be
/// opened. Per-note read failures are logged and skipped, not fatal.
pub fn reconcile_in_thread<F: Fn(IndexProgress)>(root: &Path, report: &F) -> Result<()> {
    let vault = Vault::open(root).with_context(|| format!("open vault {}", root.display()))?;
    let index = open_index(root).context("open index")?;

    let notes = vault.notes().context("enumerate notes")?;
    let total = notes.len();
    let mut on_disk = std::collections::HashSet::with_capacity(total);

    for (done, note) in notes.iter().enumerate() {
        let key = rel_to_key(&note.rel_path);
        on_disk.insert(key.clone());

        // Skip a note whose stored mtime already matches the disk mtime.
        let stored = index.mtime_by_path(&key).unwrap_or(None);
        if stored != Some(note.mtime)
            && let Err(err) = reindex_note(&vault, &index, &note.rel_path)
        {
            eprintln!("jotter: could not index {}: {err}", note.rel_path.display());
        }
        report(IndexProgress::Working { done: done + 1, total });
    }

    // Drop index rows whose file no longer exists on disk.
    if let Ok(existing) = index.all_notes() {
        for note in existing {
            if !on_disk.contains(&note.path) {
                let _ = index.remove_note_by_path(&note.path);
            }
        }
    }

    // Only now does the index know every note, so link targets can be resolved.
    if let Err(err) = resolve_links(&index) {
        eprintln!("jotter: could not resolve links: {err}");
    }

    report(IndexProgress::Done { total });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{rel_to_key, reconcile_in_thread};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn rel_to_key_uses_forward_slashes() {
        assert_eq!(rel_to_key(Path::new("a/b/c.md")), "a/b/c.md");
        assert_eq!(rel_to_key(Path::new("top.md")), "top.md");
    }

    #[test]
    fn reconcile_indexes_and_prunes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.md"), "# Alpha\nbody").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.md"), "plain").unwrap();

        reconcile_in_thread(root, &|_| {}).unwrap();

        let index = super::open_index(root).unwrap();
        assert!(index.note_by_path("a.md").unwrap().is_some());
        assert_eq!(index.note_by_path("a.md").unwrap().unwrap().title, "Alpha");
        assert!(index.note_by_path("sub/b.md").unwrap().is_some());

        // Remove a file and reconcile again; its row should be pruned.
        std::fs::remove_file(root.join("a.md")).unwrap();
        reconcile_in_thread(root, &|_| {}).unwrap();
        let index = super::open_index(root).unwrap();
        assert!(index.note_by_path("a.md").unwrap().is_none());
        assert!(index.note_by_path("sub/b.md").unwrap().is_some());
    }
}
