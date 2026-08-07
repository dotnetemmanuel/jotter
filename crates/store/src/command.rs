//! Writes: creating, updating and deleting projects, tasks and subtasks.
//!
//! Every function here opens no transaction of its own beyond the single statement
//! it runs, so nothing here ever holds the connection across anything that waits.
//! A command that stamps a creation or completion instant is the only place in this
//! crate allowed to read the clock; queries never do.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Project, Store, StoreError, Subtask, Task, TaskState};

fn now_unix() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    i64::try_from(secs).unwrap_or(i64::MAX)
}

/// Creates a project with the given name and optional due date.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the insert fails.
pub fn create_project(
    store: &Store,
    name: &str,
    due_date: Option<&str>,
) -> Result<Project, StoreError> {
    let created_at = now_unix();
    store.conn.execute(
        "INSERT INTO projects (name, due_date, created_at, archived_at) VALUES (?1, ?2, ?3, NULL)",
        (name, due_date, created_at),
    )?;
    Ok(Project {
        id: store.conn.last_insert_rowid(),
        name: name.to_owned(),
        due_date: due_date.map(str::to_owned),
        created_at,
        archived_at: None,
    })
}

/// Deletes a project. Its tasks are not deleted: the `ON DELETE SET NULL` foreign
/// key unfiles them, leaving them in place with no project.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the delete fails.
pub fn delete_project(store: &Store, project_id: i64) -> Result<(), StoreError> {
    store
        .conn
        .execute("DELETE FROM projects WHERE id = ?1", (project_id,))?;
    Ok(())
}

/// Creates a task, unstarted, optionally filed under a project.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the insert fails.
pub fn create_task(
    store: &Store,
    title: &str,
    project_id: Option<i64>,
    due_date: Option<&str>,
    notes: Option<&str>,
) -> Result<Task, StoreError> {
    let created_at = now_unix();
    let state = TaskState::NotStarted;
    store.conn.execute(
        "INSERT INTO tasks (title, state, project_id, due_date, notes, created_at, completed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
        (
            title,
            state.column_text(),
            project_id,
            due_date,
            notes,
            created_at,
        ),
    )?;
    Ok(Task {
        id: store.conn.last_insert_rowid(),
        title: title.to_owned(),
        state,
        project_id,
        due_date: due_date.map(str::to_owned),
        notes: notes.map(str::to_owned),
        created_at,
        completed_at: None,
    })
}

/// Files a task under a project.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn file_task_under_project(
    store: &Store,
    task_id: i64,
    project_id: i64,
) -> Result<(), StoreError> {
    store.conn.execute(
        "UPDATE tasks SET project_id = ?1 WHERE id = ?2",
        (project_id, task_id),
    )?;
    Ok(())
}

/// Removes a task from whatever project it is filed under, leaving it unfiled.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn unfile_task(store: &Store, task_id: i64) -> Result<(), StoreError> {
    store.conn.execute(
        "UPDATE tasks SET project_id = NULL WHERE id = ?1",
        (task_id,),
    )?;
    Ok(())
}

/// Renames a task, leaving every other field untouched.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn rename_task(store: &Store, task_id: i64, new_title: &str) -> Result<(), StoreError> {
    store.conn.execute(
        "UPDATE tasks SET title = ?1 WHERE id = ?2",
        (new_title, task_id),
    )?;
    Ok(())
}

/// Moves a task to the given state. Moving to [`TaskState::Done`] stamps the
/// completion instant, unless one is already set, so re-marking an already-done
/// task does not overwrite when it first finished. Moving to any other state
/// clears it, so a task bounced back out of done never keeps a stale completion
/// time, and a later re-mark as done gets a fresh instant.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn move_task_to_state(store: &Store, task_id: i64, state: TaskState) -> Result<(), StoreError> {
    if matches!(state, TaskState::Done) {
        store.conn.execute(
            "UPDATE tasks SET state = ?1, completed_at = COALESCE(completed_at, ?2) WHERE id = ?3",
            (state.column_text(), now_unix(), task_id),
        )?;
    } else {
        store.conn.execute(
            "UPDATE tasks SET state = ?1, completed_at = NULL WHERE id = ?2",
            (state.column_text(), task_id),
        )?;
    }
    Ok(())
}

/// Deletes a task. Its subtasks go with it: the `ON DELETE CASCADE` foreign key
/// removes them, leaving none orphaned.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the delete fails.
pub fn delete_task(store: &Store, task_id: i64) -> Result<(), StoreError> {
    store
        .conn
        .execute("DELETE FROM tasks WHERE id = ?1", (task_id,))?;
    Ok(())
}

/// Adds a subtask, unchecked, to a task's checklist.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the insert fails.
pub fn add_subtask(store: &Store, task_id: i64, title: &str) -> Result<Subtask, StoreError> {
    store.conn.execute(
        "INSERT INTO subtasks (task_id, title, done) VALUES (?1, ?2, 0)",
        (task_id, title),
    )?;
    Ok(Subtask {
        id: store.conn.last_insert_rowid(),
        task_id,
        title: title.to_owned(),
        done: false,
    })
}

/// Renames a subtask, leaving its done state untouched.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn rename_subtask(store: &Store, subtask_id: i64, new_title: &str) -> Result<(), StoreError> {
    store.conn.execute(
        "UPDATE subtasks SET title = ?1 WHERE id = ?2",
        (new_title, subtask_id),
    )?;
    Ok(())
}

/// Flips a subtask between done and not done. Never touches the task it belongs
/// to: a checklist line has no power to complete or reopen its task.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn toggle_subtask(store: &Store, subtask_id: i64) -> Result<(), StoreError> {
    store.conn.execute(
        "UPDATE subtasks SET done = NOT done WHERE id = ?1",
        (subtask_id,),
    )?;
    Ok(())
}

/// Removes a single subtask, leaving its task and any sibling subtasks in place.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the delete fails.
pub fn remove_subtask(store: &Store, subtask_id: i64) -> Result<(), StoreError> {
    store
        .conn
        .execute("DELETE FROM subtasks WHERE id = ?1", (subtask_id,))?;
    Ok(())
}
