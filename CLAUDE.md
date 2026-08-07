# jotter

A markdown vault app with two frontends over one set of core crates. A vault is a
git-backed folder of plain `.md` files. Notes are files; tasks are not.

- `jotter` is the terminal app (ratatui, crossterm), at `apps/jotter-tui/`.
- `jotter-gui` is the GTK4 app, at `apps/jotter/`.

## Read these first

- `PLAN.md` for the TUI work: every settled decision, the phase order, the non-goals,
  and the deferred follow-ups with the reason each was deferred.
- `docs/conventions-and-pitfalls.md` for coding standards, writing rules, git
  conventions and traps already paid for once.
- `docs/architecture.md` for the crate graph and data flow.
- `docs/implementation-plan.md` for the GUI phases, historical.

## Invariants

Break one of these and something further away breaks quietly.

**Nothing under `crates/` may depend on gtk, glib, gdk, pango, webkit or sourceview.**
CI fails the build if one reappears. GUI code belongs under `apps/jotter/`.

**Tasks never touch a vault.** They live in the machine-level database in the data
directory. Switching vault must not change which tasks are shown, and cloning a vault
onto another machine must give an empty task list. A `- [ ]` checkbox in a note is
plain markdown and never becomes a real task.

**`index.db` is a derived cache, `jotter.db` is authoritative.** The first is
gitignored, rebuildable and safe to delete. The second holds work that exists nowhere
else. Never treat them the same way.

**The shared catalog holds actions, never keys.** `jotter-actions` carries what the app
can do; each frontend binds its own keys. GTK accelerators and terminal key events are
different spaces.

**Every action needs a plain-key path.** Arrows, Enter, Escape, Home, End and the Page
keys are the real bindings. Vim letters are aliases only, and only where they are not
text. A test enforces this.

**Focus decides whether a key acts or types.** In a list `j`, `k` and `q` navigate; in
the editor they type letters. There is no mode to be in. Enter goes in, Escape comes
out. Tab cannot be the pane switch: the editor needs it as a tab character.

**Never capture the mouse.** Alternate scroll (`\x1b[?1007h`) turns wheel notches into
arrow keys, which keeps select-to-copy native. Taking the mouse breaks copying out of
the terminal, and copying out of the terminal matters more than clicking.

**Never more than two panes in a mode.** A list and a subject. Overlays are not panes.
A multi-column board is one pane.

**The JSONC palette is the only source of colour.** Every surface is generated from it.
Do not hand-write CSS, XML or ANSI colours anywhere.

**Nothing essential may depend on the kitty keyboard protocol.** The terminal cannot
tell Ctrl+Shift+X from Ctrl+X without it.

## Building and running

The `jotter` on PATH is a wrapper pointing at `target/release`. Rebuild with
`cargo build --release`.

**Do not build into `target/` while the user may be working.** Cargo locks it, so your
build blocks theirs, and `target/release` is the exact file their wrapper runs. Use a
separate `CARGO_TARGET_DIR`, and prefer a git worktree.

## Testing

The user manually tests every user-facing feature. Before anything reaches them it must
already have been exercised here.

**Never drive the TUI in the user's terminal.** Allocate a pseudo-terminal, feed it
keys, parse the output into a screen grid, assert on that. Nothing is displayed
anywhere. The driver lives in `tools/tui-test/`, excluded from the workspace, mirroring
`tools/gui-test/`. The GUI equivalent is the headless cage; the TUI needs no compositor.

**Always sandbox the directories.** Point `JOTTER_DATA_DIR` and `JOTTER_CONFIG_DIR` at a
fresh temp directory on every test run. `jotter.db` holds real tasks that are in no
vault, in no git repository, and in no backup. An unsandboxed run can destroy real work.

**Test data, not pictures, wherever possible.** The renderer is a document in and styled
lines out. Wrapping is a pure function. Key dispatch is pane plus key to action.

**Snapshot components, never whole screens.** A full-screen snapshot turns red whenever
anything anywhere changes, and a suite whose snapshots get accepted unread confirms
whatever the code does rather than what it should do.

**Render every screen at absurd sizes and assert no panic.** Twenty by five, two hundred
by fifty. Layout maths is full of subtractions that go wrong at width 1.

Hand over a runnable binary, not screenshots.

## Before you ship

Never commit on the strength of unit tests alone. Anything user-facing waits for the
user's confirmation: build the stage, say what to try, hold the commit until they say
it works.
