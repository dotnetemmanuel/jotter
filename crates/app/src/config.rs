//! Global config at `~/.config/jotter/config.toml`: recent vaults and per-vault
//! last-active note. IO failures are non-fatal (logged to stderr, defaults used).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many recent vaults to keep. Older entries drop off the end.
const MAX_RECENTS: usize = 20;

/// How many recently opened notes to keep per vault, for the quick switcher.
const MAX_RECENT_NOTES: usize = 10;

/// Persisted global configuration for jotter.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Recent vault roots, most-recent-first, deduped, capped at [`MAX_RECENTS`].
    #[serde(default)]
    pub recent_vaults: Vec<String>,
    /// Last-active note (vault-relative path) keyed by absolute vault root.
    #[serde(default)]
    pub last_active: BTreeMap<String, String>,
    /// Recently opened notes, most-recent-first, keyed by absolute vault root.
    #[serde(default)]
    pub recent_notes: BTreeMap<String, Vec<String>>,
    /// Whether the backlinks strip under the editor is open.
    #[serde(default)]
    pub backlinks_expanded: bool,
    /// Open tree folders (vault-relative), keyed by absolute vault root.
    #[serde(default)]
    pub expanded_folders: BTreeMap<String, Vec<String>>,
}

impl Config {
    /// Loads config from `~/.config/jotter/config.toml`, or defaults on any miss.
    ///
    /// A missing file is normal (first run) and yields defaults silently. A
    /// present-but-unparseable file logs to stderr and still yields defaults.
    #[must_use]
    pub fn load() -> Self {
        let path = config_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                eprintln!("jotter: could not read config {}: {err}", path.display());
                return Self::default();
            }
        };
        match toml::from_str(&text) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("jotter: could not parse config {}: {err}", path.display());
                Self::default()
            }
        }
    }

    /// Writes config to disk, creating the parent directory. Errors are non-fatal.
    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "jotter: could not create config dir {}: {err}",
                parent.display()
            );
            return;
        }
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Err(err) = std::fs::write(&path, text) {
                    eprintln!("jotter: could not write config {}: {err}", path.display());
                }
            }
            Err(err) => eprintln!("jotter: could not serialize config: {err}"),
        }
    }

    /// Records `root` as the most-recent vault (deduped, capped).
    pub fn push_recent(&mut self, root: &Path) {
        let key = root.to_string_lossy().into_owned();
        self.recent_vaults = push_recent(std::mem::take(&mut self.recent_vaults), key);
    }

    /// The most-recent vault root, if any exists.
    #[must_use]
    pub fn most_recent_vault(&self) -> Option<PathBuf> {
        self.recent_vaults.first().map(PathBuf::from)
    }

    /// Records `rel` as the last-active note for vault `root`.
    pub fn set_last_active(&mut self, root: &Path, rel: &Path) {
        let key = root.to_string_lossy().into_owned();
        let value = rel.to_string_lossy().into_owned();
        self.last_active.insert(key, value);
    }

    /// The last-active note (vault-relative) for `root`, if recorded.
    #[must_use]
    pub fn last_active_for(&self, root: &Path) -> Option<PathBuf> {
        let key = root.to_string_lossy();
        self.last_active.get(key.as_ref()).map(PathBuf::from)
    }

    /// Records which folders of `root` are open in the tree.
    pub fn set_expanded_folders(&mut self, root: &Path, folders: &HashSet<String>) {
        let key = root.to_string_lossy().into_owned();
        // Sorted so the file does not churn between runs that opened the same folders.
        let mut folders: Vec<String> = folders.iter().cloned().collect();
        folders.sort();
        self.expanded_folders.insert(key, folders);
    }

    /// Folders of `root` that were open when it was last closed.
    #[must_use]
    pub fn expanded_folders_for(&self, root: &Path) -> HashSet<String> {
        let key = root.to_string_lossy();
        self.expanded_folders
            .get(key.as_ref())
            .map(|folders| folders.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Records `rel` as the most recently opened note of vault `root`.
    pub fn push_recent_note(&mut self, root: &Path, rel: &Path) {
        let key = root.to_string_lossy().into_owned();
        let value = rel.to_string_lossy().into_owned();
        let entry = self.recent_notes.entry(key).or_default();
        *entry = capped(std::mem::take(entry), value, MAX_RECENT_NOTES);
    }

    /// Recently opened notes of `root`, most-recent-first.
    #[must_use]
    pub fn recent_notes_for(&self, root: &Path) -> &[String] {
        let key = root.to_string_lossy();
        self.recent_notes
            .get(key.as_ref())
            .map_or(&[], Vec::as_slice)
    }
}

/// The absolute config file path under the user config dir.
fn config_path() -> PathBuf {
    let mut dir = gtk::glib::user_config_dir();
    dir.push("jotter");
    dir.push("config.toml");
    dir
}

/// Pure recents update: prepend `key`, drop an earlier duplicate, cap the length.
fn push_recent(existing: Vec<String>, key: String) -> Vec<String> {
    capped(existing, key, MAX_RECENTS)
}

fn capped(existing: Vec<String>, key: String, cap: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(existing.len() + 1);
    for entry in existing {
        if entry != key {
            out.push(entry);
        }
    }
    // Prepend the key (moved, not cloned) so it becomes the most-recent entry.
    out.insert(0, key);
    out.truncate(cap);
    out
}

#[cfg(test)]
mod tests {
    use super::{Config, MAX_RECENTS, MAX_RECENT_NOTES, push_recent};
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn recent_notes_are_most_recent_first() {
        let mut config = Config::default();
        config.push_recent_note(Path::new("/vault"), Path::new("a.md"));
        config.push_recent_note(Path::new("/vault"), Path::new("b.md"));
        assert_eq!(config.recent_notes_for(Path::new("/vault")), ["b.md", "a.md"]);
    }

    #[test]
    fn reopening_a_note_moves_it_to_the_front() {
        let mut config = Config::default();
        for name in ["a.md", "b.md", "a.md"] {
            config.push_recent_note(Path::new("/vault"), Path::new(name));
        }
        assert_eq!(config.recent_notes_for(Path::new("/vault")), ["a.md", "b.md"]);
    }

    #[test]
    fn recent_notes_are_capped() {
        let mut config = Config::default();
        for n in 0..=MAX_RECENT_NOTES {
            config.push_recent_note(Path::new("/vault"), Path::new(&format!("{n}.md")));
        }
        assert_eq!(
            config.recent_notes_for(Path::new("/vault")).len(),
            MAX_RECENT_NOTES
        );
    }

    #[test]
    fn recent_notes_are_per_vault() {
        let mut config = Config::default();
        config.push_recent_note(Path::new("/one"), Path::new("a.md"));
        config.push_recent_note(Path::new("/two"), Path::new("b.md"));
        assert_eq!(config.recent_notes_for(Path::new("/one")), ["a.md"]);
        assert_eq!(config.recent_notes_for(Path::new("/two")), ["b.md"]);
    }

    #[test]
    fn expanded_folders_round_trip_per_vault() {
        let mut config = Config::default();
        let folders: HashSet<String> = ["notes".to_string(), "archive/old".to_string()]
            .into_iter()
            .collect();
        config.set_expanded_folders(Path::new("/one"), &folders);
        assert_eq!(config.expanded_folders_for(Path::new("/one")), folders);
        assert!(config.expanded_folders_for(Path::new("/two")).is_empty());
    }

    #[test]
    fn expanded_folders_are_stored_sorted() {
        let mut config = Config::default();
        let folders: HashSet<String> = ["z".to_string(), "a".to_string()].into_iter().collect();
        config.set_expanded_folders(Path::new("/v"), &folders);
        assert_eq!(config.expanded_folders["/v"], ["a", "z"]);
    }

    #[test]
    fn closing_every_folder_is_remembered() {
        let mut config = Config::default();
        config.set_expanded_folders(Path::new("/v"), &HashSet::from(["notes".to_string()]));
        config.set_expanded_folders(Path::new("/v"), &HashSet::new());
        assert!(config.expanded_folders_for(Path::new("/v")).is_empty());
    }

    #[test]
    fn an_unknown_vault_has_no_recent_notes() {
        let config = Config::default();
        assert!(config.recent_notes_for(Path::new("/nowhere")).is_empty());
    }

    #[test]
    fn push_prepends_new_entry() {
        let out = push_recent(vec!["a".into(), "b".into()], "c".into());
        assert_eq!(out, vec!["c", "a", "b"]);
    }

    #[test]
    fn push_dedupes_moving_to_front() {
        let out = push_recent(vec!["a".into(), "b".into(), "c".into()], "b".into());
        assert_eq!(out, vec!["b", "a", "c"]);
    }

    #[test]
    fn push_is_idempotent_at_front() {
        let out = push_recent(vec!["a".into(), "b".into()], "a".into());
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn push_caps_length() {
        let mut recents: Vec<String> = (0..MAX_RECENTS).map(|n| n.to_string()).collect();
        recents = push_recent(recents, "new".into());
        assert_eq!(recents.len(), MAX_RECENTS);
        assert_eq!(recents.first().map(String::as_str), Some("new"));
        // The oldest entry fell off the end.
        assert!(!recents.iter().any(|e| e == &(MAX_RECENTS - 1).to_string()));
    }
}
