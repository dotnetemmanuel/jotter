# TUI Phase T1: The Task Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `jotter-store`, the machine-level database holding projects, tasks and subtasks, with due-date derivation and the pace readout, fully tested with no frontend in existence.

**Architecture:** One new crate under `crates/`, purely additive. It owns `jotter.db` in the data directory, which is machine-level and never inside a vault, so switching vault never changes which tasks are shown and cloning a vault onto another machine gives an empty task list. The public surface is commands and queries with no UI-shaped state, so both frontends are rendering jobs rather than refactors.

**Tech Stack:** Rust 2024, `rusqlite` (bundled SQLite), `thiserror`, and one date crate.

## Deviation from the writing-plans skill

This plan carries **no code bodies**, by explicit instruction. It carries exact file paths, exact test names, exact commands and exact expected output. Design each implementation against the real source when you reach it.

The reason: code embedded in a plan anchors the implementer to that code, defects included, and the plan then needs rewriting every time reality diverges from it.

## Global Constraints

- Rust stable, edition 2024, `resolver = "3"`. Version and edition inherited from the workspace.
- `#![warn(clippy::pedantic)]` at the crate root. Warnings are errors.
- No panics in library code. Return `Result`. `crates/*` use `thiserror`.
- Public APIs carry `///` doc comments.
- No em dashes anywhere: prose, comments, docs, commit messages.
- Comments are one line, and only where the code cannot speak for itself.
- Commit subjects in the imperative. No apostrophe character anywhere in a commit message. No `Co-Authored-By` trailer and no generated-by attribution.
- Pin exact dependency versions at first `cargo add`. Do not float majors. `Cargo.lock` is committed.
- **Nothing under `crates/` may depend on gtk, glib, gdk, pango, webkit or sourceview.** CI enforces this.
- Every cargo command uses a `CARGO_TARGET_DIR` outside the repo. `target/release` holds the binary on the user's PATH.
- **Purely additive.** No existing file outside this crate changes, except the workspace `Cargo.lock`.

## The three decisions that shape everything below

**1. This crate never reads the clock.** Every query that depends on "now" takes the date as an argument. A crate that calls `today()` internally cannot be tested: "this task is three days overdue" becomes a fact about the day the suite runs, the suite breaks at midnight, and it gives different answers in different timezones. Passing the date in makes every derived value an ordinary pure computation over known inputs. The frontend supplies the real date; tests supply whatever date makes the case interesting. There is no exception to this rule anywhere in the crate.

**2. A due date is a calendar date, not an instant.** "Due Thursday" does not mean a moment in time, and turning it into one invents a timezone the user never chose. Store due dates as ISO `YYYY-MM-DD` text, which sorts correctly as text and needs no timezone reasoning at all. Store genuine instants (`created_at`, `done_at`) as unix seconds, because those really are moments.

**3. Two frontends will share this file.** Set WAL and a five-second busy timeout, exactly as `crates/index` now does. The reasoning and the traps are identical; read `crates/index/src/lib.rs` before writing this part rather than rediscovering them.

## File Structure

```
crates/store/
  Cargo.toml
  migrations/
    001_init.sql        projects, tasks, subtasks
  src/
    lib.rs              Store: open, migrations, connection setup
    model.rs            Project, Task, Subtask, TaskState, and the error type
    command.rs          everything that writes
    query.rs            everything that reads, including the derived values
    date.rs             calendar date handling, workday arithmetic
  tests/
    lifecycle.rs        projects, tasks, subtasks through their whole life
    derived.rs          overdue, due today, due this week, rollups, pace
    migration.rs        fresh open, reopen, and refusing a newer database
```

Package name `jotter-store`. `jotter-index` stays the vault-level derived cache: index means derived and rebuildable, store means authoritative and irreplaceable. Never treat them the same way.

Splitting commands from queries is not decoration. It is the boundary decision #8 rests on: if a query ever writes, or a command ever returns UI-shaped state, the GUI stops being a rendering job.

---

### Task 1: The crate, the schema, and opening safely

