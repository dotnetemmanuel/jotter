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
fn marking_a_task_done_twice_keeps_the_first_completion_instant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let task_id = {
        let store = Store::open(&path).unwrap();
        let task = command::create_task(&store, "Ship it", None, None, None).unwrap();
        command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();
        task.id
    };

    // Pin the first completion instant to a known value the real clock could never produce.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE tasks SET completed_at = 111111 WHERE id = ?1",
        (task_id,),
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();
    command::move_task_to_state(&store, task_id, TaskState::Done).unwrap();

    let redone = query::get_task(&store, task_id).unwrap().unwrap();
    assert_eq!(redone.completed_at, Some(111_111));
}

#[test]
fn bouncing_out_of_done_and_back_in_stamps_a_fresh_completion_instant() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Ship it", None, None, None).unwrap();

    command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();
    command::move_task_to_state(&store, task.id, TaskState::InProgress).unwrap();
    let bounced = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(bounced.completed_at, None);

    command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();
    let redone = query::get_task(&store, task.id).unwrap().unwrap();
    assert!(redone.completed_at.is_some());
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
fn a_subtask_belongs_to_exactly_one_task() {
    let store = Store::open_in_memory().unwrap();
    let first = command::create_task(&store, "Plan the trip", None, None, None).unwrap();
    let second = command::create_task(&store, "Pack", None, None, None).unwrap();

    let subtask = command::add_subtask(&store, first.id, "Book flights").unwrap();
    assert_eq!(subtask.task_id, first.id);
    assert!(!subtask.done);

    let first_subtasks = query::subtasks_for_task(&store, first.id).unwrap();
    let second_subtasks = query::subtasks_for_task(&store, second.id).unwrap();
    assert_eq!(first_subtasks, vec![subtask.clone()]);
    assert!(second_subtasks.is_empty());

    command::rename_subtask(&store, subtask.id, "Book flights and hotel").unwrap();
    let renamed = query::subtasks_for_task(&store, first.id).unwrap();
    assert_eq!(renamed[0].title, "Book flights and hotel");

    command::remove_subtask(&store, subtask.id).unwrap();
    assert!(
        query::subtasks_for_task(&store, first.id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn renaming_a_subtask_leaves_its_siblings_untouched() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Launch", None, None, None).unwrap();
    let one = command::add_subtask(&store, task.id, "Write docs").unwrap();
    let two = command::add_subtask(&store, task.id, "Cut release").unwrap();
    command::toggle_subtask(&store, two.id).unwrap();

    command::rename_subtask(&store, one.id, "Write docs and changelog").unwrap();

    let after = query::subtasks_for_task(&store, task.id).unwrap();
    let one_after = after.iter().find(|s| s.id == one.id).unwrap();
    let two_after = after.iter().find(|s| s.id == two.id).unwrap();
    assert_eq!(one_after.title, "Write docs and changelog");
    assert_eq!(two_after.title, "Cut release");
    assert!(two_after.done);
}

#[test]
fn removing_a_subtask_leaves_its_siblings_and_task_untouched() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Launch", None, None, None).unwrap();
    let one = command::add_subtask(&store, task.id, "Write docs").unwrap();
    let two = command::add_subtask(&store, task.id, "Cut release").unwrap();
    let three = command::add_subtask(&store, task.id, "Announce").unwrap();
    command::toggle_subtask(&store, three.id).unwrap();

    command::remove_subtask(&store, two.id).unwrap();

    let remaining = query::subtasks_for_task(&store, task.id).unwrap();
    let remaining_ids: Vec<i64> = remaining.iter().map(|s| s.id).collect();
    assert_eq!(remaining_ids, vec![one.id, three.id]);
    let three_after = remaining.iter().find(|s| s.id == three.id).unwrap();
    assert_eq!(three_after.title, "Announce");
    assert!(three_after.done);
    assert!(query::get_task(&store, task.id).unwrap().is_some());
}

#[test]
fn deleting_a_task_deletes_its_subtasks() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Ship it", None, None, None).unwrap();
    command::add_subtask(&store, task.id, "Write changelog").unwrap();
    command::add_subtask(&store, task.id, "Tag the release").unwrap();

    command::delete_task(&store, task.id).unwrap();

    assert!(query::get_task(&store, task.id).unwrap().is_none());
    assert!(
        query::subtasks_for_task(&store, task.id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn subtasks_come_back_in_a_stable_order() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Launch", None, None, None).unwrap();
    let one = command::add_subtask(&store, task.id, "Write docs").unwrap();
    let two = command::add_subtask(&store, task.id, "Cut release").unwrap();
    let three = command::add_subtask(&store, task.id, "Announce").unwrap();

    let first_read = query::subtasks_for_task(&store, task.id).unwrap();
    let second_read = query::subtasks_for_task(&store, task.id).unwrap();

    let expected_ids = vec![one.id, two.id, three.id];
    assert_eq!(
        first_read.iter().map(|s| s.id).collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(first_read, second_read);
}

#[test]
fn toggling_a_subtask_does_not_touch_its_task() {
    let store = Store::open_in_memory().unwrap();
    let task = command::create_task(&store, "Ship it", None, None, None).unwrap();
    command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();
    let before = query::get_task(&store, task.id).unwrap().unwrap();

    let subtask = command::add_subtask(&store, task.id, "Write changelog").unwrap();
    command::toggle_subtask(&store, subtask.id).unwrap();

    let after = query::get_task(&store, task.id).unwrap().unwrap();
    assert_eq!(after.state, before.state);
    assert_eq!(after.completed_at, before.completed_at);
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
