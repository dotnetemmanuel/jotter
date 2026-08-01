//! Generate the GTK4 chrome stylesheet for the TUI style.
//!
//! A terminal look: square corners, frames drawn in the focus color (2px where
//! one region of the app meets another, a hairline within a region), and a
//! monospace UI font. `chrome.radius`, `chrome.border_width` and `chrome.border`
//! are deliberately unused here.

use crate::model::Theme;

impl Theme {
    /// The terminal-leaning style.
    pub(super) fn tui_css(&self) -> String {
        let c = &self.chrome;
        let t = &self.typography;

        format!(
            "/* {name} tui */\n\
window {{\n  background-color: {bg};\n  color: {text};\n  font-family: {ui_font};\n  font-size: {size}px;\n}}\n\n\
window.csd {{\n  border-radius: 0;\n}}\n\n\
headerbar {{\n  background-color: {bg};\n  background-image: none;\n  color: {text};\n  border-bottom: 2px solid {focus};\n  box-shadow: none;\n  padding-left: 0;\n}}\n\n\
separator {{\n  background-color: {focus};\n  min-height: 2px;\n  min-width: 2px;\n}}\n\n\
paned > separator {{\n  background-color: {focus};\n  background-image: none;\n  min-width: 2px;\n  min-height: 2px;\n}}\n\n\
.sidebar {{\n  background-color: {bg};\n}}\n\n\
.sidebar listview {{\n  background-color: transparent;\n  color: {text};\n  padding-top: 2px;\n}}\n\n\
.sidebar listview > row {{\n  border-radius: 0;\n  padding: 1px 6px;\n  margin: 0;\n}}\n\n\
.sidebar listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.sidebar listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.sidebar listview > row:selected:hover {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.vault-name {{\n  color: {focus};\n  font-size: {small}px;\n  font-weight: bold;\n  padding: 6px 8px 4px 8px;\n  border-bottom: 2px solid {focus};\n}}\n\n\
.tree-cursor {{\n  color: {accent};\n}}\n\n\
.sidebar listview > row:selected .tree-cursor {{\n  color: {bg};\n}}\n\n\
.tree-inert {{\n  color: {muted};\n  opacity: 0.55;\n}}\n\n\
.tree-title {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.sidebar listview > row:selected .tree-title {{\n  color: alpha({bg}, 0.7);\n}}\n\n\
.tree-drop {{\n  box-shadow: inset 0 0 0 1px {accent};\n  border-radius: 0;\n}}\n\n\
.sidebar listview:drop(active), .sidebar listview > row:drop(active) {{\n  box-shadow: none;\n  outline: none;\n}}\n\n\
button {{\n  background-color: transparent;\n  background-image: none;\n  color: {focus};\n  border: none;\n  border-radius: 0;\n  box-shadow: none;\n  padding: 3px 8px;\n}}\n\n\
button:hover {{\n  background-color: {focus};\n  color: {bg};\n}}\n\n\
button:checked {{\n  background-color: {accent};\n  background-image: none;\n  color: {bg};\n}}\n\n\
button.suggested-action {{\n  background-color: transparent;\n  background-image: none;\n  color: {accent};\n}}\n\n\
button.suggested-action:hover {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
button.destructive-action {{\n  background-color: transparent;\n  background-image: none;\n  color: {danger};\n}}\n\n\
button.destructive-action:hover {{\n  background-color: {danger};\n  color: {bg};\n}}\n\n\
windowcontrols button, windowcontrols button.titlebutton {{\n  background-color: transparent;\n  background-image: none;\n  border: none;\n  box-shadow: none;\n  color: {text};\n}}\n\n\
windowcontrols button:hover, windowcontrols button.titlebutton:hover {{\n  background-color: {focus};\n  background-image: none;\n  color: {bg};\n  border-radius: 0;\n}}\n\n\
windowcontrols button image, windowcontrols button:hover image {{\n  background: none;\n  background-color: transparent;\n  background-image: none;\n  box-shadow: none;\n  color: {text};\n}}\n\n\
entry {{\n  background-color: {bg};\n  background-image: none;\n  color: {text};\n  border: 1px solid alpha({focus}, 0.55);\n  border-radius: 0;\n  box-shadow: none;\n  outline: none;\n  padding: 3px 6px;\n}}\n\n\
entry:focus-within, entry:hover, entry:focus-visible {{\n  outline: none;\n  box-shadow: none;\n}}\n\n\
entry:focus-within {{\n  border-color: {focus};\n}}\n\n\
entry > text, entry > text:focus-visible {{\n  outline: none;\n  box-shadow: none;\n}}\n\n\
row:focus, row:focus-visible, listview:focus-visible, list:focus-visible {{\n  outline: none;\n}}\n\n\
scrollbar {{\n  background-color: transparent;\n  border: none;\n}}\n\n\
scrollbar slider {{\n  background-color: {muted};\n  border: none;\n  border-radius: 0;\n  min-width: 6px;\n  min-height: 6px;\n  margin: 0;\n}}\n\n\
scrollbar slider:hover {{\n  background-color: {focus};\n}}\n\n\
popover > contents {{\n  background-color: {bg};\n  color: {text};\n  border: 1px solid {focus};\n  border-radius: 0;\n  box-shadow: none;\n}}\n\n\
.picker-scrim {{\n  background-color: alpha({bg}, 0.78);\n}}\n",
            name = self.scheme_name(),
            bg = c.background,
            overlay = c.overlay,
            text = c.text,
            muted = c.muted,
            accent = c.accent,
            focus = c.focus,
            danger = c.danger,
            ui_font = t.ui_font,
            size = t.font_size,
            small = t.font_size.saturating_sub(1),
        ) + &self.tui_parts_css()
    }

