# jotter TUI: implementation plan

jotter becomes notes plus task management, terminal first. A second frontend in the
same workspace, sharing the core crates with the GTK app. Notes stay markdown files in
a git-backed vault. Tasks live in a machine-level database that never travels.

This is the execution guide for that work. It carries intent, ordering and the
contracts between the pieces. It deliberately carries no code: design each step against
the real source when you get to it, because a plan that holds code anchors whoever
implements it to that code, bugs included.

Companion documents: `docs/architecture.md` (crate graph and data flow, GUI-era, still
accurate for the core crates), `docs/conventions-and-pitfalls.md` (coding standards,
writing rules, git conventions, known traps), `docs/implementation-plan.md` (the GUI
plan, phases 0 to 6, historical).

## What was settled

Agreed in the design session of 2026-08-07. Every decision below is closed. If one
turns out wrong during implementation, change it here first, then implement.

### Frontends and layout

- Same workspace, second frontend at `apps/jotter-tui/`.
- The TUI binary is `jotter`. The GTK binary is renamed to `jotter-gui`. Do the rename
  before the TUI exists so `jotter` never means two things depending on install date.
- ratatui plus crossterm. Separate binaries, so installing the TUI on a machine with no
  GTK works.
- Nothing under `crates/` may depend on gtk, glib, gdk, pango or webkit. This holds
  today for `parser`, `index`, `vault`, `git`, `theming` and `search`. It fails for
  `app`, `editor` and `preview`, which are GUI code filed under `crates/` by habit and
  move to `apps/jotter-gui-app`, `apps/jotter-gui-editor` and `apps/jotter-gui-preview`.
  Flat under `apps/`, not nested inside the binary crate: `crates/app/src` reaches the
  repo root through three levels of `..` and so does `apps/jotter-gui-app/src`, which
  keeps its `include_str!` of the logo working untouched. Nesting adds a level and
  breaks it. The other three `include_str!` call sites live in `theming` and `parser`,
  which never move, so only one was ever at risk.

### Notes and tasks are two different worlds

- Notes: markdown files on disk, vault-scoped, git-synced. `<vault>/.jotter/index.db`
  stays a rebuildable cache, gitignored, safe to delete.
- Tasks: a machine-level database in the data directory, never inside a vault, never
  synced, never in git. Switching vault does not change which tasks you see. Cloning a
  vault onto another machine gives an empty task list.
- A `- [ ]` checkbox in a note is plain markdown and never becomes a real task. The
  bridge is deliberately absent: it would smuggle tasks back into the vault.
- In the GUI this is a second rail icon, a mode, not a panel inside the note view.

### Task model

- Project, optional. Task, belonging to zero or one project. Subtask, a checklist
  inside one task with no state and no board presence of its own.
- Nothing ages out. A task lives an hour or six years; deletion is always a choice.
- Task state doubles as its board column.
- Due dates on both tasks and projects, the project's set by hand rather than inferred.
- Derived: overdue by N days, due today, due this week, tasks remaining, tasks overdue,
  and the pace readout, tasks left divided by workdays left against the project date.
  Which days count as workdays is config, defaulting to Monday through Friday.
- No milestone durations, no ordering, no cascade, no projected finish. Those need
  maintenance the rest of the model does not, and a projected date built on a wrong
  sequencing assumption is worse than none. Addable later as two columns if missed.

### No time tracking in v1

No timer, no sessions, no clock anywhere. An open-ended start and stop log
over-reports every time you forget to stop, one absurd entry poisons every total it
appears in, and it cannot be reconstructed afterwards.

Task completion timestamps already answer where the time went, honestly and for free.

Pomodoro can return later as its own piece of work: fixed-size blocks can only ever
undercount, which is survivable. It needs desktop notifications on three operating
systems and must be priced as that feature rather than smuggled in.

### Navigation

