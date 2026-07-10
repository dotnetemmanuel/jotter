//! Generate the preview stylesheet injected via `webkit6::UserStyleSheet`.
//!
//! This styles the rendered markdown HTML: body, headings, links, code, quotes,
//! rules, and tables. The `color-scheme` follows the active mode so native form
//! controls and scrollbars match.

use crate::model::{Mode, Theme};

impl Theme {
    /// Render the preview stylesheet as CSS.
    #[must_use]
    pub fn to_preview_css(&self) -> String {
        let p = &self.preview;
        let t = &self.typography;
        let color_scheme = match self.mode {
            Mode::Dark => "dark",
            Mode::Light => "light",
        };

        format!(
            "/* {name} */\n\
:root {{ color-scheme: {color_scheme}; }}\n\n\
body {{\n  margin: 0;\n  padding: 2rem;\n  background: {bg};\n  color: {fg};\n  font-family: {body_font};\n  font-size: {size}px;\n  line-height: {lh};\n}}\n\n\
h1, h2, h3, h4, h5, h6 {{\n  color: {heading};\n  line-height: 1.25;\n}}\n\n\
a {{\n  color: {link};\n}}\n\n\
a.broken-link {{\n  color: {muted};\n  text-decoration: underline dashed;\n}}\n\n\
code {{\n  font-family: {mono_font};\n  background: {code_bg};\n  padding: 0.15em 0.35em;\n  border-radius: 6px;\n}}\n\n\
pre {{\n  background: {code_bg};\n  padding: 1rem;\n  border-radius: 10px;\n  overflow: auto;\n}}\n\n\
pre code {{\n  background: transparent;\n  padding: 0;\n}}\n\n\
blockquote {{\n  margin: 1rem 0;\n  padding: 0.25rem 1rem;\n  border-left: 4px solid {quote_border};\n  color: {muted};\n}}\n\n\
hr {{\n  border: none;\n  border-top: 2px solid {rule};\n}}\n\n\
table {{\n  border-collapse: collapse;\n  margin: 1rem 0;\n  border: 1px solid {table_border};\n}}\n\n\
th, td {{\n  border: 1px solid {table_border};\n  padding: 0.75rem 1.25rem;\n  text-align: left;\n}}\n\n\
th {{\n  background: {code_bg};\n}}\n\n\
li:has(> input[type=\"checkbox\"]) {{\n  list-style: none;\n}}\n\n\
input[type=\"checkbox\"] {{\n  appearance: none;\n  -webkit-appearance: none;\n  width: 1.15em;\n  height: 1.15em;\n  margin-right: 0.5em;\n  vertical-align: -0.25em;\n  border: 2px solid {table_border};\n  border-radius: 5px;\n  background-color: {code_bg};\n  background-repeat: no-repeat;\n  background-position: center;\n  background-size: 0.95em;\n}}\n\n\
input[type=\"checkbox\"]:checked {{\n  background-color: {link};\n  border-color: {link};\n  background-image: url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'%3E%3Cpath fill='none' stroke='%23{bg_hex}' stroke-width='2.6' stroke-linecap='round' stroke-linejoin='round' d='M4 8.5l3 3 5-6'/%3E%3C/svg%3E\");\n}}\n\n\
img {{\n  max-width: 100%;\n}}\n",
            name = self.scheme_name(),
            bg = p.background,
            bg_hex = p.background.trim_start_matches('#'),
            fg = p.foreground,
            heading = p.heading,
            link = p.link,
            muted = p.muted,
            code_bg = p.code_background,
            quote_border = p.quote_border,
            rule = p.rule,
            table_border = p.table_border,
            body_font = t.preview_font,
            mono_font = t.mono_font,
            size = t.font_size,
            lh = t.line_height,
        )
    }
}
