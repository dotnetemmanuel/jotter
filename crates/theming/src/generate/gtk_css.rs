//! Generate the GTK4 chrome stylesheet for `gtk::CssProvider::load_from_data`.
//!
//! The look is Bauhaus-leaning: flat surfaces (no drop-shadows), crisp small
//! corners, and thick lines used structurally rather than as boxes. Text fields
//! are underlines, not framed boxes; the primary button is a solid high-contrast
//! block that pops the accent on hover; the selected tree row is a flat accent
//! block. Depth comes from color and line, not shadow.

use crate::model::{Style, Theme};

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
        let c = &self.chrome;
        let t = &self.typography;
        let r = c.radius;
        let bw = c.border_width;

        format!(
            "/* {name} */\n\
window {{\n  background-color: {bg};\n  color: {text};\n  font-family: {ui_font};\n  font-size: {size}px;\n}}\n\n\
headerbar {{\n  background-color: {bg};\n  background-image: none;\n  color: {text};\n  border-bottom: 1px solid {overlay};\n  box-shadow: none;\n  padding-left: 0;\n}}\n\n\
separator {{\n  background-color: {overlay};\n  min-height: 1px;\n  min-width: 1px;\n}}\n\n\
paned > separator {{\n  background-color: {overlay};\n  background-image: none;\n  min-width: 1px;\n  min-height: 1px;\n}}\n\n\
.sidebar {{\n  background-color: {bg};\n}}\n\n\
.sidebar listview {{\n  background-color: transparent;\n  color: {text};\n  padding-top: 6px;\n}}\n\n\
.sidebar listview > row {{\n  border-radius: {r}px;\n  padding: 3px 8px;\n  margin: 1px 6px;\n}}\n\n\
.sidebar listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.sidebar listview > row:focus-within {{\n  box-shadow: inset 2px 0 0 0 {accent};\n}}\n\n\
.vault-name {{\n  color: {muted};\n  font-size: {small}px;\n  font-weight: bold;\n  padding: 8px 10px 4px 10px;\n}}\n\n\
.tree-inert {{\n  color: {muted};\n  opacity: 0.55;\n}}\n\n\
.tree-title {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.sidebar listview > row:selected .tree-title {{\n  color: alpha({bg}, 0.7);\n}}\n\n\
.tree-drop {{\n  box-shadow: inset 0 0 0 2px {accent};\n  border-radius: {r}px;\n}}\n\n\
.sidebar listview:drop(active), .sidebar listview > row:drop(active) {{\n  box-shadow: none;\n  outline: none;\n}}\n\n\
.sidebar listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.sidebar listview > row:selected:hover {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
button {{\n  background-color: {surface};\n  background-image: none;\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: none;\n  padding: 6px 14px;\n}}\n\n\
button:hover {{\n  background-color: {overlay};\n}}\n\n\
button:checked {{\n  background-color: {accent};\n  background-image: none;\n  color: {bg};\n  border-color: {accent};\n}}\n\n\
button.suggested-action {{\n  background-color: {text};\n  background-image: none;\n  color: {bg};\n  border-color: {text};\n}}\n\n\
button.suggested-action:hover {{\n  background-color: {accent};\n  border-color: {accent};\n  color: {bg};\n}}\n\n\
button.destructive-action {{\n  background-color: {danger};\n  background-image: none;\n  color: {bg};\n  border-color: {danger};\n}}\n\n\
button.destructive-action:hover {{\n  background-color: {danger};\n}}\n\n\
windowcontrols button, windowcontrols button.titlebutton {{\n  background-color: transparent;\n  background-image: none;\n  border: none;\n  box-shadow: none;\n  color: {text};\n}}\n\n\
windowcontrols button:hover, windowcontrols button.titlebutton:hover {{\n  background-color: {overlay};\n  background-image: none;\n  color: {text};\n  border-radius: {r}px;\n}}\n\n\
windowcontrols button image, windowcontrols button:hover image {{\n  background: none;\n  background-color: transparent;\n  background-image: none;\n  box-shadow: none;\n  color: {text};\n}}\n\n\
entry {{\n  background-color: transparent;\n  background-image: none;\n  color: {text};\n  border: none;\n  border-bottom: {bw}px solid {border};\n  border-radius: 0;\n  box-shadow: none;\n  outline: none;\n  padding: 6px 2px;\n}}\n\n\
entry:focus-within, entry:hover, entry:focus-visible {{\n  outline: none;\n  box-shadow: none;\n}}\n\n\
entry:focus-within {{\n  border-bottom-color: {accent};\n}}\n\n\
entry > text, entry > text:focus-visible {{\n  outline: none;\n  box-shadow: none;\n}}\n\n\
row:focus, row:focus-visible, listview:focus-visible, list:focus-visible {{\n  outline: none;\n}}\n\n\
popover > contents {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: none;\n}}\n\n\
.picker-scrim {{\n  background-color: alpha({bg}, 0.45);\n}}\n\n",
            name = self.scheme_name(),
            bg = c.background,
            surface = c.surface,
            overlay = c.overlay,
            text = c.text,
            accent = c.accent,
            border = c.border,
            danger = c.danger,
            muted = c.muted,
            ui_font = t.ui_font,
            size = t.font_size,
            small = t.font_size.saturating_sub(1),
        ) + &self.classic_parts_css()
    }

    /// The half of the stylesheet that dresses jotter's own widgets, split
    /// from the general chrome so neither is a wall of text.
    fn classic_parts_css(&self) -> String {
        let c = &self.chrome;
        let t = &self.typography;
        let r = c.radius;
        let bw = c.border_width;

        format!(
            ".rail {{\n  background-color: {bg};\n  padding: 10px 6px;\n}}\n\n\
.rail-button {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {muted};\n  padding: 10px 4px;\n  min-height: 0;\n  min-width: 0;\n  border-radius: {r}px;\n}}\n\n\
.rail-button label {{\n  font-family: {editor_font};\n  font-size: {rail_size}px;\n  padding-bottom: 1px;\n  border-bottom: 2px solid transparent;\n}}\n\n\
.rail-button:hover {{\n  background-color: {overlay};\n  color: {text};\n}}\n\n\
.rail-button:checked {{\n  background: none;\n  color: {accent};\n}}\n\n\
.rail-button:checked label {{\n  border-bottom-color: {accent};\n}}\n\n\
.rail-settings {{\n  padding-left: 3px;\n  padding-right: 6px;\n}}\n\n\
.font-tick {{\n  color: {accent};\n}}\n\n\
.font-list {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.font-list > row {{\n  padding: 2px 8px;\n  border-radius: {r}px;\n}}\n\n\
.font-list > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.font-list > row:selected {{\n  background-color: {overlay};\n  color: {text};\n  box-shadow: inset 2px 0 0 0 {accent};\n}}\n\n\
.settings {{\n  background-color: {bg};\n}}\n\n\
.settings-label {{\n  color: {muted};\n}}\n\n\
.settings-close {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {text};\n  padding: 4px 8px;\n  min-height: 0;\n  min-width: 0;\n}}\n\n\
.settings-close:hover {{\n  background-color: {overlay};\n  border-radius: {r}px;\n}}\n\n\
.keysheet-heading {{\n  color: {accent};\n  font-size: {small}px;\n  font-weight: bold;\n}}\n\n\
.keysheet-keys {{\n  color: {muted};\n  font-family: {editor_font};\n  font-size: {small}px;\n}}\n\n\
.theme-button {{\n  padding: 6px;\n}}\n\n\
.theme-name {{\n  font-size: {small}px;\n}}\n\n\
.conflict {{\n  background-color: {bg};\n  padding: 10px 12px;\n}}\n\n\
.conflict-header {{\n  padding-bottom: 4px;\n  border-bottom: {bw}px solid {overlay};\n}}\n\n\
.conflict-heading {{\n  font-weight: bold;\n}}\n\n\
.conflict-progress {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.conflict-title {{\n  font-family: {editor_font};\n  font-size: {small}px;\n  font-weight: bold;\n  padding: 2px 6px;\n  border-radius: {r}px;\n}}\n\n\
.conflict-title.conflict-incoming {{\n  color: {focus};\n}}\n\n\
.conflict-title.conflict-yours {{\n  color: {accent};\n}}\n\n\
.conflict-title.conflict-resolution {{\n  color: {text};\n}}\n\n\
.conflict-body {{\n  font-family: {editor_font};\n  border-radius: {r}px;\n  padding: 6px 8px;\n}}\n\n\
.conflict-body.conflict-incoming, .conflict-body.conflict-incoming text {{\n  background-color: alpha({focus}, 0.12);\n  color: {text};\n}}\n\n\
.conflict-body.conflict-yours, .conflict-body.conflict-yours text {{\n  background-color: alpha({accent}, 0.12);\n  color: {text};\n}}\n\n\
.conflict-body.conflict-resolution, .conflict-body.conflict-resolution text {{\n  background-color: {surface};\n  color: {text};\n}}\n\n\
.conflict-actions {{\n  padding-top: 4px;\n}}\n\n\
.conflict-action {{\n  padding: 4px 10px;\n}}\n\n\
.status-size {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {muted};\n  font-size: {small}px;\n  padding: 0 8px;\n  min-height: 0;\n}}\n\n\
.status-size:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.size-step {{\n  padding: 2px 10px;\n  min-width: 0;\n}}\n\n\
.size-value {{\n  font-family: {editor_font};\n}}\n\n\
.status-git {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {muted};\n  font-family: {editor_font};\n  font-size: {small}px;\n  padding: 0 8px;\n  min-height: 0;\n}}\n\n\
.status-git:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.status-broken {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {danger};\n  font-size: {small}px;\n  padding: 0 8px;\n  min-height: 0;\n}}\n\n\
.status-broken:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.backlinks {{\n  background-color: {bg};\n}}\n\n\
.backlinks-header {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {muted};\n  font-size: {small}px;\n  padding: 3px 10px;\n  min-height: 0;\n}}\n\n\
.backlinks-header:hover {{\n  background: none;\n  color: {text};\n}}\n\n\
.search-results {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.search-results > row {{\n  border-radius: {r}px;\n  padding: 1px 6px;\n  margin: 0 6px;\n}}\n\n\
.search-results > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.search-results > row:selected {{\n  background-color: {overlay};\n  color: {text};\n  box-shadow: inset 2px 0 0 0 {accent};\n}}\n\n\
.panel-back {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {text};\n  padding: 0 4px;\n  margin: 0;\n  min-height: 0;\n  min-width: 0;\n}}\n\n\
.panel-back label {{\n  font-family: {editor_font};\n  font-size: {back_size}px;\n  margin-bottom: 3px;\n}}\n\n\
.panel-back:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.tags-heading {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.tag-row {{\n  padding: 3px 4px;\n}}\n\n\
.search-heading {{\n  margin-top: 14px;\n  padding: 2px 2px 4px 2px;\n  border-bottom: 1px solid alpha({border}, 0.25);\n}}\n\n\
.search-results > row:first-child .search-heading {{\n  margin-top: 2px;\n}}\n\n\
.search-name {{\n  font-weight: bold;\n}}\n\n\
.search-folder {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.search-count {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.search-snippet {{\n  color: {muted};\n  margin-left: 5px;\n  padding: 1px 0 1px 10px;\n  border-left: 1px solid alpha({border}, 0.18);\n}}\n\n\
.completion list {{\n  background-color: transparent;\n  color: {text};\n  font-family: {ui_font};\n}}\n\n\
.completion list > row {{\n  border-radius: {r}px;\n  padding: 2px 8px;\n}}\n\n\
.completion list > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.completion list > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.picker {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  padding: 8px 10px 6px 10px;\n}}\n\n\
.picker entry {{\n  font-size: {picker_size}px;\n  padding-bottom: 8px;\n}}\n\n\
.picker listview {{\n  background-color: transparent;\n  color: {text};\n  margin-top: 6px;\n}}\n\n\
.picker listview > row {{\n  border-radius: {r}px;\n  padding: 4px 8px;\n}}\n\n\
.picker listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.picker listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.picker-detail {{\n  color: {muted};\n}}\n\n\
.picker listview > row:selected .picker-detail {{\n  color: alpha({bg}, 0.65);\n}}\n\n\
tooltip {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n}}\n",
            bg = c.background,
            surface = c.surface,
            overlay = c.overlay,
            text = c.text,
            accent = c.accent,
            focus = c.focus,
            border = c.border,
            danger = c.danger,
            muted = c.muted,
            editor_font = t.editor_font,
            ui_font = t.ui_font,
            picker_size = t.font_size + 4,
            small = t.font_size.saturating_sub(1),
            back_size = t.font_size + 1,
            rail_size = t.font_size + 6,
        )
    }
}