- Modes on top, replacing the whole screen, mirroring the GUI rail. Notes and tasks.
- Never more than two panes inside a mode: a list on the left, a subject on the right.
  Tab is therefore a toggle, not a cycle. A four-column board is still one pane.
- Switcher, command palette, search results and the help sheet are overlays, not panes,
  and swallow every key until Escape.
- Enter goes in, Escape comes out, at every level up to quitting. Tab cannot be the
  pane switch because the editor needs it as a tab character.
- Focus decides whether a key acts or types. `j`, `k` and `q` navigate in a list and
  type letters in the editor. There is no mode to be in.
- Arrows, Enter, Escape, Home, End and the Page keys are the real bindings. Vim letters
  are aliases, only where they are not text. A test over the action catalog fails the
  build if any action has a vim binding and no plain-key path.
- Tree: Up and Down move, Left and Right collapse and expand, Enter opens a file and
  toggles a folder, `n` and `N` (also Ctrl+Down and Ctrl+Up) jump between folders.
- Board: Left and Right change column, Up and Down move within one, Enter opens.
- Preview: `n` and `N` step between the actionable things in the document, links and
  images together, Enter activates. A link opens the note, an image opens the system
  viewer.

### Mouse

Never captured. Emit `\x1b[?1007h` at startup so the terminal turns wheel notches into
arrow keys. Scrolling works and select-to-copy stays native, which is the point.

Accepted cost: the wheel drives the focused pane rather than the one under the pointer,
in the editor a notch moves the caret rather than scrolling under it, and there are no
clicks anywhere.

Owning selection ourselves would buy clicks back, but it means drawing selection across
wrapped lines in a scrolling viewport and pushing to the clipboard. Its own job, later,
if ever.

### Keys and the shared catalog

- `jotter-actions` holds the action catalog only, never keys: every action, its title,
  its help text, the mode and pane it belongs to. Both frontends generate their command
  palette and their help sheet from it, so neither can drift.
- Bindings are per-frontend. GTK accelerators and terminal key events are different
  spaces, and the terminal cannot tell Ctrl+Shift+X from Ctrl+X without the kitty
  keyboard protocol, which nothing essential may depend on. The existing GUI keysheet
  already proved the seam by storing action names and asking GTK for labels at display
  time.
- Ctrl+S saves and Ctrl+Z undoes, CUA style, accepting the loss of shell suspend.
- Copy and paste stay with the terminal. Ctrl+C remains the abort for a hung app, and
  binding it to copy would rebuild what not capturing the mouse already preserved.

### Markdown in the terminal

- `ratatui-textarea` 0.9.2 for editing: soft wrap with wrap-aware cursor movement is
  real, undo and redo are there, defaults are emacs-ish so the CUA remap is work.
- No styling while you type. The widget offers one style for the whole area, one for
  the cursor line, one for selection and one for search matches, with no hook for
  arbitrary ranges.
- Formatting appears on toggle to preview, matching the GUI.
- Live preview stays a named follow-up, not an alternative. Width-preserving glyph
  substitution cannot do tables, images or reflowed paragraphs, so it never replaces
  the preview renderer, it adds a second styled surface on top of it. When it happens,
  patch the widget rather than replace it: wrapping, cursor movement and undo already
  work and should stay.
- Parser split: the front half is shared (comrak settings, wikilink resolution,
  frontmatter, syntect colours, which already returns byte ranges rather than markup).
  The terminal backend walks that into an owned document structure. The HTML backend is
  left untouched; rewriting a preview you use daily buys nothing and risks a regression.
- The owned structure exists for a specific reason: a terminal re-wraps on every
  resize, and without it, dragging a window edge re-parses the note on every frame.
  comrak allocates its tree in an arena that dies with the function, so it cannot be
  handed back to a caller anyway.
- `syntect` stays, already in the tree and already reading the palette. `two-face` is a
  when-a-language-is-missing decision, not a v1 one.
- `unicode-width` for wrapping, table columns, CJK and emoji.

### Images

