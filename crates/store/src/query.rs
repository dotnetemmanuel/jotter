//! Reads: fetching projects and tasks back. Nothing here writes, and nothing here
//! reads the clock; anything time-derived belongs to a command or to the caller.

use rusqlite::Row;
use time::{Date, Weekday};

use crate::{Project, Store, StoreError, Subtask, Task, TaskState, date};

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

/// Fetches every task that is not done and has a due date, for the derived
/// listings that only ever care about tasks still needing attention.
fn undone_dated_tasks(store: &Store) -> Result<Vec<Task>, StoreError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE due_date IS NOT NULL AND state != ?1
         ORDER BY due_date IS NULL, due_date ASC, created_at ASC, id ASC"
    );
    let mut stmt = store.conn.prepare(&sql)?;
    let rows = stmt.query_map((TaskState::Done.column_text(),), extract_task_row)?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row_to_task(row?)?);
    }
    Ok(tasks)
}

/// Lists every task overdue as of `today`: not done, dated, and that date already
/// passed. See [`crate::date::is_overdue`] for the boundary cases.
///
/// An overdue task whose date falls in the current calendar week also appears in
/// [`tasks_due_this_week`]; a caller showing both lists will show it twice.
///
/// # Errors
/// Returns [`StoreError::InvalidDueDate`] if a stored due date does not parse,
/// [`StoreError::UnknownTaskState`] if a stored state does not parse, or
/// [`StoreError::Sqlite`] if the query fails.
pub fn overdue_tasks(store: &Store, today: Date) -> Result<Vec<Task>, StoreError> {
    let mut overdue = Vec::new();
    for task in undone_dated_tasks(store)? {
        let Some(due_text) = task.due_date.as_deref() else {
            continue;
        };
        let due = date::parse_due_date(due_text)?;
        if date::is_overdue(Some(due), false, today) {
            overdue.push(task);
        }
    }
    Ok(overdue)
}

/// Lists every task due in the same calendar week as `today`: not done, dated,
/// and that date within the week. See [`crate::date::is_due_this_week`] for what
/// counts as "this week".
///
/// A task already overdue also appears here if its date falls in the current
/// calendar week; a caller showing both [`overdue_tasks`] and this list will show
/// it twice.
///
/// # Errors
/// Returns [`StoreError::InvalidDueDate`] if a stored due date does not parse,
/// [`StoreError::UnknownTaskState`] if a stored state does not parse, or
/// [`StoreError::Sqlite`] if the query fails.
pub fn tasks_due_this_week(store: &Store, today: Date) -> Result<Vec<Task>, StoreError> {
    let mut due_this_week = Vec::new();
    for task in undone_dated_tasks(store)? {
        let Some(due_text) = task.due_date.as_deref() else {
            continue;
        };
        let due = date::parse_due_date(due_text)?;
        if date::is_due_this_week(due, false, today) {
            due_this_week.push(task);
        }
    }
    Ok(due_this_week)
}

/// Counts a project's tasks that are not yet done: [`TaskState::NotStarted`] and
/// [`TaskState::InProgress`] both count, [`TaskState::Done`] does not.
///
/// # Errors
/// Returns [`StoreError::Sqlite`] if the query fails.
pub fn unfinished_task_count(store: &Store, project_id: i64) -> Result<i64, StoreError> {
    store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND state != ?2",
            (project_id, TaskState::Done.column_text()),
            |row| row.get(0),
        )
        .map_err(Into::into)
}

/// The pace `project` needs as of `today`: its unfinished task count divided by
/// the workdays remaining until its due date. See [`crate::date::pace`] for what
/// each edge case (no due date, a passed deadline, a weekend deadline, nothing
/// left to do) returns and why those answers are deliberately distinct.
///
/// # Errors
/// Returns [`StoreError::InvalidDueDate`] if the project's stored due date does
/// not parse, or [`StoreError::Sqlite`] if the query fails.
pub fn project_pace(
    store: &Store,
    project: &Project,
    today: Date,
    workdays: &[Weekday],
) -> Result<Option<f64>, StoreError> {
    let due_date = project
        .due_date
        .as_deref()
        .map(date::parse_due_date)
        .transpose()?;
    let tasks_left = unfinished_task_count(store, project.id)?;
    Ok(date::pace(tasks_left, due_date, today, workdays))
}
