#![warn(clippy::pedantic)]
//! Where jotter keeps its config and data, shared by the GUI and terminal frontends.
//!
//! The directory is XDG on Linux and macOS (`$XDG_CONFIG_HOME` or `~/.config`, and
//! the data equivalent under `~/.local/share`), and the native Windows folder
//! (`%APPDATA%`) there, via the etcetera crate's default CLI strategy.
//! `JOTTER_CONFIG_DIR` and `JOTTER_DATA_DIR` override the answer outright, for
//! tests that must not touch a real user's files.

use std::path::{Path, PathBuf};

use etcetera::BaseStrategy;
use thiserror::Error;

/// Errors from locating the platform base directory.
#[derive(Debug, Error)]
pub enum PathsError {
    /// The platform gave no home directory to build config or data paths under.
    #[error("could not locate the home directory: {0}")]
    HomeDir(#[from] etcetera::HomeDirError),
}

/// The directory jotter keeps its own configuration in.
///
/// `JOTTER_CONFIG_DIR`, if set to a non-empty value, is used verbatim instead.
///
/// # Errors
///
/// Returns [`PathsError`] if the platform cannot locate the user's home directory.
pub fn config_dir() -> Result<PathBuf, PathsError> {
    let base = etcetera::choose_base_strategy()?.config_dir();
    Ok(resolve(std::env::var("JOTTER_CONFIG_DIR").ok().as_deref(), &base))
}

/// The directory jotter keeps its own data in.
///
/// `JOTTER_DATA_DIR`, if set to a non-empty value, is used verbatim instead.
///
/// # Errors
///
/// Returns [`PathsError`] if the platform cannot locate the user's home directory.
pub fn data_dir() -> Result<PathBuf, PathsError> {
    let base = etcetera::choose_base_strategy()?.data_dir();
    Ok(resolve(std::env::var("JOTTER_DATA_DIR").ok().as_deref(), &base))
}

/// Resolves a directory from an optional override and the platform base directory.
///
/// A non-empty override is used exactly as given, with no `jotter` component
/// appended. Otherwise `jotter` is joined onto `base`. An empty override is
/// treated as absent, since an unset and an empty shell variable arrive
/// identically here.
#[must_use]
pub fn resolve(override_dir: Option<&str>, base: &Path) -> PathBuf {
    match override_dir {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => base.join("jotter"),
    }
}
