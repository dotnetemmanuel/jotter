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
        let r = self.chrome.radius;
        let bw = self.chrome.border_width;
        let quote_bar = bw * 2;
        let color_scheme = match self.mode {
            Mode::Dark => "dark",
            Mode::Light => "light",
        };

        format!(
            "/* {name} */\n\
:root {{ color-scheme: {color_scheme}; }}\n\n\
body {{\n  margin: 0;\n  padding: 2.5rem 2.75rem;\n  background: {bg};\n  color: {fg};\n  font-family: {body_font};\n  font-size: {size}px;\n  line-height: {lh};\n}}\n\n\
h1, h2, h3, h4, h5, h6 {{\n  color: {heading};\n  line-height: 1.1;\n  font-weight: 800;\n  letter-spacing: -0.015em;\n  margin: 1.6em 0 0.5em;\n}}\n\n\
h1 {{\n  font-size: 2.4em;\n  margin-top: 0.2em;\n}}\n\n\
h2 {{\n  font-size: 1.7em;\n}}\n\n\
h3 {{\n  font-size: 1.32em;\n}}\n\n\
h4 {{\n  font-size: 1.12em;\n}}\n\n\
a {{\n  color: {link};\n  text-decoration-thickness: 2px;\n  text-underline-offset: 2px;\n}}\n\n\
a.broken-link, a[href^=\"jotter-new:\"] {{\n  color: {muted};\n  text-decoration: underline dashed;\n}}\n\n\
code {{\n  font-family: {mono_font};\n  background: {code_bg};\n  padding: 0.15em 0.35em;\n  border-radius: {r}px;\n}}\n\n\
pre {{\n  background: {code_bg};\n  padding: 1rem;\n  border: {bw}px solid {table_border};\n  border-radius: {r}px;\n  overflow: auto;\n}}\n\n\
pre code {{\n  background: transparent;\n  padding: 0;\n}}\n\n\
blockquote {{\n  margin: 1rem 0;\n  padding: 0.25rem 1rem;\n  border-left: {quote_bar}px solid {quote_border};\n  color: {muted};\n}}\n\n\
hr {{\n  border: none;\n  border-top: {bw}px solid {rule};\n  margin: 2rem 0;\n}}\n\n\
table {{\n  border-collapse: separate;\n  border-spacing: 0;\n  margin: 1rem 0;\n  border: {bw}px solid {table_border};\n  border-radius: {r}px;\n}}\n\n\
th, td {{\n  border-right: 1px solid {table_border};\n  border-bottom: 1px solid {table_border};\n  padding: 0.5rem 0.9rem;\n  text-align: left;\n}}\n\n\
th:last-child, td:last-child {{\n  border-right: none;\n}}\n\n\
tr:last-child td {{\n  border-bottom: none;\n}}\n\n\
th {{\n  background: {code_bg};\n}}\n\n\
th:first-child {{\n  border-top-left-radius: {r}px;\n}}\n\n\
th:last-child {{\n  border-top-right-radius: {r}px;\n}}\n\n\
tr:last-child td:first-child {{\n  border-bottom-left-radius: {r}px;\n}}\n\n\
tr:last-child td:last-child {{\n  border-bottom-right-radius: {r}px;\n}}\n\n\
li:has(> input[type=\"checkbox\"]) {{\n  list-style: none;\n}}\n\n\
input[type=\"checkbox\"] {{\n  appearance: none;\n  -webkit-appearance: none;\n  width: 1.15em;\n  height: 1.15em;\n  margin-right: 0.5em;\n  vertical-align: -0.25em;\n  border: {bw}px solid {table_border};\n  border-radius: {r}px;\n  background-color: {code_bg};\n  background-repeat: no-repeat;\n  background-position: center;\n  background-size: 0.95em;\n}}\n\n\
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
