//! Debounced, recursive filesystem watcher over the vault root.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use notify::EventKind;
use notify::event::{ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::error::VaultError;
use crate::ignore::is_ignored_path;

/// The default debounce window. Filesystem editors often touch a file several
/// times per save, so a short debounce collapses those into one event.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// A vault-relative filesystem change emitted by the watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultChange {
    /// A note appeared.
    Created(PathBuf),
    /// A note was written to.
    Modified(PathBuf),
    /// A note disappeared.
    Removed(PathBuf),
    /// A note moved from one relative path to another.
    Renamed {
        /// The old vault-relative path.
        from: PathBuf,
        /// The new vault-relative path.
        to: PathBuf,
    },
}

/// A running watcher. Hold this guard for as long as changes should be
/// delivered. Dropping it stops the watch thread.
///
/// The type parameters are fixed to the recommended platform watcher and cache
/// so callers do not have to name them.
pub struct WatchGuard {
    // Kept alive to keep the background thread watching. Named to document intent.
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

/// Starts a recursive debounced watch on `root`.
///
/// Returns the receiver to drain (synchronous, drain it on a timer) and a guard
/// the caller must hold. Events whose path is ignored (dotfiles, `.git`,
/// `.trash`, `.jotter`) are dropped before delivery. Paths are made relative to
/// `root`; an event outside `root` is dropped.
///
/// # Errors
/// Returns [`VaultError::Watch`] if the platform watcher cannot be created or the
/// root cannot be watched.
pub fn watch(root: &Path) -> Result<(Receiver<VaultChange>, WatchGuard), VaultError> {
    watch_with_debounce(root, DEFAULT_DEBOUNCE)
}

/// Like [`watch`] but with a caller-chosen debounce window. Exposed mainly so
/// tests can widen the window.
///
/// # Errors
/// Returns [`VaultError::Watch`] if the watcher cannot be created or armed.
pub fn watch_with_debounce(
    root: &Path,
    debounce: Duration,
) -> Result<(Receiver<VaultChange>, WatchGuard), VaultError> {
    let watch_root = root.to_path_buf();
    let filter_root = watch_root.clone();
    let (tx, rx) = std::sync::mpsc::channel::<VaultChange>();

    let handler = move |result: DebounceEventResult| {
        let Ok(events) = result else {
            // Errors from the debouncer are reported per batch. We drop them
            // here; the caller re-enumerates on demand, so a lost event is not
            // fatal. Nothing to send.
            return;
        };
        for change in events
            .into_iter()
            .flat_map(|event| translate(&filter_root, &event))
        {
            // If the receiver is gone the caller stopped caring; stop trying.
            if tx.send(change).is_err() {
                return;
            }
        }
    };

    let mut debouncer = new_debouncer(debounce, None, handler)?;
    debouncer.watch(&watch_root, RecursiveMode::Recursive)?;

    Ok((
        rx,
        WatchGuard {
            _debouncer: debouncer,
        },
    ))
}

/// Turns one debounced event into zero or more vault-relative changes.
fn translate(root: &Path, event: &notify::Event) -> Vec<VaultChange> {
    match event.kind {
        EventKind::Create(_) => single(root, &event.paths, VaultChange::Created),
        EventKind::Remove(_) => single(root, &event.paths, VaultChange::Removed),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => rename(root, &event.paths),
        // A one-sided rename half looks like a create or a remove to us.
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            single(root, &event.paths, VaultChange::Created)
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            single(root, &event.paths, VaultChange::Removed)
        }
        EventKind::Modify(_) => single(root, &event.paths, VaultChange::Modified),
        // Access and Other events carry no note-content meaning.
        EventKind::Access(_) | EventKind::Any | EventKind::Other => Vec::new(),
    }
}

/// Builds one change from the last path of the event, if it survives the filter.
fn single(root: &Path, paths: &[PathBuf], make: fn(PathBuf) -> VaultChange) -> Vec<VaultChange> {
    paths
        .last()
        .and_then(|abs| relativize_kept(root, abs))
        .map(make)
        .into_iter()
        .collect()
}

/// Builds a rename change from a two-path event, filtering each side.
fn rename(root: &Path, paths: &[PathBuf]) -> Vec<VaultChange> {
    let from = paths.first().and_then(|abs| relativize_kept(root, abs));
    let to = paths.get(1).and_then(|abs| relativize_kept(root, abs));
    match (from, to) {
        (Some(from), Some(to)) => vec![VaultChange::Renamed { from, to }],
        // If one side is ignored, treat the kept side as create or remove.
        (Some(from), None) => vec![VaultChange::Removed(from)],
        (None, Some(to)) => vec![VaultChange::Created(to)],
        (None, None) => Vec::new(),
    }
}

/// Relativizes `abs` against `root`, returning `None` if it is outside the root
/// or if the relative path is ignored.
fn relativize_kept(root: &Path, abs: &Path) -> Option<PathBuf> {
    let rel = abs.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() || is_ignored_path(rel) {
        return None;
    }
    Some(rel.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{VaultChange, watch_with_debounce};
    use std::time::Duration;
    use tempfile::TempDir;

    // Filesystem event timing varies by platform and load, so this is opt-in
    // (run with `cargo test -- --ignored`) and tolerant. It must never hang.
    #[test]
    #[ignore = "timing-dependent filesystem watcher smoke test"]
    fn watcher_reports_a_create() {
        let dir = TempDir::new().unwrap();
        let (rx, _guard) =
            watch_with_debounce(dir.path(), Duration::from_millis(100)).expect("watch");

        std::fs::write(dir.path().join("new.md"), "hello").unwrap();

        // Drain for up to a couple of seconds looking for any change to new.md.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut saw = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(change) => {
                    let hit = matches!(
                        &change,
                        VaultChange::Created(p) | VaultChange::Modified(p)
                            if p.as_os_str() == "new.md"
                    );
                    if hit {
                        saw = true;
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(saw, "expected a create or modify event for new.md");
    }
}
