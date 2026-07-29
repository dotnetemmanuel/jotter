# jotter: implementation plan

A native GTK4 markdown vault app. A vault is a git-backed folder of plain `.md`
files. jotter opens it, indexes it, edits it, previews it, and syncs it. No cloud,
no proprietary format, git as the source of truth.

This document is the execution guide. Work top to bottom. Do not start a phase
until the previous phase meets its acceptance criteria. Ship phases 0 through 5
before touching any v1.5 feature.

## Status (2026-07-28)

Phases 0 through 3 are complete. Phase 4 is complete: frontmatter, tags,
backlinks, the tag page, and the broken-link report.
Phase 3 was split: wikilinks landed first because they carry the correctness risk
and are testable without a GUI, and the pickers share a widget with search.

Phase 2 shipped `crates/vault` (enumerate with ignore rules, atomic IO,
delete-to-trash, debounced `notify` watcher), `crates/index` (SQLite, migrations
by `user_version`, FTS5 with `contentless_delete=1`), and the `app` wiring:
sidebar `TreeListModel`, `Ctrl+B`, background indexer, watcher drain, right-click
file operations, and recents plus last-active note in the config. The context
menu targets the row under the pointer via a hit test rather than the current
selection, so operations land in the folder that was actually clicked, and empty
space targets the vault root.

Phase 2.5 landed as a flat, Bauhaus-leaning take on the neo-brutalist brief:
small crisp corners, no drop shadows, and thick lines used structurally instead
of as boxes. The sidebar recolors with the theme, entries are underlines rather
than framed boxes, and the preview picked up bold geometric headings, framed code
blocks, and an accent quote bar. Chrome and preview are driven by the same theme
tokens, so `Ctrl+T` restyles every surface at once.

Phase 1 shipped the theming crate, the instant edit-preview toggle, and a polish
pass:

- Preview theme CSS is embedded as an author `<style>` per render, not a
  `webkit6::UserStyleSheet` (that dropped table cell padding or the body
  background depending on the injection level).
- Scroll-to-heading on toggle is reliable: the fresh file loads first, then the
  `#anchor` navigation runs on the load-finished signal as a same-document scroll.
- Table cell padding with rounded outer corners (separate borders, rounded corner
  cells, no clip).
- `Ctrl+T` switches the active theme between light and dark, recoloring the
  preview in place via `reload_bypass_cache` so the scroll position is preserved.
- App id is `dev.jotter.Jotter`.

## Identity and paths

- App name / binary / crate: `jotter`
- Global config: `~/.config/jotter/config.toml`
- User themes: `~/.config/jotter/themes/*.json`
- Keybindings: `~/.config/jotter/keys.toml`
- Logs: `~/.local/state/jotter/log/`
- Per-vault dir: `<vault>/.jotter/` (holds `index.db`, `config.toml`, state)
- Vault ignore list at root: `.git`, `.trash`, `.jotter`, any dotfile
- Trash target for deletes: `<vault>/.trash/` (never `unlink`)

## Toolchain and hard dependencies

- Rust: latest stable, edition 2024. Verified on 1.96.1 (rustup stable). Pinned via
  `rust-toolchain.toml` (`channel = "stable"`, components rustfmt + clippy) so the
  workspace tracks stable and every checkout gets the same tools.
- Set `edition = "2024"` in every crate manifest and `resolver = "3"` in the
  workspace root (the edition-2024 default resolver).
- System packages (document in README): `gtk4`, `libadwaita` (optional chrome),
  `gtksourceview5`, `webkitgtk-6.0`, `sqlite`, `libgit2`, `libssh2`.
- Pin `webkitgtk-6.0` minimum in the README. It updates independently of GTK4.

## Crate dependency map (pin exact versions at first `cargo add`)

