# TUI Phase T0: Make Room Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear the GTK dependency out of `crates/`, split the binary name, give both frontends one way to find their directories, make the index safe for two writers, and put a guard in CI so none of it silently regresses.

**Architecture:** T0 is preparation, not feature work. `crates/app`, `crates/editor` and `crates/preview` are GUI code that was filed under `crates/` by habit; they move to `apps/` where the GTK dependency is legal. Path resolution comes out of the GUI and into a core crate so the TUI can use it. The index gains WAL and a busy timeout, which it needs the moment a second binary exists. CI enforces the boundary from then on.

**Tech Stack:** Rust 2024, Cargo workspace, `rusqlite` (bundled SQLite), an XDG-strategy path crate, GitHub Actions.

## Deviation from the writing-plans skill

This plan carries **no code bodies**, by explicit instruction. It carries exact file paths, exact test names, exact commands and exact expected output. Design each implementation against the real source when you reach it.

The reason: on previous work, code embedded in a plan anchored the implementer to that code, defects included, and the plan then needed rewriting every time reality diverged from it.

## Global Constraints

- Rust stable, edition 2024, `resolver = "3"`. Pinned in `rust-toolchain.toml`.
- `#![warn(clippy::pedantic)]` at every crate root. Warnings are errors.
- No panics in library code. `crates/*` use `thiserror`; app and binary use `anyhow`.
- Public APIs carry `///` doc comments.
- No em dashes anywhere: prose, comments, docs, commit messages, UI strings.
- Comments are one line, and only where the code cannot speak for itself.
- Commit subjects in the imperative. No apostrophe character anywhere in a commit message. No `Co-Authored-By` trailer and no generated-by attribution.
- Pin exact dependency versions at first `cargo add`. Do not float majors.
- `Cargo.lock` is committed.
- **The GUI must behave identically at the end of T0.** Nothing here is user-visible except the binary name.

## Prerequisite

**`preview-perf` must land on `main` before Task 1 runs.** Task 1 relocates `crates/preview`, and `preview-perf` has unfinished work inside that exact directory. Doing this first guarantees a conflict that has to be resolved by hand across a moved path.

Check before starting: `git rev-list --left-right --count main...preview-perf` must report `0 0`, or `preview-perf` must be merged and deleted.

## File Structure

Where things end up, and what each is responsible for.

```
crates/
  paths/       NEW. Where jotter keeps config and data, per OS, with overrides.
  index/       Vault-level SQLite. Gains WAL and a busy timeout.
  parser/ vault/ git/ theming/ search/    unchanged
apps/
  jotter-gui/           was apps/jotter. Binary crate, now named jotter-gui.
  jotter-gui-app/       was crates/app. GTK state graph and command dispatcher.
  jotter-gui-editor/    was crates/editor. GtkSourceView wrapper.
  jotter-gui-preview/   was crates/preview. WebKit wrapper.
.github/workflows/
  ci.yml       NEW. fmt, clippy, tests per platform, and the GTK boundary guard.
```

Flat under `apps/` is deliberate. `crates/app/src/lib.rs` reaches the repo root through three levels of `..`, and so does `apps/jotter-gui-app/src/lib.rs`, so every `include_str!` pointing at `resources/` keeps working untouched. Nesting the crates inside `apps/jotter-gui/` would add a level and break all four of them.

---

### Task 1: Relocate the GTK crates and split the binary name

**Files:**
- Move: `crates/app/` to `apps/jotter-gui-app/`
- Move: `crates/editor/` to `apps/jotter-gui-editor/`
- Move: `crates/preview/` to `apps/jotter-gui-preview/`
- Move: `apps/jotter/` to `apps/jotter-gui/`
- Modify: `Cargo.toml` (workspace members)
- Modify: all four moved `Cargo.toml` files (package names and path dependencies)
- Modify: `apps/jotter-gui/src/main.rs` (crate name in `use`)
- Modify: `install.sh`
- Modify: `docs/architecture.md` (the workspace layout and dependency direction blocks)

**Interfaces:**
- Consumes: nothing. This is the first task.
- Produces: package names `jotter-gui`, `jotter-gui-app`, `jotter-gui-editor`, `jotter-gui-preview`. The binary produced by `apps/jotter-gui` is named `jotter-gui`. Every core crate under `crates/` keeps its existing package name unchanged.

- [ ] **Step 1: Confirm the prerequisite**

Run: `git rev-list --left-right --count main...preview-perf`
Expected: `0	0`, or the branch is gone. If `preview-perf` is still ahead, stop and land it first.