- Half-blocks, via `ratatui-image`. Two pixels per cell, drawn as ordinary coloured
  characters, so scrolling, clipping and resizing all ride on machinery that has to
  work anyway. Identical in Ghostty, in tmux, over ssh and on Windows Terminal.
- One key opens the real file in the system image viewer, at full resolution. That is
  the answer for screenshots, which half-blocks cannot render legibly at any pane size.
- Kitty graphics later, behind a config switch, as a quality upgrade. The escape hatch
  above removes its urgency.
- Only decode what is on screen, cached by file, modification time and target size.
- A missing or undecodable file draws a framed placeholder with the alt text and the
  filename, which doubles as the broken-image report.

### Theming

- The JSONC palette stays the single source of truth. This is already proven: four
  generators (GTK CSS, SourceView XML, preview CSS, terminal-look CSS) run off it today.
- The TUI generator is a fifth, and the easiest, because a terminal cell has only a
  foreground, a background, and bold, italic, underline, dim and reverse.
- The geometry survives. `radius` picks between square and rounded box-drawing corners,
  `border_width` between a hairline and a heavy line. Retro82 and Event Horizon come out
  looking like themselves from fields already in the file.
- At 256 colours, quantise. The colour cube plus greys preserves hue and the
  relationships between colours; a slightly-off Event Horizon is still Event Horizon.
- At 16 colours, stop pretending and use the terminal palette. Colours 0 to 15 are
  whatever the user set, so mapping onto them produces someone else's scheme wearing
  our name, and two jotter themes would become indistinguishable.
- Split by whether an element must be accurate or must be readable. Chrome, headings
  and muted text quantise fine. Selection, cursor and focus ring use reverse video
  rather than a colour, so they are readable by construction at any depth. Syntax
  highlighting falls back to bold, italic and dim on a few hues at 16 colours.
- Ship a `terminal` theme that uses colour indices, sitting in the list beside the
  others. It is a preference, for people who theme their terminal once and want
  everything to match, and is kept separate from the automatic degradation.
- Detection: `COLORTERM`, then `TERM`, then assume 16, with a config override because
  detection is wrong often enough, tmux especially.
- When degradation is active, the theme picker's active row says so. No banner, no
  startup message, no nagging. The answer sits where the question gets asked.
- Skip `extends`. Today a theme file missing a colour fails to load loudly because the
  fields are not optional. Inheritance makes every field optional and turns "this theme
  is complete" from a type guarantee into a runtime check, for a feature that pays off
  with many user themes, of which there are currently zero.
- `Style::Tui` keeps its name; it is a user-facing setting already shipped. The new
  generator is named for what it produces, ANSI colours, so neither name lies.

### Cross-process concurrency

- WAL, a five-second busy timeout, short write transactions, never one held across a UI
  tick. None of this is set today. It does not bite with one binary and bites
  immediately with two: the second writer gets an error instead of waiting.
- Change detection by polling `PRAGMA data_version`. It moves when another connection
  commits and stays put for your own writes, so it has no false positives, and the TUI
  is already sitting in a poll waiting for a keystroke, so it needs no timer or thread.
  Watching the `-wal` file would deliver your own writes, checkpoints, and nothing at
  all on a network mount.
- Clean buffer, file changed on disk: reload it, keep the scroll position, say nothing.
- Dirty buffer: leave it alone, mark the status line, do not prompt. You may be
  mid-sentence with ten minutes to go, and that is the worst possible moment to ask.
  The question gets asked at save, which is a natural stopping point.
- At save: keep mine, take theirs, or show the difference. The third is close to free,
  because `jotter-git` already exposes conflict parsing, choices and application as
  plain logic, and `conflict_model.rs` is GTK-free. Only the drawing is GTK.

### Paths and naming

- Config, unchanged and hand-editable: `~/.config/jotter/config.toml`, plus `themes/`
  and `keys.toml`. Holds what you set and jotter reads. Key remapping is per-frontend,
  so `keys.toml` needs a section per frontend rather than one shared table.