- vault:   `notify`, `notify-debouncer-full`, `walkdir`, `thiserror`
- index:   `rusqlite` (features `bundled`, `modern_sqlite`, `functions`), `thiserror`
- parser:  `comrak`, `gray_matter`, `syntect`, `regex`, `once_cell`, `thiserror`
- git:     `git2`, `thiserror`
- theming: `serde`, `serde_json`, `serde_jsonc` (or manual comment strip), `thiserror`
- editor:  `gtk4`, `sourceview5`
- preview: `gtk4`, `webkit6`
- app:     `gtk4`, `relm4` (evaluate in phase 1), `anyhow`, `tracing`, `tracing-subscriber`, `serde`, `toml`
- binary:  depends on `app` only; stays thin.

Rule: `crates/*` use `thiserror` and return `Result`. `app` and the binary use
`anyhow` at the boundary. No panics in library code.

---

## Phase 0: skeleton (target: one evening)

Goal: `cargo run` opens a clean Wayland-native GTK4 window on Hyprland.

Tasks:
1. Workspace `Cargo.toml` with `members = ["crates/*", "apps/jotter"]`.
2. Stub every crate in the architecture with an empty `lib.rs` and one `hello()`
   test that passes.
3. `apps/jotter/src/main.rs`: build a `gtk::Application`, connect `activate`,
   create `ApplicationWindow` titled "jotter", default size 1400x900, present it.
   Add a `HeaderBar` and a placeholder body label.
4. Set the app id to `dev.jotter.Jotter` (reverse-DNS, stable for GTK).

Acceptance:
- `cargo build --workspace` clean, no warnings.
- `cargo test --workspace` green (all `hello()` tests).
- `cargo run` shows a native window under Hyprland that opens and closes cleanly.
- Commit: "phase 0: workspace skeleton and empty GTK window".

---

## Phase 1: editor loop (target: 1 week)

Goal: instant edit-preview toggle on a single file. No vault, no git.

### 1a. Theming crate first (pure, testable without a GUI)

This is the highest-leverage pure-logic unit. Build and test it with zero GTK.

Tasks:
- Define serde structs for the theme JSON (palette, chrome, editor, preview, code,
  typography). See `docs/architecture.md` for the type shape.
- JSONC parse: strip `//` and `/* */` comments, then `serde_json`. Comment strip
  must not touch `//` or `/*` inside string values.
- Palette resolver: `$foo` resolves to `palette.foo`. One hop only. A `$foo`
  whose value is another `$bar` is a load error. An undefined `$foo` is a load
  error naming the exact field path.
- Three generators, each a pure function of the parsed theme:
  - `to_gtk_css() -> String` for app chrome (`gtk::CssProvider::load_from_data`).
  - `to_sourceview_scheme_xml() -> String` (GtkSourceView style scheme XML).
  - `to_preview_css() -> String` (embedded as an author `<style>` per render).
- Bundle `resources/themes/retro82.json` (default) and `event-horizon.json` via
  `include_str!`. Each file carries a dark and a light palette; default is retro82 dark.

Acceptance:
- `insta` snapshot tests for all three generator outputs against retro82 and
  event-horizon (dark plus the light path).
- Unit tests: missing required palette key -> specific error; bad `$ref` -> specific
  error; unknown top-level field -> ignored, no error.

### 1b. Editor / preview toggle

Tasks:
- `crates/editor`: wrap `sourceview5::View` + `Buffer`. Load the markdown language
  spec. Register the generated style scheme with `StyleSchemeManager`. Enable
  current-line highlight, right-margin guide, bracket matching. Line numbers off
  by default (config-gated later).
- `crates/preview`: wrap `webkit6::WebView`. JavaScript disabled by default. Embed
  the preview CSS as an author `<style>` in each rendered document. Expose
  `render(html, anchor)`.
- `crates/parser`: `markdown_to_html(src) -> String` using comrak with GFM
  extensions (tables, strikethrough, tasklists, autolinks, footnotes). Strip
  frontmatter before parsing. Wire syntect for fenced code blocks (bundled theme
  derived from the active theme code map). Wikilinks come in phase 3, leave a seam.
