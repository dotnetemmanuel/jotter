# TUI Appearance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a second visual style to jotter, TUI, switchable live from a row in the settings window, drawn in the same theme colors as the existing style now named classic.

**Architecture:** `jotter_theming` gains a `Style` enum and a `style` field on the resolved `Theme`; `to_gtk_css` dispatches to either today's stylesheet or a new TUI one. `crates/app` stores the choice in `config.appearance.style`, stamps it onto every resolved theme in `appearance::resolve`, and pushes it into the widgets that carry character-level idioms from `apply_theme`, which every repaint path already funnels through.

**Tech Stack:** Rust 2024, GTK4 (gtk4-rs 0.11, feature `v4_12`), insta for generator snapshots, cage plus grim and wtype for offscreen GUI checks.

## Global Constraints

- The spec is `docs/superpowers/specs/2026-07-31-tui-appearance-design.md`. It wins over this plan wherever they disagree.
- No em dash in any output: prose, comments, commit messages, docs.
- No apostrophe anywhere in a commit message. Use a heredoc for the commit.
- No `Co-Authored-By` or any generated-by trailer on commits.
- Comments only where the code is genuinely not self-explanatory, and never more than one short line.
- Commit messages default to a subject line alone.
- **Never commit before the change has been exercised in the running app.** Tasks marked **GUI GATE** end by presenting a screenshot to the user and waiting for their word before committing. Do not commit those tasks on green unit tests alone.
- Classic output must not change. `crates/theming/tests/snapshots/generators__*_gtk_css.snap` are the guard: if one of them changes, the change is a bug.
- No theme file under `resources/themes/` is edited by any task in this plan.
- Lint clean: the workspace builds with `#![warn(clippy::pedantic)]` in `crates/theming`. Run `cargo clippy --workspace --all-targets` before each commit.
- The app is run offscreen only, never on the user's desktop. See "Running the app" below.

## Running the app

```sh
cargo build --release
tools/gui-test/cage-run.sh ./target/release/jotter /path/to/test-vault
```

The script prints the socket cage took (usually `wayland-0`). Then:

```sh
export WAYLAND_DISPLAY=wayland-0
grim /tmp/claude-1000/shot.png                  # screenshot
wtype -M ctrl -k b -m ctrl                      # keys
tools/gui-test/wlpoint/target/release/wlpoint "m:120:200,w:200,d,w:100,u"   # pointer
```

Build the pointer once: `cd tools/gui-test/wlpoint && cargo build --release`.
Coordinates are pixels against a 1280x720 output unless `WLPOINT_EXTENT` says otherwise.
The config the app reads is `~/.config/jotter/config.toml`; the style can be forced there for a screenshot without touching the settings window:

```toml
[appearance]
style = "tui"
```

## File structure

**Created:**
- `crates/theming/src/generate/tui_css.rs` - the whole TUI stylesheet, chrome half and parts half, mirroring the split in `gtk_css.rs`.
- `crates/app/src/style.rs` - the pure text helpers the TUI idioms need (bracketing, headings, cursors, markers). No GTK types, fully unit-tested.
- `crates/theming/tests/style.rs` - assertions about the TUI stylesheet that a snapshot cannot express.

**Modified:**
- `crates/theming/src/model.rs` - `Style` enum, `Theme.style`, `Theme::with_style`.
- `crates/theming/src/resolve.rs` - stamp `Style::Classic` on the resolved theme.
- `crates/theming/src/lib.rs` - export `Style`, mention the second stylesheet in the crate doc.
- `crates/theming/src/generate/mod.rs` - declare `tui_css`.
- `crates/theming/src/generate/gtk_css.rs` - dispatch on style, rename the existing bodies to `classic_css` and `classic_parts_css`.
- `crates/theming/tests/generators.rs` - snapshots for the TUI sheet.
- `crates/app/src/config.rs` - `Appearance.style`.
- `crates/app/src/appearance.rs` - `style_of`, `style_name`, and the `ui_font` pin in `apply`.
- `crates/app/src/settings.rs` - the Style row, `Change::Style`, `Handle::show_style`, bracketed button faces.
- `crates/app/src/lib.rs` - handle `Change::Style`, restyle hook in `apply_theme`, tree gutter, status bar segments, vault name heading.
- `crates/app/src/results.rs` - row cursor and `set_style`.
- `crates/app/src/search_panel.rs`, `drill.rs`, `git_panel.rs`, `backlinks.rs` - `set_style`, heading text, `.panel-bar` class.
- `crates/app/src/picker.rs` - title line, prompt label, row cursor.
- `crates/app/src/complete.rs` - style passed at construction (no content change beyond the class).
- `crates/app/src/conflict_view.rs` - `set_style`, bracketed actions, uppercased pane titles.
- `crates/app/src/keysheet.rs` - uppercased section headings.
- `docs/architecture.md` - one paragraph on the style axis.

---

### Task 1: Style in the theming crate, and the TUI chrome stylesheet

**Files:**
- Modify: `crates/theming/src/model.rs`
- Modify: `crates/theming/src/resolve.rs:39-48`
- Modify: `crates/theming/src/lib.rs:22-25`
- Modify: `crates/theming/src/generate/mod.rs`
- Modify: `crates/theming/src/generate/gtk_css.rs`
- Create: `crates/theming/src/generate/tui_css.rs`
- Create: `crates/theming/tests/style.rs`

**Interfaces:**
- Produces: `jotter_theming::Style` (`Style::Classic`, `Style::Tui`, `Style::as_str() -> &'static str`, `Default = Classic`); `Theme.style: Style`; `Theme::with_style(self, Style) -> Theme`. `Theme::to_gtk_css` keeps its signature and dispatches on `self.style`.
- Consumes: nothing.

- [ ] **Step 1: Write the failing test**

Create `crates/theming/tests/style.rs`:

```rust
//! What the TUI stylesheet must say, beyond what a snapshot pins down.

use jotter_theming::{Mode, Style, Theme, ThemeFile};

fn tui(id: &str, mode: Mode) -> Theme {
    let src = jotter_theming::bundled::BUNDLED
        .iter()
        .find(|b| b.id == id)
        .unwrap_or_else(|| panic!("theme {id} is bundled"))
        .source;
    ThemeFile::from_jsonc(src)
        .expect("theme parses")
        .resolve(mode)
        .expect("theme resolves")
        .with_style(Style::Tui)
}

#[test]
fn a_resolved_theme_is_classic_until_asked_otherwise() {
    let theme = tui("retro82", Mode::Dark);
    assert_eq!(theme.style, Style::Tui);
    let plain = ThemeFile::from_jsonc(
        jotter_theming::bundled::BUNDLED
            .iter()
            .find(|b| b.id == "retro82")
            .unwrap()
            .source,
    )
    .unwrap()
    .resolve(Mode::Dark)
    .unwrap();
    assert_eq!(plain.style, Style::Classic);
}

#[test]
fn the_tui_sheet_has_no_rounded_corners() {
    let css = tui("retro82", Mode::Dark).to_gtk_css();
    for line in css.lines() {
        let Some(radius) = line.trim().strip_prefix("border-radius:") else {
            continue;
        };
        assert_eq!(
            radius.trim().trim_end_matches(';'),
            "0",
            "a TUI corner is not square: {line}"
        );
    }
}

#[test]
fn the_tui_sheet_draws_its_structure_in_the_focus_color() {
    let theme = tui("event-horizon", Mode::Dark);
    let focus = theme.chrome.focus.clone();
    let css = theme.to_gtk_css();
    assert!(
        css.contains(&format!("border-bottom: 1px solid {focus}")),
        "the headerbar rule should be a focus hairline"
    );
    assert!(
        css.contains(".sidebar {"),
        "the sidebar block should still be styled"
    );
}

#[test]
fn the_tui_sheet_sets_the_ui_font_the_theme_was_given() {
    let mut theme = tui("retro82", Mode::Dark);
    theme.typography.ui_font = "\"CaskaydiaMono Nerd Font\", monospace".to_string();
    let css = theme.to_gtk_css();
    assert!(css.contains("font-family: \"CaskaydiaMono Nerd Font\", monospace;"));
}

#[test]
fn the_classic_sheet_is_untouched_by_the_new_arm() {
    let theme = tui("retro82", Mode::Dark).with_style(Style::Classic);
    let css = theme.to_gtk_css();
    assert!(css.contains("border-radius: 3px"), "classic keeps its corners");
}
```

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -p jotter-theming --test style`
Expected: FAIL, `Style` is not found in `jotter_theming`.

- [ ] **Step 3: Add the Style enum and the theme field**

In `crates/theming/src/model.rs`, after the `Mode` block:

```rust
/// Which visual language the chrome is drawn in.
///
/// A user choice rather than a theme property, so it never appears in a theme
/// file: the app stamps it onto the theme it resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Small corners, thick structural lines, a sans UI font.
    #[default]
    Classic,
    /// A terminal look: square corners, hairline frames, a monospace UI font.
    Tui,
}