**Files:**
- Create: `crates/store/Cargo.toml`
- Create: `crates/store/migrations/001_init.sql`
- Create: `crates/store/src/lib.rs`
- Create: `crates/store/src/model.rs`
- Create: `crates/store/tests/migration.rs`
- Modify: `Cargo.lock` (from the new crate)

**Interfaces:**
- Consumes: `jotter-paths` from T0, for the data directory. Its `data_dir()` returns `Result<PathBuf, _>` and honours `JOTTER_DATA_DIR`.
- Produces: a `Store` type with `open(path)` and `open_in_memory()`, both returning `Result<Store, StoreError>`. A `StoreError` built with `thiserror`, carrying at least a SQLite variant and a variant for a database written by a newer version. The `model.rs` types other tasks build on: `Project`, `Task`, `Subtask`, `TaskState`.

**Read first:** `crates/index/src/lib.rs` and `crates/index/migrations/`. This crate deliberately mirrors that one's migration machinery, connection setup and error style. Matching it is the point; inventing a second way of doing the same thing is the failure mode.

**The schema.** Three tables. `projects` has a name, an optional due date, a creation instant, and an optional archive instant. `tasks` has a title, a state, an optional project, an optional due date, optional free-text notes, a creation instant, and an optional completion instant. `subtasks` have a title, a done flag, and belong to exactly one task.

Deleting a project must not delete its tasks: they become unfiled. Deleting a task must delete its subtasks, which have no meaning alone. `crates/index` already enables foreign keys per connection, which is required for either rule to fire; do the same.

`TaskState` is a fixed set for v1: not started, in progress, done. Store it as text so the database is readable by a human poking at it with `sqlite3`. Reading an unrecognised value is an error, not a silent default: a row written by a future version must not be quietly downgraded to "not started".

**No ordering column.** Tasks within a state sort by due date with undated last, then by creation. Manual reordering is a real feature and a later one; adding a position column now means machinery with nothing driving it.

- [ ] **Step 1: Write the failing tests**

`crates/store/tests/migration.rs`, four tests:

- `a_fresh_store_is_at_the_current_version` opens an in-memory store and reads `PRAGMA user_version` back, expecting the highest migration number.
- `reopening_applies_nothing_and_changes_nothing` opens an on-disk store in a temp directory, drops it, reopens, and expects the same version and no error.
- `a_database_from_a_newer_version_is_refused` sets `user_version` above the highest known migration on an on-disk file, then expects `open` to return the newer-version error rather than migrating, deleting, or proceeding.
- `an_on_disk_store_is_in_wal_mode_with_a_busy_timeout` reads both pragmas back, expecting `wal` and `5000`.

Put these in an integration file only if they need nothing private. **If any test needs the connection, put it in an in-file `#[cfg(test)] mod tests` instead and read the pragma off the private field directly, the way `crates/index` does.** Do not add public accessors to serve a test; that exact mistake was made and reverted in T0.

`tempfile` is a dev-dependency in the sibling crates already.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p jotter-store`
Expected: FAIL to compile, the crate does not exist.

- [ ] **Step 3: Create the crate and the schema**

Manifest, migration file, `Store` with the same numbered forward-only machinery `jotter-index` uses. Refusing a newer database is the one genuinely new behaviour: read `user_version` before migrating and return the error if it exceeds what this binary knows.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p jotter-store`
Expected: PASS, four tests.

- [ ] **Step 5: Confirm the boundary still holds**

Run the `guard-core-is-gui-free` script from `.github/workflows/ci.yml` locally.
Expected: PASS, and the new crate appears in its derived list. That list comes from `cargo metadata` rather than a hardcoded name, so it should pick the crate up with no edit; confirm it actually did rather than assuming.

- [ ] **Step 6: Commit**

Subject: `add the machine-level task store with its schema`

---

### Task 2: Projects and tasks

**Files:**
- Create: `crates/store/src/command.rs`
- Create: `crates/store/src/query.rs`
- Create: `crates/store/tests/lifecycle.rs`
- Modify: `crates/store/src/lib.rs` (wire the modules in)