- `app`: put editor and preview in a `GtkStack` occupying one window region.
  `Ctrl+E` switches pages. On switch to preview: parse buffer, load HTML.
- Debounce: when already on the preview page and the buffer changes, re-render with
  a 150 ms debounce. Primary flow stays toggle-driven.
- Position preservation: cache caret line and scroll per mode. Edit -> preview
  scrolls preview to the heading nearest the caret line (build a line->heading-anchor
  map during render). Preview -> edit restores the exact caret.

Acceptance:
- Open a sample `.md` from a path arg. `Ctrl+E` toggles edit/preview and feels
  instant (no visible resize, stack transition only).
- Editor and preview both wear Event Horizon colors from the single theme JSON.
- `insta` snapshot test for the markdown-to-HTML pipeline on a fixture doc.
- Commit: "phase 1: theming crate and instant edit-preview toggle".

Pitfall guard: wrap any programmatic buffer edits in
`buffer.begin_user_action()` / `end_user_action()` so undo history stays coherent.

---

## Phase 2: vault (target: 1 week)

Goal: open a folder as a vault, browse and mutate it, index scaffolding live.

Tasks:
- `crates/vault`: open a vault root, enumerate `.md` (ignore `.git`, `.trash`,
  `.jotter`, dotfiles). Note IO: read, write (atomic via temp + rename), create,
  rename, delete-to-trash.
- `notify` watcher: recursive on the vault root, debounced with
  `notify-debouncer-full`, ignore list applied. Emit a typed change stream.
- File tree sidebar in `app`: collapsible folders, right-click menu (new note,
  new folder, rename, delete-to-trash). `Ctrl+B` toggles the sidebar.
- `crates/index`: open SQLite at `<vault>/.jotter/index.db`. Migrations under
  `crates/index/migrations/NNN_description.sql`, applied on connection open. Ship
  `001_init.sql` with the schema from `docs/architecture.md`.
- Startup: on open, compare on-disk mtimes to the index and reindex changed files
  on a background thread. UI must be interactive before indexing finishes. Show
  progress in the status bar.
- Persist last-active note per vault; reopen it on next launch.
- Global config: recent vaults list (last N). Open-vault dialog plus recent list.

Acceptance:
- Launch with `jotter <folder>`; tree renders, indexing runs in the background,
  status bar shows progress, UI stays responsive.
- Create / rename / delete-to-trash all work and the watcher reflects external
  edits made by another editor.
- Reindex on external change is incremental (only changed files).
- Commit: "phase 2: vault, file tree, watcher, index scaffolding".

Pitfall guard: inotify has a per-user watch limit. Document the
`fs.inotify.max_user_watches` bump in the README. Never index on the UI thread.

---

## Phase 2.5: visual language (target: 3 to 4 days)

Goal: a deliberate neo-brutalist pass over the whole UI, done once phase 2 exists
so every surface (chrome, sidebar, file tree, editor, preview) can be styled
together against a single theme source. Until now the CSS was functional defaults
out of the theming generators, never a design pass.

Reference: https://dribbble.com/search/neo-brutalism (hard borders, flat blocks,
offset/hard drop shadows, chunky focus rings, high-contrast accents, generous
padding, minimal gradients). Keep the worf-inspired direction and the two bundled
themes both working in light and dark.

Tasks:
- Extend the theme JSON schema with the neo-brutalist tokens: border widths and
  colors, hard shadow offset/color, corner radius, focus-ring style, accent block
  colors. Version the schema and update both bundled themes plus their snapshots.
- `crates/theming/src/generate/gtk_css.rs`: style the native surfaces (header bar,
  sidebar, file tree rows, buttons, entries, scrollbars, selection, focus) with the
  new tokens. Note GTK CSS is a subset of web CSS, so express what GTK supports and
  keep the look reading the same across the two engines.
