//! The ignore files jotter writes for its own derived state.

use tempfile::TempDir;

#[test]
fn the_index_directory_gets_an_ignore_file() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".jotter")).unwrap();
    jotter_git::write_ignores(tmp.path()).unwrap();

    let text = std::fs::read_to_string(tmp.path().join(".jotter/.gitignore")).unwrap();
    assert!(text.contains("index.db"));
    assert!(text.contains("index.db-wal"));
    // Not a blanket ignore: per-vault settings may want committing later.
    assert!(!text.contains("*\n"));
    // The ignore file hides itself, so jotter adds nothing to the user's history.
    assert!(text.contains("\n.gitignore\n"));
}

#[test]
fn the_trash_is_ignored_whole() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".trash")).unwrap();
    jotter_git::write_ignores(tmp.path()).unwrap();

    let text = std::fs::read_to_string(tmp.path().join(".trash/.gitignore")).unwrap();
    assert_eq!(text.trim(), "*");
}

#[test]
fn a_missing_trash_is_not_created() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".jotter")).unwrap();
    jotter_git::write_ignores(tmp.path()).unwrap();

    assert!(!tmp.path().join(".trash").exists());
}

#[test]
fn an_edited_ignore_file_is_left_alone() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".jotter")).unwrap();
    std::fs::write(tmp.path().join(".jotter/.gitignore"), "mine\n").unwrap();

    jotter_git::write_ignores(tmp.path()).unwrap();

    let text = std::fs::read_to_string(tmp.path().join(".jotter/.gitignore")).unwrap();
    assert_eq!(text, "mine\n");
}

#[test]
fn the_index_directory_is_created_when_missing() {
    let tmp = TempDir::new().unwrap();
    jotter_git::write_ignores(tmp.path()).unwrap();

    assert!(tmp.path().join(".jotter/.gitignore").exists());
}