impl Style {
    /// The lowercase name used in the config file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Style::Classic => "classic",
            Style::Tui => "tui",
        }
    }
}
```

Add the field to `Theme` (after `mode`):

```rust
    /// The visual language the chrome is drawn in.
    pub style: Style,
```

And the builder, in the existing `impl Theme` block:

```rust
    /// The same theme drawn in another style.
    #[must_use]
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
```

In `crates/theming/src/resolve.rs`, add `style: Style::Classic,` to the `Theme { .. }` literal after `mode,` and add `Style` to the `use crate::model::{...}` list.

In `crates/theming/src/lib.rs`, add `Style` to the `pub use model::{...}` list (alphabetical: after `Preview`).

- [ ] **Step 4: Split the classic sheet out and dispatch**

In `crates/theming/src/generate/gtk_css.rs`, change the entry point and rename the two bodies. The bodies themselves are not edited, only their names and visibility:

```rust
impl Theme {
    /// Render the application chrome as GTK4 CSS, in this theme's style.
    #[must_use]
    pub fn to_gtk_css(&self) -> String {
        match self.style {
            Style::Classic => self.classic_css(),
            Style::Tui => self.tui_css(),
        }
    }

    /// The bauhaus-leaning default.
    fn classic_css(&self) -> String {
        // ... today's to_gtk_css body, unchanged, ending in `+ &self.classic_parts_css()`
    }

    /// The half of the stylesheet that dresses jotter's own widgets, split
    /// from the general chrome so neither is a wall of text.
    fn classic_parts_css(&self) -> String {
        // ... today's parts_css body, unchanged
    }
}
```

Add `use crate::model::{Style, Theme};` at the top of the file.
In `crates/theming/src/generate/mod.rs`, add `mod tui_css;`.

- [ ] **Step 5: Write the TUI chrome stylesheet**

Create `crates/theming/src/generate/tui_css.rs`. Build the string with `format!` exactly the way `gtk_css.rs` does (one `\n\`-continued literal, named arguments at the end). The CSS it must produce is below, with `{placeholders}` naming the values to substitute: `bg` = `chrome.background`, `overlay` = `chrome.overlay`, `text` = `chrome.text`, `muted` = `chrome.muted`, `accent` = `chrome.accent`, `focus` = `chrome.focus`, `danger` = `chrome.danger`, `ui_font` = `typography.ui_font`, `size` = `typography.font_size`, `small` = `typography.font_size.saturating_sub(1)`.

`chrome.radius`, `chrome.border_width`, `chrome.surface` and `chrome.border` are deliberately unused in this sheet: TUI is square, hairline, and flat.

```css
/* {name} tui */
window {
  background-color: {bg};
  color: {text};
  font-family: {ui_font};
  font-size: {size}px;
}

headerbar {
  background-color: {bg};
  background-image: none;
  color: {text};
  border-bottom: 1px solid {focus};
  box-shadow: none;
  padding-left: 0;
}

separator {
  background-color: alpha({focus}, 0.45);
  min-height: 1px;
  min-width: 1px;
}

paned > separator {
  background-color: alpha({focus}, 0.45);
  background-image: none;
  min-width: 1px;
  min-height: 1px;
}

.sidebar {
  background-color: {bg};
}

.sidebar listview {
  background-color: transparent;
  color: {text};
  padding-top: 2px;
}

.sidebar listview > row {
  border-radius: 0;
  padding: 1px 6px;
  margin: 0;
}

.sidebar listview > row:hover {
  background-color: {overlay};
}

.sidebar listview > row:selected {
  background-color: {accent};
  color: {bg};
}

.sidebar listview > row:selected:hover {
  background-color: {accent};
  color: {bg};
}

.vault-name {
  color: {focus};
  font-size: {small}px;
  font-weight: bold;
  padding: 6px 8px 2px 8px;
  border-bottom: 1px solid alpha({focus}, 0.45);
}

.tree-cursor {
  color: {accent};
}

.sidebar listview > row:selected .tree-cursor {
  color: {bg};
}

.tree-inert {
  color: {muted};
  opacity: 0.55;
}

.tree-title {
  color: {muted};
  font-size: {small}px;
}

.sidebar listview > row:selected .tree-title {
  color: alpha({bg}, 0.7);
}

.tree-drop {
  box-shadow: inset 0 0 0 1px {accent};
  border-radius: 0;
}

.sidebar listview:drop(active), .sidebar listview > row:drop(active) {
  box-shadow: none;
  outline: none;
}

button {
  background-color: transparent;
  background-image: none;
  color: {focus};
  border: none;
  border-radius: 0;
  box-shadow: none;
  padding: 3px 8px;
}

button:hover {
  background-color: {focus};
  color: {bg};
}

button:checked {
  background-color: {accent};
  background-image: none;
  color: {bg};
}

button.suggested-action {
  background-color: transparent;
  background-image: none;
  color: {accent};
}

button.suggested-action:hover {
  background-color: {accent};
  color: {bg};
}

button.destructive-action {
  background-color: transparent;
  background-image: none;
  color: {danger};
}

button.destructive-action:hover {
  background-color: {danger};
  color: {bg};
}

windowcontrols button, windowcontrols button.titlebutton {
  background-color: transparent;
  background-image: none;
  border: none;
  box-shadow: none;
  color: {text};
}

windowcontrols button:hover, windowcontrols button.titlebutton:hover {
  background-color: {focus};
  background-image: none;
  color: {bg};
  border-radius: 0;
}

windowcontrols button image, windowcontrols button:hover image {
  background: none;
  background-color: transparent;
  background-image: none;
  box-shadow: none;
  color: {text};
}

entry {
  background-color: {bg};
  background-image: none;
  color: {text};
  border: 1px solid alpha({focus}, 0.55);
  border-radius: 0;
  box-shadow: none;
  outline: none;
  padding: 3px 6px;
}

entry:focus-within, entry:hover, entry:focus-visible {
  outline: none;
  box-shadow: none;
}

entry:focus-within {
  border-color: {focus};
}

entry > text, entry > text:focus-visible {
  outline: none;
  box-shadow: none;
}

row:focus, row:focus-visible, listview:focus-visible, listbox:focus-visible {
  outline: none;
}

scrollbar {
  background-color: transparent;
  border: none;
}

scrollbar slider {
  background-color: {muted};
  border: none;
  border-radius: 0;
  min-width: 6px;
  min-height: 6px;
  margin: 0;
}

scrollbar slider:hover {
  background-color: {focus};
}

popover > contents {
  background-color: {bg};
  color: {text};
  border: 1px solid {focus};
  border-radius: 0;
  box-shadow: none;
}

.picker-scrim {
  background-color: alpha({bg}, 0.6);
}
```

End the function with `+ &self.tui_parts_css()`, and add a stub for now so it compiles:

```rust
    fn tui_parts_css(&self) -> String {
        String::new()
    }
```

Task 2 fills it in.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p jotter-theming`
Expected: `--test style` passes; `--test generators` passes with the existing snapshots byte for byte unchanged (`git status` must show no change under `crates/theming/tests/snapshots/`).

- [ ] **Step 7: Add the TUI snapshots**

Append to `crates/theming/tests/generators.rs`:

```rust
#[test]
fn retro82_dark_tui_gtk_css() {
    insta::assert_snapshot!(resolve("retro82", Mode::Dark).with_style(Style::Tui).to_gtk_css());
}

#[test]
fn event_horizon_dark_tui_gtk_css() {
    insta::assert_snapshot!(
        resolve("event-horizon", Mode::Dark)
            .with_style(Style::Tui)
            .to_gtk_css()
    );
}
```

Add `Style` to that file's `use jotter_theming::{...}` line.

Run: `INSTA_UPDATE=always cargo test -p jotter-theming --test generators`
Then read the two new `.snap` files and check they are the stylesheet above with the retro82 and event-horizon colors substituted.

- [ ] **Step 8: Lint and commit**