- `crates/theming/src/generate/preview_css.rs`: bring the rendered pane in line
  (headings, blockquotes, tables, code blocks, inline code, links, task lists,
  horizontal rules) so chrome and preview feel like one design.
- Keep both engines driven by the same theme source so light/dark and theme
  switching restyle everything at once with no visual drift between chrome and
  preview.

Acceptance:
- Chrome, sidebar, tree, editor, and preview share one coherent neo-brutalist look
  in all four theme/mode combinations, with no drift between GTK and preview.
- `Ctrl+T` light/dark and a theme switch restyle every surface live.
- Generator snapshot tests updated and green.
- Commit: "phase 2.5: neo-brutalist visual language across chrome and preview".

---

## Phase 3a: wikilinks (done)

Goal: links between notes that resolve, render, and can be followed.

Shipped:
- `crates/parser/src/wikilink.rs`: one code-aware `scan` reporting the byte range,
  target, heading, and alias of every `[[...]]`. Code-awareness is a hybrid, so
  indented code needs no hand-rolled detection: comrak block source positions mark
  code blocks, frontmatter, and raw HTML as opaque, and the scanner tracks inline
  backtick runs itself. The same spans drive rendering, indexing, and click
  handling, so those three can never disagree about where a link is.
- `render` takes a `LinkResolver`, so `crates/parser` still has no dependency on
  `crates/index` and tests resolve through a closure. Links are rewritten to
  markdown links carrying `jotter-note:` or `jotter-new:`, and unresolved ones are
  styled by scheme in the preview CSS.
- Resolution lives in `crates/app/src/links.rs`: a stem and path map rebuilt from
  the index on every structural change. Bare stems match anywhere in the vault,
  case-insensitively, a `/` in the target names a vault-relative path, and a stem
  collision resolves to the lexicographically first path.
- `links` rows are written by `reindex_note` under their raw targets and resolved
  by a pass that runs after a full index and after each structural change, so a
  note linking to one indexed later is not stuck broken.
- Following: a plain click in the preview, `Ctrl+Click` in the editor. A broken
  target with near matches (edit distance over separator-insensitive text) opens a
  chooser, and picking a match rewrites the `[[...]]` in the source; with no near
  match the note is created beside its source and opened. External links now go to
  the system browser instead of navigating the preview away from the note.

## Phase 3b: search and pickers (done)

Goal: finding notes.

Shipped:
- `crates/search`: a pure subsequence matcher returning a score and the matched
  byte positions. Scoring rewards word starts, adjacent runs, and the filename
  over the folder, with candidate length as the tiebreak. Smart case throughout.
- `crates/app/src/picker.rs`: one overlay widget (entry plus list) that knows
  nothing about what it lists. `Ctrl+O` fills it with notes ranked over path and
  title, `Ctrl+P` opens the same overlay with `>` typed, which switches it to
  commands; the leading `>` flips modes live and each key toggles it shut. The
  empty note list shows the last ten notes opened in this vault, kept in config.
- `crates/app/src/complete.rs`: `[[` completion in a caret popover. It inserts
  the shortest target that reaches the note (bare stem when unique, path when
  not), which is exactly what 3a resolution expects.
- `crates/app/src/search_panel.rs` plus `search.rs`: `Ctrl+Shift+F` swaps the
  sidebar to full-text search. Query building is a pure function; ranking is
  bm25 through the new `Index::search_notes`.

Two things worth remembering:
- `notes_fts` is contentless, so `snippet()` and `highlight()` do not work.
  Snippets come from reading the matched files, which also yields line numbers,
  so a result opens the note at the line that matched.
- `scan_inert` reports finished lookalike spans, not regions, so it cannot answer
  "is this half-typed `[[` inside code". `wikilink::dead_ranges` does that.