- [ ] **Step 2: Record the baseline**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: PASS. Write the passing test count down. Task 1 is correct only if this number is identical at the end.

- [ ] **Step 3: Move the four directories with git**

Use `git mv` for each, so history follows the files rather than showing four deletions and four additions.

- [ ] **Step 4: Update the workspace members**

`Cargo.toml` at the root: members become `crates/*` and `apps/*`. The `exclude` for `tools/gui-test/wlpoint` stays exactly as it is.

- [ ] **Step 5: Rename the four packages and fix path dependencies**

Each moved `Cargo.toml` gets its new package name. Every `path = "../..."` dependency between the moved crates changes because they are now siblings under `apps/` rather than siblings under `crates/`, and their dependencies on core crates now point out of `apps/` and into `crates/`.

`apps/jotter-gui/Cargo.toml` also needs its `[[bin]]` name changed to `jotter-gui`.

- [ ] **Step 6: Fix the one `use` in the binary**

`apps/jotter-gui/src/main.rs` refers to the app crate by name. Update it to `jotter_gui_app`.

- [ ] **Step 7: Build**

Run: `cargo build --workspace`
Expected: PASS. Any `include_str!` failure here means a directory landed at the wrong depth; re-check against the File Structure block rather than editing the include path.

- [ ] **Step 8: Confirm the test count is unchanged**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: PASS, with the same number of tests as Step 2. A different number means a crate stopped being built, not that a test was fixed.

- [ ] **Step 9: Confirm the binary name changed and nothing else did**

Run: `ls target/debug/jotter-gui && ls target/debug/jotter 2>&1`
Expected: `jotter-gui` exists, `jotter` does not.

- [ ] **Step 10: Update install.sh**

The build target, the installed binary path, and the desktop entry `Exec` line all move to `jotter-gui`. Leave `APP_ID`, `Name=jotter` and the icon handling alone: changing the app id orphans the icon cache and any pinned launcher entry, and the launcher entry is still the graphical jotter as far as the user is concerned. Update the two `echo` lines at the end so they name the new binary.

- [ ] **Step 11: Update the architecture doc**

`docs/architecture.md` opens with a workspace layout block and a dependency direction block. Both now describe a layout that no longer exists. Correct them to match the File Structure block above.

- [ ] **Step 12: Run the GUI and confirm it is unchanged**

Do this off-screen, per `CLAUDE.md`, with a separate `CARGO_TARGET_DIR` and sandboxed directories. Open a vault, open a note, toggle to preview, run a search.

This is a relocation, so the risk is not subtle logic but a resource that silently stopped loading. Check specifically that the theme applied, the logo drew, and a fenced code block came out coloured. All three come through `include_str!` from `resources/`.

- [ ] **Step 13: Commit**

Subject: `move the GTK crates into apps and rename the binary to jotter-gui`

Body, two lines at most: that `crates/` is now GUI-free, and that the flat layout under `apps/` keeps the `resources/` include paths at the same depth.

---

### Task 2: Extract path resolution into a core crate

`crates/app/src/config.rs` finds the config directory through `gtk::glib::user_config_dir()`. After Task 1 that call is legal where it sits, but the TUI cannot use it and needs the same answer. This task moves the question into a core crate and adds the directory overrides that testing depends on.

**Files:**
- Create: `crates/paths/Cargo.toml`
- Create: `crates/paths/src/lib.rs`
- Create: `crates/paths/tests/resolve.rs`
- Modify: `apps/jotter-gui-app/src/config.rs` (drop the glib helper, call the crate)
- Modify: `apps/jotter-gui-app/Cargo.toml` (add the dependency)

**Interfaces:**
- Consumes: the crate layout from Task 1.
- Produces: a crate named `jotter-paths` exposing two directory lookups, one for config and one for data. Each returns `Result<PathBuf, _>` with a `thiserror` error type, because finding the platform base directory can fail on a system with no home directory, and the global constraints forbid panicking in library code. It also exposes the pure resolution function that both wrappers call, taking the override value and an already-resolved base directory as arguments and returning a plain `PathBuf`, since with both inputs in hand the resolution itself cannot fail.

  This makes `config_dir()` fallible at the GUI call site in `apps/jotter-gui-app/src/config.rs`, which currently calls it unconditionally. That crate uses `anyhow` at the boundary, so surfacing it there is the intended shape rather than a workaround.

**The design constraint that matters:** the environment is process-global, and Rust runs tests in parallel threads. A test that sets an environment variable to check the override will race any other test reading it, and in edition 2024 setting one is `unsafe` for exactly that reason. So the resolution logic must be a pure function taking its inputs as arguments, with a thin wrapper that reads the environment and calls it. Test the pure function. Do not write tests that mutate the environment.

