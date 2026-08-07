//! Repository discovery and status, against real repositories in temp dirs.

use std::path::Path;

use git2::{Oid, Repository, RepositoryInitOptions, Signature};
use jotter_git::{ChangeKind, Repo};
use tempfile::TempDir;

/// A repository on `main`, so the tests do not depend on the default branch name.
fn init(dir: &Path) -> Repository {
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    Repository::init_opts(dir, &options).unwrap()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

/// Stages everything and commits, returning the new commit id.
fn commit_all(repo: &Repository, message: &str) -> Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let who = Signature::now("Test", "test@example.com").unwrap();
    let parents = match repo.head().ok().and_then(|head| head.target()) {
        Some(oid) => vec![repo.find_commit(oid).unwrap()],
        None => Vec::new(),
    };
    let borrowed: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &who, &who, message, &tree, &borrowed)
        .unwrap()
}

/// Points `refs/remotes/origin/main` at `oid` and tracks it from `main`.
///
/// The remote never has to exist for status: an upstream is a ref plus config.
fn set_upstream(repo: &Repository, oid: Oid) {
    if repo.find_remote("origin").is_err() {
        repo.remote("origin", "/nonexistent/remote.git").unwrap();
    }
    repo.reference("refs/remotes/origin/main", oid, true, "test")
        .unwrap();
    let mut branch = repo.find_branch("main", git2::BranchType::Local).unwrap();
    branch.set_upstream(Some("origin/main")).unwrap();
}

#[test]
fn a_plain_directory_is_not_a_repo() {
    let tmp = TempDir::new().unwrap();
    assert!(Repo::discover(tmp.path()).is_none());
}

#[test]
fn a_repo_root_is_found() {
    let tmp = TempDir::new().unwrap();
    init(tmp.path());
    assert!(Repo::discover(tmp.path()).is_some());
}

#[test]
fn a_vault_inside_a_repo_is_left_alone() {
    let tmp = TempDir::new().unwrap();
    init(tmp.path());
    let vault = tmp.path().join("notes");
    std::fs::create_dir_all(&vault).unwrap();
    assert!(Repo::discover(&vault).is_none());
}

#[test]
fn the_branch_is_named() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    commit_all(&repo, "first");

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert_eq!(status.branch, "main");
    assert!(status.upstream.is_none());
}

#[test]
fn a_repo_without_commits_still_reports() {
    let tmp = TempDir::new().unwrap();
    init(tmp.path());
    write(tmp.path(), "a.md", "one");

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert_eq!(status.branch, "main");
    assert_eq!(status.changed.len(), 1);
}

#[test]
fn changes_are_listed_by_kind() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "kept.md", "one");
    write(tmp.path(), "gone.md", "two");
    commit_all(&repo, "first");

    write(tmp.path(), "kept.md", "one, edited");
    std::fs::remove_file(tmp.path().join("gone.md")).unwrap();
    write(tmp.path(), "notes/fresh.md", "three");

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    let kinds: Vec<(&str, ChangeKind)> = status
        .changed
        .iter()
        .map(|change| (change.path.as_str(), change.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("gone.md", ChangeKind::Deleted),
            ("kept.md", ChangeKind::Modified),
            ("notes/fresh.md", ChangeKind::New),
        ]
    );
}

#[test]
fn a_clean_repo_has_nothing_changed() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    commit_all(&repo, "first");

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert!(status.changed.is_empty());
    assert!(status.is_clean());
}

#[test]
fn ahead_and_behind_count_against_the_upstream() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    let first = commit_all(&repo, "first");
    set_upstream(&repo, first);

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((status.ahead, status.behind), (0, 0));

    write(tmp.path(), "b.md", "two");
    commit_all(&repo, "second");
    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert_eq!((status.ahead, status.behind), (1, 0));
}

#[test]
fn upstream_commits_count_as_behind() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    let first = commit_all(&repo, "first");

    // A commit only the remote has: same parent, different content.
    let who = Signature::now("Other", "other@example.com").unwrap();
    let parent = repo.find_commit(first).unwrap();
    let tree = parent.tree().unwrap();
    let theirs = repo
        .commit(None, &who, &who, "theirs", &tree, &[&parent])
        .unwrap();
    set_upstream(&repo, theirs);

    write(tmp.path(), "b.md", "two");
    commit_all(&repo, "mine");

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert_eq!((status.ahead, status.behind), (1, 1));
}

#[test]
fn the_index_database_is_not_reported_as_a_change() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    commit_all(&repo, "first");

    std::fs::create_dir_all(tmp.path().join(".jotter")).unwrap();
    jotter_git::write_ignores(tmp.path()).unwrap();
    write(tmp.path(), ".jotter/index.db", "binary junk");

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert!(
        status.changed.is_empty(),
        "index database leaked into status: {:?}",
        status.changed
    );
}

#[test]
fn trashed_notes_are_not_reported_as_changes() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    commit_all(&repo, "first");

    write(tmp.path(), ".trash/old.md", "deleted note");
    jotter_git::write_ignores(tmp.path()).unwrap();

    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert!(status.changed.is_empty());
}