Saving landed alongside phase 3a (it was missing entirely: `vault.write_note` had
no caller). `Ctrl+S` writes through the vault and reindexes, or writes the file
directly in single-file mode, gated on a dirty flag. Switching notes and closing
the window both save first, silently. Still absent: any autosave on a timer, and
any conflict handling if the file changed on disk under an unsaved buffer.

---

## Phase 4: backlinks, tags, frontmatter (complete)

Shipped on branch `phase-4-backlinks`:
- `crates/parser/src/frontmatter.rs` over `gray_matter` (YAML, TOML, JSON) and
  `crates/parser/src/tags.rs` for inline `#tag`, which reuses `dead_ranges` so
  code, headings, and URL fragments are not tags. Both feed `Index::set_tags`.
- Backlinks strip under the editor, showing the line each link sits on. The
  results list moved to `results.rs`, shared by search, backlinks, and tags.
- Tag page (`Ctrl+Shift+T`): tags alphabetically with counts, then the notes
  carrying one. Escape and a back arrow step one level at a time.
- Broken-link report, sharing that two-level page through `drill.rs`: missing
  targets with how many notes point at each, then the dead lines themselves.
  Opened from the palette or from a status-bar count that hides when the vault
  is clean, and refreshed in place as links break and heal.
- Off-plan but requested: folder rename and delete with a confirming dialog, a
  window title carrying the open note, and tree selection that survives a
  rebuild (the watcher rebuilds again after any in-app change).

Original tasks:
- Backlinks panel below the preview: `SELECT src_note_id FROM links WHERE
  dst_path = ? AND resolved = 1`, show each linker with a snippet of the linking line.
- Broken-link report: `SELECT dst_path, count(*) FROM links WHERE resolved = 0
  GROUP BY dst_path`, surfaced as a command.
- Frontmatter via `gray_matter` (YAML/TOML/JSON). Recognize `title`, `tags`,
  `aliases`, `created`, `updated`. Store everything else raw in `notes.frontmatter`.
  Title resolution order: frontmatter title -> first H1 -> filename.
- Tag view: flat list of tags with counts (from the `tags` table). Click a tag to
  filter the note list. Recognize inline `#tag` and frontmatter tags.

Acceptance:
- Backlinks panel updates when the active note changes and after reindex.
- Tag view counts match the index; filtering works.
- Frontmatter title overrides filename in the tree and switchers.
- Commit: "phase 4: backlinks, tag view, frontmatter".

---

## Phase 5: git (target: 1 week)

Goal: full sync loop with real credentials.

Tasks:
- `crates/git` over `git2`. Status: current branch, ahead/behind, dirty flag.
- Credentials via `RemoteCallbacks::credentials`:
  - SSH: `Cred::ssh_key_from_agent(username)` first, then `Cred::ssh_key` reading
    `~/.ssh/id_ed25519` then `id_rsa`.
  - HTTPS: `Cred::credential_helper` (libsecret / gnome-keyring).
  - Identity: `git2::Config::open_default()` for user.name and user.email. If
    missing, prompt once and offer to write to global config.
- Status bar: branch, ahead/behind counts, dirty indicator.
- Sidebar git panel: staged / unstaged changes with per-file diff.
- Actions: stage all, commit (message), push, pull (fetch + merge), fetch.
- Auto-commit toggle: every N minutes commit all with a generated message. Off by
  default.
- Conflict handling: on merge conflict, open the affected note with conflict markers
  visible; provide accept-ours / accept-theirs / manual buttons.
- Git status poller: every 30s and on every save.

Acceptance:
- Clone-backed vault: commit, push, pull all succeed with SSH agent and with an
  HTTPS credential helper on Arch.
- Conflict path is reachable and resolvable from inside the app.
- Commit: "phase 5: git status, diff panel, commit, push, pull, conflicts".

