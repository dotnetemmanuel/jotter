use jotter_store::{Store, StoreError, TaskState, command, query};

#[test]
fn a_task_can_exist_with_no_project() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Buy milk", None, None, None).unwrap();
    assert_eq!(task.project_id, None);

    let fetched = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(fetched, task);
}

#[test]
fn a_task_can_be_filed_under_a_project_and_unfiled_again() {
    let store = Store::open_in_memory().unwrap();
    let project = command::create_project(&store, "Home", None).unwrap();
    let task = command::create_task(&store, "Fix the sink", None, None, None).unwrap();

    command::file_task_under_project(&store, task.id, project.id).unwrap();
    let filed = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(filed.project_id, Some(project.id));

    command::unfile_task(&store, task.id).unwrap();
    let unfiled = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(unfiled.project_id, None);
}

#[test]
fn deleting_a_project_unfiles_its_tasks_rather_than_deleting_them() {
    let store = Store::open_in_memory().unwrap();
    let project = command::create_project(&store, "Trip", None).unwrap();
    let pack = command::create_task(&store, "Pack", Some(project.id), None, None).unwrap();
    let book = command::create_task(&store, "Book flight", Some(project.id), None, None).unwrap();

    command::delete_project(&store, project.id).unwrap();

    assert!(query::get_project(&store, project.id).unwrap().is_none());
    let pack = query::get_task(&store, pack.id).unwrap().unwrap();
    let book = query::get_task(&store, book.id).unwrap().unwrap();
    assert_eq!(pack.project_id, None);
    assert_eq!(book.project_id, None);
}

#[test]
fn moving_a_task_to_done_records_when() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Ship it", None, None, None).unwrap();
    assert_eq!(task.completed_at, None);

    command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();

    let done = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(done.state, TaskState::Done);
    assert!(done.completed_at.is_some());
}

#[test]
fn moving_a_task_out_of_done_clears_when() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Ship it", None, None, None).unwrap();
    command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();

    command::move_task_to_state(&store, task.id, TaskState::InProgress).unwrap();

    let bounced = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(bounced.state, TaskState::InProgress);
    assert_eq!(bounced.completed_at, None);
}

#[test]
fn tasks_in_a_state_sort_by_due_date_then_creation() {
    let store = Store::open_in_memory().unwrap();
    let later = command::create_task(&store, "Later", None, Some("2026-08-10"), None).unwrap();
    let earlier = command::create_task(&store, "Earlier", None, Some("2026-08-01"), None).unwrap();
    let undated_first = command::create_task(&store, "Undated first", None, None, None).unwrap();
    let undated_second = command::create_task(&store, "Undated second", None, None, None).unwrap();

    let ordered = query::tasks_in_state(&store, TaskState::NotStarted).unwrap();
    let ids: Vec<i64> = ordered.iter().map(|task| task.id).collect();

    assert_eq!(
        ids,
        vec![earlier.id, later.id, undated_first.id, undated_second.id]
    );
}

#[test]
fn renaming_a_task_leaves_everything_else_alone() {
    let store = Store::open_in_memory().unwrap();
    let project = command::create_project(&store, "Home", None).unwrap();
    let task = command::create_task(
        &store,
        "Old title",
        Some(project.id),
        Some("2026-08-01"),
        Some("some notes"),
    )
    .unwrap();

    command::rename_task(&store, task.id, "New title").unwrap();

    let renamed = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(renamed.title, "New title");
    assert_eq!(renamed.project_id, task.project_id);
    assert_eq!(renamed.due_date, task.due_date);
    assert_eq!(renamed.notes, task.notes);
    assert_eq!(renamed.state, task.state);
    assert_eq!(renamed.created_at, task.created_at);
    assert_eq!(renamed.completed_at, task.completed_at);
}

#[test]
fn an_unrecognised_state_in_the_database_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    drop(Store::open(&path).unwrap());

    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO tasks (title, state, created_at) VALUES ('Mystery', 'wat', 0)",
        [],
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();
    let err = query::get_task(&store, 1).unwrap_err();

    assert!(matches!(err, StoreError::UnknownTaskState(ref state) if state == "wat"));
}