- Data: `~/.local/share/jotter/jotter.db`. Holds tasks and the state jotter constantly
  rewrites behind your back, which is why it is not in the TOML file: that file is
  rewritten wholesale, so with two frontends running, whichever exits last silently
  overwrites the other's recent vaults and last-opened note.
- `JOTTER_DATA_DIR` and `JOTTER_CONFIG_DIR` override both. Tests point them at a fresh
  temp directory every run; hand-driving points them at a `jotter-dev` folder that
  persists. The TUI shows a dim `dev` marker in the status line whenever it is not on
  the real directory.
- Per vault, unchanged: `<vault>/.jotter/` holding `index.db` and the `.gitignore` that
  excludes it. Per-vault settings are never stored inside the vault: they stay keyed by
  path on the machine, and split by the same rule as everything else, so a per-vault
  theme choice is config and a per-vault last-opened note is state in the database.
- Replace `glib::user_config_dir()` with a GUI-free resolver that keeps the Linux path
  exactly where it is. XDG-style everywhere on Unix, which is what CLI tools do and
  what keeps one code path.
- Vault switching happens in-process. Only the notes side swaps; tasks are unaffected.
- Crates: `jotter-actions` (catalog), `jotter-store` (the machine-level database).
  `jotter-index` stays the vault-level derived cache. Index means derived, store means
  authoritative.
- Everything stays path-only. No crates.io. Publishing means coordinated version bumps,
  path dependencies becoming version dependencies, and promising semver on an API only
  we call. Publishing later is easy; unpublishing is not.

### Testing and distribution

- Most tests are data, not pictures: the renderer is a document in and styled lines
  out, wrapping is a pure function, key dispatch is pane plus key to action, and the
  catalog invariant is a walk over a list.
- Snapshot components, never whole screens. A full-screen capture turns red when
  anything anywhere changes, and forty snapshots accepted without reading them means
  the suite confirms whatever the code does.
- Write the absurd-size test first: render every screen at twenty by five and at two
  hundred by fifty and assert no panic. Layout maths is full of subtractions that go
  wrong when the width is 1, and dragging a window past that point is not silly.
- The TUI is far more testable than the GUI. No display server, no headless cage, no
  mutex on a single harness. Much of what is verified by hand today becomes `cargo test`.
- Distribution for now is CI only: build and test on Linux, macOS and Windows, which is
  what keeps the cross-platform claim honest, plus the guard that fails the build if a
  GTK crate reappears in the core tree. No Homebrew, no npm wrapper, no AUR package.
  When other people turn up, a tag-triggered job attaching prebuilt binaries covers
  them all with one workflow file and no per-platform package to keep alive.

## Phases

Work top to bottom. Do not start a phase until the previous one meets its acceptance.
Each phase is independently shippable and leaves the GUI working.

### T0: make room

No user-visible change, and the GUI must behave identically at the end of it.

- Move `crates/app`, `crates/editor` and `crates/preview` to flat siblings under
  `apps/`, and rename `apps/jotter` to `apps/jotter-gui`.
- Rename the GTK binary to `jotter-gui`. Update the PATH wrapper the same day. Between
  T0 and T3 there is no `jotter` binary at all, which is the point: the name is free
  when the TUI arrives rather than being taken by the app it is replacing.
- CI guard: fail the build if gtk, glib, gdk, pango, webkit or sourceview appears in
  the dependency tree of any crate under `crates/`.
- Replace the glib path helper, keeping the Linux paths exactly where they are.
- Set WAL and the busy timeout in `jotter-index`.

Acceptance: the GUI opens a vault, edits, previews, searches and syncs exactly as
before. The guard fails when a GTK dependency is added to a core crate on purpose.

### T1: shared foundations, no UI

