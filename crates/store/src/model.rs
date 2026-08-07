//! The row types the store reads and writes, and the fixed states a task can hold.

use crate::StoreError;

/// A project groups tasks under a name and an optional due date.
///
/// Deleting a project never deletes its tasks: the migration unlinks them
/// (`ON DELETE SET NULL` on `tasks.project_id`) instead, so they become unfiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Stable row id.
    pub id: i64,
    /// Display name.
    pub name: String,
    /// Calendar date the project is due, as `YYYY-MM-DD`, or `None` if undated.
    pub due_date: Option<String>,
    /// Creation instant, unix seconds.
    pub created_at: i64,
    /// Archive instant, unix seconds, or `None` while active.
    pub archived_at: Option<i64>,
}

/// A task: the unit of work the store exists to hold.
///
/// `project_id` is optional and, unlike `subtasks.task_id`, survives the deletion
/// of what it points to: losing a project must not lose the tasks filed under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Stable row id.
    pub id: i64,
    /// Title.
    pub title: String,
    /// Lifecycle state.
    pub state: TaskState,
    /// The project this task is filed under, or `None` if unfiled.
    pub project_id: Option<i64>,
    /// Calendar date the task is due, as `YYYY-MM-DD`, or `None` if undated.
    pub due_date: Option<String>,
    /// Free-text notes, or `None`.
    pub notes: Option<String>,
    /// Creation instant, unix seconds.
    pub created_at: i64,
    /// Completion instant, unix seconds, or `None` while not done.
    pub completed_at: Option<i64>,
}

/// A subtask: a checklist line with no meaning apart from the task it belongs to.
///
/// Deleting the owning task deletes its subtasks (`ON DELETE CASCADE` on
/// `subtasks.task_id`); a subtask is never left orphaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtask {
    /// Stable row id.
    pub id: i64,
    /// The task this subtask belongs to.
    pub task_id: i64,
    /// Title.
    pub title: String,
    /// Whether the subtask is checked off.
    pub done: bool,
}

/// The fixed lifecycle states a task can hold in v1.
///
/// Stored as literal text (see `column_text`) rather than an integer, so the
/// database reads as plain words under `sqlite3`. A row written by a future
/// version with a state this build does not recognise is refused, not guessed at:
/// see [`TaskState::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Not yet started.
    NotStarted,
    /// Underway.
    InProgress,
    /// Finished.
    Done,
}

impl TaskState {
    /// The literal text this state is stored as.
    #[must_use]
    pub fn column_text(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }

    /// Parses a state stored by [`TaskState::column_text`].
    ///
    /// # Errors
    /// Returns [`StoreError::UnknownTaskState`] for any text that is not one of the
    /// fixed variants, rather than defaulting it to [`TaskState::NotStarted`].
    pub fn parse(text: &str) -> Result<Self, StoreError> {
        match text {
            "not_started" => Ok(Self::NotStarted),
            "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            other => Err(StoreError::UnknownTaskState(other.to_owned())),
        }
    }
}
