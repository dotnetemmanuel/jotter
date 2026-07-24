//! File-tree model for the sidebar.
//!
//! Each node is a `gtk::StringObject` whose string is the vault-relative path of a
//! folder or a `.md` file (the empty string is the root). Children are computed
//! lazily by scanning one directory level on demand, so a large vault never loads
//! entirely into the tree. Folders sort before files, both alphabetical.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use gtk::gio;

use jotter_vault::is_ignored_component;

/// One directory entry discovered when scanning a folder for tree children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The single name (no path), for display and sorting.
    pub name: String,
    /// True for a directory, false for a `.md` file.
    pub is_dir: bool,
}

/// Scans one directory level under `root` at vault-relative `rel`, returning the
/// folders and `.md` files that belong in the tree, sorted (folders first, then
/// alphabetical, case-insensitive). Ignored names (dotfiles, `.git`, `.trash`,
/// `.jotter`) and non-markdown files are dropped. IO errors yield an empty list.
#[must_use]
pub fn scan_children(root: &Path, rel: &Path) -> Vec<Entry> {
    let dir = root.join(rel);
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(err) => {
            eprintln!("jotter: could not read dir {}: {err}", dir.display());
            return Vec::new();
        }
    };

    let mut entries: Vec<Entry> = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if is_ignored_component(&name) {
            continue;
        }
        let is_dir = item.file_type().is_ok_and(|t| t.is_dir());
        // Keep folders (they may hold notes) and markdown files; drop other files.
        if is_dir || is_markdown_name(&name) {
            entries.push(Entry { name, is_dir });
        }
    }
    sort_entries(&mut entries);
    entries
}

/// True if `name` ends in a case-insensitive `.md` extension.
fn is_markdown_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Orders entries: directories before files, then case-insensitive by name.
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
}

/// True if the vault-relative `rel` names a directory under `root`.
#[must_use]
pub fn is_dir_path(root: &Path, rel: &str) -> bool {
    // The empty path is the root itself, which is always a directory.
    if rel.is_empty() {
        return true;
    }
    root.join(rel).is_dir()
}

/// Joins a parent vault-relative path and a child name with a forward slash.
///
/// The root (empty parent) yields just the child. Kept as `/`-joined strings so
/// the values round-trip through `gtk::StringObject` regardless of platform.
#[must_use]
pub fn join_rel(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}/{child}")
    }
}

/// Builds the root `gio::ListStore` of top-level nodes for a vault `root`.
///
/// Each node is a `StringObject` holding a vault-relative path.
#[must_use]
pub fn root_store(root: &Path) -> gio::ListStore {
    let store = gio::ListStore::new::<gtk::StringObject>();
    for entry in scan_children(root, Path::new("")) {
        store.append(&gtk::StringObject::new(&entry.name));
    }
    store
}

/// Builds the children `gio::ListStore` for the folder at vault-relative `rel`,
/// or `None` for a file node (files have no children). Used as the
/// `TreeListModel` create-child function.
#[must_use]
pub fn child_store(root: &Path, rel: &str) -> Option<gio::ListStore> {
    if !is_dir_path(root, rel) {
        return None;
    }
    let store = gio::ListStore::new::<gtk::StringObject>();
    for entry in scan_children(root, &PathBuf::from(rel)) {
        store.append(&gtk::StringObject::new(&join_rel(rel, &entry.name)));
    }
    Some(store)
}

/// The display label for a node: its last path component, or the vault name for
/// the root. Empty falls back to the string as-is.
#[must_use]
pub fn label_for(rel: &str) -> &str {
    rel.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(rel)
}

#[cfg(test)]
mod tests {
    use super::{Entry, join_rel, label_for, scan_children, sort_entries};
    use std::path::Path;
    use tempfile::TempDir;

    fn dir(name: &str) -> Entry {
        Entry { name: name.into(), is_dir: true }
    }
    fn file(name: &str) -> Entry {
        Entry { name: name.into(), is_dir: false }
    }

    #[test]
    fn folders_sort_before_files_then_alpha() {
        let mut entries = vec![
            file("zeta.md"),
            dir("beta"),
            file("alpha.md"),
            dir("Alpha"),
        ];
        sort_entries(&mut entries);
        assert_eq!(
            entries,
            vec![dir("Alpha"), dir("beta"), file("alpha.md"), file("zeta.md")]
        );
    }

    #[test]
    fn join_rel_handles_root_and_nested() {
        assert_eq!(join_rel("", "a.md"), "a.md");
        assert_eq!(join_rel("sub", "a.md"), "sub/a.md");
        assert_eq!(join_rel("a/b", "c.md"), "a/b/c.md");
    }

    #[test]
    fn label_for_takes_last_component() {
        assert_eq!(label_for("a/b/c.md"), "c.md");
        assert_eq!(label_for("top.md"), "top.md");
        assert_eq!(label_for("folder"), "folder");
    }

    #[test]
    fn scan_skips_ignored_and_non_markdown() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("keep.md"), "x").unwrap();
        std::fs::write(root.join("skip.txt"), "x").unwrap();
        std::fs::write(root.join(".hidden.md"), "x").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();

        let entries = scan_children(root, Path::new(""));
        assert_eq!(
            entries,
            vec![
                Entry { name: "sub".into(), is_dir: true },
                Entry { name: "keep.md".into(), is_dir: false },
            ]
        );
    }
}