Run: `cargo clippy --workspace --all-targets`
Expected: no warnings from `crates/theming`.

```bash
git add crates/theming
git commit -F - <<'EOF'
theming: a style axis, and the TUI chrome sheet
EOF
```

---

### Task 2: The TUI parts stylesheet

**Files:**
- Modify: `crates/theming/src/generate/tui_css.rs`
- Modify: `crates/theming/tests/style.rs`

**Interfaces:**
- Consumes: `Theme::tui_parts_css` stub from Task 1.
- Produces: the finished TUI stylesheet. New CSS classes the app will attach in later tasks: `.tree-cursor`, `.row-cursor`, `.panel-bar`, `.picker-title`, `.picker-prompt`.

- [ ] **Step 1: Write the failing test**

Append to `crates/theming/tests/style.rs`:

```rust
#[test]
fn the_tui_sheet_dresses_every_widget_class_the_app_uses() {
    let css = tui("retro82", Mode::Dark).to_gtk_css();
    for class in [
        ".rail", ".rail-button", ".font-list", ".settings", ".settings-label",
        ".settings-close", ".keysheet-heading", ".keysheet-keys", ".theme-button",
        ".theme-name", ".conflict", ".conflict-header", ".conflict-title",
        ".conflict-body", ".conflict-actions", ".status-size", ".status-git",
        ".status-broken", ".backlinks", ".backlinks-header", ".search-results",
        ".panel-back", ".tags-heading", ".tag-row", ".search-heading",
        ".search-name", ".search-folder", ".search-count", ".search-snippet",
        ".completion", ".picker", ".picker-detail", ".panel-bar", ".picker-title",
        ".picker-prompt", ".row-cursor",
    ] {
        assert!(css.contains(class), "the TUI sheet says nothing about {class}");
    }
}

#[test]
fn the_tui_row_cursor_inverts_on_the_selected_row() {
    let theme = tui("retro82", Mode::Dark);
    let css = theme.to_gtk_css();
    assert!(css.contains(".search-results > row:selected .row-cursor"));
}
```

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -p jotter-theming --test style`
Expected: FAIL, "the TUI sheet says nothing about .rail".

- [ ] **Step 3: Write the parts sheet**

Replace the `tui_parts_css` stub with a `format!` over this CSS, same placeholders as Task 1 plus `editor_font` = `typography.editor_font`, `picker_size` = `typography.font_size + 4`, `back_size` = `typography.font_size + 1`, `rail_size` = `typography.font_size + 6`.

```css
.rail {
  background-color: {bg};
  padding: 6px 0;
}

.rail-button {
  background: none;
  border: none;
  box-shadow: none;
  color: {muted};
  padding: 8px 4px;
  min-height: 0;
  min-width: 0;
  border-radius: 0;
}

.rail-button label {
  font-family: {editor_font};
  font-size: {rail_size}px;
  border-bottom: none;
}

.rail-button:hover {
  background-color: {overlay};
  color: {text};
}

.rail-button:checked {
  background: none;
  color: {accent};
  box-shadow: inset 2px 0 0 0 {accent};
}

.rail-settings {
  padding-left: 3px;
  padding-right: 6px;
}

.font-tick {
  color: {accent};
}

.font-list {
  background-color: transparent;
  color: {text};
}

.font-list > row {
  padding: 1px 6px;
  border-radius: 0;
}

.font-list > row:hover {
  background-color: {overlay};
}

.font-list > row:selected {
  background-color: {accent};
  color: {bg};
  box-shadow: none;
}

.settings {
  background-color: {bg};
}

.settings-label {
  color: {focus};
}

.settings-close {
  background: none;
  border: none;
  box-shadow: none;
  color: {text};
  padding: 3px 8px;
  min-height: 0;
  min-width: 0;
}

.settings-close:hover {
  background-color: {focus};
  color: {bg};
}

.keysheet-heading {
  color: {focus};
  font-size: {small}px;
  font-weight: bold;
}

.keysheet-keys {
  color: {muted};
  font-family: {editor_font};
  font-size: {small}px;
}

.theme-button {
  padding: 4px;
  border: 1px solid transparent;
}

.theme-button:checked {
  background: none;
  color: {text};
  border-color: {accent};
}

.theme-name {
  font-size: {small}px;
}

.conflict {
  background-color: {bg};
  padding: 8px 10px;
}

.conflict-header {
  padding-bottom: 4px;
  border-bottom: 1px solid alpha({focus}, 0.45);
}

.conflict-heading {
  font-weight: bold;
}

.conflict-progress {
  color: {muted};
  font-size: {small}px;
}

.conflict-title {
  font-family: {editor_font};
  font-size: {small}px;
  font-weight: bold;
  padding: 0 2px 2px 2px;
  border-radius: 0;
  border-bottom: 1px solid alpha({focus}, 0.45);
}

.conflict-title.conflict-incoming {
  color: {focus};
}

.conflict-title.conflict-yours {
  color: {accent};
}

.conflict-title.conflict-resolution {
  color: {text};
}

.conflict-body {
  font-family: {editor_font};
  border-radius: 0;
  padding: 4px 6px;
  border: 1px solid alpha({focus}, 0.45);
}

.conflict-body, .conflict-body text {
  background-color: {bg};
  color: {text};
}

.conflict-body.conflict-incoming {
  border-color: {focus};
}

.conflict-body.conflict-yours {
  border-color: {accent};
}

.conflict-actions {
  padding-top: 4px;
}

.conflict-action {
  padding: 3px 8px;
}

.status-size, .status-git, .status-broken {
  background: none;
  border: none;
  box-shadow: none;
  font-family: {editor_font};
  font-size: {small}px;
  padding: 0 6px;
  min-height: 0;
}

.status-size, .status-git {
  color: {muted};
}

.status-broken {
  color: {danger};
}

.status-size:hover, .status-git:hover, .status-broken:hover {
  background: none;
  color: {accent};
}

.backlinks {
  background-color: {bg};
}

.backlinks-header {
  background: none;
  border: none;
  box-shadow: none;
  color: {focus};
  font-family: {editor_font};
  font-size: {small}px;
  padding: 2px 8px;
  min-height: 0;
}

.backlinks-header:hover {
  background: none;
  color: {accent};
}

.panel-bar {
  border-bottom: 1px solid alpha({focus}, 0.45);
}

.search-results {
  background-color: transparent;
  color: {text};
}

.search-results > row {
  border-radius: 0;
  padding: 0 6px;
  margin: 0;
}

.search-results > row:hover {
  background-color: {overlay};
}

.search-results > row:selected {
  background-color: {accent};
  color: {bg};
  box-shadow: none;
}

.row-cursor {
  color: {accent};
  font-family: {editor_font};
}

.search-results > row:selected .row-cursor {
  color: {bg};
}

.panel-back {
  background: none;
  border: none;
  box-shadow: none;
  color: {focus};
  padding: 0 4px;
  margin: 0;
  min-height: 0;
  min-width: 0;
}

.panel-back label {
  font-family: {editor_font};
  font-size: {back_size}px;
}

.panel-back:hover {
  background: none;
  color: {accent};
}

.tags-heading {
  color: {focus};
  font-size: {small}px;
  font-weight: bold;
}

.tag-row {
  padding: 1px 4px;
}

.search-heading {
  margin-top: 10px;
  padding: 0 2px 2px 2px;
  border-bottom: 1px solid alpha({focus}, 0.3);
}

.search-results > row:first-child .search-heading {
  margin-top: 0;
}

.search-name {
  font-weight: bold;
}

.search-folder {
  color: {muted};
  font-size: {small}px;
}

.search-count {
  color: {muted};
  font-size: {small}px;
}

.search-results > row:selected .search-folder, .search-results > row:selected .search-count {
  color: alpha({bg}, 0.7);
}

.search-snippet {
  color: {muted};
  margin-left: 0;
  padding: 0 0 0 8px;
  border-left: 1px solid alpha({focus}, 0.3);
}

.completion listbox {
  background-color: transparent;
  color: {text};
}

.completion listbox > row {
  border-radius: 0;
  padding: 1px 6px;
}

.completion listbox > row:selected {
  background-color: {accent};
  color: {bg};
}

.picker {
  background-color: {bg};
  color: {text};
  border: 1px solid {focus};
  border-radius: 0;
  padding: 0;
}

.picker-title {
  color: {focus};
  font-family: {editor_font};
  font-size: {small}px;
  padding: 2px 6px 0 6px;
}

