//! Writes: creating and updating projects and tasks.
//!
//! Every function here opens no transaction of its own beyond the single statement
//! it runs, so nothing here ever holds the connection across anything that waits.
//! A command that stamps a creation or completion instant is the only place in this
//! crate allowed to read the clock; queries never do.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Project, Store, StoreError, Task, TaskState};

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
/// completion instant; moving to any other state clears it, so a task bounced back
/// out of done never keeps a stale completion time.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the update fails.
pub fn move_task_to_state(store: &Store, task_id: i64, state: TaskState) -> Result<(), StoreError> {
    let completed_at = matches!(state, TaskState::Done).then(now_unix);
    store.conn.execute(
        "UPDATE tasks SET state = ?1, completed_at = ?2 WHERE id = ?3",
        (state.column_text(), completed_at, task_id),
    )?;
    Ok(())
}
