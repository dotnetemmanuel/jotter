//! Resolving a conflicted rebase.
//!
//! Every assertion here is about file content, never exit codes: during a rebase
//! git's `--ours` is the upstream side and `--theirs` is the commit being
//! replayed, so a sign error would quietly discard the user's writing while
//! every command still reported success.

use std::path::{Path, PathBuf};
use std::process::Command;

use jotter_git::conflict::Choice;
use jotter_git::{Repo, Side};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=test@example.com", "-c", "user.name=Test"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write(dir: &Path, rel: &str, text: &str) {
    std::fs::write(dir.join(rel), text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

/// A vault mid-rebase, with `note.md` conflicting: the remote wrote THEIRS, we
/// wrote MINE.
struct Conflicted {
    _tmp: TempDir,
    vault: PathBuf,
}

fn conflicted() -> Conflicted {
    let tmp = TempDir::new().unwrap();
    let remote = tmp.path().join("remote.git");
    Command::new("git")
        .args(["init", "-q", "--bare", "--initial-branch=main"])
        .arg(&remote)
        .status()
        .unwrap();

    let seed = tmp.path().join("seed");
    Command::new("git")
        .args(["clone", "-q"])
        .arg(&remote)
        .arg(&seed)
        .status()
        .unwrap();
    write(&seed, "note.md", "# Note\n\nshared line\n");
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-qm", "first"]);
    git(&seed, &["push", "-q", "-u", "origin", "main"]);

    let vault = tmp.path().join("vault");
    Command::new("git")
        .args(["clone", "-q"])
        .arg(&remote)
        .arg(&vault)
        .status()
        .unwrap();
    git(&vault, &["config", "user.email", "test@example.com"]);
    git(&vault, &["config", "user.name", "Test"]);

    // They write, and push.
    write(&seed, "note.md", "# Note\n\nTHEIRS\n");
    git(&seed, &["commit", "-qam", "theirs"]);
    git(&seed, &["push", "-q"]);

    // We write, and sync into the conflict.
    write(&vault, "note.md", "# Note\n\nMINE\n");
    let report = Repo::discover(&vault).unwrap().sync("mine").unwrap();
    assert_eq!(report.conflicts, ["note.md"], "fixture did not conflict");

    Conflicted { _tmp: tmp, vault }
}

#[test]
fn keeping_mine_keeps_what_i_wrote() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    repo.resolve("note.md", Side::Mine).unwrap();

    let text = read(&world.vault, "note.md");
    assert!(text.contains("MINE"), "my writing was lost: {text}");
    assert!(!text.contains("THEIRS"));
    assert!(!text.contains("<<<<"), "markers left behind: {text}");
}

#[test]
fn taking_theirs_takes_the_other_side() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    repo.resolve("note.md", Side::Theirs).unwrap();

    let text = read(&world.vault, "note.md");
    assert!(text.contains("THEIRS"), "wrong side kept: {text}");
    assert!(!text.contains("MINE"));
}

#[test]
fn status_names_the_conflicted_note() {
    let world = conflicted();
    let status = Repo::discover(&world.vault).unwrap().status().unwrap();
    assert_eq!(status.conflicts, ["note.md"]);
    assert!(status.rebase_in_progress);
}

#[test]
fn a_fully_answered_rebase_is_still_in_progress() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();
    repo.resolve("note.md", Side::Mine).unwrap();

    // Nothing conflicts any more, but the rebase still needs finishing, and the
    // user still needs a way back to the button that finishes it.
    let status = repo.status().unwrap();
    assert!(status.conflicts.is_empty());
    assert!(status.rebase_in_progress);

    repo.continue_rebase().unwrap();
    assert!(!repo.status().unwrap().rebase_in_progress);
}

#[test]
fn a_resolved_note_is_no_longer_conflicted() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    repo.resolve("note.md", Side::Mine).unwrap();

    assert_eq!(repo.conflict_state(), Some(Vec::new()));
}

#[test]
fn continuing_finishes_the_rebase_and_keeps_both_commits() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();
    repo.resolve("note.md", Side::Mine).unwrap();

    repo.continue_rebase().unwrap();

    assert!(repo.conflict_state().is_none(), "still mid-rebase");
    assert!(read(&world.vault, "note.md").contains("MINE"));
    let log = git(&world.vault, &["log", "--format=%s"]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["mine", "theirs", "first"]);
}

#[test]
fn a_hand_edited_note_is_marked_resolved_as_it_stands() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    write(&world.vault, "note.md", "# Note\n\nMINE and THEIRS, merged by hand\n");
    repo.mark_resolved("note.md").unwrap();
    repo.continue_rebase().unwrap();

    assert!(read(&world.vault, "note.md").contains("merged by hand"));
    assert!(repo.conflict_state().is_none());
}

#[test]
fn aborting_puts_everything_back() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    repo.abort_rebase().unwrap();

    assert!(repo.conflict_state().is_none());
    // My commit survives, on my side of the fork, with their work not yet taken.
    assert!(read(&world.vault, "note.md").contains("MINE"));
    let log = git(&world.vault, &["log", "--format=%s"]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["mine", "first"]);
}

#[test]
fn continuing_with_a_conflict_left_refuses() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    let err = repo.continue_rebase().unwrap_err();

    assert!(repo.conflict_state().is_some(), "the rebase was abandoned");
    assert!(!err.to_string().is_empty());
}

#[test]
fn a_conflicted_note_parses_into_one_answerable_block() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();

    let spans = repo.conflict_spans("note.md").unwrap();

    assert_eq!(jotter_git::conflict::count(&spans), 1);
}

#[test]
fn answering_every_block_writes_and_stages_the_note() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();
    let spans = repo.conflict_spans("note.md").unwrap();

    repo.write_resolved("note.md", &spans, &[Choice::Both]).unwrap();

    let text = read(&world.vault, "note.md");
    assert!(text.contains("THEIRS"), "incoming side missing: {text}");
    assert!(text.contains("MINE"), "your side missing: {text}");
    assert!(!text.contains("<<<<"));
    assert_eq!(repo.conflict_state(), Some(Vec::new()), "not staged");

    repo.continue_rebase().unwrap();
    assert!(repo.conflict_state().is_none());
}

#[test]
fn a_note_with_a_block_still_open_is_written_but_not_staged() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();
    let spans = repo.conflict_spans("note.md").unwrap();

    repo.write_resolved("note.md", &spans, &[Choice::Unresolved]).unwrap();

    // Still conflicted as far as git is concerned, so continuing is refused.
    assert_eq!(repo.conflict_state(), Some(vec!["note.md".to_string()]));
    assert!(repo.continue_rebase().is_err());
}

#[test]
fn a_hand_written_answer_reaches_disk() {
    let world = conflicted();
    let repo = Repo::discover(&world.vault).unwrap();
    let spans = repo.conflict_spans("note.md").unwrap();

    repo.write_resolved(
        "note.md",
        &spans,
        &[Choice::Custom("neither, something better".to_string())],
    )
    .unwrap();

    assert!(read(&world.vault, "note.md").contains("neither, something better"));
    repo.continue_rebase().unwrap();
    assert!(repo.conflict_state().is_none());
}