.picker-prompt {
  color: {accent};
  font-family: {editor_font};
  font-size: {picker_size}px;
  padding: 0 0 0 6px;
}

.picker entry {
  font-size: {picker_size}px;
  border: none;
  border-bottom: 1px solid alpha({focus}, 0.45);
  padding: 4px 6px;
}

.picker listview {
  background-color: transparent;
  color: {text};
  margin-top: 0;
}

.picker listview > row {
  border-radius: 0;
  padding: 1px 6px;
}

.picker listview > row:hover {
  background-color: {overlay};
}

.picker listview > row:selected {
  background-color: {accent};
  color: {bg};
}

.picker-detail {
  color: {muted};
}

.picker listview > row:selected .picker-detail {
  color: alpha({bg}, 0.65);
}

.picker listview > row:selected .row-cursor {
  color: {bg};
}

tooltip {
  background-color: {bg};
  color: {text};
  border: 1px solid {focus};
  border-radius: 0;
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jotter-theming`
Expected: PASS. The two TUI snapshots from Task 1 will now differ; refresh them with `INSTA_UPDATE=always cargo test -p jotter-theming --test generators` and read the diff (`git diff crates/theming/tests/snapshots/`) to confirm only the two TUI snapshots changed.

- [ ] **Step 5: Lint and commit**

Run: `cargo clippy --workspace --all-targets`

```bash
git add crates/theming
git commit -F - <<'EOF'
theming: the TUI parts sheet
EOF
```

---

### Task 3: Config and appearance plumbing

**Files:**
- Modify: `crates/app/src/config.rs:45-62`
- Modify: `crates/app/src/appearance.rs`

**Interfaces:**
- Consumes: `jotter_theming::Style`, `Theme::with_style` from Task 1.
- Produces: `config::Appearance.style: Option<String>`; `appearance::style_of(&Appearance) -> Style`; `appearance::style_name(Style) -> &'static str`. `appearance::resolve` returns a theme already carrying the style, with `ui_font` pinned when it is TUI.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block at the bottom of `crates/app/src/appearance.rs`:

```rust
    #[test]
    fn the_style_round_trips_through_the_config() {
        let tui = Appearance {
            style: Some(super::style_name(Style::Tui).to_string()),
            ..Appearance::default()
        };
        assert_eq!(super::style_of(&tui), Style::Tui);
        assert_eq!(super::style_of(&Appearance::default()), Style::Classic);
    }

    #[test]
    fn an_unknown_style_falls_back_rather_than_failing() {
        let nonsense = Appearance {
            style: Some("ansi-art".to_string()),
            ..Appearance::default()
        };
        assert_eq!(super::style_of(&nonsense), Style::Classic);
    }

    #[test]
    fn the_tui_ui_font_is_the_theme_editor_font_not_the_chosen_one() {
        let mut applied = theme();
        let theme_editor_font = applied.typography.editor_font.clone();
        apply(
            &mut applied,
            &Appearance {
                style: Some("tui".to_string()),
                editor_font: Some("Iosevka".to_string()),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.style, Style::Tui);
        assert_eq!(applied.typography.ui_font, theme_editor_font);
        assert_eq!(applied.typography.editor_font, "Iosevka");
    }

    #[test]
    fn classic_leaves_the_ui_font_alone() {
        let untouched = theme();
        let mut applied = theme();
        apply(
            &mut applied,
            &Appearance {
                editor_font: Some("Iosevka".to_string()),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.style, Style::Classic);
        assert_eq!(applied.typography.ui_font, untouched.typography.ui_font);
    }
```

Add `Style` to that test module's `use jotter_theming::Mode;` line, making it `use jotter_theming::{Mode, Style};`.

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -p jotter-app appearance`
Expected: FAIL, no field `style` on `Appearance`.

- [ ] **Step 3: Add the config field**

In `crates/app/src/config.rs`, inside `struct Appearance`, after `mode`:

```rust
    /// Which visual language the chrome is drawn in: classic or tui.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
```

- [ ] **Step 4: Add the style helpers and the font pin**

In `crates/app/src/appearance.rs`, change the import to `use jotter_theming::{Mode, Style, Theme, ThemeFile};` and rewrite `apply`:

```rust
/// Applies the user's choices to `theme`, leaving untouched fields alone.
pub fn apply(theme: &mut Theme, appearance: &Appearance) {
    theme.style = style_of(appearance);
    if theme.style == Style::Tui {
        // The theme's own editor font, not the user's: the rail draws Nerd Font
        // glyphs, and a chosen font need not carry them.
        theme.typography.ui_font = theme.typography.editor_font.clone();
    }
    if let Some(font) = font_of(appearance.editor_font.as_deref()) {
        theme.typography.editor_font = font;
    }
    if let Some(font) = font_of(appearance.preview_font.as_deref()) {
        theme.typography.preview_font = font;
    }
    if let Some(size) = size_of(appearance.font_size) {
        theme.typography.font_size = size;
    }
}

/// The style the config asks for, defaulting to classic.
#[must_use]
pub fn style_of(appearance: &Appearance) -> Style {
    match appearance.style.as_deref() {
        Some("tui") => Style::Tui,
        _ => Style::Classic,
    }
}

/// How a style is written in the config.
#[must_use]
pub fn style_name(style: Style) -> &'static str {
    style.as_str()
}
```

`resolve` needs no change: it calls `apply`, which now stamps the style.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p jotter-app`
Expected: PASS.

- [ ] **Step 6: Lint and commit**

Run: `cargo clippy --workspace --all-targets`

```bash
git add crates/app/src/config.rs crates/app/src/appearance.rs
git commit -F - <<'EOF'
app: carry the chosen style onto the resolved theme
EOF
```

---

### Task 4: The settings row, and the dress switching live (GUI GATE)

**Files:**
- Modify: `crates/app/src/settings.rs`
- Modify: `crates/app/src/lib.rs:2470-2495` (the `Change` match), `crates/app/src/lib.rs` (where settings is opened, `settings::Current` is built)

**Interfaces:**
- Consumes: `appearance::style_of`, `appearance::style_name`, `config::Appearance.style`.
- Produces: `settings::Change::Style(Style)`; `settings::Current.style: Style`; `settings::Handle::show_style(&self, Style)`. After this task the whole CSS half of the feature works.

- [ ] **Step 1: Find where settings is opened**

Run: `grep -n "settings::open\|settings::Current" crates/app/src/lib.rs`
Read that block: it builds a `Current` and passes a closure that matches on `Change`.

- [ ] **Step 2: Add the variant, the field, and the row**

In `crates/app/src/settings.rs`:

```rust
use jotter_theming::{Mode, Style, Theme};

pub enum Change {
    /// Switch to this theme id.
    Theme(String),
    /// Switch to light or dark.
    Mode(Mode),
    /// Switch between the classic look and the TUI one.
    Style(Style),
    /// Set the editor font family, or clear it back to the theme's own.
    EditorFont(Option<String>),
    /// Set the font size, which the editor and the preview share.
    Size(u32),
    /// Set the preview's font family, or clear it back to the theme's own.
    PreviewFont(Option<String>),
}
```

Add `pub style: Style,` to `Current`.

Add a `style_buttons` function modelled on `mode_buttons`, minus the swatch repaint:

```rust
/// The classic and TUI pair, and the flag that keeps a programmatic toggle from
/// echoing back as a request to change the style.
fn style_buttons(
    current: Style,
    quiet: &Rc<Cell<bool>>,
    on_change: &Rc<impl Fn(Change) + 'static>,
) -> (gtk::Box, gtk::ToggleButton, gtk::ToggleButton) {
    let classic = gtk::ToggleButton::with_label("Classic");
    let tui = gtk::ToggleButton::with_label("TUI");
    tui.set_group(Some(&classic));
    match current {
        Style::Classic => classic.set_active(true),
        Style::Tui => tui.set_active(true),
    }
    for (button, style) in [(&classic, Style::Classic), (&tui, Style::Tui)] {
        let switched = Rc::clone(on_change);
        let hushed = Rc::clone(quiet);
        button.connect_toggled(move |button| {
            if button.is_active() && !hushed.get() {
                switched(Change::Style(style));
            }
        });
    }
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.append(&classic);
    row.append(&tui);
    (row, classic, tui)
}
```

In `open`, build it and attach it as the first grid row, moving the existing rows down by one:

```rust
    let (styles, classic, tui) = style_buttons(current.style, &quiet, &on_change);
    grid.attach(&row_label("Style"), 0, 0, 1, 1);
    grid.attach(&styles, 1, 0, 1, 1);
    grid.attach(&row_label("Theme"), 0, 1, 1, 1);
    grid.attach(&themes_scroller, 1, 1, 1, 1);
    grid.attach(&row_label("Mode"), 0, 2, 1, 1);
    grid.attach(&modes, 1, 2, 1, 1);
```

Then bump the row indices in `size_row` (2 becomes 3) and `attach_font_rows` (3 and 4 become 4 and 5).

Add to `Handle`:

```rust
    /// Reflects the app's current style back into the controls.
    sync_style: Rc<dyn Fn(Style)>,
```

```rust
    /// Updates the controls to match `style` without reporting a change back.
    pub fn show_style(&self, style: Style) {
        (self.sync_style)(style);
    }
```

And build it beside `sync`:

```rust
    let sync_style: Rc<dyn Fn(Style)> = {
        let classic = classic.clone();
        let tui = tui.clone();
        let quiet = Rc::clone(&quiet);
        Rc::new(move |style| {
            quiet.set(true);
            match style {
                Style::Classic => classic.set_active(true),
                Style::Tui => tui.set_active(true),
            }
            quiet.set(false);
        })
    };
```

- [ ] **Step 3: Handle the change in the app**

In `crates/app/src/lib.rs`, in the `Change` match:

```rust
        settings::Change::Style(style) => set_appearance(&changing, |appearance| {
            appearance.style = Some(appearance::style_name(style).to_string());
        }),
```

And where `settings::Current` is built, add:

```rust
        style: appearance::style_of(&state.config.borrow().appearance),
```

`set_appearance` already saves the config, re-resolves the theme and calls `apply_theme`, which reloads the chrome CSS provider. Nothing else is needed for the dress.

- [ ] **Step 4: Build and run**

Run: `cargo build --release && cargo test --workspace`
Expected: build clean, tests pass.

Run the app offscreen against a test vault:

```sh
tools/gui-test/cage-run.sh ./target/release/jotter /path/to/test-vault
export WAYLAND_DISPLAY=wayland-0
grim /tmp/claude-1000/classic.png
```

Open settings (click the cogwheel at the bottom of the rail with `wlpoint`), click TUI, screenshot again. Then check the flip both ways, and check `~/.config/jotter/config.toml` gained `style = "tui"`.

- [ ] **Step 5: GUI GATE**

Present both screenshots to the user with what to look at: square corners everywhere, hairline rules in the theme's focus color, the whole UI in CaskaydiaMono, the selected tree row as a flat inverted block. Ask them to confirm before committing. Do not commit until they say so.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/settings.rs crates/app/src/lib.rs
git commit -F - <<'EOF'
settings: a style row that switches the dress live
EOF
```

---

### Task 5: The idiom text helpers

**Files:**
- Create: `crates/app/src/style.rs`
- Modify: `crates/app/src/lib.rs` (add `mod style;` beside the other module declarations)

**Interfaces:**
- Consumes: `jotter_theming::Style`.
- Produces:
  - `style::button(Style, &str) -> String`
  - `style::heading(Style, &str) -> String`
  - `style::segment(Style, &str) -> String`
  - `style::cursor(Style, bool) -> &'static str`
  - `style::tree_gutter(Style, bool, bool, bool) -> String`

- [ ] **Step 1: Write the failing test**

Create `crates/app/src/style.rs` with only the tests and the doc comment first:

```rust
//! The text the TUI style writes that classic does not.
//!
//! Pure string work, kept out of the widgets so the idioms can be tested
//! without a display.

use jotter_theming::Style;

#[cfg(test)]
mod tests {
    use super::{button, cursor, heading, segment, tree_gutter};
    use jotter_theming::Style::{Classic, Tui};

    #[test]
    fn a_classic_button_keeps_its_bare_label() {
        assert_eq!(button(Classic, "Continue"), "Continue");
    }

    #[test]
    fn a_tui_button_wears_brackets() {
        assert_eq!(button(Tui, "Continue"), "[ Continue ]");
    }

    #[test]
    fn a_tui_button_is_not_bracketed_twice() {
        assert_eq!(button(Tui, "[ Continue ]"), "[ Continue ]");
    }

    #[test]
    fn a_classic_heading_is_written_as_given() {
        assert_eq!(heading(Classic, "12 tags"), "12 tags");
    }

    #[test]
    fn a_tui_heading_is_upper_case() {
        assert_eq!(heading(Tui, "12 tags"), "12 TAGS");
    }

    #[test]
    fn a_classic_status_segment_is_bare() {
        assert_eq!(segment(Classic, "15px \u{21ba}"), "15px \u{21ba}");
    }

    #[test]
    fn a_tui_status_segment_is_bracketed() {
        assert_eq!(segment(Tui, "15px \u{21ba}"), "[ 15px \u{21ba} ]");
    }

    #[test]
    fn an_empty_segment_stays_empty() {
        assert_eq!(segment(Tui, ""), "");
    }

    #[test]
    fn classic_has_no_row_cursor() {
        assert_eq!(cursor(Classic, true), "");
        assert_eq!(cursor(Classic, false), "");
    }

    #[test]
    fn the_tui_cursor_marks_only_the_selected_row() {
        assert_eq!(cursor(Tui, true), ">");
        assert_eq!(cursor(Tui, false), " ");
    }

    #[test]
    fn a_classic_tree_gutter_is_empty() {
        assert_eq!(tree_gutter(Classic, true, false, false), "");
    }

    #[test]
    fn a_tui_folder_shows_which_way_it_points() {
        assert_eq!(tree_gutter(Tui, true, false, false), " \u{25b8}");
        assert_eq!(tree_gutter(Tui, true, true, false), " \u{25be}");
    }

    #[test]
    fn a_tui_file_has_no_marker_but_keeps_the_column() {
        assert_eq!(tree_gutter(Tui, false, false, false), "  ");
    }

    #[test]
    fn a_selected_tui_row_carries_the_cursor_ahead_of_the_marker() {
        assert_eq!(tree_gutter(Tui, true, true, true), ">\u{25be}");
        assert_eq!(tree_gutter(Tui, false, false, true), "> ");
    }
}
```

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -p jotter-app style`
Expected: FAIL, `button` and friends are not found.

- [ ] **Step 3: Write the helpers**

Above the test module in the same file:

```rust
/// A command button's face: bracketed in TUI, bare in classic.
#[must_use]
pub fn button(style: Style, label: &str) -> String {
    if style != Style::Tui || label.starts_with("[ ") {
        return label.to_string();
    }
    format!("[ {label} ]")
}

/// A panel heading: upper case in TUI, as written in classic.
#[must_use]
pub fn heading(style: Style, text: &str) -> String {
    match style {
        Style::Tui => text.to_uppercase(),
        Style::Classic => text.to_string(),
    }
}

/// A status bar item: bracketed in TUI, bare in classic. An empty item stays
/// empty, since the bar hides those rather than showing empty brackets.
#[must_use]
pub fn segment(style: Style, text: &str) -> String {
    if style != Style::Tui || text.is_empty() {
        return text.to_string();
    }
    format!("[ {text} ]")
}

/// The cursor a list row wears: only the selected row gets it, and only in TUI.
#[must_use]
pub fn cursor(style: Style, selected: bool) -> &'static str {
    match (style, selected) {
        (Style::Classic, _) => "",
        (Style::Tui, true) => ">",
        (Style::Tui, false) => " ",
    }
}

/// The tree's left gutter: the row cursor, then the folder marker. Files keep
/// the column so names stay aligned down the tree.
#[must_use]
pub fn tree_gutter(style: Style, expandable: bool, expanded: bool, selected: bool) -> String {
    if style != Style::Tui {
        return String::new();
    }
    let marker = match (expandable, expanded) {
        (true, true) => "\u{25be}",
        (true, false) => "\u{25b8}",
        (false, _) => " ",
    };
    format!("{}{marker}", cursor(style, selected))
}
```

Add `mod style;` to the module list in `crates/app/src/lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jotter-app style`
Expected: PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo clippy --workspace --all-targets`

```bash
git add crates/app/src/style.rs crates/app/src/lib.rs
git commit -F - <<'EOF'
app: the text helpers the TUI idioms need
EOF
```

---

### Task 6: Tree idioms (GUI GATE)

