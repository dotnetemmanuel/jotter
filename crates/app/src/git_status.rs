//! The git segment of the status bar: what it says, and where it comes from.
//!
//! Reading status walks the working tree, which is too slow to do on the UI
//! thread of a large vault, so [`read`] is meant to run on a worker and the
//! result is applied when it arrives.

use std::path::Path;

use jotter_git::{Repo, Status};

/// How often the status is re-read while a vault is open, in seconds.
pub const POLL_SECONDS: u32 = 30;

/// The branch glyph, from the Nerd Font the status segment is set in.
const BRANCH: char = '\u{e0a0}';
/// Commits waiting to be pushed.
const AHEAD: char = '\u{2191}';
/// Commits waiting to be pulled.
const BEHIND: char = '\u{2193}';
/// Notes changed since the last commit.
const CHANGED: char = '\u{25cf}';

/// Reads the status of the vault at `root`, or `None` when it is not a repo.
///
/// Errors are logged rather than surfaced: a status poll that fails must not
/// interrupt writing, and the next poll is thirty seconds away.
#[must_use]
pub fn read(root: &Path) -> Option<Status> {
    let repo = Repo::discover(root)?;
    match repo.status() {
        Ok(status) => Some(status),
        Err(err) => {
            eprintln!("jotter: could not read git status: {err}");
            None
        }
    }
}

/// The status-bar text: the branch, and only the counts that are not zero.
#[must_use]
pub fn label(status: &Status) -> String {
    let mut parts = vec![format!("{BRANCH} {}", status.branch)];
    if status.ahead > 0 {
        parts.push(format!("{AHEAD}{}", status.ahead));
    }
    if status.behind > 0 {
        parts.push(format!("{BEHIND}{}", status.behind));
    }
    if !status.changed.is_empty() {
        // The dot sits tight against a digit, so it gets a space the arrows do not need.
        parts.push(format!("{CHANGED} {}", status.changed.len()));
    }
    parts.join(" ")
}

/// The tooltip, which says in words what the glyphs compress.
#[must_use]
pub fn tooltip(status: &Status) -> String {
    let mut parts = vec![format!("on {}", status.branch)];
    match &status.upstream {
        Some(upstream) => parts.push(format!("tracking {upstream}")),
        None => parts.push("no tracking branch".to_string()),
    }
    if status.ahead > 0 {
        parts.push(format!("{} to push", status.ahead));
    }
    if status.behind > 0 {
        parts.push(format!("{} to pull", status.behind));
    }
    parts.push(match status.changed.len() {
        0 => "nothing changed".to_string(),
        1 => "1 note changed".to_string(),
        many => format!("{many} notes changed"),
    });
    parts.join(", ")
}

/// The message a sync commits under: what changed, and when.
///
/// Generated rather than asked for, because a sync you have to answer a dialog
/// for is a sync you stop doing.
#[must_use]
pub fn commit_message(changed: usize, when: &str) -> String {
    let notes = match changed {
        1 => "1 note".to_string(),
        many => format!("{many} notes"),
    };
    format!("jotter: {notes}, {when}")
}

/// What to say in the status bar once a sync finishes.
#[must_use]
pub fn sync_summary(report: &jotter_git::SyncReport) -> String {
    if !report.conflicts.is_empty() {
        return match report.conflicts.len() {
            1 => format!("Conflict in {}: resolve it to finish", report.conflicts[0]),
            many => format!("{many} notes conflict: resolve them to finish"),
        };
    }

    let mut parts = Vec::new();
    match report.committed {
        0 => {}
        1 => parts.push("committed 1 note".to_string()),
        many => parts.push(format!("committed {many} notes")),
    }
    if report.pulled > 0 {
        parts.push(format!("pulled {}", report.pulled));
    }
    if report.pushed > 0 {
        parts.push(format!("pushed {}", report.pushed));
    }

    if parts.is_empty() {
        return if report.remote_exists {
            "Already in sync".to_string()
        } else {
            "Nothing to commit, and no remote to sync with".to_string()
        };
    }

    let mut summary = parts.join(", ");
    if !report.remote_exists {
        summary.push_str(" (no remote)");
    }
    // Sentence case: the status bar is prose, not a log line.
    let mut chars = summary.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => summary,
    }
}

