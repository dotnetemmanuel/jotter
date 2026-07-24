//! The ignore rule shared by enumeration and the watcher.

use std::path::Path;

/// Directory and file names that are never notes, matched anywhere in the tree.
const IGNORED_NAMES: [&str; 3] = [".git", ".trash", ".jotter"];

/// Returns true when a single path component should be ignored.
///
/// Any name starting with a dot is ignored (this subsumes the explicit list,
/// but the list is kept for clarity and to document intent).
#[must_use]
pub fn is_ignored_component(name: &str) -> bool {
    name.starts_with('.') || IGNORED_NAMES.contains(&name)
}

/// Returns true when any component of `rel_path` is ignored.
///
/// `rel_path` is expected to be relative to the vault root. A path is ignored
/// if it sits inside a dotdir, is a dotfile, or lives under `.git` / `.trash` /
/// `.jotter` at any depth.
#[must_use]
pub fn is_ignored_path(rel_path: &Path) -> bool {
    rel_path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_none_or(is_ignored_component)
    })
}

#[cfg(test)]
mod tests {
    use super::{is_ignored_component, is_ignored_path};
    use std::path::Path;

    #[test]
    fn plain_names_are_kept() {
        assert!(!is_ignored_component("notes"));
        assert!(!is_ignored_component("Daily 2026.md"));
    }

    #[test]
    fn dot_names_are_ignored() {
        assert!(is_ignored_component(".git"));
        assert!(is_ignored_component(".trash"));
        assert!(is_ignored_component(".jotter"));
        assert!(is_ignored_component(".hidden"));
        assert!(is_ignored_component(".DS_Store"));
    }

    #[test]
    fn nested_dotdir_ignores_whole_subtree() {
        assert!(is_ignored_path(Path::new(".git/config")));
        assert!(is_ignored_path(Path::new("sub/.trash/old.md")));
        assert!(is_ignored_path(Path::new("a/b/.jotter/index.db")));
        assert!(is_ignored_path(Path::new("a/.hidden/note.md")));
    }

    #[test]
    fn ordinary_nested_paths_are_kept() {
        assert!(!is_ignored_path(Path::new("a/b/c.md")));
        assert!(!is_ignored_path(Path::new("note.md")));
    }
}