**Files:**
- Modify: `crates/app/src/lib.rs:1085-1148` (`tree_factory`), `crates/app/src/lib.rs:3155-3200` (`apply_theme`), `crates/app/src/lib.rs:582-596` (`build_tree_page`), `crates/app/src/lib.rs:876-880` (where the vault name is set)

**Interfaces:**
- Consumes: `style::tree_gutter`, `style::heading`, `state.theme.borrow().style`.
- Produces: `restyle(state: &Rc<State>)`, called from `apply_theme`, which every later task extends with one more line.

- [ ] **Step 1: Add the gutter label in the factory setup**

In `tree_factory`'s `connect_setup`, before the existing name label:

```rust
        let line = gtk::Box::new(Orientation::Horizontal, 6);
        let gutter = Label::builder().halign(gtk::Align::Start).build();
        gutter.add_css_class("tree-cursor");
        line.append(&gutter);
        line.append(&Label::builder().halign(gtk::Align::Start).build());
```

The bind closure reads the three labels as `first_child` (gutter), its `next_sibling` (name) and `last_child` (title). Update the existing destructuring accordingly:

```rust
        let (Some(gutter), Some(name), Some(title)) = (
            line.first_child().and_downcast::<Label>(),
            line.first_child().and_then(|first| first.next_sibling()).and_downcast::<Label>(),
            line.last_child().and_downcast::<Label>(),
        ) else {
            return;
        };
```

- [ ] **Step 2: Fill the gutter on bind**

At the end of the bind closure, after the `openable` block:

```rust
        let style = bind_state.theme.borrow().style;
        expander.set_hide_expander(style == Style::Tui);
        expander.set_indent_for_icon(style != Style::Tui);
        gutter.set_visible(style == Style::Tui);
        gutter.set_text(&crate::style::tree_gutter(
            style,
            row.is_expandable(),
            row.is_expanded(),
            item.is_selected(),
        ));
```

Add `use jotter_theming::Style;` to the imports at the top of `lib.rs` (the existing line imports `Mode`, `Theme`, `ThemeFile`).

- [ ] **Step 3: Keep the gutter honest when selection or expansion changes**

A bound row does not rebind when it is selected or when a folder opens, so both need a signal. In `tree_factory`, before building the factory:

```rust
    // Handlers on the bound row, dropped on unbind: a rebind would otherwise
    // leave the old row still writing into this widget.
    let bound: Rc<RefCell<HashMap<gtk::ListItem, (TreeListRow, glib::SignalHandlerId)>>> =
        Rc::new(RefCell::new(HashMap::new()));
```

In `connect_setup`, after `item.set_child(...)`, connect the selection notify once for the life of the item widget:

```rust
        let selected_state = Rc::clone(&setup_state);
        item.connect_notify_local(Some("selected"), move |item, _| {
            refresh_gutter(&selected_state, item);
        });
```

In `connect_bind`, after filling the gutter:

```rust
        let expanding = Rc::clone(&bind_state);
        let listed = item.clone();
        let handler = row.connect_notify_local(Some("expanded"), move |_, _| {
            refresh_gutter(&expanding, &listed);
        });
        if let Some((old_row, old)) = bound.borrow_mut().insert(item.clone(), (row.clone(), handler))
        {
            old_row.disconnect(old);
        }
```

And add a `connect_unbind` that drops the handler:

```rust
    let unbinding = Rc::clone(&bound);
    factory.connect_unbind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let Some((row, handler)) = unbinding.borrow_mut().remove(item) {
            row.disconnect(handler);
        }
    });
```

Then the shared refresh, as a free function beside `tree_factory`:

```rust
/// Rewrites one tree row's gutter from the row's current state.
fn refresh_gutter(state: &Rc<State>, item: &gtk::ListItem) {
    let Some(row) = item.item().and_downcast::<TreeListRow>() else {
        return;
    };
    let Some(gutter) = item
        .child()
        .and_downcast::<TreeExpander>()
        .and_then(|expander| expander.child())
        .and_downcast::<gtk::Box>()
        .and_then(|line| line.first_child())
        .and_downcast::<Label>()
    else {
        return;
    };
    gutter.set_text(&crate::style::tree_gutter(
        state.theme.borrow().style,
        row.is_expandable(),
        row.is_expanded(),
        item.is_selected(),
    ));
}
```

Add `use std::collections::HashMap;` and `glib` to the imports if they are not already there (`HashMap` is already imported in `lib.rs`; check before adding).

- [ ] **Step 4: Uppercase the vault name and restyle on a style change**

In `build_tree_page`, nothing changes. Where the vault name is set (around line 876), wrap the text:

```rust
    let name = vaults::known(&[root.display().to_string()]) /* existing expression */;
    state.vault_name.set_text(&style::heading(state.theme.borrow().style, &name));
```

Add the restyle entry point beside `apply_theme`:

```rust
/// Re-applies the character-level idioms after a style change.
///
/// The dress is CSS and lands with the provider; these are widget contents, so
/// they are rewritten here. Idempotent: `apply_theme` runs on every repaint.
fn restyle(state: &Rc<State>) {
    refresh_vault_name(state);
    rebuild_tree(state);
}
```

`refresh_vault_name` is the existing vault-name assignment lifted into a function taking `&Rc<State>` and reading the root from `state.session`. Call `restyle(state)` as the last line of `apply_theme`, after `*state.theme.borrow_mut() = next;`.

- [ ] **Step 5: Build and check**

Run: `cargo build --release && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: clean.

Run the app offscreen in TUI (`style = "tui"` in the config), expand a folder, select a note, screenshot. Then switch to classic in the settings window and screenshot again.

Check: `▸` on closed folders, `▾` on open ones, `>` on the selected row only, no GTK expander arrow in TUI, and the arrow back in classic with no leftover gutter column. Expanding a folder must flip its marker immediately. Scroll the tree to be sure a recycled row does not carry another row's cursor.

- [ ] **Step 6: GUI GATE**

Present both screenshots and wait for the user before committing.

- [ ] **Step 7: Commit**

```bash
git add crates/app/src/lib.rs
git commit -F - <<'EOF'
tree: markers and a row cursor in the TUI style
EOF
```

---

### Task 7: Results, panels and headings (GUI GATE)

**Files:**
- Modify: `crates/app/src/results.rs`
- Modify: `crates/app/src/search_panel.rs`, `crates/app/src/drill.rs`, `crates/app/src/git_panel.rs`, `crates/app/src/backlinks.rs`
- Modify: `crates/app/src/lib.rs` (`restyle`)

**Interfaces:**
- Consumes: `style::cursor`, `style::heading`, `results::List`.
- Produces: `results::List::set_style(&self, Style)`; `search_panel::Panel::set_style(&self, Style)`; `drill::Panel::set_style(&self, Style)`; `git_panel::Panel::set_style(&self, Style)`; `backlinks::Strip::set_style(&self, Style)`. All idempotent.

- [ ] **Step 1: Add the style to the results list**

In `crates/app/src/results.rs`, add a field to `List`:

```rust
    /// The visual language the rows are drawn in.
    style: Cell<Style>,
```

Initialise it to `Cell::new(Style::Classic)` in `new`, and add `use std::cell::Cell;` plus `use jotter_theming::Style;`.

```rust
    /// Redraws the rows in `style`, which changes the row cursor.
    pub fn set_style(&self, style: Style) {
        if self.style.get() == style {
            return;
        }
        self.style.set(style);
        let hits = self.hits.borrow().clone();
        self.set_hits(&hits);
    }
