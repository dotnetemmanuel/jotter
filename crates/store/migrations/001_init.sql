CREATE TABLE projects (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL,
  due_date     TEXT,
  created_at   INTEGER NOT NULL,
  archived_at  INTEGER
);

CREATE TABLE tasks (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  title         TEXT NOT NULL,
  state         TEXT NOT NULL,
  project_id    INTEGER REFERENCES projects(id) ON DELETE SET NULL,
  due_date      TEXT,
  notes         TEXT,
  created_at    INTEGER NOT NULL,
  completed_at  INTEGER
);
CREATE INDEX idx_tasks_project ON tasks(project_id);

CREATE TABLE subtasks (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id  INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  title    TEXT NOT NULL,
  done     INTEGER NOT NULL
);
CREATE INDEX idx_subtasks_task ON subtasks(task_id);
