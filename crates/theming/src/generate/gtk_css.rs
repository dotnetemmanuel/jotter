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
headerbar {{\n  background-color: {surface};\n  background-image: none;\n  color: {text};\n  border-bottom: {bw}px solid {border};\n  box-shadow: none;\n}}\n\n\
.sidebar {{\n  background-color: {surface};\n  border-right: {bw}px solid {border};\n}}\n\n\
.sidebar listview {{\n  background-color: transparent;\n  color: {text};\n}}\n\n\
.sidebar listview > row {{\n  border-radius: {r}px;\n  padding: 3px 8px;\n  margin: 1px 6px;\n}}\n\n\
.sidebar listview > row:hover {{\n  background-color: {overlay};\n}}\n\n\
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
entry {{\n  background-color: transparent;\n  background-image: none;\n  color: {text};\n  border: none;\n  border-bottom: {bw}px solid {border};\n  border-radius: 0;\n  box-shadow: none;\n  padding: 6px 2px;\n}}\n\n\
entry:focus-within {{\n  border-bottom-color: {accent};\n}}\n\n\
popover > contents {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: none;\n}}\n\n\
tooltip {{\n  background-color: {surface};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n}}\n",
            name = self.scheme_name(),
            bg = c.background,
            surface = c.surface,
            overlay = c.overlay,
            text = c.text,
            accent = c.accent,
            border = c.border,
            danger = c.danger,
            ui_font = t.ui_font,
            size = t.font_size,
        )
    }
}
