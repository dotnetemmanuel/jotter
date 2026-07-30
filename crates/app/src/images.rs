//! Where an image written in a note actually lives.
//!
//! Notes written in Obsidian name an image by filename alone and rely on the
//! whole vault being searched, so `github.png` finds `images/github.png`. That is
//! the same resolution jotter already gives `[[wikilinks]]`, so images get it too:
//! relative to the note first, because a path that says what it means should win,
//! then by filename anywhere in the vault.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use jotter_parser::ImageLocator;

/// Resolves image sources for one render.
pub struct Locator<'a> {
    /// The folder holding the note, tried first.
    base: Option<PathBuf>,
    /// Vault root to search by filename, absent outside a vault.
    vault: Option<&'a Path>,
    /// Filename to result, so a vault is walked once per name per session.
    seen: &'a RefCell<HashMap<String, Option<PathBuf>>>,
}

impl<'a> Locator<'a> {
    /// Builds a locator for a note in `base`, searching `vault` when a relative
    /// path finds nothing.
    pub fn new(
        base: Option<PathBuf>,
        vault: Option<&'a Path>,
        seen: &'a RefCell<HashMap<String, Option<PathBuf>>>,
    ) -> Self {
        Self { base, vault, seen }
    }
}

impl ImageLocator for Locator<'_> {
    fn locate(&self, src: &str) -> Option<PathBuf> {
        if let Some(base) = &self.base {
            let beside = base.join(src);
            if beside.is_file() {
                return Some(beside);
            }
        }
        let name = Path::new(src).file_name()?.to_str()?.to_owned();
        if let Some(found) = self.seen.borrow().get(&name) {
            return found.clone().or_else(|| self.fallback(src));
        }
        let found = self.vault.and_then(|root| find_by_name(root, &name));
        self.seen.borrow_mut().insert(name, found.clone());
        found.or_else(|| self.fallback(src))
    }
}

impl Locator<'_> {
    /// Nothing was found, so point at where the note said and let it show as
    /// broken there rather than against the preview's own cache directory.
    fn fallback(&self, src: &str) -> Option<PathBuf> {
        self.base.as_ref().map(|base| base.join(src))
    }
}

/// The first file named `name` anywhere under `root`, ignoring the usual places.
///
/// Breadth is not worth the complexity: a vault holding two images of the same
/// name is already ambiguous, and Obsidian picks one too.
fn find_by_name(root: &Path, name: &str) -> Option<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(as_str) = file_name.to_str() else {
                continue;
            };
            if jotter_vault::is_ignored_component(as_str) {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                dirs.push(path);
            } else if as_str == name {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Locator, find_by_name};
    use jotter_parser::ImageLocator;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// A vault with an image in a subfolder, the shape Obsidian notes use.
    fn vault() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("images")).unwrap();
        std::fs::write(tmp.path().join("images/github.png"), "x").unwrap();
        std::fs::create_dir_all(tmp.path().join(".jotter")).unwrap();
        std::fs::write(tmp.path().join(".jotter/github.png"), "x").unwrap();
        std::fs::write(tmp.path().join("beside.png"), "x").unwrap();
        tmp
    }

    #[test]
    fn a_bare_filename_is_found_in_a_subfolder() {
        let tmp = vault();
        let seen = RefCell::new(HashMap::new());
        let locator = Locator::new(Some(tmp.path().to_path_buf()), Some(tmp.path()), &seen);
        assert_eq!(locator.locate("github.png"), Some(tmp.path().join("images/github.png")));
    }

    #[test]
    fn a_path_relative_to_the_note_wins_over_the_search() {
        let tmp = vault();
        let seen = RefCell::new(HashMap::new());
        let locator = Locator::new(Some(tmp.path().to_path_buf()), Some(tmp.path()), &seen);
        assert_eq!(locator.locate("beside.png"), Some(tmp.path().join("beside.png")));
    }

    #[test]
    fn a_subfolder_path_that_exists_is_used_as_written() {
        let tmp = vault();
        let seen = RefCell::new(HashMap::new());
        let locator = Locator::new(Some(tmp.path().to_path_buf()), Some(tmp.path()), &seen);
        assert_eq!(
            locator.locate("images/github.png"),
            Some(tmp.path().join("images/github.png"))
        );
    }

    #[test]
    fn the_search_skips_ignored_folders() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".jotter")).unwrap();
        std::fs::write(tmp.path().join(".jotter/hidden.png"), "x").unwrap();
        assert_eq!(find_by_name(tmp.path(), "hidden.png"), None);
    }

    #[test]
    fn a_missing_image_points_at_the_note_folder() {
        let tmp = vault();
        let seen = RefCell::new(HashMap::new());
        let locator = Locator::new(Some(tmp.path().to_path_buf()), Some(tmp.path()), &seen);
        // Not found anywhere, so it resolves beside the note and shows as broken
        // there rather than against the preview cache directory.
        assert_eq!(locator.locate("nope.png"), Some(tmp.path().join("nope.png")));
    }

    #[test]
    fn the_result_is_cached_per_filename() {
        let tmp = vault();
        let seen = RefCell::new(HashMap::new());
        let locator = Locator::new(Some(tmp.path().to_path_buf()), Some(tmp.path()), &seen);
        locator.locate("github.png");
        assert!(seen.borrow().contains_key("github.png"));
        // Removing the file does not change the answer while it is cached.
        std::fs::remove_file(tmp.path().join("images/github.png")).unwrap();
        assert_eq!(locator.locate("github.png"), Some(tmp.path().join("images/github.png")));
    }

    #[test]
    fn with_no_vault_only_the_note_folder_is_tried() {
        let tmp = vault();
        let seen = RefCell::new(HashMap::new());
        let locator = Locator::new(Some(tmp.path().to_path_buf()), None, &seen);
        assert_eq!(locator.locate("github.png"), Some(tmp.path().join("github.png")));
    }
}
