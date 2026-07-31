//! Generate the GTK4 chrome stylesheet for the TUI style.
//!
//! A terminal look: square corners, hairline frames drawn in the focus color,
//! and a monospace UI font. `chrome.radius`, `chrome.border_width`,
//! `chrome.surface` and `chrome.border` are deliberately unused here.

use crate::model::Theme;

impl Theme {
    /// The terminal-leaning style.
    pub(super) fn tui_css(&self) -> String {
        let c = &self.chrome;
        let t = &self.typography;

        format!(
            "/* {name} tui */\n\
window {{\n  background-color: {bg};\n  color: {text};\n  font-family: {ui_font};\n  font-size: {size}px;\n}}\n\n\
headerbar {{\n  background-color: {bg};\n  background-image: none;\n  color: {text};\n  border-bottom: 1px solid {focus};\n  box-shadow: none;\n  padding-left: 0;\n}}\n\n\
separator {{\n  background-color: alpha({focus}, 0.45);\n  min-height: 1px;\n  min-width: 1px;\n}}\n\n\
paned > separator {{\n  background-color: alpha({focus}, 0.45);\n  background-image: none;\n  min-width: 1px;\n  min-height: 1px;\n}}\n\n\
.sidebar {{\n  background-color: {bg};\n}}\n\n\
.sidebar listview {{\n  background-color: transparent;\n  color: {text};\n  padding-top: 2px;\n}}\n\n\
.sidebar listview > row {{\n  border-radius: 0;\n  padding: 1px 6px;\n  margin: 0;\n}}\n\n\
.sidebar listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.sidebar listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.sidebar listview > row:selected:hover {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.vault-name {{\n  color: {focus};\n  font-size: {small}px;\n  font-weight: bold;\n  padding: 6px 8px 2px 8px;\n  border-bottom: 1px solid alpha({focus}, 0.45);\n}}\n\n\
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
row:focus, row:focus-visible, listview:focus-visible, listbox:focus-visible {{\n  outline: none;\n}}\n\n\
scrollbar {{\n  background-color: transparent;\n  border: none;\n}}\n\n\
scrollbar slider {{\n  background-color: {muted};\n  border: none;\n  border-radius: 0;\n  min-width: 6px;\n  min-height: 6px;\n  margin: 0;\n}}\n\n\
scrollbar slider:hover {{\n  background-color: {focus};\n}}\n\n\
popover > contents {{\n  background-color: {bg};\n  color: {text};\n  border: 1px solid {focus};\n  border-radius: 0;\n  box-shadow: none;\n}}\n\n\
.picker-scrim {{\n  background-color: alpha({bg}, 0.6);\n}}\n",
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

    /// Stub for Task 2, which will read `self` to dress jotter's own widgets.
    #[allow(clippy::unused_self)]
    fn tui_parts_css(&self) -> String {
        String::new()
    }
}
