//! Generate the GTK4 chrome stylesheet for `gtk::CssProvider::load_from_data`.
//!
//! The look is Bauhaus-leaning: flat surfaces (no drop-shadows), crisp small
//! corners, and thick lines used structurally rather than as boxes. Text fields
//! are underlines, not framed boxes; the primary button is a solid high-contrast
//! block that pops the accent on hover; the selected tree row is a flat accent
//! block. Depth comes from color and line, not shadow.

use crate::model::Theme;

impl Theme {
    /// Render the application chrome as GTK4 CSS.
    #[must_use]
    pub fn to_gtk_css(&self) -> String {
        let c = &self.chrome;
        let t = &self.typography;
        let r = c.radius;
        let bw = c.border_width;

        format!(
            "/* {name} */\n\
window {{\n  background-color: {bg};\n  color: {text};\n  font-family: {ui_font};\n  font-size: {size}px;\n}}\n\n\
headerbar {{\n  background-color: {bg};\n  background-image: none;\n  color: {text};\n  border-bottom: 1px solid {overlay};\n  box-shadow: none;\n}}\n\n\
separator {{\n  background-color: {overlay};\n  min-height: 1px;\n  min-width: 1px;\n}}\n\n\
paned > separator {{\n  background-color: {overlay};\n  background-image: none;\n  min-width: 1px;\n  min-height: 1px;\n}}\n\n\
.sidebar {{\n  background-color: {bg};\n}}\n\n\
.sidebar listview {{\n  background-color: transparent;\n  color: {text};\n  padding-top: 6px;\n}}\n\n\
.sidebar listview > row {{\n  border-radius: {r}px;\n  padding: 3px 8px;\n  margin: 1px 6px;\n}}\n\n\
.sidebar listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.tree-title {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.sidebar listview > row:selected .tree-title {{\n  color: alpha({bg}, 0.7);\n}}\n\n\
.sidebar listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.sidebar listview > row:selected:hover {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
button {{\n  background-color: {surface};\n  background-image: none;\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: none;\n  padding: 6px 14px;\n}}\n\n\
button:hover {{\n  background-color: {overlay};\n}}\n\n\
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
row:focus, row:focus-visible, listview:focus-visible, listbox:focus-visible {{\n  outline: none;\n}}\n\n\
popover > contents {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: none;\n}}\n\n\
.picker-scrim {{\n  background-color: alpha({bg}, 0.45);\n}}\n\n\
.backlinks {{\n  background-color: {bg};\n}}\n\n\
.backlinks-header {{\n  background: none;\n  border: none;\n  box-shadow: none;\n  color: {muted};\n  font-size: {small}px;\n  padding: 3px 10px;\n  min-height: 0;\n}}\n\n\
.backlinks-header:hover {{\n  background: none;\n  color: {text};\n}}\n\n\
.search-results {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.search-results > row {{\n  border-radius: {r}px;\n  padding: 1px 6px;\n  margin: 0 6px;\n}}\n\n\
.search-results > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.search-results > row:selected {{\n  background-color: {overlay};\n  color: {text};\n  box-shadow: inset 2px 0 0 0 {accent};\n}}\n\n\
.search-heading {{\n  margin-top: 14px;\n  padding: 2px 2px 4px 2px;\n  border-bottom: 1px solid alpha({border}, 0.25);\n}}\n\n\
.search-results > row:first-child .search-heading {{\n  margin-top: 2px;\n}}\n\n\
.search-name {{\n  font-weight: bold;\n}}\n\n\
.search-folder {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.search-count {{\n  color: {muted};\n  font-size: {small}px;\n}}\n\n\
.search-snippet {{\n  color: {muted};\n  margin-left: 5px;\n  padding: 1px 0 1px 10px;\n  border-left: 1px solid alpha({border}, 0.18);\n}}\n\n\
.completion listbox {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.completion listbox > row {{\n  border-radius: {r}px;\n  padding: 2px 8px;\n}}\n\n\
.completion listbox > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.picker {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  padding: 8px 10px 6px 10px;\n}}\n\n\
.picker entry {{\n  font-size: {picker_size}px;\n  padding-bottom: 8px;\n}}\n\n\
.picker listview {{\n  background-color: transparent;\n  color: {text};\n  margin-top: 6px;\n}}\n\n\
.picker listview > row {{\n  border-radius: {r}px;\n  padding: 4px 8px;\n}}\n\n\
.picker listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
.picker listview > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}\n\n\
.picker-detail {{\n  color: {muted};\n}}\n\n\
.picker listview > row:selected .picker-detail {{\n  color: alpha({bg}, 0.65);\n}}\n\n\
tooltip {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n}}\n",
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
            picker_size = t.font_size + 4,
            small = t.font_size.saturating_sub(1),
        )
    }
}
