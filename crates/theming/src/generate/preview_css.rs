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
table {{\n  border-collapse: collapse;\n}}\n\n\
th, td {{\n  border: 1px solid {table_border};\n  padding: 0.4rem 0.6rem;\n}}\n\n\
img {{\n  max-width: 100%;\n}}\n",
            name = self.scheme_name(),
            bg = p.background,
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
