//! Which folders jotter already treats as vaults, and what to say before it
//! adopts a new one.
//!
//! Opening a folder writes a `.jotter` directory into it and indexes every
//! markdown file underneath, which is not something to do to a folder someone
//! pointed at by accident.

use std::path::{Path, PathBuf};

/// Enough notes to answer "is this a notes folder or my home directory?"
/// without walking a huge tree to the end.
const COUNT_CAP: usize = 500;

/// The jotter state directory, whose presence means the folder is already a vault.
const MARKER: &str = ".jotter";

/// A vault jotter knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    /// Absolute path.
    pub path: PathBuf,
    /// Last path component, which is what the picker shows first.
    pub name: String,
}

/// Whether `root` has been opened as a vault before.
#[must_use]
pub fn is_vault(root: &Path) -> bool {
    root.join(MARKER).is_dir()
}

/// The known vaults still on disk, most recent first.
///
/// A remembered path whose folder has gone is dropped rather than offered: the
/// picker should never show a row that cannot open.
#[must_use]
pub fn known(recents: &[String]) -> Vec<Known> {
    recents
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .map(|path| Known {
            name: name_of(&path),
            path,
        })
        .collect()
}

/// The name to show for a vault: its folder name, or the path when there is none.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

/// How many markdown files a folder holds, counting no further than the cap.
///
/// Used only to tell the user what they are about to index, so an exact answer
/// for a tree of fifty thousand files is not worth the walk.
#[must_use]
pub fn count_notes(root: &Path) -> (usize, bool) {
    let Ok(vault) = jotter_vault::Vault::open(root) else {
        return (0, false);
    };
    let Ok(notes) = vault.notes() else {
        return (0, false);
    };
    let total = notes.len();
    (total.min(COUNT_CAP), total > COUNT_CAP)
}

/// What to ask before adopting a folder as a vault.
#[must_use]
pub fn adopt_question(root: &Path, notes: usize, more: bool) -> String {
    let count = match (notes, more) {
        (_, true) => format!("more than {COUNT_CAP} notes"),
        (0, _) => "no notes yet".to_string(),
        (1, _) => "1 note".to_string(),
        (many, _) => format!("{many} notes"),
    };
    format!(
        "Open \"{}\" as a jotter vault?\n\nIt holds {count}. jotter will index them and keep its index in a .jotter folder inside.",
        root.display()
    )
}

#[cfg(test)]
mod tests {
    use super::{COUNT_CAP, adopt_question, is_vault, known, name_of};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn a_folder_with_no_marker_is_not_a_vault_yet() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_vault(tmp.path()));
        std::fs::create_dir(tmp.path().join(".jotter")).unwrap();
        assert!(is_vault(tmp.path()));
    }

    #[test]
    fn a_vault_that_has_gone_is_not_offered() {
        let tmp = TempDir::new().unwrap();
        let gone = tmp.path().join("deleted-since");
        let here = tmp.path().join("still-here");
        std::fs::create_dir(&here).unwrap();

        let found = known(&[gone.display().to_string(), here.display().to_string()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, here);
    }

    #[test]
    fn a_vault_is_named_by_its_folder() {
        assert_eq!(name_of(Path::new("/home/you/notes")), "notes");
    }

    #[test]
    fn the_question_counts_the_notes() {
        let question = adopt_question(Path::new("/tmp/notes"), 12, false);
        assert!(question.contains("/tmp/notes"));
        assert!(question.contains("12 notes"));
    }

    #[test]
    fn one_note_reads_singular_and_none_says_so() {
        assert!(adopt_question(Path::new("/tmp/a"), 1, false).contains("1 note."));
        assert!(adopt_question(Path::new("/tmp/a"), 0, false).contains("no notes yet"));
    }

    #[test]
    fn a_huge_folder_says_more_rather_than_a_wrong_number() {
        let question = adopt_question(Path::new("/home/you"), COUNT_CAP, true);
        assert!(question.contains(&format!("more than {COUNT_CAP}")));
    }
}