```

- [ ] **Step 2: Draw the cursor on the heading row**

`heading_row` gains a style parameter and a leading label:

```rust
fn heading_row(style: Style, path: &str, matches: usize, badge: Option<&str>) -> gtk::Box {
    let (folder, stem) = split_path(path);

    let row = gtk::Box::new(Orientation::Horizontal, 6);
    row.add_css_class("search-heading");

    let mark = crate::style::cursor(style, false);
    if !mark.is_empty() {
        let cursor = gtk::Label::builder().xalign(0.0).label(mark).build();
        cursor.add_css_class("row-cursor");
        row.append(&cursor);
    }
    // ... the rest unchanged
```

The cursor text is a placeholder here: `set_hits` does not know which row is selected. Update it after selection instead, in `List::new`, beside the existing `connect_row_activated`:

```rust
        let cursored = Rc::clone(&shared);
        shared.rows.connect_row_selected(move |rows, selected| {
            let style = cursored.style.get();
            if style != Style::Tui {
                return;
            }
            let mut index = 0;
            while let Some(row) = rows.row_at_index(index) {
                let here = selected.is_some_and(|chosen| chosen == &row);
                set_row_cursor(&row, crate::style::cursor(style, here));
                index += 1;
            }
        });
```

with:

```rust
/// Rewrites the cursor label of one result row, where it has one.
fn set_row_cursor(row: &gtk::ListBoxRow, mark: &str) {
    let Some(cursor) = row
        .child()
        .and_downcast::<gtk::Box>()
        .and_then(|line| line.first_child())
        .and_downcast::<gtk::Label>()
    else {
        return;
    };
    if cursor.has_css_class("row-cursor") {
        cursor.set_text(mark);
    }
}
```

Pass `self.style.get()` into `heading_row` from `set_hits`. Snippet rows are indented by CSS and keep no cursor.

- [ ] **Step 3: Add set_style to the four panels**

Each is the same shape. `search_panel::Panel`:

```rust
    /// Redraws the panel in `style`.
    pub fn set_style(&self, style: Style) {
        self.results.set_style(style);
    }
```

`backlinks::Strip`: same, plus the header text, which already carries a `▾`/`▸`:

```rust
    pub fn set_style(&self, style: Style) {
        self.style.set(style);
        self.results.set_style(style);
        self.redraw();
    }
```

with a `style: Cell<Style>` field, and `redraw` wrapping its label through `crate::style::heading(self.style.get(), &header_text(count, showing))`.

`drill::Panel` and `git_panel::Panel`: store `style: Cell<Style>`, delegate to their lists, and re-apply their heading through `crate::style::heading` wherever `heading.set_label(...)` is called today.

Add `.panel-bar` to the horizontal `bar` box in `search_panel.rs`, `drill.rs` and `git_panel.rs`:

```rust
        bar.add_css_class("panel-bar");
```

- [ ] **Step 4: Call them from restyle**

In `crates/app/src/lib.rs`, extend `restyle`:

```rust
fn restyle(state: &Rc<State>) {
    let style = state.theme.borrow().style;
    refresh_vault_name(state);
    rebuild_tree(state);
    state.search_panel.set_style(style);
    state.tags_panel.set_style(style);
    state.report_panel.set_style(style);
    state.git_panel.set_style(style);
    state.backlinks.set_style(style);
}
```

- [ ] **Step 5: Run the tests and build**

Run: `cargo test --workspace && cargo build --release && cargo clippy --workspace --all-targets`
Expected: clean. The existing `results.rs` and `search_panel.rs` unit tests still pass untouched.

- [ ] **Step 6: Check in the app**

In TUI: open search (the palette or the keybinding shown in `?`), type a query, arrow down the results, and confirm the `>` follows the selection and that the panel bar has a rule under it. Open the tags page and the git page, and open the backlinks strip on a note that has backlinks. Screenshot each. Then flip to classic and confirm no stray cursor column is left.

- [ ] **Step 7: GUI GATE**

Present the screenshots and wait before committing.

- [ ] **Step 8: Commit**

```bash
git add crates/app/src
git commit -F - <<'EOF'
panels: TUI headings and a cursor on the selected result
EOF
```

---

### Task 8: Status bar, palette and completion (GUI GATE)

**Files:**
- Modify: `crates/app/src/lib.rs` (`show_git_status`, `refresh_broken`, `refresh_size_indicator`, `restyle`)
- Modify: `crates/app/src/picker.rs`

**Interfaces:**
- Consumes: `style::segment`, `style::cursor`, `style::heading`.
- Produces: `picker::open` gains a `style: Style` parameter and a `title: &str` parameter; `refresh_git_segment(state: &Rc<State>)`.

- [ ] **Step 1: Bracket the status segments**

In `show_git_status`:

```rust
    let style = state.theme.borrow().style;
    state.git.set_label(&style::segment(style, &git_status::label(&status)));
```

In `refresh_broken`:

```rust
    let style = state.theme.borrow().style;
    state.broken.set_label(&style::segment(style, &broken_label(missing.len())));
```

In `refresh_size_indicator`:

```rust
    let style = state.theme.borrow().style;
    state.size.set_label(&style::segment(style, &format!("{chosen}px \u{21ba}")));
```

Add the git re-label so a style change does not wait for the next git poll:

```rust
/// Re-labels the git segment from the last status read, after a style change.
fn refresh_git_segment(state: &Rc<State>) {
    let label = state
        .git_last
        .borrow()
        .as_ref()
        .map(|status| git_status::label(status));
    if let Some(label) = label {
        state.git.set_label(&style::segment(state.theme.borrow().style, &label));
    }
}
```

Extend `restyle`:

```rust
    refresh_git_segment(state);
    refresh_broken(state);
    refresh_size_indicator(state);
```

`refresh_size_indicator` is already called by `set_appearance` after `apply_theme`; calling it inside `restyle` too is harmless and keeps every path correct.

- [ ] **Step 2: Give the picker a title, a prompt and a cursor**

In `crates/app/src/picker.rs`, thread the style and a title through:

```rust
pub fn open<S, A, C>(
    overlay: &gtk::Overlay,
    style: Style,
    title: &str,
    placeholder: &str,
    initial_query: &str,
    source: S,
    activate: A,
    on_close: C,
) -> Handle
```

In `build_panel`, when the style is TUI, put a title line above the entry and a prompt beside it:

```rust
    let panel = gtk::Box::new(Orientation::Vertical, 0);
    panel.add_css_class("picker");
    // ... halign, valign, margin, size as today
    if style == Style::Tui {
        let heading = gtk::Label::builder()
            .xalign(0.0)
            .label(crate::style::heading(style, title))
            .build();
        heading.add_css_class("picker-title");
        panel.append(&heading);

        let line = gtk::Box::new(Orientation::Horizontal, 0);
        let prompt = gtk::Label::new(Some(">"));
        prompt.add_css_class("picker-prompt");
        line.append(&prompt);
        entry.set_hexpand(true);
        line.append(&entry);
        panel.append(&line);
    } else {
        panel.append(&entry);
    }
    panel.append(&scroller);
```

In `row_factory`, prepend a cursor label in setup and fill it on bind, exactly as the tree does:

```rust
    factory.connect_setup(move |_, item| {
        let line = gtk::Box::new(Orientation::Horizontal, 8);
        let cursor = gtk::Label::builder().xalign(0.0).build();
        cursor.add_css_class("row-cursor");
        cursor.set_visible(style == Style::Tui);
        line.append(&cursor);
        // ... label and detail as today
```

```rust
            cursor.set_text(crate::style::cursor(style, item.is_selected()));
```

plus the same `connect_notify_local(Some("selected"), ...)` in setup so the cursor follows the arrow keys. The picker rows are simple (no expansion), so no unbind bookkeeping is needed: the closure only reads the item it was created for.

Update both call sites in `lib.rs` (the quick switcher and the command palette) to pass `state.theme.borrow().style` and a title: `"Quick switch"` and `"Command palette"` respectively. Check the exact call sites with `grep -n "picker::open" crates/app/src/lib.rs`.

The completion popup needs no content change: its dress is already covered by `.completion` in the TUI sheet.

- [ ] **Step 3: Build and test**

Run: `cargo test --workspace && cargo build --release && cargo clippy --workspace --all-targets`
Expected: clean.

- [ ] **Step 4: Check in the app**

In TUI: open the palette and the switcher, arrow down, screenshot each. Confirm the title line, the `>` prompt, the cursor on the selected row, and bracketed status items at the bottom right. Change the font size with Ctrl+scroll to make the size segment appear, and confirm it reads `[ 17px ↺ ]`. Flip to classic and confirm the palette has no title line and the status items are bare.

- [ ] **Step 5: GUI GATE**

Present the screenshots and wait before committing.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -F - <<'EOF'
status bar and palette: TUI segments, title and prompt
EOF
```

---

### Task 9: Conflict view, settings and keysheet buttons (GUI GATE)

**Files:**
- Modify: `crates/app/src/conflict_view.rs`
- Modify: `crates/app/src/settings.rs`
- Modify: `crates/app/src/keysheet.rs`
- Modify: `crates/app/src/lib.rs` (`restyle`)

**Interfaces:**
- Consumes: `style::button`, `style::heading`.
- Produces: `conflict_view::View::set_style(&self, Style)`; `settings::Handle::show_style` now also re-labels the window's own buttons; `keysheet::open` gains a `style: Style` parameter.

- [ ] **Step 1: Store the conflict actions and add set_style**

In `crates/app/src/conflict_view.rs`, add to `View`:

```rust
    /// Every action button with the label it wears in classic, so a style
    /// change can rewrite the faces.
    actions: Vec<(gtk::Button, String)>,
    style: std::cell::Cell<Style>,
```

Fill it in `new` after the buttons are built:

```rust
        let actions = vec![
            (take_incoming.clone(), "Take incoming".to_string()),
            (take_yours.clone(), "Take yours".to_string()),
            (keep_both.clone(), "Take both".to_string()),
            (edit.clone(), "Edit by hand".to_string()),
            (save.clone(), "Save edit".to_string()),
            (proceed.clone(), "Continue".to_string()),
            (abort.clone(), "Abort sync".to_string()),
        ];
```

```rust
    /// Redraws the page in `style`: bracketed actions and upper-case pane titles.
    pub fn set_style(&self, style: Style) {
        self.style.set(style);
        for (button, label) in &self.actions {
            button.set_label(&crate::style::button(style, label));
        }
        self.incoming.set_title(&crate::style::heading(style, "Incoming"));
        self.resolution.set_title(&crate::style::heading(style, "Resolution"));
        self.yours.set_title(&crate::style::heading(style, "Yours"));
    }
```

If any other code path calls `Pane::set_title` with a computed title, wrap that call through `crate::style::heading(self.style.get(), ...)` too. Check with `grep -n "set_title" crates/app/src/conflict_view.rs`.

Add `state.conflict.set_style(style);` to `restyle`.

- [ ] **Step 2: Bracket the settings and keysheet buttons**

In `crates/app/src/settings.rs`, the Light/Dark and Classic/TUI toggles and the size steps are the label-faced buttons. Build them through the helper. In `mode_buttons`:

```rust
    let light = gtk::ToggleButton::with_label(&crate::style::button(style, "Light"));
    let dark = gtk::ToggleButton::with_label(&crate::style::button(style, "Dark"));
```

which means `mode_buttons`, `style_buttons` and `size_row` each take a `style: Style`. `Current` already carries it, so `open` passes `current.style` down.

Extend `sync_style` so flipping the switch relabels the window it lives in:

```rust
    let sync_style: Rc<dyn Fn(Style)> = {
        let faces = vec![
            (classic.clone().upcast::<gtk::Button>(), "Classic".to_string()),
            (tui.clone().upcast::<gtk::Button>(), "TUI".to_string()),
            (light.clone().upcast::<gtk::Button>(), "Light".to_string()),
            (dark.clone().upcast::<gtk::Button>(), "Dark".to_string()),
        ];
        // ... plus the existing quiet/set_active block
        Rc::new(move |style| {
            // set_active under quiet, as before
            for (button, label) in &faces {
                button.set_label(&crate::style::button(style, label));
            }
        })
    };
```

and call `handle.show_style(style)` from the app after a style change, beside the existing `handle.show_mode(mode)` call in `set_theme_mode`. Put it in `set_appearance`'s style arm or, simpler, at the end of `restyle`:

```rust
    if let Some(handle) = state.settings.borrow().as_ref() {
        handle.show_style(style);
    }
```

In `crates/app/src/keysheet.rs`, pass the style into `open` and wrap the section headings:

```rust
    heading.set_label(&crate::style::heading(style, &section.name));
```

Update the `keysheet::open` call site in `lib.rs` to pass `state.theme.borrow().style`.

- [ ] **Step 3: Build and test**

Run: `cargo test --workspace && cargo build --release && cargo clippy --workspace --all-targets`
Expected: clean.

- [ ] **Step 4: Check in the app**

Settings and keysheet are easy: open both in TUI, screenshot, flip the style with the settings window open and watch the buttons relabel in place.

The conflict view needs a repository stuck in a conflicted rebase. Build one in a scratch vault:

```sh
cd /tmp/claude-1000 && rm -rf conflict-vault && mkdir conflict-vault && cd conflict-vault
git init -q && printf '# note\n\nbase\n' > note.md && git add . && git commit -qm base
git switch -qc theirs && printf '# note\n\nincoming\n' > note.md && git commit -qam incoming
git switch -q - && printf '# note\n\nyours\n' > note.md && git commit -qam yours
git rebase theirs || true      # leaves the conflict in place
```

Open that vault in the app, confirm the resolver takes the pane, screenshot it in TUI and in classic.

- [ ] **Step 5: GUI GATE**

Present the screenshots and wait before committing.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -F - <<'EOF'
conflict view, settings and keysheet: the TUI faces
EOF
```

---

### Task 10: Documentation and the final sweep

**Files:**
- Modify: `docs/architecture.md`

- [ ] **Step 1: Document the style axis**

Find the theming section of `docs/architecture.md` (`grep -n -i "theming\|theme" docs/architecture.md`) and add a short paragraph after it:

> A theme is resolved for a mode (light or dark) and then drawn in a style: `classic`, the bauhaus-leaning default, or `tui`, a terminal look. The style is a user choice held in `config.appearance.style`, not a theme property, so it never appears in a theme file: `appearance::resolve` stamps it onto the resolved `Theme` and `Theme::to_gtk_css` dispatches on it. The colors come from the same tokens either way, with `focus` carrying the structure in TUI. Character-level idioms (tree markers, row cursors, bracketed buttons) live in `crates/app/src/style.rs` and are re-applied by `restyle`, which `apply_theme` calls on every repaint.

- [ ] **Step 2: Sweep both styles across both themes and both modes**

For each of the four combinations of theme and mode, in TUI and in classic, run the app offscreen and screenshot the main window with a note open and the tree expanded. Eight screenshots. Look for: a color that vanishes into its background, a frame that disappears in light mode, a row whose selected text is unreadable.

- [ ] **Step 3: Full test run**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: clean, and `git status` shows no unexpected snapshot churn.

- [ ] **Step 4: GUI GATE**

Present the eight screenshots. This is the sign-off on the whole feature, and the point at which a color mapping is worth changing if any combination reads badly.

- [ ] **Step 5: Commit**

```bash
git add docs/architecture.md
git commit -F - <<'EOF'
docs: the style axis in the architecture notes
EOF
```

---

## Self-review

**Spec coverage:**

| Spec section | Task |
|---|---|
| `Style` enum, `Theme.style`, dispatch, `tui_css.rs` | 1, 2 |
| Preview CSS and sourceview untouched | 1 (no change made to either) |
| `config.appearance.style` | 3 |
| `style_of`, `style_name`, `ui_font` pinned before the override | 3 |
| `apply_theme` pushes the style into the widgets | 6 (`restyle`), extended in 7, 8, 9 |
| The dress: radius 0, hairline frames, flat blocks, frameless buttons, boxed entries, thin scrollbars, square tooltips | 1, 2 |
| Tree: hidden expander, `▸`/`▾`, `>` cursor | 6 |
| Section headers uppercase in focus with a rule | 6 (vault name), 7 (panel bars) |
| Rail: left accent bar, no underline | 2 (CSS only, as the spec says the glyphs stay) |
| Buttons bracketed, icon-only buttons untouched | 9 |
| Status bar: mono, top rule, bracketed segments | 2 (rule and font), 8 (segments) |
| Picker title, prompt, cursor | 8 |
| Completion popup dress | 2 |
| Conflict view framed panes and headers | 2, 9 |
| Keysheet and settings dress plus bracketed buttons | 2, 9 |
| Settings Style row, first, live | 4 |
| Tests: theming assertions, appearance round trip, font pin | 1, 2, 3 |
| Manual pass across styles, themes and modes | 10 |
| Out of scope: preview, editor, key-hint line, per-theme overrides, shortcut | never introduced |

**Placeholders:** none. Every step carries the code or the exact command it needs.

**Type consistency:** `Style` is `jotter_theming::Style` throughout. `style::button`, `style::heading`, `style::segment`, `style::cursor`, `style::tree_gutter` are defined in Task 5 and used with those names and signatures in Tasks 6 to 9. `set_style(&self, Style)` is the name on `results::List`, all four panels and `conflict_view::View`. `restyle(&Rc<State>)` is introduced in Task 6 and extended in 7, 8 and 9.

**One risk worth naming:** `TreeExpander::set_hide_expander` and `set_indent_for_icon` need GTK 4.10 or newer, and the crate is built with the `v4_12` feature, so both are available. If either method is missing at compile time, the feature flag is the thing to check first.
