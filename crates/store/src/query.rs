//! Reads: fetching projects and tasks back. Nothing here writes, and nothing here
//! reads the clock; anything time-derived belongs to a command or to the caller.

use rusqlite::Row;

use crate::{Project, Store, StoreError, Subtask, Task, TaskState};

struct TaskRow {
    id: i64,
    title: String,
    state: String,
    project_id: Option<i64>,
    due_date: Option<String>,
    notes: Option<String>,
    created_at: i64,
    completed_at: Option<i64>,
}

fn extract_task_row(row: &Row) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get(0)?,
        title: row.get(1)?,
        state: row.get(2)?,
        project_id: row.get(3)?,
        due_date: row.get(4)?,
        notes: row.get(5)?,
        created_at: row.get(6)?,
        completed_at: row.get(7)?,
    })
}

fn row_to_task(row: TaskRow) -> Result<Task, StoreError> {
    Ok(Task {
        id: row.id,
        title: row.title,
        state: TaskState::parse(&row.state)?,
        project_id: row.project_id,
        due_date: row.due_date,
        notes: row.notes,
        created_at: row.created_at,
        completed_at: row.completed_at,
    })
}

const TASK_COLUMNS: &str =
    "id, title, state, project_id, due_date, notes, created_at, completed_at";

/// Reads back a task by id, or `None` if no task has that id.
///
/// # Errors
/// Returns [`StoreError::UnknownTaskState`] if the stored state text does not match
/// a known variant, or [`StoreError::Sqlite`] if the query fails.
pub fn get_task(store: &Store, id: i64) -> Result<Option<Task>, StoreError> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1");
    match store.conn.query_row(&sql, (id,), extract_task_row) {
        Ok(row) => Ok(Some(row_to_task(row)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Reads back a project by id, or `None` if no project has that id.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the query fails.
pub fn get_project(store: &Store, id: i64) -> Result<Option<Project>, StoreError> {
    let result = store.conn.query_row(
        "SELECT id, name, due_date, created_at, archived_at FROM projects WHERE id = ?1",
        (id,),
        |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                due_date: row.get(2)?,
                created_at: row.get(3)?,
                archived_at: row.get(4)?,
            })
        },
    );
    match result {
        Ok(project) => Ok(Some(project)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Lists every task in the given state, ordered by due date (undated last), then
/// by creation order among ties.
///
/// # Errors
/// Returns [`StoreError::UnknownTaskState`] if a stored state does not match a
/// known variant, or [`StoreError::Sqlite`] if the query fails.
pub fn tasks_in_state(store: &Store, state: TaskState) -> Result<Vec<Task>, StoreError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE state = ?1
         ORDER BY due_date IS NULL, due_date ASC, created_at ASC, id ASC"
    );
    let mut stmt = store.conn.prepare(&sql)?;
    let rows = stmt.query_map((state.column_text(),), extract_task_row)?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row_to_task(row?)?);
    }
    Ok(tasks)
}

/// Lists a task's subtasks in a stable order: insertion order, since `id` is
/// monotonically increasing and, unlike the task's second-resolution
/// `created_at`, never ties.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the query fails.
pub fn subtasks_for_task(store: &Store, task_id: i64) -> Result<Vec<Subtask>, StoreError> {
    let mut stmt = store.conn.prepare(
        "SELECT id, task_id, title, done FROM subtasks WHERE task_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map((task_id,), |row| {
        Ok(Subtask {
            id: row.get(0)?,
            task_id: row.get(1)?,
            title: row.get(2)?,
            done: row.get(3)?,
        })
    })?;
    let mut subtasks = Vec::new();
    for row in rows {
        subtasks.push(row?);
    }
    Ok(subtasks)
}