- [ ] **Step 1: Write the failing tests**

`crates/paths/tests/resolve.rs`, four tests against the pure resolution function:

- `an_override_wins_over_the_base_directory` gives both an override and a base, expects the override, used exactly as given with no `jotter` component appended.
- `no_override_appends_the_app_directory_to_the_base` gives only a base, expects the base with `jotter` joined onto it.
- `an_empty_override_is_ignored` gives an override that is the empty string, expects the same answer as no override, because an unset variable and an empty one arrive identically from a shell.
- `the_config_and_data_lookups_resolve_independently` confirms a config override does not affect the data answer and the reverse.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p jotter-paths`
Expected: FAIL to compile, because the crate does not exist yet.

- [ ] **Step 3: Create the crate**

Manifest with the workspace version and edition, `#![warn(clippy::pedantic)]` at the root, `thiserror` only if a fallible path appears (it should not: a missing home directory is the one real failure, and the platform crate already handles it).

Add an XDG-strategy path dependency. **The Linux answer must not move:** it is `$XDG_CONFIG_HOME` when set and `~/.config` otherwise, because the GUI already writes there and a migration would be churn for nothing. Prefer a strategy that gives the same XDG answer on macOS too, which is what terminal tools conventionally do and what keeps one code path. Windows uses `%APPDATA%`.

The overrides are `JOTTER_CONFIG_DIR` and `JOTTER_DATA_DIR`, and they are absolute paths used verbatim.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p jotter-paths`
Expected: PASS, four tests.

- [ ] **Step 5: Switch the GUI over**

In `apps/jotter-gui-app/src/config.rs`, delete the glib helper and call `jotter-paths` instead. `themes.rs` calls `config_dir()` and should keep compiling untouched.

- [ ] **Step 6: Confirm the GUI path did not move**

Run the GUI off-screen and confirm it reads the existing `~/.config/jotter/config.toml`: the vault it opens on launch should be the one from the recents list, not an empty picker. A wrong path here looks exactly like a first run.

- [ ] **Step 7: Confirm the override works**

Run the GUI off-screen with `JOTTER_CONFIG_DIR` pointed at an empty temp directory. Expected: it behaves as a first run and writes into that directory, leaving the real one untouched. Confirm the real `~/.config/jotter/config.toml` modification time is unchanged afterwards.

- [ ] **Step 8: Commit**

Subject: `find config and data through a core crate rather than glib`

---

### Task 3: WAL and a busy timeout on the index

Two binaries will open the same `index.db`. Right now neither WAL nor a busy timeout is set anywhere in the repo, so the second writer gets an immediate error rather than waiting.

**Files:**
- Modify: `crates/index/src/lib.rs` (the shared `init`)
- Create or modify: `crates/index/tests/concurrency.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: no API change. `Index::open` and `Index::open_in_memory` keep their signatures. The behaviour change is that an on-disk index is in WAL mode and every connection waits up to five seconds for a lock.

**Two traps to know before writing it:**

`journal_mode` is not an ordinary pragma. Setting it returns a row, so the pragma-update call that works for `foreign_keys` will not work here; use the rusqlite call that reads the result back. And the mode is written into the database file, so it persists across opens, which means the assertion in Step 1 has to hold on a reopened database and not only on a fresh one.

`busy_timeout` is the opposite: per connection, set on every open, never persisted.

WAL on an in-memory database is a no-op that reports `memory` rather than `wal`. Do not assert `wal` against `open_in_memory`, and do not special-case it in the implementation either.

- [ ] **Step 1: Write the failing tests**

`crates/index/tests/concurrency.rs`, three tests:

- `an_on_disk_index_is_in_wal_mode` opens an index in a temp directory and reads `PRAGMA journal_mode` back, expecting `wal`.
- `wal_survives_a_reopen` opens, drops, reopens the same file, and expects `wal` again.
- `every_connection_gets_a_busy_timeout` opens an index and reads `PRAGMA busy_timeout` back, expecting `5000`.

`tempfile` is already a dev-dependency of this crate.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p jotter-index --test concurrency`
Expected: FAIL. The journal mode reads `delete` and the busy timeout reads `0`.

- [ ] **Step 3: Set both in the shared init**

Both go in `Index::init`, which is the one path `open` and `open_in_memory` share, so neither can be added later without picking them up.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p jotter-index --test concurrency`
Expected: PASS, three tests.

- [ ] **Step 5: Confirm nothing else regressed**

