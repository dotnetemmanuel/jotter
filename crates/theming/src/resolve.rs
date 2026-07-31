//! Resolve a [`ThemeFile`] for one [`Mode`] into a concrete [`Theme`].
//!
//! Palette values are validated as `#RRGGBB` colors (a `$ref` in the palette is
//! rejected: references belong in the structural sections). Then every color
//! field in the structural sections has its `$token` reference replaced with the
//! matching palette color. Non-reference strings (such as font stacks) pass
//! through untouched.

use std::collections::BTreeMap;

use crate::error::ThemeError;
use crate::model::{Chrome, Code, Editor, Mode, Preview, Style, Theme, ThemeFile};

impl ThemeFile {
    /// Resolve this theme for the given mode.
    ///
    /// # Errors
    /// Returns [`ThemeError::InvalidPaletteColor`] or
    /// [`ThemeError::PaletteRefNotAllowed`] when a palette value is malformed,
    /// and [`ThemeError::UndefinedRef`] when a structural field references a
    /// palette token that does not exist.
    pub fn resolve(&self, mode: Mode) -> Result<Theme, ThemeError> {
        let palette = match mode {
            Mode::Dark => &self.palette.dark,
            Mode::Light => &self.palette.light,
        };
        validate_palette(palette, mode.as_str())?;

        let mut chrome = self.chrome.clone();
        let mut editor = self.editor.clone();
        let mut preview = self.preview.clone();
        let mut code = self.code.clone();

        resolve_fields(chrome.color_fields_mut(), palette)?;
        resolve_fields(editor.color_fields_mut(), palette)?;
        resolve_fields(preview.color_fields_mut(), palette)?;
        resolve_fields(code.color_fields_mut(), palette)?;

        Ok(Theme {
            id: self.id.clone(),
            label: self.label.clone(),
            mode,
            style: Style::Classic,
            chrome,
            editor,
            preview,
            code,
            typography: self.typography.clone(),
        })
    }
}

/// Check that every palette value is a concrete `#RRGGBB` color.
fn validate_palette(
    palette: &BTreeMap<String, String>,
    mode: &'static str,
) -> Result<(), ThemeError> {
    for (token, value) in palette {
        if value.starts_with('$') {
            return Err(ThemeError::PaletteRefNotAllowed {
                mode,
                token: token.clone(),
                value: value.clone(),
            });
        }
        if !is_hex_color(value) {
            return Err(ThemeError::InvalidPaletteColor {
                mode,
                token: token.clone(),
                value: value.clone(),
            });
        }
    }
    Ok(())
}

/// Replace every `$token` reference in the given fields with its palette color.
fn resolve_fields(
    fields: Vec<(String, &mut String)>,
    palette: &BTreeMap<String, String>,
) -> Result<(), ThemeError> {
    for (field, slot) in fields {
        if let Some(token) = slot.strip_prefix('$') {
            let color = palette.get(token).ok_or_else(|| ThemeError::UndefinedRef {
                field: field.clone(),
                token: token.to_string(),
            })?;
            *slot = color.clone();
        }
    }
    Ok(())
}

fn is_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Chrome {
    /// The color-bearing fields, keyed by dotted path for error messages.
    fn color_fields_mut(&mut self) -> Vec<(String, &mut String)> {
        vec![
            ("chrome.background".into(), &mut self.background),
            ("chrome.surface".into(), &mut self.surface),
            ("chrome.overlay".into(), &mut self.overlay),
            ("chrome.text".into(), &mut self.text),
            ("chrome.muted".into(), &mut self.muted),
            ("chrome.accent".into(), &mut self.accent),
            ("chrome.focus".into(), &mut self.focus),
            ("chrome.border".into(), &mut self.border),
            ("chrome.danger".into(), &mut self.danger),
        ]
    }
}

impl Editor {
    fn color_fields_mut(&mut self) -> Vec<(String, &mut String)> {
        vec![
            ("editor.background".into(), &mut self.background),
            ("editor.foreground".into(), &mut self.foreground),
            ("editor.cursor".into(), &mut self.cursor),
            ("editor.current_line".into(), &mut self.current_line),
            ("editor.selection".into(), &mut self.selection),
            ("editor.right_margin".into(), &mut self.right_margin),
            ("editor.line_number".into(), &mut self.line_number),
            (
                "editor.line_number_current".into(),
                &mut self.line_number_current,
            ),
            ("editor.syntax.heading".into(), &mut self.syntax.heading),
            ("editor.syntax.emphasis".into(), &mut self.syntax.emphasis),
            ("editor.syntax.strong".into(), &mut self.syntax.strong),
            ("editor.syntax.link".into(), &mut self.syntax.link),
            (
                "editor.syntax.inline_code".into(),
                &mut self.syntax.inline_code,
            ),
            (
                "editor.syntax.block_quote".into(),
                &mut self.syntax.block_quote,
            ),
            (
                "editor.syntax.list_marker".into(),
                &mut self.syntax.list_marker,
            ),
            (
                "editor.syntax.thematic_break".into(),
                &mut self.syntax.thematic_break,
            ),
            ("editor.syntax.comment".into(), &mut self.syntax.comment),
        ]
    }
}

impl Preview {
    fn color_fields_mut(&mut self) -> Vec<(String, &mut String)> {
        vec![
            ("preview.background".into(), &mut self.background),
            ("preview.foreground".into(), &mut self.foreground),
            ("preview.heading".into(), &mut self.heading),
            ("preview.link".into(), &mut self.link),
            ("preview.muted".into(), &mut self.muted),
            ("preview.code_background".into(), &mut self.code_background),
            ("preview.quote_border".into(), &mut self.quote_border),
            ("preview.rule".into(), &mut self.rule),
            ("preview.table_border".into(), &mut self.table_border),
        ]
    }
}

impl Code {
    fn color_fields_mut(&mut self) -> Vec<(String, &mut String)> {
        vec![
            ("code.background".into(), &mut self.background),
            ("code.foreground".into(), &mut self.foreground),
            ("code.keyword".into(), &mut self.keyword),
            ("code.string".into(), &mut self.string),
            ("code.comment".into(), &mut self.comment),
            ("code.function".into(), &mut self.function),
            ("code.number".into(), &mut self.number),
            ("code.type".into(), &mut self.type_name),
            ("code.variable".into(), &mut self.variable),
        ]
    }
}