#[cfg(test)]
mod tests {
    use super::{commit_message, label, sync_summary, tooltip};
    use jotter_git::{Change, ChangeKind, Status, SyncReport};

    fn status(ahead: usize, behind: usize, changed: usize) -> Status {
        Status {
            branch: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead,
            behind,
            changed: (0..changed)
                .map(|n| Change {
                    path: format!("note-{n}.md"),
                    kind: ChangeKind::Modified,
                })
                .collect(),
        }
    }

    #[test]
    fn a_synced_vault_shows_only_the_branch() {
        assert_eq!(label(&status(0, 0, 0)), "\u{e0a0} main");
    }

    #[test]
    fn commits_to_push_show_as_an_up_arrow() {
        assert_eq!(label(&status(2, 0, 0)), "\u{e0a0} main \u{2191}2");
    }

    #[test]
    fn commits_to_pull_show_as_a_down_arrow() {
        assert_eq!(label(&status(0, 3, 0)), "\u{e0a0} main \u{2193}3");
    }

    #[test]
    fn changed_notes_show_as_a_dot() {
        assert_eq!(label(&status(0, 0, 4)), "\u{e0a0} main \u{25cf} 4");
    }

    #[test]
    fn everything_at_once_keeps_its_order() {
        assert_eq!(
            label(&status(2, 1, 3)),
            "\u{e0a0} main \u{2191}2 \u{2193}1 \u{25cf} 3"
        );
    }

    #[test]
    fn the_tooltip_spells_it_out() {
        assert_eq!(
            tooltip(&status(2, 1, 1)),
            "on main, tracking origin/main, 2 to push, 1 to pull, 1 note changed"
        );
    }

    #[test]
    fn a_branch_with_no_upstream_says_so() {
        let mut status = status(0, 0, 0);
        status.upstream = None;
        assert_eq!(
            tooltip(&status),
            "on main, no tracking branch, nothing changed"
        );
    }

    #[test]
    fn the_commit_message_counts_the_notes() {
        assert_eq!(
            commit_message(3, "2026-07-29 14:02"),
            "jotter: 3 notes, 2026-07-29 14:02"
        );
        assert_eq!(
            commit_message(1, "2026-07-29 14:02"),
            "jotter: 1 note, 2026-07-29 14:02"
        );
    }

    fn report(committed: usize, pulled: usize, pushed: usize) -> SyncReport {
        SyncReport {
            committed,
            pulled,
            pushed,
            conflicts: Vec::new(),
            remote_exists: true,
        }
    }

    #[test]
    fn a_full_sync_reports_every_step() {
        assert_eq!(
            sync_summary(&report(3, 1, 1)),
            "Committed 3 notes, pulled 1, pushed 1"
        );
    }

    #[test]
    fn steps_that_did_nothing_are_not_mentioned() {
        assert_eq!(sync_summary(&report(2, 0, 1)), "Committed 2 notes, pushed 1");
    }

    #[test]
    fn a_sync_with_nothing_to_do_says_so() {
        assert_eq!(sync_summary(&report(0, 0, 0)), "Already in sync");
    }

    #[test]
    fn a_vault_with_no_remote_is_told_plainly() {
        let mut report = report(2, 0, 0);
        report.remote_exists = false;
        assert_eq!(sync_summary(&report), "Committed 2 notes (no remote)");

        let mut empty = report.clone();
        empty.committed = 0;
        assert_eq!(
            sync_summary(&empty),
            "Nothing to commit, and no remote to sync with"
        );
    }

    #[test]
    fn a_conflict_outranks_everything_else() {
        let mut report = report(1, 1, 0);
        report.conflicts = vec!["notes/deep.md".to_string()];
        assert_eq!(
            sync_summary(&report),
            "Conflict in notes/deep.md: resolve it to finish"
        );

        report.conflicts.push("index.md".to_string());
        assert_eq!(
            sync_summary(&report),
            "2 notes conflict: resolve them to finish"
        );
    }
}
