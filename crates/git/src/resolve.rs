//! Resolving a conflicted rebase: pick a side, or say you fixed it yourself.

use std::path::PathBuf;

use crate::conflict::{self, Choice, Span};
use crate::{GitError, Repo, run::git};

/// Which version of a conflicted note to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// What the user wrote here.
    Mine,
    /// What arrived from the remote.
    Theirs,
}

impl Side {
    /// The checkout flag that keeps this side **during a rebase**.
    ///
    /// Inverted on purpose. A rebase checks the upstream out as HEAD and replays
    /// the user's commit on top, so git's `--ours` is the remote and `--theirs`
    /// is the user. Reading these as the words suggest would silently throw away
    /// whichever side the user asked to keep.
    fn checkout_flag(self) -> &'static str {
        match self {
            Side::Mine => "--theirs",
            Side::Theirs => "--ours",
        }
    }
}

impl Repo {
    /// Reads a conflicted note and splits it into answerable blocks.
    ///
    /// # Errors
    /// [`GitError::Io`] if the note cannot be read, or [`GitError::Command`] if
    /// its markers do not close.
    pub fn conflict_spans(&self, path: &str) -> Result<Vec<Span>, GitError> {
        let text = std::fs::read_to_string(self.note_path(path))?;
        conflict::parse(&text).map_err(|err| GitError::Command(format!("{path}: {err}")))
    }

    /// Writes `choices` back into `path` and stages it.
    ///
    /// Staging is what tells git the block is settled, so a note left with an
    /// unanswered block is written but deliberately not staged: the rebase must
    /// not be allowed to continue over it.
    ///
    /// # Errors
    /// [`GitError::Io`] if the note cannot be written, or [`GitError::Command`]
    /// if git will not stage it.
    pub fn write_resolved(&self, path: &str, spans: &[Span], choices: &[Choice]) -> Result<(), GitError> {
        std::fs::write(self.note_path(path), conflict::apply(spans, choices))?;
        if choices.iter().any(Choice::is_unresolved) || choices.len() < conflict::count(spans) {
            return Ok(());
        }
        self.mark_resolved(path)
    }

    /// Absolute path of a vault-relative note.
    fn note_path(&self, path: &str) -> PathBuf {
        self.root().join(path)
    }

    /// Resolves `path` by keeping one side whole, and stages it.
    ///
    /// # Errors
    /// [`GitError::Command`] with git's own message if the path is not conflicted
    /// or the checkout fails.
    pub fn resolve(&self, path: &str, side: Side) -> Result<(), GitError> {
        git(
            self.root(),
            &["checkout", side.checkout_flag(), "--", path],
        )?;
        self.mark_resolved(path)
    }

    /// Stages `path` as it stands on disk, for a note edited by hand.
    ///
    /// # Errors
    /// [`GitError::Command`] if git cannot stage it.
    pub fn mark_resolved(&self, path: &str) -> Result<(), GitError> {
        git(self.root(), &["add", "--", path])?;
        Ok(())
    }

    /// Replays the rest of the rebase.
    ///
    /// # Errors
    /// [`GitError::Command`] while any conflict is still unresolved, leaving the
    /// rebase exactly as it was.
    pub fn continue_rebase(&self) -> Result<(), GitError> {
        // core.editor is neutered because a rebase that pauses for a commit
        // message would hang a GUI with no terminal behind it.
        git(
            self.root(),
            &["-c", "core.editor=true", "rebase", "--continue"],
        )?;
        Ok(())
    }

    /// Puts the vault back exactly as it was before the sync that conflicted.
    ///
    /// # Errors
    /// [`GitError::Command`] if git cannot unwind it.
    pub fn abort_rebase(&self) -> Result<(), GitError> {
        git(self.root(), &["rebase", "--abort"])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Side;

    #[test]
    fn mine_is_theirs_to_git_during_a_rebase() {
        assert_eq!(Side::Mine.checkout_flag(), "--theirs");
        assert_eq!(Side::Theirs.checkout_flag(), "--ours");
    }
}