    /// The half of the stylesheet that dresses jotter's own widgets.
    fn tui_parts_css(&self) -> String {
        let c = &self.chrome;
        let t = &self.typography;

        format!(
            ".rail {{\n  background-color: {bg};\n  padding: 6px 0;\n}}\n\n\
.rail-button {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {muted};\n  padding: 8px 4px;\n  min-height: 0;\n  min-width: 0;\n  border-radius: 0;\n}}\n\n\
.rail-button label {{\n  font-family: {ui_font};\n  font-size: {rail_size}px;\n  border-bottom: none;\n}}\n\n\
.rail-button:hover {{\n  background-color: {overlay};\n  color: {text};\n}}\n\n\
.rail-button:checked {{\n  background: none;\n  color: {accent};\n  box-shadow: inset 2px 0 0 0 {accent};\n}}\n\n\
.rail-settings {{\n  padding-left: 3px;\n  padding-right: 6px;\n}}\n\n\
.font-tick {{\n  color: {accent};\n}}\n\n\
.font-list {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.font-list > row {{\n  padding: 1px 6px;\n  border-radius: 0;\n}}\n\n\
.font-list > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.font-list > row:selected {{\n  background-color: {accent};\n  color: {bg};\n  box-shadow: none;\n}}\n\n\
.settings {{\n  background-color: {bg};\n}}\n\n\
.settings-label {{\n  color: {focus};\n}}\n\n\
.settings-close {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {text};\n  padding: 3px 8px;\n  min-height: 0;\n  min-width: 0;\n}}\n\n\
.settings-close:hover {{\n  background-color: {focus};\n  color: {bg};\n}}\n\n\
.keysheet-heading {{\n  color: {focus};\n  font-size: {small}px;\n  font-weight: bold;\n}}\n\n\
.keysheet-keys {{\n  color: {muted};\n  font-family: {ui_font};\n  font-size: {small}px;\n}}\n\n\
.theme-button {{\n  padding: 4px;\n  border: 1px solid transparent;\n}}\n\n\
.theme-button:checked {{\n  background: none;\n  color: {text};\n  border-color: {accent};\n}}\n\n\
.theme-name {{\n  font-size: {small}px;\n}}\n\n\
.conflict {{\n  background-color: {bg};\n  padding: 8px 10px;\n}}\n\n\
.conflict-header {{\n  padding-bottom: 4px;\n  border-bottom: 1px solid alpha({focus}, 0.45);\n}}\n\n\
.conflict-heading {{\n  font-weight: bold;\n}}\n\n\
.conflict-progress {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.conflict-title {{\n  font-family: {ui_font};\n  font-size: {small}px;\n  font-weight: bold;\n  padding: 0 2px 2px 2px;\n  border-radius: 0;\n  border-bottom: 1px solid alpha({focus}, 0.45);\n}}\n\n\
.conflict-title.conflict-incoming {{\n  color: {focus};\n}}\n\n\
.conflict-title.conflict-yours {{\n  color: {accent};\n}}\n\n\
.conflict-title.conflict-resolution {{\n  color: {text};\n}}\n\n\
.conflict-body {{\n  font-family: {editor_font};\n  border-radius: 0;\n  padding: 4px 6px;\n  border: 1px solid alpha({focus}, 0.45);\n}}\n\n\
.conflict-body, .conflict-body text {{\n  background-color: {bg};\n  color: {text};\n}}\n\n\
.conflict-body.conflict-incoming {{\n  border-color: {focus};\n}}\n\n\
.conflict-body.conflict-yours {{\n  border-color: {accent};\n}}\n\n\
.conflict-actions {{\n  padding-top: 4px;\n}}\n\n\
.conflict-action {{\n  padding: 3px 8px;\n}}\n\n\
.status-size, .status-git, .status-broken {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  font-family: {ui_font};\n  font-size: {small}px;\n  padding: 0 6px;\n  min-height: 0;\n}}\n\n\
.status-size, .status-git {{\n  color: {muted};\n}}\n\n\
.status-broken {{\n  color: {danger};\n}}\n\n\
.status-joiner {{\n  color: alpha({muted}, 0.6);\n  font-family: {ui_font};\n  font-size: {small}px;\n  padding: 0;\n}}\n\n\
.status-size:hover, .status-git:hover, .status-broken:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.backlinks {{\n  background-color: {bg};\n}}\n\n\
.backlinks-header {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {focus};\n  font-family: {ui_font};\n  font-size: {small}px;\n  padding: 2px 8px;\n  min-height: 0;\n}}\n\n\
.backlinks-header:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.panel-bar {{\n  padding-bottom: 6px;\n  border-bottom: 2px solid {focus};\n}}\n\n\
.search-results {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.search-results > row {{\n  border-radius: 0;\n  padding: 0 6px;\n  margin: 0;\n}}\n\n\
.search-results > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.search-results > row:selected {{\n  background-color: {accent};\n  color: {bg};\n  box-shadow: none;\n}}\n\n\
.row-cursor {{\n  color: {accent};\n  font-family: {ui_font};\n}}\n\n\
.search-results > row:selected .row-cursor {{\n  color: {bg};\n}}\n\n\
.panel-back {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {focus};\n  padding: 0 4px;\n  margin: 0;\n  min-height: 0;\n  min-width: 0;\n}}\n\n\
.panel-back label {{\n  font-family: {ui_font};\n  font-size: {back_size}px;\n}}\n\n\
.panel-back:hover {{\n  background: none;\n  color: {accent};\n}}\n\n\
.tags-heading {{\n  color: {focus};\n  font-size: {small}px;\n  font-weight: bold;\n}}\n\n\
.tag-row {{\n  padding: 1px 4px;\n}}\n\n\
.search-heading {{\n  margin-top: 18px;\n  padding: 0 2px 3px 2px;\n  border-bottom: 1px solid alpha({focus}, 0.3);\n}}\n\n\
.search-results > row:first-child .search-heading {{\n  margin-top: 0;\n}}\n\n\
.search-name {{\n  font-weight: bold;\n}}\n\n\
.search-folder {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.search-count {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.search-results > row:selected .search-folder, .search-results > row:selected .search-count {{\n  color: alpha({bg}, 0.7);\n}}\n\n\
.search-snippet {{\n  color: {muted};\n  margin-left: 0;\n  padding: 0 0 0 8px;\n  border-left: 1px solid alpha({focus}, 0.3);\n}}\n\n\
.completion list {{\n  background-color: transparent;\n  color: {text};\n  font-family: {ui_font};\n}}\n\n\
.completion list > row {{\n  border-radius: 0;\n  padding: 1px 6px;\n}}\n\n\
.completion list > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.completion list > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.picker {{\n  background-color: {surface};\n  color: {text};\n  border: 1px solid {focus};\n  border-radius: 0;\n  padding: 0;\n}}\n\n\
.picker-title {{\n  color: {focus};\n  font-family: {ui_font};\n  font-size: {small}px;\n  padding: 2px 6px 2px 6px;\n  border-bottom: 1px solid alpha({focus}, 0.45);\n}}\n\n\
.picker entry {{\n  background-color: transparent;\n  font-size: {picker_size}px;\n  border: none;\n  border-bottom: 1px solid alpha({focus}, 0.45);\n  padding: 4px 6px;\n}}\n\n\
.picker listview {{\n  background-color: transparent;\n  color: {text};\n  margin-top: 0;\n}}\n\n\
.picker listview > row {{\n  border-radius: 0;\n  padding: 1px 6px;\n}}\n\n\
.picker listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.picker listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.picker-detail {{\n  color: {muted};\n}}\n\n\
.picker listview > row:selected .picker-detail {{\n  color: alpha({bg}, 0.65);\n}}\n\n\
.picker listview > row:selected .row-cursor {{\n  color: {bg};\n}}\n\n\
tooltip {{\n  background-color: {bg};\n  color: {text};\n  border: 1px solid {focus};\n  border-radius: 0;\n}}\n",
            bg = c.background,
            surface = c.surface,
            overlay = c.overlay,
            text = c.text,
            accent = c.accent,
            focus = c.focus,
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
