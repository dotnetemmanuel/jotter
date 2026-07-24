//! Error type for vault operations.

use std::path::PathBuf;

/// Errors returned by vault operations.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The vault root does not exist or is not a directory.
    #[error("vault root is not a directory: {0}")]
    NotADirectory(PathBuf),

    /// A relative path escaped the vault root (contained `..` or was absolute).
    #[error("path escapes the vault root: {0}")]
    PathEscapesRoot(PathBuf),

    /// A create was requested but the target already exists.
    #[error("note already exists: {0}")]
    AlreadyExists(PathBuf),

    /// The underlying filesystem operation failed.
    #[error("io error at {path}: {source}")]
    Io {
        /// The path the operation was acting on.
        path: PathBuf,
        /// The underlying io error.
        source: std::io::Error,
    },

    /// A note was read but was not valid UTF-8.
    #[error("note is not valid utf-8: {0}")]
    NotUtf8(PathBuf),

    /// Setting up or running the filesystem watcher failed.
    #[error("watcher error: {0}")]
    Watch(#[from] notify::Error),
}

impl VaultError {
    /// Wraps an io error together with the path it occurred on.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
