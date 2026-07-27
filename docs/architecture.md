# jotter: architecture reference

Companion to `implementation-plan.md`. This is the map: crate graph, data flow,
data model, and the theming pipeline. Keep it accurate as the code lands.

## Workspace layout

```
crates/
  vault/     filesystem, watcher, note IO
  index/     SQLite schema, FTS5, migrations
  parser/    comrak + wikilinks + syntect + frontmatter
  git/       git2 wrapper, credentials, sync status
  theming/   JSON theme loader, palette resolver, CSS/scheme generators
  editor/    SourceView wrapper, key bindings, style scheme loader
  preview/   WebKit wrapper, CSS injection, edit-preview transition
  app/       glue crate, application state, command dispatcher
apps/
  jotter/    binary crate, wires everything, GTK Application (stays thin)
resources/
  ui/        .ui files (GTK Builder XML)
  themes/    bundled theme JSONC (retro82 default, event-horizon), each with a dark and light palette
  icons/     SVG icons (jotter.svg lives here)
```

## Dependency direction

```
binary -> app -> { vault, index, parser, git, theming, editor, preview }
editor  -> theming        (consumes generated SourceView scheme)
preview -> theming, parser (consumes generated preview CSS + rendered HTML)
parser  -> theming        (syntect code theme derived from the theme code map)
index   -> (standalone)
vault   -> (standalone)
git     -> (standalone)
theming -> (standalone, pure logic, no GTK)
```

Each `crates/*` compiles standalone with its own tests. `app` owns the state graph
and command dispatcher. The binary only constructs the GTK Application and hands off.

## Core data flow

- Open vault -> `vault` enumerates notes -> `index` upserts rows and FTS -> `app`
  renders the tree and opens the last-active note.
- Edit -> buffer changes -> on save, `vault` writes atomically, `index` reindexes
  that one note, `git` poller marks dirty.
- Toggle to preview -> `parser` (frontmatter strip -> wikilink rewrite -> comrak ->
  syntect) produces HTML + a line-to-heading anchor map -> `preview` loads it with
  injected CSS and scrolls to the anchor nearest the caret line.
- Follow a link -> `preview` reports the clicked uri instead of navigating ->
  `jotter-note:` opens that note, `jotter-new:` offers near matches or creates it,
  anything else goes to the system browser. Wikilinks resolve through an in-memory
  stem and path map the app rebuilds from the index on every structural change, so
  rendering never queries the database per link.
- External change -> `notify` debounced event -> `index` incremental reindex ->
  affected UI panels refresh (tree, backlinks, tags).

## Command dispatcher

`app` exposes a single command enum dispatched from keybindings, the command palette,
and menus. Every user action is a command so the palette and `keys.toml` share one
source of truth. Commands are the plugin seam for later (do not build the plugin
system now, just route everything through commands).

## Data model (SQLite, migration 001_init.sql)

```sql
CREATE TABLE notes (
  id           INTEGER PRIMARY KEY,
  path         TEXT NOT NULL UNIQUE,   -- relative to vault root
  title        TEXT NOT NULL,          -- frontmatter title, else first H1, else filename
  mtime        INTEGER NOT NULL,       -- unix seconds
  size         INTEGER NOT NULL,
  frontmatter  TEXT                    -- raw serialized frontmatter, nullable
);

CREATE TABLE tags (
  note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  PRIMARY KEY (note_id, tag)
);

CREATE TABLE links (
  src_note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  target       TEXT NOT NULL,          -- link target exactly as written in the note
  dst_path     TEXT NOT NULL,          -- resolved relative path, or the target if unresolved
  resolved     INTEGER NOT NULL,       -- 0 or 1
  PRIMARY KEY (src_note_id, target)
);

CREATE INDEX idx_links_dst ON links(dst_path);
CREATE INDEX idx_tags_tag ON tags(tag);

CREATE VIRTUAL TABLE notes_fts USING fts5(
  title, body,
  content='',
  contentless_delete=1,
  tokenize='unicode61 remove_diacritics 2'
);
```

Key queries:
- Backlinks: `SELECT DISTINCT src_note_id FROM links WHERE dst_path = ? AND resolved = 1`.
- Broken links: `SELECT dst_path, count(*) FROM links WHERE resolved = 0 GROUP BY dst_path`.

`target` is never rewritten, so re-resolution always asks the same question a fresh
scan of the note would: a stem stays a stem even after it once matched a path.
Storing only the resolved path narrows it, and a stem whose winning note is deleted
then goes broken instead of falling through to the note still on disk. Indexing
writes targets unresolved; the resolve pass fills in `dst_path` and `resolved`. It
runs once after a full index (not per note, which would be quadratic) and after
every incremental reindex, since an edit can add or remove links.

Two targets in one note can resolve to the same note (`[[standup]]` and
`[[work/standup]]`), hence the `DISTINCT` in the backlinks query.

Migrations are numbered `NNN_description.sql` and applied in order on connection open.
A `schema_version` pragma or a meta table tracks the applied migration number.

## Theming pipeline

The `theming` crate is pure logic, no GTK, fully unit-testable. One JSON in, three
strings out, all pure functions of the parsed theme so a runtime theme swap is:
parse -> regenerate three strings -> replace on their providers. No restart.

Theme struct sections (serde): `name`, `id`, `type`, optional metadata, then
`palette`, `chrome`, `editor` (with `editor.syntax`), `preview`, `code`, `typography`.

Rules:
- Every color is `#RRGGBB` or a `$palette_key` reference. One hop, no chains.
- `$foo` -> `palette.foo`. Undefined ref -> load error naming the exact field.
- Unknown fields ignored (forward-compatible themes).
- Missing optional fields fall back to defaults derived from `palette.text` /
  `palette.background`. Missing required palette fields fail the load with a specific
  message shown in the theme picker.

Generators:
- `to_gtk_css()` -> app chrome CSS for `gtk::CssProvider::load_from_data`.
- `to_sourceview_scheme_xml()` -> GtkSourceView 5 style scheme, registered with
  `StyleSchemeManager`.
- `to_preview_css()` -> stylesheet injected via `webkit6::UserStyleSheet`.

Theme sources: bundled `resources/themes/*.json` via `include_str!`; user
`~/.config/jotter/themes/*.json`. A user theme with the same `id` overrides the
bundle. Ship retro82 (default, dark mode) and event-horizon. Each file carries a
dark and a light palette plus shared structural sections whose colors are
`$token` references, so one file resolves to a full light or dark theme for the
selected mode. A valid theme file (parses and every `$ref` resolves) appears in
the settings dropdown. Format is JSONC (comments allowed); strip comments before
serde. The chrome is neo-brutalist: thick borders, hard offset shadows, rounded
corners.

## Config resolution

Global `~/.config/jotter/config.toml` (serde + toml). Per-vault
`<vault>/.jotter/config.toml` overrides global. Keybindings in
`~/.config/jotter/keys.toml`. Logs under `~/.local/state/jotter/log/` via tracing.