**Interfaces:**
- Consumes: `Store`, `Project`, `Task`, `TaskState`, `StoreError` from Task 1.
- Produces: commands creating and updating projects and tasks, and queries reading them back. Later tasks call these to set up their fixtures, so name them for what they do to the domain rather than to the tables.

**Every write is one short transaction.** Never hold one open across anything that waits. A second frontend is blocked for as long as a transaction lives, and the busy timeout only buys five seconds.

**Deleting a project unfiles its tasks rather than destroying them.** Someone tidying up their project list must not silently lose a fortnight of tasks. Prove it with a test rather than trusting the foreign key clause.

- [ ] **Step 1: Write the failing tests**

`crates/store/tests/lifecycle.rs`. At minimum:

- `a_task_can_exist_with_no_project` creates a task with no project and reads it back.
- `a_task_can_be_filed_under_a_project_and_unfiled_again`.
- `deleting_a_project_unfiles_its_tasks_rather_than_deleting_them` creates a project with two tasks, deletes the project, expects both tasks to still exist with no project.
- `moving_a_task_to_done_records_when` sets the state to done and expects a completion instant to be set, having been absent before.
- `moving_a_task_out_of_done_clears_when` moves it back and expects the instant cleared, so a task bounced out of done does not keep a stale completion time.
- `tasks_in_a_state_sort_by_due_date_then_creation` creates dated and undated tasks out of order and expects undated ones last.
- `renaming_a_task_leaves_everything_else_alone`.
- `an_unrecognised_state_in_the_database_is_an_error` writes a bogus state string directly and expects reading it back to fail rather than default.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p jotter-store --test lifecycle`
Expected: FAIL to compile, the commands do not exist.

- [ ] **Step 3: Implement the commands and queries**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p jotter-store --test lifecycle`
Expected: PASS.

- [ ] **Step 5: Confirm nothing regressed**

Run: `cargo test -p jotter-store` and `cargo clippy -p jotter-store --all-targets`
Expected: PASS and clean.

- [ ] **Step 6: Commit**

Subject: `add project and task commands and queries`

---

### Task 3: Subtasks

**Files:**
- Modify: `crates/store/src/command.rs`, `crates/store/src/query.rs`
- Modify: `crates/store/tests/lifecycle.rs`

**Interfaces:**
- Consumes: everything from Task 2.
- Produces: commands to add, rename, toggle and remove a subtask, and a query returning a task's subtasks in a stable order.

A subtask is a checklist line inside one task. It has no state of its own beyond done or not, no due date, no project, and no presence on the board. Do not give it any of those, however natural it looks: the moment a subtask can be scheduled it is a task, and then there are two task types to keep in step forever.

- [ ] **Step 1: Write the failing tests**

Appended to `crates/store/tests/lifecycle.rs`:

- `a_subtask_belongs_to_exactly_one_task`.
- `deleting_a_task_deletes_its_subtasks` and confirms none are orphaned.
- `subtasks_come_back_in_a_stable_order` adds several and reads them back twice, expecting the same order both times.
- `toggling_a_subtask_does_not_touch_its_task` confirms the parent task's state and completion instant are unchanged, so a checklist cannot silently complete a task.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p jotter-store --test lifecycle`
Expected: FAIL.

