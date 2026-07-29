//! Rescuing a vault where the index database was committed before jotter
//! started writing ignore files.

use std::path::Path;

use git2::{Repository, RepositoryInitOptions, Signature};
use jotter_git::Repo;
use tempfile::TempDir;

fn init(dir: &Path) -> Repository {
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    Repository::init_opts(dir, &options).unwrap()
}

/// Commits `paths` explicitly, ignore rules and all, the way `git add -f` would.
fn commit_paths(repo: &Repository, paths: &[&str]) {
    let mut index = repo.index().unwrap();
    for path in paths {
        index.add_path(Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let who = Signature::now("Test", "test@example.com").unwrap();
    let parents = match repo.head().ok().and_then(|head| head.target()) {
        Some(oid) => vec![repo.find_commit(oid).unwrap()],
        None => Vec::new(),
    };
    let borrowed: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &who, &who, "commit", &tree, &borrowed)
        .unwrap();
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_clean_vault_tracks_nothing_of_jotters() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    commit_paths(&repo, &["a.md"]);
    jotter_git::write_ignores(tmp.path()).unwrap();

    assert!(!Repo::discover(tmp.path()).unwrap().tracks_jotter());
}

#[test]
fn a_committed_index_is_noticed() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    write(tmp.path(), ".jotter/index.db", "binary junk");
    commit_paths(&repo, &["a.md", ".jotter/index.db"]);

    assert!(Repo::discover(tmp.path()).unwrap().tracks_jotter());
}

#[test]
fn untracking_leaves_the_database_on_disk() {
    let tmp = TempDir::new().unwrap();
    let repo = init(tmp.path());
    write(tmp.path(), "a.md", "one");
    write(tmp.path(), ".jotter/index.db", "binary junk");
    commit_paths(&repo, &["a.md", ".jotter/index.db"]);
    jotter_git::write_ignores(tmp.path()).unwrap();

    let repo = Repo::discover(tmp.path()).unwrap();
    repo.untrack_jotter().unwrap();

    assert!(!Repo::discover(tmp.path()).unwrap().tracks_jotter());
    assert!(tmp.path().join(".jotter/index.db").exists());

    // The removal is staged, not left as a surprise for the next sync to explain.
    let status = Repo::discover(tmp.path()).unwrap().status().unwrap();
    assert_eq!(
        status
            .changed
            .iter()
            .map(|change| change.path.as_str())
            .collect::<Vec<_>>(),
        [".jotter/index.db"]
    );
}
