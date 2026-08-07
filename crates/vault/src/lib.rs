#![warn(clippy::pedantic)]
//! Filesystem layer for a jotter vault: a local folder of `.md` notes.
//!
//! A [`Vault`] wraps a root directory. It enumerates notes (ignoring `.git`,
//! `.trash`, `.jotter`, and any dotfile), reads and writes them safely (writes
//! are atomic via temp file plus rename), and can start a debounced recursive
//! [`watch`] over the tree. This crate is standalone: no GTK, no other jotter
//! crate.

mod error;
mod ignore;
mod watcher;

pub use error::VaultError;
pub use ignore::{is_ignored_component, is_ignored_path};
pub use watcher::{VaultChange, WatchGuard, watch, watch_with_debounce};

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

/// The directory a vault uses for soft deletes.
const TRASH_DIR: &str = ".trash";

/// One markdown note discovered during enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteEntry {
    /// Path relative to the vault root, using the platform separator.
    pub rel_path: PathBuf,
    /// Last modification time, unix seconds. Zero if the mtime is before the epoch.
    pub mtime: i64,
    /// File size in bytes.
    pub size: u64,
}

/// A vault: a directory of markdown notes rooted at [`Vault::root`].
///
/// All note IO takes paths relative to the root. Paths that would escape the
/// root (absolute, or containing `..`) are rejected with
/// [`VaultError::PathEscapesRoot`].
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Opens `root` as a vault, verifying it exists and is a directory.
    ///
    /// # Errors
    /// Returns [`VaultError::NotADirectory`] if `root` is missing or is not a
    /// directory.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, VaultError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(VaultError::NotADirectory(root));
        }
        Ok(Self { root })
    }

    /// The vault root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Enumerates every `.md` note under the root, recursively.
    ///
    /// Ignored subtrees (`.git`, `.trash`, `.jotter`, any dotdir) are not
    /// descended into, and dotfiles are skipped. Returned paths are relative to
    /// the root. Entries are returned in an unspecified order.
    ///
    /// # Errors
    /// Returns [`VaultError::Io`] if the root cannot be walked.
    pub fn notes(&self) -> Result<Vec<NoteEntry>, VaultError> {
        let mut out = Vec::new();
        let walker = WalkDir::new(&self.root).into_iter().filter_entry(|entry| {
            // Never descend into an ignored component. The root has an empty
            // relative path and must always pass.
            match entry.path().strip_prefix(&self.root) {
                Ok(rel) if rel.as_os_str().is_empty() => true,
                Ok(rel) => !is_ignored_path(rel),
                Err(_) => false,
            }
        });

        for entry in walker {
            let entry = entry.map_err(|err| {
                let path = err.path().unwrap_or(&self.root).to_path_buf();
                VaultError::io(path, io_error_from_walkdir(err))
            })?;

            if !entry.file_type().is_file() || !is_markdown(entry.path()) {
                continue;
            }

            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let meta = entry
                .metadata()
                .map_err(|err| VaultError::io(entry.path(), io_error_from_walkdir(err)))?;
            out.push(NoteEntry {
                rel_path: rel.to_path_buf(),
                mtime: mtime_seconds(&meta),
                size: meta.len(),
            });
        }
        Ok(out)
    }

    /// Reads a note as UTF-8 text.
    ///
    /// # Errors
    /// [`VaultError::PathEscapesRoot`] for an escaping path, [`VaultError::Io`]
    /// if the read fails, [`VaultError::NotUtf8`] if the bytes are not UTF-8.
    pub fn read_note(&self, rel: impl AsRef<Path>) -> Result<String, VaultError> {
        let abs = self.resolve(rel.as_ref())?;
        let bytes = std::fs::read(&abs).map_err(|err| VaultError::io(&abs, err))?;
        String::from_utf8(bytes).map_err(|_| VaultError::NotUtf8(rel.as_ref().to_path_buf()))
    }

    /// Writes a note atomically, overwriting any existing content.
    ///
    /// Writes to a temp file in the same directory then renames it over the
    /// target, so a reader never sees a half-written file. Parent directories
    /// must already exist (use [`Vault::create_note`] to create them).
    ///
    /// # Errors
    /// [`VaultError::PathEscapesRoot`] for an escaping path, or [`VaultError::Io`]
    /// if any step fails.
    pub fn write_note(
        &self,
        rel: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<(), VaultError> {
        let abs = self.resolve(rel.as_ref())?;
        self.atomic_write(&abs, contents.as_ref())
    }

    /// Creates a note, failing if it already exists. Creates parent directories.
    ///
    /// # Errors
    /// [`VaultError::AlreadyExists`] if the target is present,
    /// [`VaultError::PathEscapesRoot`] for an escaping path, or
    /// [`VaultError::Io`] on any filesystem failure.
    pub fn create_note(
        &self,
        rel: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<(), VaultError> {
        let abs = self.resolve(rel.as_ref())?;
        if abs.exists() {
            return Err(VaultError::AlreadyExists(rel.as_ref().to_path_buf()));
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|err| VaultError::io(parent, err))?;
        }
        self.atomic_write(&abs, contents.as_ref())
    }

    /// Renames (moves) a note. Creates the destination parent directories.
    ///
    /// # Errors
    /// [`VaultError::PathEscapesRoot`] if either path escapes the root, or
    /// [`VaultError::Io`] if the move fails.
    pub fn rename_note(
        &self,
        from_rel: impl AsRef<Path>,
        to_rel: impl AsRef<Path>,
    ) -> Result<(), VaultError> {
        let from = self.resolve(from_rel.as_ref())?;
        let to = self.resolve(to_rel.as_ref())?;
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|err| VaultError::io(parent, err))?;
        }
        std::fs::rename(&from, &to).map_err(|err| VaultError::io(&from, err))
    }

    /// Moves a note into `<root>/.trash/` instead of hard-deleting it.
    ///
    /// The trashed name keeps the original file name and, on collision, gains a
    /// numeric suffix so an earlier trashed copy is never clobbered. The
    /// `.trash` directory is created if missing.
    ///
    /// # Errors
    /// [`VaultError::PathEscapesRoot`] for an escaping path, or
    /// [`VaultError::Io`] if the move fails.
    pub fn delete_to_trash(&self, rel: impl AsRef<Path>) -> Result<(), VaultError> {
        let abs = self.resolve(rel.as_ref())?;
        let trash = self.root.join(TRASH_DIR);
        std::fs::create_dir_all(&trash).map_err(|err| VaultError::io(&trash, err))?;

        let file_name = abs.file_name().map_or_else(
            || std::ffi::OsString::from("note"),
            std::ffi::OsStr::to_os_string,
        );
        let target = unique_in(&trash, &file_name);
        std::fs::rename(&abs, &target).map_err(|err| VaultError::io(&abs, err))
    }

    /// Resolves a vault-relative path to an absolute one, rejecting escapes.
    ///
    /// Absolute inputs and inputs containing a `..` component are rejected; a
    /// `.` component is allowed and dropped.
    fn resolve(&self, rel: &Path) -> Result<PathBuf, VaultError> {
        let mut out = self.root.clone();
        for component in rel.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                // Absolute roots, prefixes, and parent hops can leave the vault.
                Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                    return Err(VaultError::PathEscapesRoot(rel.to_path_buf()));
                }
            }
        }
        Ok(out)
    }

    /// Writes `contents` to `abs` atomically via a same-directory temp + rename.
    fn atomic_write(&self, abs: &Path, contents: &[u8]) -> Result<(), VaultError> {
        let dir = abs.parent().unwrap_or(&self.root);
        let mut tmp = tempfile_in(dir)?;
        tmp.write_all(contents)
            .map_err(|err| VaultError::io(abs, err))?;
        tmp.flush().map_err(|err| VaultError::io(abs, err))?;
        // Rename is atomic within a filesystem; the temp lives in the same dir.
        tmp.persist(abs).map_err(|err| VaultError::io(abs, err))
    }
}

