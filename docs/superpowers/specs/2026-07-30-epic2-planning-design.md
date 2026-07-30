# Epic 2: planning, tasks and focus

Design agreed 2026-07-30. jotter grows a second half: a personal planner with
task tracking, a timer, and a focus mode, living beside the markdown vault rather
than inside it.

The reference products are [Blitzit](https://www.blitzit.app) and
[Super Productivity](https://super-productivity.com). Blitzit is the minimal end,
four columns and one big focus mode. Super Productivity is the rich end, projects
and folders and tags and a clock timeline. **This sits between them**: it takes
the estimate-against-capacity idea and leaves behind clock timelines, the
Inbox/Planner/Schedule/Boards/Habits spread, and every third-party integration.

## What it is not

Nothing here touches a vault, and switching vaults never affects tracking. Tasks
are not notes, not files, and not synced anywhere. Real deadline dates stay in
Google Calendar; the deadline field here exists only so a task can show one coming.

The one deliberate exception is the wikilinks inside a task's notes, which are a
convenience for reaching a note, never a place tasks are stored.

## Model

**Tags, and nothing else.** No lists, no projects, no folders. A task can carry
several tags, so a thing that is both work and an errand does not have to choose,
and cross-cutting views are possible. Notes already have tags, so the app has one
idea of a tag rather than two.

**A dateless backlog plus dated days.** The date comes from which weekday column
you drag a task into; dragging between days re-dates it. Nothing carries a date
until you plan it.

**Estimates are optional.** A task with an estimate counts down from it; a task
without one counts up. The capacity meter says how many of today's tasks are
unestimated rather than quietly undercounting.

**Deadlines are their own date**, independent of the day you plan to do the work,
so you can plan Wednesday for a Friday deadline.

## Schema

`~/.local/state/jotter/tasks.db`, one global database, migrations numbered against
`PRAGMA user_version` exactly as `crates/index` does.

```sql
CREATE TABLE tasks (
  id            INTEGER PRIMARY KEY,
  title         TEXT NOT NULL,
  notes         TEXT,              -- markdown, may hold [[wikilinks]]
  notes_vault   TEXT,              -- vault root those wikilinks were written against
  estimate_mins INTEGER,           -- nullable: no estimate means the timer counts up
  planned_on    TEXT,              -- 'YYYY-MM-DD'; NULL means backlog
  first_planned_on TEXT,           -- the day it was first planned, so slip is derivable
  deadline_on   TEXT,              -- nullable, independent of planned_on
  position      INTEGER NOT NULL,  -- order within its day or the backlog
  done_at       TEXT,              -- NULL means open
  recurrence_id INTEGER REFERENCES recurrences(id) ON DELETE SET NULL,
  created_at    TEXT NOT NULL
);

CREATE TABLE subtasks (
  id        INTEGER PRIMARY KEY,
  task_id   INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  title     TEXT NOT NULL,
  position  INTEGER NOT NULL,
  done_at   TEXT
);

CREATE TABLE task_tags (
  task_id  INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  PRIMARY KEY (task_id, tag)
);

CREATE TABLE time_entries (
  id           INTEGER PRIMARY KEY,
  task_id      INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  started_at   TEXT NOT NULL,
  ended_at     TEXT,             -- NULL means running right now
  heartbeat_at TEXT              -- refreshed every 60s while running
);

CREATE TABLE recurrences (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,   -- 'daily' | 'weekly' | 'weekdays'
  interval_days INTEGER,         -- for 'daily': every N days
  weekdays      TEXT             -- for 'weekdays': '1,3,5', Monday = 1
);

CREATE INDEX idx_tasks_planned ON tasks(planned_on);
CREATE INDEX idx_task_tags_tag ON task_tags(tag);
CREATE INDEX idx_time_entries_task ON time_entries(task_id);
CREATE INDEX idx_time_entries_started ON time_entries(started_at);
```

The whole schema lands in the first migration even though later stages fill parts
of it. Unlike `.jotter/index.db`, which is a derived cache that can be deleted and
rebuilt from the files, **this database is the source of truth**. Reshaping real
task data later is the one genuinely expensive mistake available, so the shape is
settled before any of it is written.

### Five choices worth remembering

**Time is a log, not a number.** `time_entries` rather than a `spent_seconds`
column, because "where did my week go" has to attribute a task worked across
Monday and Tuesday to both days. Pause and resume fall out for free, and a running
timer is just the row with a null `ended_at`.

**`heartbeat_at` exists for crashes.** Without it, a dangling entry has no end and
no way to know when the process died, leaving a choice between inventing time and
discarding it. Startup closes any dangling entry at its last heartbeat: accurate
to the minute, nothing invented.

**Subtasks are their own table.** They are a checklist: a title and done, with no
date, estimate, or timer. As `tasks` rows with a parent they would need
`WHERE parent_id IS NULL` in every query in every view, and it would be possible
to plan a subtask onto Thursday by accident. The cost is that promoting a subtask
to a real task is an insert plus a delete.

**Slip is derived.** `first_planned_on` rather than a carried-over flag, so the
marker can say "slipped from Monday" instead of merely "moved".

**Recurrence carries the rule only.** Completing an instance clones it forward, so
edits to this week's title or tags carry into next week's with no template to
drift out of sync.

## Behaviour

**Capacity meter** reads `4h30 of 6h · 3 unestimated`. Over capacity it turns
`warning` orange, never crimson: crimson belongs to deadlines alone.

**Rollover** runs on the first open of each day, and on the midnight flip if jotter
is left running. Away three days means everything unfinished from any past date
lands on today, with `first_planned_on` intact so the slip is visible.

**One timer at a time.** Starting a task closes the running entry and opens a new
one. A pomodoro break also closes the entry and opens a break countdown, so break
time never counts as task time; resuming opens a fresh entry.

**Deadline chip** shows whenever `deadline_on` is set, and goes crimson when there
is no slack left: the deadline is today or past, or the day the task is planned for
is on or after it. Otherwise it sits muted.

**Subtasks never auto-complete their parent.** A parent shows `2/5` and is still
ticked by hand, because "all children done" and "this is finished" are different
claims.

**Recurrence**: completing an instance clones it forward with the next date from
the rule. Deleting asks whether you mean this occurrence or the series; ending the
series drops the rule and leaves existing instances alone.

**Cross-vault links.** A task's notes record which vault root a wikilink was
written against. Clicking it under a different vault offers to switch and then
opens the note; one click does both. A vault that has moved or gone degrades to
the broken-link styling jotter already has.

**Reports**: time by tag over a range, where a task with two tags counts under
both, so column totals can exceed the hours worked and the report says so rather
than quietly splitting time it cannot attribute. Estimate accuracy covers
completed tasks that had an estimate.

**Failure is contained**, which is the payoff for keeping this separate. If
`tasks.db` will not open or a migration fails, the planner shows the error and
every note-taking feature keeps working. A broken task database must never cost
anyone their editor.

## The deadline colour

A new palette token, `deadline`, used for nothing else. It exists because
retro82's `danger` is `#f85525`, which that theme actually calls *pink*; the
theme's real red is `#ff2447` and jotter had no equivalent. Event-horizon has no
spare red at all, since its `danger` already *is* its red, so it needs a value of
its own.

| theme | dark | light |
|---|---|---|
| retro82 | `#ff2447` | `#c8102e` |
| event-horizon | `#f4245c` | `#a80f30` |

## Placement

A full-width mode in the main stack, reached from the rail, exactly as the conflict
resolver is. A week of seven columns needs the width, and it keeps jotter
single-pane: you are either writing or planning. `Ctrl+B` still collapses only the
tree.

A running task normally appears as a **slim strip** near the status bar, so the
editor stays usable. In jotter, working often *means* writing, which is why the
Blitzit-style screen takeover is not the default. A **full focus view** is
available on demand for work that is not writing.

The rail gains a third button under Notes, on `Ctrl+D`. Capacity and pomodoro
lengths live in `config.toml` beside `appearance`, because they are preferences
rather than data.

## Structure

**`crates/tasks`** holds pure Rust: SQLite and the domain rules, no GTK. Mirrors
`crates/index` with a `Store` type, `thiserror` errors, and migrations as numbered
`.sql` files. All the error-prone logic lives here and is tested without a display.

**New modules in `app`**, none of it added to `lib.rs`, which is already 4,449
lines:

| module | what it owns |
|---|---|
| `planner.rs` | the full-width mode: seven day columns plus the backlog |
| `day.rs` | today's ordered list and the capacity meter |
| `focus.rs` | the slim running-task strip, and the full focus view |
| `task_edit.rs` | editing one task: title, estimate, deadline, tags, notes, subtasks |
| `reports.rs` | time by tag, estimate accuracy |

## Stages

One commit each, in the rhythm phases 5 and 6 used: built, tested, approved, then
committed.

1. **Planning and tracking together.** The crate, the whole schema, backlog plus
   week plus day, tags, drag and reorder, capacity meter, rollover, and a working
   timer in the slim strip. Usable the day it lands.
2. Focus view, pomodoro, breaks.
3. Subtasks and recurrence.
4. Task notes with wikilinks and the cross-vault switch.
5. Reports.

Stage 1 has four fairly independent pieces (schema and store, date logic, planner
views, timer), so those parallelise across agents. Later stages converge on the
planner UI and are better done in sequence.

## Testing

`crates/tasks` gets unit tests against in-memory SQLite, following
`Index::open_in_memory`. The logic worth testing is pure and tested directly:
recurrence arithmetic across month ends and DST, rollover after a multi-day gap,
capacity with unestimated tasks, slip, reordering. The views are verified through
`tools/gui-test`, which can drive real drags.