Pitfall guard: `git2` SSH needs `libssh2` at build time. If SSH auth misbehaves on
Arch, shell out to the `git` binary via `std::process::Command` for network ops.
Same fallback for partial clone, sparse checkout, and awkward rebases.

---

## Phase 6: app shell and settings

Requested 2026-07-29, after phase 4 landed. Three pieces, in this order:

- **Keybinding sheet on `Ctrl+H`.** A dialog listing every binding the app has,
  read from the same table that registers the accelerators so it cannot drift.
  `Ctrl+H` for help, chosen so insert link keeps `Ctrl+K`.
- **Icon rail left of the file tree.** Always visible, notes icon at the top and
  a cogwheel at the bottom. `Ctrl+B` keeps collapsing only the tree, so the rail
  stays put and the sidebar stack moves inside it. The rail is a third column in
  the paned layout, not a page of the sidebar stack.
- **Settings page behind the cogwheel.** Editor font and rendered-markdown font
  chosen from the fonts installed on the system (enumerated through pango), a
  size for each, and a theme picked from the app theme folder. Writes through
  `Config`, applies live the way `Ctrl+T` already restyles every surface.

- **Drag and drop in the tree.** Move a note into a folder by dragging it, the
  one vault operation with no path today: `Vault::rename_note` already moves
  across folders and creates parent directories, but the tree UI joins the typed
  name onto the note's existing parent, so a rename cannot leave its folder.
  Needs drop targets on folder rows and on the root, a moved-note reindex (the
  rename path already does this), and a link rewrite: bare `[[stem]]` links
  survive a move by design, path-form `[[notes/plan]]` links do not, so the move
  rewrites them. The `links` table already names every note pointing at the moved
  one (`Index::linking_notes`), so the move reads those notes, rewrites the
  path-form targets, and reindexes them. The same machinery serves a rename,
  which today silently breaks every `[[stem]]` link pointing at the old name.

Font choice and theme choice both reach into `crates/theming`, which currently
takes typography from the theme file. Settings must override it per user without
editing the bundled themes.

---

## Phase 7: polish (ongoing, after 0 to 5 daily-drive for two weeks)

- Keybinding customization via `keys.toml` (all bindings remappable).
- Recent-vaults picker refinement, per-vault settings overriding global.
- Reload-themes command in the palette (live theme file watching is v1.5).
- Then daily-drive on real notes for two weeks before choosing v1.5 work.

## Keybindings (defaults, all remappable)

Ctrl+O quick switcher, Ctrl+P command palette, Ctrl+Shift+F full-text search,
Ctrl+N new note (current folder), Ctrl+Shift+N new note (root), Ctrl+S save,
Ctrl+B toggle sidebar, Ctrl+E switch mode, Ctrl+T toggle theme light/dark,
Ctrl+/ toggle line comment,
Ctrl+H keybinding sheet (phase 6), Ctrl+K insert link,
Ctrl+Shift+K insert wikilink, Alt+Left back, Alt+Right forward,
F2 rename, Ctrl+Shift+G sync vault (commit all, pull, push as one action;
the planned Ctrl+G chords went with the separate stage and push actions).

## Startup sequence (implement in phase 2, complete by phase 5)

1. Parse CLI args (`jotter [vault_path]`). No arg -> recent-vaults picker.
2. Open the SQLite index; create and migrate if missing.
3. Compare mtimes to the index; reindex changed files in the background.
4. Open the last-active note.
5. Start the notify watcher.
6. Start the git status poller (30s plus on save).
UI must be interactive before indexing finishes.

## Definition of done for v1

Phases 0 through 5 complete, each acceptance block green, warnings-as-errors clean,
`cargo test --workspace` green, opens cold in under a second on the target machine.
Then two weeks of real daily-driving before v1.5.

## Non-goals for v1 (do not build)

No plugin system (leave seams only), no graph view, no mobile or sync service, no
live-preview WYSIWYG, no split pane, no canvas. See `docs/conventions-and-pitfalls.md`.