- [ ] **Step 3: Implement**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p jotter-store --test lifecycle`
Expected: PASS.

- [ ] **Step 5: Commit**

Subject: `add subtasks as a checklist inside one task`

---

### Task 4: Due dates, and the pace readout

This is where the only real logic in the phase lives. Everything above is storage.

**Files:**
- Create: `crates/store/src/date.rs`
- Create: `crates/store/tests/derived.rs`
- Modify: `crates/store/src/query.rs`
- Modify: `crates/store/Cargo.toml` (the date dependency)

**Interfaces:**
- Consumes: everything from Tasks 2 and 3.
- Produces: date handling, and queries taking `today` plus a workday set and returning derived values.

**Pick a date crate** that can parse an ISO date, give a weekday, and step between dates. `time` and `chrono` both do. **No timezone handling is needed anywhere**, because every date crossing this boundary is a local calendar date the caller supplies. If you find yourself reaching for a timezone type, stop: something has turned a date into an instant.

**The pace readout** is tasks left divided by workdays remaining until the project's due date. It answers "how many a day would I have to close from here". Which days count as workdays is a parameter, defaulting to Monday through Friday.

Work out the state table before writing, because the empty cells are where the defects live. At minimum: the project has no due date; the due date is today; the due date has passed; no tasks remain; every remaining task is already done; the remaining workdays are zero because the deadline is this weekend. **Decide what each of those returns rather than letting it fall out of the arithmetic**, and note that "tasks left divided by workdays left" divides by zero in at least two of them.

- [ ] **Step 1: Write the failing tests**

`crates/store/tests/derived.rs`. Every test passes an explicit `today`. At minimum:

- `a_task_due_before_today_is_overdue_by_the_right_number_of_days`.
- `a_task_due_today_is_due_today_and_not_overdue`, the boundary that off-by-one errors land on.
- `a_task_with_no_due_date_is_never_overdue`.
- `a_done_task_is_never_overdue_even_if_its_date_has_passed`, because a completed task should not keep nagging.
- `due_this_week_uses_calendar_days_not_a_rolling_seven`, or the opposite; pick one, state it in the doc comment, and test the one you picked.
- `a_project_counts_only_its_own_unfinished_tasks`.
- `the_pace_is_tasks_left_over_workdays_left`, with numbers chosen so the answer is not 1.
- `a_weekend_between_today_and_the_deadline_does_not_count`.
- `a_project_with_no_due_date_has_no_pace`.
- `a_deadline_already_past_has_no_pace_rather_than_a_negative_one`.
- `a_deadline_with_no_workdays_before_it_has_no_pace_rather_than_a_division_by_zero`.
- `a_project_with_nothing_left_has_a_pace_of_zero_rather_than_no_pace`, which is a different answer from the case above and the pair is the point.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p jotter-store --test derived`
Expected: FAIL.

- [ ] **Step 3: Implement**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p jotter-store --test derived`
Expected: PASS.

- [ ] **Step 5: Prove the crate never reads the clock**

Run: `grep -rnE "SystemTime::now|Instant::now|Utc::now|Local::now|OffsetDateTime::now" crates/store/src/`
Expected: **no matches in any query path.** A creation or completion instant has to come from somewhere, so if a command stamps one itself, that is the one permitted place and it must be named in your report. Nothing that computes a derived value may read the clock.

- [ ] **Step 6: Run the full suite and the boundary guard**

Run: `cargo test -p jotter-store`, `cargo clippy -p jotter-store --all-targets`, and the CI guard script.
Expected: PASS, clean, and the crate present in the guard's derived list.

- [ ] **Step 7: Commit**

Subject: `derive overdue, due soon, and the project pace readout`

---

## Acceptance for T1

- `cargo test --workspace` passes, with the new crate's tests included and every pre-existing test still passing.
- No file outside `crates/store/` changed, except `Cargo.lock`.
- The GUI is untouched and still behaves exactly as it does today.
- No query reads the clock.
- The CI boundary guard picks the new crate up without being edited.

There is nothing to hand over and nothing for the owner to click. That is the point of doing this phase before any frontend exists: the whole thing is verifiable as data.

## Not in T1

Named so they are not pulled in by accident:

- **No `jotter-actions`.** Moved to T3, alongside the palette and help sheet that read it.
- **No moving app state out of `config.toml`.** Moved to T3, when a second writer actually exists. Touching it earlier risks the user's recent vaults and last-opened notes for no benefit.
- **No time tracking.** No timer, no sessions, no clock. Settled in `PLAN.md` and not revisited here.
- **No milestones, durations, ordering or projected finish dates.**
- **No manual task ordering.** Sort order is due date then creation until something needs otherwise.
- **No link from a task to a note.** Tasks are vault-independent, and a pointer into a vault is a design decision nobody has taken.
- **No `ratatui`, no `crossterm`, no `apps/jotter-tui/`.**
