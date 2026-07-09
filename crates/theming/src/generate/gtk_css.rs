//! Generate the GTK4 chrome stylesheet for `gtk::CssProvider::load_from_data`.
//!
//! The look is neo-brutalist with softened corners: thick borders, hard offset
//! drop-shadows (no blur), and a shared corner radius. Pressing a control drops
//! its shadow so it reads as physically pushed in.

use crate::model::Theme;

impl Theme {
    /// Render the application chrome as GTK4 CSS.
    #[must_use]
    pub fn to_gtk_css(&self) -> String {
        let c = &self.chrome;
        let t = &self.typography;
        let r = c.radius;
        let bw = c.border_width;
        let so = c.shadow_offset;

        format!(
            "/* {name} */\n\
window {{\n  background-color: {bg};\n  color: {text};\n  font-family: {ui_font};\n  font-size: {size}px;\n}}\n\n\
headerbar {{\n  background-color: {surface};\n  background-image: none;\n  color: {text};\n  border-bottom: {bw}px solid {border};\n  box-shadow: none;\n}}\n\n\
.sidebar {{\n  background-color: {surface};\n  border-right: {bw}px solid {border};\n}}\n\n\
button {{\n  background-color: {surface};\n  background-image: none;\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: {so}px {so}px 0 {border};\n  padding: 4px 10px;\n}}\n\n\
button:hover {{\n  background-color: {overlay};\n}}\n\n\
button:active {{\n  box-shadow: 0 0 0 {border};\n}}\n\n\
button.suggested-action {{\n  background-color: {accent};\n  background-image: none;\n  color: {bg};\n}}\n\n\
button.suggested-action:hover {{\n  background-color: {accent};\n}}\n\n\
button.destructive-action {{\n  background-color: {danger};\n  background-image: none;\n  color: {bg};\n}}\n\n\
button.destructive-action:hover {{\n  background-color: {danger};\n}}\n\n\
entry {{\n  background-color: {bg};\n  background-image: none;\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  padding: 4px 8px;\n}}\n\n\
entry:focus-within {{\n  border-color: {focus};\n  box-shadow: {so}px {so}px 0 {focus};\n}}\n\n\
popover > contents {{\n  background-color: {overlay};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n  box-shadow: {so}px {so}px 0 {border};\n}}\n\n\
tooltip {{\n  background-color: {overlay};\n  color: {text};\n  border: {bw}px solid {border};\n  border-radius: {r}px;\n}}\n",
            name = self.scheme_name(),
            bg = c.background,
            surface = c.surface,
            overlay = c.overlay,
            text = c.text,
            accent = c.accent,
            focus = c.focus,
            border = c.border,
            danger = c.danger,
            ui_font = t.ui_font,
            size = t.font_size,
        )
    }
}
