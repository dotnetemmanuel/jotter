#![warn(clippy::pedantic)]
//! Git for a jotter vault: what state the repository is in, and what jotter
//! should keep out of it.
//!
//! Local reads (status, ahead and behind, the working tree) go through `git2`
//! in-process. Anything touching a remote is left to the `git` binary elsewhere,
//! so the user's ssh agent, credential helpers, and signing keep working exactly
//! as they do in a terminal. That also means this crate builds without libgit2's
//! network transports.
//!
//! A vault with no repository is not an error: [`Repo::discover`] returns `None`
//! and the caller shows nothing about git at all.

use std::path::{Path, PathBuf};

use thiserror::Error;

mod ignore;

pub use ignore::write_ignores;

/// Errors surfaced by the crate.
#[derive(Debug, Error)]
pub enum GitError {
    /// A libgit2 call failed.
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
}

/// What changed about one path since the last commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Not in the last commit.
    New,
    /// In the last commit, with different content now.
    Modified,
    /// In the last commit, gone from the working tree.
    Deleted,
    /// Moved or renamed since the last commit.
    Renamed,
}

impl ChangeKind {
    /// The word shown beside the path.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::New => "new",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Renamed => "renamed",
        }
    }
}

/// One changed path, vault-relative with forward slashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Path relative to the vault root.
    pub path: String,
    /// How it differs from the last commit.
    pub kind: ChangeKind,
}

/// A snapshot of the repository, cheap enough to poll.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// Current branch, or a short commit id when the head is detached.
    pub branch: String,
    /// Tracking branch such as `origin/main`, absent when the branch tracks nothing.
    pub upstream: Option<String>,
    /// Commits the branch has that its upstream does not.
    pub ahead: usize,
    /// Commits the upstream has that the branch does not.
    pub behind: usize,
    /// Changed paths, sorted, so the list does not reshuffle between polls.
    pub changed: Vec<Change>,
}

impl Status {
    /// Whether the working tree matches the last commit.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.changed.is_empty()
    }
}

/// A vault's repository.
pub struct Repo {
    inner: git2::Repository,
    root: PathBuf,
}

impl Repo {
    /// Opens the repository whose working tree *is* `vault_root`, if there is one.
    ///
    /// Deliberately not a walk up the tree: a vault living inside a larger
    /// repository stays untouched, because committing everything from jotter
    /// would then sweep up whatever else that repository holds.
    #[must_use]
    pub fn discover(vault_root: &Path) -> Option<Self> {
        let inner = git2::Repository::open(vault_root).ok()?;
        let workdir = inner.workdir()?.canonicalize().ok()?;
        if workdir != vault_root.canonicalize().ok()? {
            return None;
        }
        Some(Self {
            inner,
            root: vault_root.to_path_buf(),
        })
    }

    /// The vault root, which is also the working tree.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads branch, tracking counts, and the changed paths.
    ///
    /// # Errors
    /// Returns [`GitError::Git`] if the working tree or the refs cannot be read.
    pub fn status(&self) -> Result<Status, GitError> {
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .include_ignored(false);

        let mut changed: Vec<Change> = self
            .inner
            .statuses(Some(&mut options))?
            .iter()
            .filter_map(|entry| {
                Some(Change {
                    path: entry.path().ok()?.to_string(),
                    kind: kind_of(entry.status())?,
                })
            })
            .collect();
        changed.sort_by(|one, other| one.path.cmp(&other.path));

        let (ahead, behind) = self.tracking()?;
        Ok(Status {
            branch: self.branch_name(),
            upstream: self.upstream_name(),
            ahead,
            behind,
            changed,
        })
    }

    /// The branch name, a short commit id when detached, or the branch a repo
    /// without commits is waiting to create.
    fn branch_name(&self) -> String {
        if let Ok(head) = self.inner.head()
            && let Ok(name) = head.shorthand()
        {
            return name.to_string();
        }
        // An unborn head has no target to shorten, but the ref it points at names
        // the branch the first commit will create.
        self.inner
            .find_reference("HEAD")
            .ok()
            .and_then(|head| head.symbolic_target().ok().flatten().map(str::to_string))
            .and_then(|target| target.strip_prefix("refs/heads/").map(str::to_string))
            .unwrap_or_default()
    }

    /// The tracking branch of the current branch, if it has one.
    fn upstream_name(&self) -> Option<String> {
        let head = self.inner.head().ok()?;
        let branch = git2::Branch::wrap(head);
        let upstream = branch.upstream().ok()?;
        upstream.name().ok().flatten().map(str::to_string)
    }

    /// Commits ahead of and behind the tracking branch, zero without one.
    fn tracking(&self) -> Result<(usize, usize), GitError> {
        let Ok(head) = self.inner.head() else {
            return Ok((0, 0));
        };
        let Some(local) = head.target() else {
            return Ok((0, 0));
        };
        let branch = git2::Branch::wrap(head);
        let Ok(upstream) = branch.upstream() else {
            return Ok((0, 0));
        };
        let Some(remote) = upstream.get().target() else {
            return Ok((0, 0));
        };
        Ok(self.inner.graph_ahead_behind(local, remote)?)
    }
}

/// The single change worth reporting for a status entry.
///
/// A path can carry index and working-tree bits at once (staged then edited
/// again); the working tree is what the user sees, so it wins.
fn kind_of(status: git2::Status) -> Option<ChangeKind> {
    use git2::Status as S;
    if status.intersects(S::WT_DELETED | S::INDEX_DELETED) {
        return Some(ChangeKind::Deleted);
    }
    if status.intersects(S::WT_RENAMED | S::INDEX_RENAMED) {
        return Some(ChangeKind::Renamed);
    }
    if status.intersects(S::WT_NEW | S::INDEX_NEW) {
        return Some(ChangeKind::New);
    }
    if status.intersects(S::WT_MODIFIED | S::INDEX_MODIFIED | S::WT_TYPECHANGE | S::INDEX_TYPECHANGE)
    {
        return Some(ChangeKind::Modified);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{ChangeKind, Status, kind_of};

    #[test]
    fn a_clean_status_says_so() {
        assert!(Status::default().is_clean());
    }

    #[test]
    fn an_untracked_file_is_new() {
        assert_eq!(kind_of(git2::Status::WT_NEW), Some(ChangeKind::New));
    }

    #[test]
    fn a_deleted_file_reads_as_deleted_however_it_was_staged() {
        assert_eq!(kind_of(git2::Status::WT_DELETED), Some(ChangeKind::Deleted));
        assert_eq!(
            kind_of(git2::Status::INDEX_DELETED),
            Some(ChangeKind::Deleted)
        );
    }

    #[test]
    fn a_file_staged_then_edited_again_reads_as_modified() {
        let both = git2::Status::INDEX_MODIFIED | git2::Status::WT_MODIFIED;
        assert_eq!(kind_of(both), Some(ChangeKind::Modified));
    }

    #[test]
    fn an_unchanged_file_is_not_a_change() {
        assert_eq!(kind_of(git2::Status::CURRENT), None);
    }

    #[test]
    fn kinds_carry_their_word() {
        assert_eq!(ChangeKind::New.label(), "new");
        assert_eq!(ChangeKind::Deleted.label(), "deleted");
    }
}