/// True if `path` ends in a case-insensitive `.md` extension.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Reads the mtime as unix seconds, clamping pre-epoch times to zero.
fn mtime_seconds(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|dur| i64::try_from(dur.as_secs()).ok())
        .unwrap_or(0)
}

/// Picks a non-colliding path for `name` inside `dir`.
fn unique_in(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let name = Path::new(name);
    let stem = name.file_stem().and_then(|s| s.to_str()).unwrap_or("note");
    let ext = name.extension().and_then(|s| s.to_str());
    for n in 1.. {
        let candidate = match ext {
            Some(ext) => dir.join(format!("{stem}-{n}.{ext}")),
            None => dir.join(format!("{stem}-{n}")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    // The loop above only exits via return; this is unreachable in practice.
    first
}

/// A minimal named temp file in a chosen directory, persisted by rename.
///
/// Rolled by hand so the crate needs no runtime temp-file dependency for its one
/// use. The name mixes pid, parent pid, a counter, and nanos, and is opened with
/// `create_new`, so two concurrent writers never collide.
struct TempFile {
    path: PathBuf,
    // Taken on persist so drop knows the file was moved and must not be removed.
    file: Option<std::fs::File>,
}

impl TempFile {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        self.file
            .as_mut()
            .expect("file handle present until persist")
            .write_all(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        use std::io::Write;
        let file = self
            .file
            .as_mut()
            .expect("file handle present until persist");
        file.flush()?;
        file.sync_all()
    }

    /// Renames the temp file over `dest`, consuming the guard.
    fn persist(mut self, dest: &Path) -> std::io::Result<()> {
        // Drop the handle before rename for portability; harmless on unix.
        drop(self.file.take());
        std::fs::rename(&self.path, dest)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        // If we never persisted, do not leave the temp behind.
        if self.file.is_some() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Creates a fresh temp file inside `dir`, retrying on the rare name clash.
fn tempfile_in(dir: &Path) -> Result<TempFile, VaultError> {
    let pid = std::process::id();
    for attempt in 0u64.. {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(".jotter-tmp-{pid}-{attempt}-{nanos}");
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                return Ok(TempFile {
                    path,
                    file: Some(file),
                });
            }
            // A name clash just means we try the next name.
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(VaultError::io(dir, err)),
        }
    }
    // The loop only returns; this satisfies the type checker.
    unreachable!("temp file naming loop never exhausts u64")
}

/// Adapts a walkdir error into a plain io error for the vault error type.
fn io_error_from_walkdir(err: walkdir::Error) -> std::io::Error {
    err.into_io_error()
        .unwrap_or_else(|| std::io::Error::other("walk error"))
}

#[cfg(test)]
mod tests {
    use super::{NoteEntry, Vault, VaultError};
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn vault() -> (TempDir, Vault) {
        let dir = TempDir::new().expect("temp dir");
        let vault = Vault::open(dir.path()).expect("open vault");
        (dir, vault)
    }

    fn rel_paths(mut entries: Vec<NoteEntry>) -> Vec<PathBuf> {
        entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        entries.into_iter().map(|e| e.rel_path).collect()
    }

    #[test]
    fn open_rejects_non_directory() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        match Vault::open(&file) {
            Err(VaultError::NotADirectory(p)) => assert_eq!(p, file),
            other => panic!("expected NotADirectory, got {other:?}"),
        }
    }

    #[test]
    fn notes_finds_nested_md_and_ignores_junk() {
        let (dir, vault) = vault();
        let root = dir.path();
        std::fs::write(root.join("top.md"), "a").unwrap();
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("sub/deep/nested.md"), "b").unwrap();
        // Non-md and ignored trees must not appear.
        std::fs::write(root.join("readme.txt"), "c").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config.md"), "d").unwrap();
        std::fs::create_dir_all(root.join(".trash")).unwrap();
        std::fs::write(root.join(".trash/old.md"), "e").unwrap();
        std::fs::create_dir_all(root.join(".jotter")).unwrap();
        std::fs::write(root.join(".jotter/state.md"), "f").unwrap();
        std::fs::write(root.join(".hidden.md"), "g").unwrap();
        std::fs::create_dir_all(root.join(".secret")).unwrap();
        std::fs::write(root.join(".secret/x.md"), "h").unwrap();

        let found = rel_paths(vault.notes().unwrap());
        assert_eq!(
            found,
            vec![PathBuf::from("sub/deep/nested.md"), PathBuf::from("top.md")]
        );
    }

    #[test]
    fn notes_returns_mtime_and_size() {
        let (dir, vault) = vault();
        std::fs::write(dir.path().join("n.md"), "hello world").unwrap();
        let entries = vault.notes().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, "hello world".len() as u64);
        assert!(entries[0].mtime > 0);
    }

    #[test]
    fn read_write_roundtrip() {
        let (_dir, vault) = vault();
        vault.write_note("a.md", "first").unwrap();
        assert_eq!(vault.read_note("a.md").unwrap(), "first");
    }

    #[test]
    fn write_overwrites_atomically() {
        let (_dir, vault) = vault();
        vault.write_note("a.md", "old content longer").unwrap();
        vault.write_note("a.md", "new").unwrap();
        assert_eq!(vault.read_note("a.md").unwrap(), "new");
    }

    #[test]
    fn write_leaves_no_temp_files() {
        let (dir, vault) = vault();
        vault.write_note("a.md", "x").unwrap();
        let stray: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(".jotter-tmp"))
            .collect();
        assert!(stray.is_empty(), "temp files left behind: {stray:?}");
    }

    #[test]
    fn create_fails_on_existing() {
        let (_dir, vault) = vault();
        vault.create_note("a.md", "one").unwrap();
        match vault.create_note("a.md", "two") {
            Err(VaultError::AlreadyExists(p)) => assert_eq!(p, PathBuf::from("a.md")),
            other => panic!("expected AlreadyExists, got {other:?}"),
        }
        // Original content is untouched.
        assert_eq!(vault.read_note("a.md").unwrap(), "one");
    }

    #[test]
    fn create_makes_parent_dirs() {
        let (dir, vault) = vault();
        vault.create_note("x/y/z.md", "deep").unwrap();
        assert!(dir.path().join("x/y/z.md").is_file());
        assert_eq!(vault.read_note("x/y/z.md").unwrap(), "deep");
    }

    #[test]
    fn rename_moves_content() {
        let (dir, vault) = vault();
        vault.create_note("from.md", "body").unwrap();
        vault.rename_note("from.md", "to/here.md").unwrap();
        assert!(!dir.path().join("from.md").exists());
        assert_eq!(vault.read_note("to/here.md").unwrap(), "body");
    }

    #[test]
    fn delete_moves_to_trash() {
        let (dir, vault) = vault();
        vault.create_note("gone.md", "bye").unwrap();
        vault.delete_to_trash("gone.md").unwrap();
        assert!(!dir.path().join("gone.md").exists());
        let trashed = dir.path().join(".trash/gone.md");
        assert!(trashed.is_file());
        assert_eq!(std::fs::read_to_string(trashed).unwrap(), "bye");
    }

    #[test]
    fn delete_avoids_trash_collisions() {
        let (dir, vault) = vault();
        vault.create_note("dup.md", "first").unwrap();
        vault.delete_to_trash("dup.md").unwrap();
        vault.create_note("dup.md", "second").unwrap();
        vault.delete_to_trash("dup.md").unwrap();

        let trash = dir.path().join(".trash");
        let names: Vec<String> = std::fs::read_dir(&trash)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "expected two trashed files, got {names:?}");
        assert!(names.iter().any(|n| n == "dup.md"));
        assert!(names.iter().any(|n| n == "dup-1.md"));
    }

    #[test]
    fn escaping_paths_are_rejected() {
        let (_dir, vault) = vault();
        for bad in ["../outside.md", "a/../../b.md", "/etc/passwd"] {
            match vault.read_note(bad) {
                Err(VaultError::PathEscapesRoot(_)) => {}
                other => panic!("expected PathEscapesRoot for {bad}, got {other:?}"),
            }
        }
    }

    #[test]
    fn curdir_component_is_allowed() {
        let (_dir, vault) = vault();
        vault.write_note("./a.md", "ok").unwrap();
        assert_eq!(vault.read_note("a.md").unwrap(), "ok");
    }

    #[test]
    fn non_utf8_read_reports_not_utf8() {
        let (dir, vault) = vault();
        std::fs::write(dir.path().join("bin.md"), [0xff, 0xfe, 0x00]).unwrap();
        match vault.read_note("bin.md") {
            Err(VaultError::NotUtf8(p)) => assert_eq!(p, Path::new("bin.md")),
            other => panic!("expected NotUtf8, got {other:?}"),
        }
    }
}
