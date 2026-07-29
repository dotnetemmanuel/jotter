//! Sync: commit everything, take what the remote has, then hand over what we have.
//!
//! Committing is libgit2 (fast, and it already knows the working tree). Fetch,
//! rebase, and push are the `git` binary, so credentials, signing, and hooks are
//! the user's own. Rebase rather than merge: a notes vault is a single writer in
//! two places, and merge commits nobody reads are noise.

use crate::{Change, GitError, Repo, run::git};

/// What a sync did, for the sentence the status bar shows afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Notes in the commit this sync made. Zero when there was nothing to commit.
    pub committed: usize,
    /// Commits taken from the remote.
    pub pulled: usize,
    /// Commits handed to the remote.
    pub pushed: usize,
    /// Paths the rebase could not replay. Non-empty means it is still in progress.
    pub conflicts: Vec<String>,
    /// Whether the branch has a remote to sync with at all.
    pub remote_exists: bool,
}

impl Repo {
    /// Commits, fetches, rebases, and pushes, in that order.
    ///
    /// Stops at the first conflict and leaves the rebase in progress, because the
    /// user resolves it in the app. A vault with no remote commits and stops,
    /// which is not an error: it is a vault that has not been shared yet.
    ///
    /// # Errors
    /// [`GitError::Command`] if git refuses, or [`GitError::Git`] if the working
    /// tree cannot be read. Refuses outright while a rebase is unresolved.
    pub fn sync(&self, message: &str) -> Result<SyncReport, GitError> {
        if self.conflict_state().is_some() {
            return Err(GitError::Command(
                "a conflict from an earlier sync is still unresolved".to_string(),
            ));
        }

        let mut report = SyncReport {
            committed: self.commit_all(message)?,
            ..SyncReport::default()
        };

        let Some(remote) = self.remote_name()? else {
            return Ok(report);
        };
        report.remote_exists = true;

        git(self.root(), &["fetch", "--quiet", &remote])?;
        let upstream = self.upstream_or_default(&remote);
        report.pulled = self.count_between("HEAD", &upstream);

        if report.pulled > 0 {
            match git(self.root(), &["rebase", "--quiet", &upstream]) {
                Ok(_) => {}
                Err(err) => {
                    let conflicts = self.conflict_state().unwrap_or_default();
                    if conflicts.is_empty() {
                        return Err(err);
                    }
                    report.conflicts = conflicts;
                    return Ok(report);
                }
            }
        }

        report.pushed = self.count_between(&upstream, "HEAD");
        if report.pushed > 0 {
            // -u on a branch that has no upstream yet, so the first sync of a new
            // branch does not fail on a missing tracking ref.
            let branch = self.status()?.branch;
            let args: Vec<&str> = if self.upstream_ref().is_some() {
                vec!["push", "--quiet"]
            } else {
                vec!["push", "--quiet", "-u", &remote, &branch]
            };
            git(self.root(), &args)?;
        }
        Ok(report)
    }

    /// Stages everything and commits it, returning how many paths it held.
    ///
    /// Returns zero without committing when the working tree matches the last
    /// commit, so a sync of an unchanged vault adds nothing to history.
    ///
    /// # Errors
    /// [`GitError::Git`] if the tree cannot be staged or the identity is unset.
    pub fn commit_all(&self, message: &str) -> Result<usize, GitError> {
        let changed: Vec<Change> = self.status()?.changed;
        if changed.is_empty() {
            return Ok(0);
        }

        let mut index = self.inner().index()?;
        index.add_all(["*"], git2::IndexAddOption::DEFAULT, None)?;
        // add_all does not notice a file that vanished, which is exactly what a
        // deleted note is.
        index.update_all(["*"], None)?;
        index.write()?;

        let tree = self.inner().find_tree(index.write_tree()?)?;
        let who = self.inner().signature()?;
        let parents = match self.inner().head().ok().and_then(|head| head.target()) {
            Some(oid) => vec![self.inner().find_commit(oid)?],
            None => Vec::new(),
        };
        let borrowed: Vec<&git2::Commit> = parents.iter().collect();
        self.inner()
            .commit(Some("HEAD"), &who, &who, message, &tree, &borrowed)?;
        Ok(changed.len())
    }

    /// The paths a rebase stopped on, or `None` when no rebase is in progress.
    #[must_use]
    pub fn conflict_state(&self) -> Option<Vec<String>> {
        if self.inner().state() == git2::RepositoryState::Clean {
            return None;
        }
        let mut index = self.inner().index().ok()?;
        // The rebase ran in another process, so the handle's cached index predates
        // the conflict it is being asked about.
        index.read(true).ok()?;
        let mut paths: Vec<String> = index
            .conflicts()
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|conflict| {
                let entry = conflict.our.or(conflict.their).or(conflict.ancestor)?;
                String::from_utf8(entry.path).ok()
            })
            .collect();
        paths.sort();
        paths.dedup();
        Some(paths)
    }

    /// The remote this branch syncs with, or `None` when the vault has no remote.
    fn remote_name(&self) -> Result<Option<String>, GitError> {
        if let Some(upstream) = self.upstream_ref()
            && let Some((remote, _)) = upstream.split_once('/')
        {
            return Ok(Some(remote.to_string()));
        }
        Ok(self
            .inner()
            .remotes()?
            .iter()
            .find_map(|name| name.ok().flatten().map(str::to_string)))
    }

    /// The tracking branch, as `origin/main`.
    fn upstream_ref(&self) -> Option<String> {
        let head = self.inner().head().ok()?;
        let upstream = git2::Branch::wrap(head).upstream().ok()?;
        upstream.name().ok().flatten().map(str::to_string)
    }

    /// The ref to rebase onto: the tracking branch, or the same branch on `remote`.
    fn upstream_or_default(&self, remote: &str) -> String {
        self.upstream_ref().unwrap_or_else(|| {
            let branch = self
                .status()
                .map_or_else(|_| "main".to_string(), |status| status.branch);
            format!("{remote}/{branch}")
        })
    }

    /// Commits `to` has that `from` does not, zero when either side is missing.
    fn count_between(&self, from: &str, to: &str) -> usize {
        let range = format!("{from}..{to}");
        // An unborn branch or a remote ref that does not exist yet counts as
        // nothing to do, rather than as a failure worth reporting.
        git(self.root(), &["rev-list", "--count", &range])
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(0)
    }
}