- `jotter-actions`: the catalog, plus the test that every action has a plain-key path.
- `jotter-store`: `jotter.db`, forward-only numbered migrations gated on
  `user_version`, refusing to open a database newer than the binary. Projects, tasks,
  subtasks, and the app state moved out of the TOML file. Commands and queries only,
  with no UI-shaped state, so the GUI is a rendering job later rather than a refactor.
- Path resolution and the directory overrides.

Acceptance: full test coverage with no frontend in existence. Task and project
lifecycle, due-date derivation and the pace readout all exercised as data.

### T2: the terminal renderer, still no app

- Split the parser: shared front half, owned document structure for the terminal.
- Document to styled lines at a given width. Wrapping, tables, code blocks through the
  existing syntect ranges, callouts, tasks, wikilinks dimmed when unresolved.
- The ANSI theme generator, including the 256 and 16 colour paths and reverse video for
  the contrast-critical elements.
- Snapshot tests at fixed widths. The absurd-size panic test.

Acceptance: a note renders correctly at 40, 80 and 200 columns, in both themes, at
truecolor and at 16 colours, with nothing panicking at any size.

### T3: the shell

- ratatui and crossterm skeleton, modes, two panes, Enter and Escape, overlays.
- Alternate scroll on, mouse never captured.
- Vault open, tree, note open, read-only, with the preview renderer from T2.
- Switcher and command palette, generated from the catalog and ranked by
  `jotter-search`. The `?` sheet, also from the catalog.
- The PTY test harness in `tools/tui-test/`, excluded from the workspace.

Acceptance: navigate a real vault and read notes in it, driven end to end through the
harness. Hand over a binary.

### T4: editing

- `ratatui-textarea` with the CUA remap, save, undo.
- Editor and preview toggle.
- The dirty-buffer versus disk-change rule, including the resolver view at save.
- Link following and images: half-blocks, the viewer escape hatch, the placeholder.

Acceptance: jotter is a daily driver for notes. Hand over a binary.

### T5: tasks mode

- Board, projects, tasks, subtasks, due dates, the pace readout.
- The dev-directory marker in the status line.

Acceptance: a week of real use without opening the GUI. Hand over a binary.

### T6: the rest

- Full-text search over the vault, through the existing FTS index.
- `data_version` polling and live refresh with both binaries open.
- Settings and theme switching, including the `terminal` theme and the degradation note.

### T7: git in the terminal

Not designed in the 2026-08-07 session. `jotter-git` is already GUI-free and
`conflict_model.rs` is too, so the logic is reusable and only the views are missing.
Design the surface before building it: what sync looks like, where status lives, and
how the conflict resolver behaves in two panes.

## Non-goals for v1

Unchanged from the GUI plan except where noted, and pushed back on where they deserved
it during the design session.

- No sync service, no server, no multi-device conflict resolution. Git handles the
  notes; the database is local by design.
- No plugin system.
- No LSP, no multi-cursor, no code-editor features. The editor is for prose.
- No mobile, no web, no collaborative editing.
- No side-by-side preview. At 100 columns a tree plus two prose panes leaves about 40
  columns each, which is worse than useless for reading.
- No timer, no session tracking, no pomodoro.
- No milestone durations, ordering or projected timelines.
- No note-to-task bridge in either direction.
- No `extends` in theme files.
- No crates.io publishing, no Homebrew, no npm, no AUR.
- Not reproducing every GUI feature. Link autocomplete on typing `[[` is acceptable to
  lose and can be reimplemented differently later.

## Named follow-ups

Deferred deliberately, each with the reason it is deferred rather than cancelled.

- **Live preview while typing.** Patch `ratatui-textarea` to take styles for byte
  ranges rather than replacing it. Plausibly upstreamable, which would mean not
  carrying a fork.
- **Kitty graphics.** Behind a config switch, once the preview viewport is settled.
- **Pomodoro.** Its own piece of work, priced as the desktop-notification feature it is.
- **Mouse clicks.** Requires owning selection and the clipboard.
- **Milestones and cascade.** Two columns on top of the existing model.
- **Release pipeline.** When people other than the author turn up.
