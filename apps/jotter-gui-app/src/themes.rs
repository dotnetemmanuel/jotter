//! Which themes jotter can offer: the bundled ones, plus whatever the user has
//! dropped in `~/.config/jotter/themes/`.
//!
//! A user theme with the same id as a bundled one replaces it, which is how a
//! tweaked copy of a shipped theme is meant to work.

use std::path::{Path, PathBuf};

use jotter_theming::ThemeFile;

/// One theme jotter can switch to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Theme id, as written in the file and stored in the config.
    pub id: String,
    /// Where it came from: `None` for a bundled theme.
    pub path: Option<PathBuf>,
}

/// The themes folder inside the config directory.
#[must_use]
pub fn user_dir() -> PathBuf {
    crate::config::config_dir().join("themes")
}

/// Every theme available, bundled first, then user themes by file name.
#[must_use]
pub fn available() -> Vec<Entry> {
    let mut entries: Vec<Entry> = jotter_theming::bundled::BUNDLED
        .iter()
        .map(|bundled| Entry {
            id: bundled.id.to_string(),
            path: None,
        })
        .collect();
    entries.extend(user_themes(&user_dir()));
    entries
}

/// Themes found in `dir`, sorted by id, replacing bundled ids they shadow.
fn user_themes(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Entry> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let id = id_in(&path)?;
            Some(Entry {
                id,
                path: Some(path),
            })
        })
        .collect();
    found.sort_by(|one, other| one.id.cmp(&other.id));
    found
}

/// The id a theme file declares, or `None` if it will not parse.
fn id_in(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let file = ThemeFile::from_jsonc(&text).ok()?;
    Some(file.id)
}

/// Loads the theme file for `id`, preferring a user theme over a bundled one.
///
/// # Errors
/// Returns a [`jotter_theming::ThemeError`] if the file will not parse.
pub fn load(id: &str) -> Result<ThemeFile, jotter_theming::ThemeError> {
    if let Some(entry) = user_themes(&user_dir()).into_iter().find(|entry| entry.id == id)
        && let Some(path) = entry.path
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        return ThemeFile::from_jsonc(&text);
    }
    let bundled = jotter_theming::bundled::BUNDLED
        .iter()
        .find(|bundled| bundled.id == id)
        .unwrap_or(&jotter_theming::bundled::BUNDLED[0]);
    ThemeFile::from_jsonc(bundled.source)
}

#[cfg(test)]
mod tests {
    use super::{available, load, user_themes};
    use tempfile::TempDir;

    #[test]
    fn the_bundled_themes_are_always_offered() {
        let ids: Vec<String> = available().into_iter().map(|entry| entry.id).collect();
        assert!(ids.contains(&"retro82".to_string()));
        assert!(ids.contains(&"event-horizon".to_string()));
    }

    #[test]
    fn a_missing_themes_folder_is_not_an_error() {
        let tmp = TempDir::new().unwrap();
        assert!(user_themes(&tmp.path().join("nothing-here")).is_empty());
    }

    #[test]
    fn files_that_are_not_themes_are_skipped() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("notes.md"), "# not a theme").unwrap();
        std::fs::write(tmp.path().join("broken.json"), "{ not json").unwrap();
        assert!(user_themes(tmp.path()).is_empty());
    }

    #[test]
    fn a_user_theme_is_found_by_the_id_inside_it() {
        let tmp = TempDir::new().unwrap();
        let source = jotter_theming::bundled::BUNDLED[0]
            .source
            .replacen("\"retro82\"", "\"mine\"", 1);
        std::fs::write(tmp.path().join("whatever-the-file-is-called.json"), source).unwrap();

        let found = user_themes(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "mine");
    }

    #[test]
    fn an_unknown_id_falls_back_to_the_default_theme() {
        assert_eq!(load("no-such-theme").unwrap().id, "retro82");
    }

    #[test]
    fn a_bundled_id_loads_that_theme() {
        assert_eq!(load("event-horizon").unwrap().id, "event-horizon");
    }
}
