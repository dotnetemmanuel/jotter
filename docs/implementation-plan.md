# jotter: implementation plan

A native GTK4 markdown vault app. A vault is a git-backed folder of plain `.md`
files. jotter opens it, indexes it, edits it, previews it, and syncs it. No cloud,
no proprietary format, git as the source of truth.

This document is the execution guide. Work top to bottom. Do not start a phase
until the previous phase meets its acceptance criteria. Ship phases 0 through 5
before touching any v1.5 feature.

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
4. Set the app id to `se.mindfulstack.jotter` (reverse-DNS, stable for GTK).

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
  - `to_preview_css() -> String` (injected via `webkit6::UserStyleSheet`).
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
- `crates/preview`: wrap `webkit6::WebView`. JavaScript disabled by default. Inject
  the preview CSS as a `UserStyleSheet`. Expose `render(html: &str)`.
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

## Phase 3: wikilinks and search (target: 1 week)

Goal: linking and finding notes.

Tasks:
- Wikilink preprocess in `crates/parser`: scan for `[[target]]`, `[[target|alias]]`,
  `[[target#heading]]`. Must be code-aware: skip inside fenced blocks and inline
  code. Resolve target against the index (case-insensitive, first match wins).
  Unresolved links get `class="broken-link"`. Rewrite to standard markdown links
  before comrak.
- Populate `links` table on index: resolved relative path or raw target, plus the
  `resolved` flag.
- FTS5: populate `notes_fts` (title, body) with `unicode61 remove_diacritics 2`.
  Background build on first open of a large vault, with progress. Never block UI.
- `Ctrl+Click` follows a wikilink. `[[` opens an autocomplete popover fed from the
  index (titles and paths).
- Quick switcher (`Ctrl+O`): fuzzy over note title and path.
- Command palette (`Ctrl+P`): fuzzy over commands and notes together.
- Full-text search (`Ctrl+Shift+F`): FTS5-backed, side panel, snippet highlights.

Acceptance:
- Typing `[[` autocompletes from the index; following a link opens the target.
- Broken links render with the broken-link class in preview.
- Quick switcher, command palette, and FTS search all return correct results on a
  multi-hundred-note fixture vault.
- Commit: "phase 3: wikilinks, autocomplete, quick switcher, command palette, FTS".

Pitfall guard: never rewrite wikilinks inside code fences or inline code. Preprocess
with code-context awareness, not a blind document-wide regex.

---

## Phase 4: backlinks, tags, frontmatter (target: 3 to 4 days)

Tasks:
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

## Phase 6: polish (ongoing, after 0 to 5 daily-drive for two weeks)

- Keybinding customization via `keys.toml` (all bindings remappable).
- Recent-vaults picker refinement, per-vault settings overriding global.
- Reload-themes command in the palette (live theme file watching is v1.5).
- Then daily-drive on real notes for two weeks before choosing v1.5 work.

## Keybindings (defaults, all remappable)

Ctrl+O quick switcher, Ctrl+P command palette, Ctrl+Shift+F full-text search,
Ctrl+N new note (current folder), Ctrl+Shift+N new note (root), Ctrl+S save,
Ctrl+B toggle sidebar, Ctrl+E switch mode, Ctrl+/ toggle line comment,
Ctrl+K insert link, Ctrl+Shift+K insert wikilink, Alt+Left back, Alt+Right forward,
F2 rename, Ctrl+G S stage all, Ctrl+G C commit, Ctrl+G P push, Ctrl+G F pull.

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
