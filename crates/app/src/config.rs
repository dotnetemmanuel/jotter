//! Global config at `~/.config/jotter/config.toml`: recent vaults and per-vault
//! last-active note. IO failures are non-fatal (logged to stderr, defaults used).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many recent vaults to keep. Older entries drop off the end.
const MAX_RECENTS: usize = 20;

/// Persisted global configuration for jotter.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Recent vault roots, most-recent-first, deduped, capped at [`MAX_RECENTS`].
    #[serde(default)]
    pub recent_vaults: Vec<String>,
    /// Last-active note (vault-relative path) keyed by absolute vault root.
    #[serde(default)]
    pub last_active: BTreeMap<String, String>,
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
    let mut out = Vec::with_capacity(existing.len() + 1);
    for entry in existing {
        if entry != key {
            out.push(entry);
        }
    }
    // Prepend the key (moved, not cloned) so it becomes the most-recent entry.
    out.insert(0, key);
    out.truncate(MAX_RECENTS);
    out
}

#[cfg(test)]
mod tests {
    use super::{MAX_RECENTS, push_recent};

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