Run: `cargo test -p jotter-index`
Expected: PASS, the full index suite. Migrations run inside transactions and WAL changes when those become visible to other connections, so the existing migration tests are the ones to watch.

- [ ] **Step 6: Confirm the new files are already ignored**

Run: `cat crates/git/src/ignore.rs | grep -A5 index.db`
Expected: `index.db-wal` and `index.db-shm` are already in the generated ignore list. They are, and this step exists to prove it rather than to change it. If they were not, a vault would start committing its WAL.

- [ ] **Step 7: Commit**

Subject: `open the index in WAL mode with a busy timeout`

Body, one line: that two binaries will share this file and the second writer currently errors instead of waiting.

---

### Task 4: CI, with the boundary guard

There is no `.github/` in this repository. This task creates it.

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the crate layout from Task 1. The guard's list of core crates is exactly the packages under `crates/`.
- Produces: nothing consumed by later tasks.

**The platform constraint that shapes this:** the GUI cannot build on macOS or Windows, because GTK4, GtkSourceView and WebKitGTK are not available there. So a workspace-wide build on those runners will fail, and that failure would say nothing useful. Linux builds and tests everything. macOS and Windows build and test the core crates under `crates/` only, which is precisely the set that has to work everywhere and the set the TUI will be built on.

- [ ] **Step 1: Write the workflow**

Jobs:

- **lint**, Linux only: `cargo fmt --check`, then `cargo clippy --workspace --all-targets` with warnings as errors. Needs the GTK system packages installed.
- **test-linux**: `cargo test --workspace`. Needs the GTK system packages.
- **test-portable**, on macOS and Windows: build and test each package under `crates/` by name. No system packages needed. This job is the cross-platform claim, so it must not silently degrade to building nothing; list the packages explicitly rather than using a glob that could match an empty set.
- **guard-core-is-gui-free**, Linux only: see the next step.

- [ ] **Step 2: Write the guard**

For each package under `crates/`, run `cargo tree --edges normal --package <name>` and fail if the output contains any of `gtk`, `glib`, `gdk`, `pango`, `webkit`, `sourceview`, `gobject`, `graphene` or `cairo`.

Two things to get right. Search normal edges only: dev-dependencies are allowed to pull anything and a build-dependency match is not what this guards. And fail the job on a match, which means being careful with `grep` in a pipeline, since `grep` exits non-zero when it finds nothing, which is the success case here and the opposite of the usual shape.

- [ ] **Step 3: Verify the guard actually catches the thing it exists for**

This is the only step in Task 4 that proves anything. A guard nobody has seen fail is a guard nobody knows works.

Temporarily add a `gtk` dependency to `crates/theming/Cargo.toml`. Run the guard command locally.
Expected: it fails, and the message names `jotter-theming`.

Then remove the dependency and run it again.
Expected: it passes.

Do not commit the temporary dependency. Resolving the tree rewrites `Cargo.lock` as well, so `git status` must show **both** `crates/theming/Cargo.toml` and `Cargo.lock` clean before the next step. Restore the lock with `git checkout -- Cargo.lock` if it moved.

- [ ] **Step 4: Verify the portable job list is complete**

Run: `ls crates/`
Compare against the package list in the `test-portable` job. Every directory must appear. A crate added later and forgotten here silently stops being checked on Windows, which is the failure this project is most likely to make.

- [ ] **Step 5: Commit**

Subject: `add CI with a guard against GTK in the core crates`

Body, two lines at most: that the GUI cannot build on macOS or Windows, so those runners cover `crates/` only.

---

## Acceptance for T0

All four tasks committed, and:

- `cargo test --workspace` passes with the same test count as before Task 1.
- The GUI opens a vault, edits, previews, searches and syncs exactly as it did, with the theme, the logo and code colouring all intact.
- `target/release/jotter-gui` exists and `target/release/jotter` does not.
- `~/.config/jotter/config.toml` is still the file the GUI reads, and `JOTTER_CONFIG_DIR` redirects it.
- A deliberate GTK dependency added to any crate under `crates/` fails CI.

Hand the GUI binary over and get confirmation before T0 is called done. It is a pure relocation, which is exactly the kind of change that looks finished and is not.

## Not in T0

Named here so they are not pulled in by accident:

- No `jotter-actions` and no `jotter-store`. Those are T1.
- No `ratatui`, no `crossterm`, no `apps/jotter-tui/`. The TUI does not exist yet.
- No `data_version` polling. WAL and the busy timeout are the mechanical half; change detection is T6.
- No parser split. That is T2.
- No moving app state out of `config.toml` into the database. The database does not exist yet; that is T1.
