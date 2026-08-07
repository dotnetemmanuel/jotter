//! Sync against a real remote: a bare repository on disk, so fetch, rebase, and
//! push are exercised for real with no network and no credentials.

use std::path::{Path, PathBuf};
use std::process::Command;

use jotter_git::Repo;
use tempfile::TempDir;

/// Runs git in `dir` with an identity, so commits and rebases never depend on
/// whatever the machine running the tests has configured.
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
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A bare remote plus two clones of it, the way two machines share one vault.
struct World {
    _tmp: TempDir,
    remote: PathBuf,
    one: PathBuf,
    two: PathBuf,
}

fn world() -> World {
    let tmp = TempDir::new().unwrap();
    let remote = tmp.path().join("remote.git");
    Command::new("git")
        .args(["init", "-q", "--bare", "--initial-branch=main"])
        .arg(&remote)
        .status()
        .unwrap();

    let one = tmp.path().join("one");
    Command::new("git")
        .args(["clone", "-q"])
        .arg(&remote)
        .arg(&one)
        .status()
        .unwrap();
    write(&one, "index.md", "# Index\n");
    git(&one, &["add", "-A"]);
    git(&one, &["commit", "-qm", "first"]);
    git(&one, &["push", "-q", "-u", "origin", "main"]);

    let two = tmp.path().join("two");
    Command::new("git")
        .args(["clone", "-q"])
        .arg(&remote)
        .arg(&two)
        .status()
        .unwrap();
    // Identity for the clone that syncs through jotter.
    git(&two, &["config", "user.email", "test@example.com"]);
    git(&two, &["config", "user.name", "Test"]);
    git(&one, &["config", "user.email", "test@example.com"]);
    git(&one, &["config", "user.name", "Test"]);

    World {
        _tmp: tmp,
        remote,
        one,
        two,
    }
}

/// Commit subjects on `branch`, newest first.
fn log(dir: &Path, branch: &str) -> Vec<String> {
    git(dir, &["log", "--format=%s", branch])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_clean_vault_has_nothing_to_do() {
    let world = world();
    let report = Repo::discover(&world.two).unwrap().sync("jotter").unwrap();

    assert_eq!(report.committed, 0);
    assert_eq!(report.pushed, 0);
    assert!(report.conflicts.is_empty());
    assert!(report.remote_exists);
}

#[test]
fn changes_are_committed_and_pushed() {
    let world = world();
    write(&world.two, "new.md", "# New\n");
    write(&world.two, "index.md", "# Index\n\nEdited.\n");

    let report = Repo::discover(&world.two)
        .unwrap()
        .sync("jotter: 2 notes")
        .unwrap();

    assert_eq!(report.committed, 2);
    assert_eq!(report.pushed, 1);
    assert_eq!(log(&world.remote, "main")[0], "jotter: 2 notes");
}

#[test]
fn upstream_work_is_rebased_under_local_work() {
    let world = world();
    write(&world.one, "theirs.md", "# Theirs\n");
    git(&world.one, &["add", "-A"]);
    git(&world.one, &["commit", "-qm", "theirs"]);
    git(&world.one, &["push", "-q"]);

    write(&world.two, "mine.md", "# Mine\n");
    let report = Repo::discover(&world.two).unwrap().sync("mine").unwrap();

    assert_eq!(report.committed, 1);
    assert!(report.conflicts.is_empty());
    // Rebased, so the remote work sits under mine with no merge commit.
    assert_eq!(log(&world.remote, "main"), ["mine", "theirs", "first"]);
    assert!(world.two.join("theirs.md").exists());
}

#[test]
fn a_conflict_is_reported_and_left_for_resolution() {
    let world = world();
    write(&world.one, "index.md", "# Index\n\nTheir line.\n");
    git(&world.one, &["add", "-A"]);
    git(&world.one, &["commit", "-qm", "theirs"]);
    git(&world.one, &["push", "-q"]);

    write(&world.two, "index.md", "# Index\n\nMy line.\n");
    let repo = Repo::discover(&world.two).unwrap();
    let report = repo.sync("mine").unwrap();

    assert_eq!(report.conflicts, ["index.md"]);
    assert_eq!(report.pushed, 0);
    // The rebase is still in progress, waiting for the user to resolve it.
    assert!(repo.conflict_state().is_some());
    // Their commit did not reach the remote through us.
    assert_eq!(log(&world.remote, "main")[0], "theirs");
}

#[test]
fn a_vault_with_no_remote_commits_and_says_so() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    Command::new("git")
        .args(["init", "-q", "--initial-branch=main"])
        .arg(root)
        .status()
        .unwrap();
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    write(root, "index.md", "# Index\n");

    let report = Repo::discover(root).unwrap().sync("first notes").unwrap();

    assert_eq!(report.committed, 1);
    assert!(!report.remote_exists);
    assert_eq!(report.pushed, 0);
    assert_eq!(log(root, "main"), ["first notes"]);
}

#[test]
fn sync_refuses_while_a_rebase_is_in_progress() {
    let world = world();
    write(&world.one, "index.md", "# Index\n\nTheir line.\n");
    git(&world.one, &["add", "-A"]);
    git(&world.one, &["commit", "-qm", "theirs"]);
    git(&world.one, &["push", "-q"]);

    write(&world.two, "index.md", "# Index\n\nMy line.\n");
    let repo = Repo::discover(&world.two).unwrap();
    repo.sync("mine").unwrap();

    let err = repo.sync("again").unwrap_err();
    assert!(
        err.to_string().contains("conflict"),
        "unhelpful message: {err}"
    );
}
