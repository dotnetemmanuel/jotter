//! Serde model for theme files and the resolved theme the generators consume.
//!
//! A theme file carries two palettes (dark and light) plus structural sections
//! whose color fields are `$token` references into the active palette. Resolving
//! a file for one [`Mode`] rewrites every reference to a concrete color and
//! yields a [`Theme`], which is what the generators turn into CSS and XML.

use std::collections::BTreeMap;

use serde::Deserialize;

/// A theme mode: which of the two palettes is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The dark palette.
    Dark,
    /// The light palette.
    Light,
}

impl Mode {
    /// The lowercase name used in ids, scheme names, and error messages.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }
}

fn default_mode() -> Mode {
    Mode::Dark
}

/// A theme file as it sits on disk: metadata, both palettes, and structural
/// sections with unresolved `$token` references.
#[derive(Debug, Clone, Deserialize)]
pub struct ThemeFile {
    /// Stable identifier, unique within the themes folder.
    pub id: String,
    /// Human-readable name shown in the settings dropdown.
    pub label: String,
    /// Mode selected when the theme is first chosen.
    #[serde(default = "default_mode")]
    pub default_mode: Mode,
    /// The dark and light color palettes.
    pub palette: Palettes,
    /// Application chrome (window, header, sidebar, controls).
    pub chrome: Chrome,
    /// Source editor colors.
    pub editor: Editor,
    /// Rendered-preview colors.
    pub preview: Preview,
    /// Fenced code block colors.
    pub code: Code,
    /// Fonts and metrics, shared across modes.
    pub typography: Typography,
}

/// The two palettes carried by a theme file.
#[derive(Debug, Clone, Deserialize)]
pub struct Palettes {
    /// Token to `#RRGGBB` map for the dark mode.
    pub dark: BTreeMap<String, String>,
    /// Token to `#RRGGBB` map for the light mode.
    pub light: BTreeMap<String, String>,
}

/// Application chrome colors and neo-brutalist geometry.
#[derive(Debug, Clone, Deserialize)]
pub struct Chrome {
    /// Window background.
    pub background: String,
    /// Raised surface (sidebar, header).
    pub surface: String,
    /// Higher raised surface (popovers, hover).
    pub overlay: String,
    /// Primary text.
    pub text: String,
    /// De-emphasized text.
    pub muted: String,
    /// Accent used for active and primary controls.
    pub accent: String,
    /// Focus ring color.
    pub focus: String,
    /// Border color for the thick neo-brutalist outlines.
    pub border: String,
    /// Destructive-action color.
    pub danger: String,
    /// Corner radius in pixels.
    pub radius: u32,
    /// Border thickness in pixels.
    pub border_width: u32,
    /// Hard drop-shadow offset in pixels (no blur).
    pub shadow_offset: u32,
}

/// Source editor colors.
#[derive(Debug, Clone, Deserialize)]
pub struct Editor {
    /// Editor background.
    pub background: String,
    /// Default text color.
    pub foreground: String,
    /// Caret color.
    pub cursor: String,
    /// Current-line highlight.
    pub current_line: String,
    /// Selection background.
    pub selection: String,
    /// Right-margin guide.
    pub right_margin: String,
    /// Line number color.
    pub line_number: String,
    /// Current line number color.
    pub line_number_current: String,
    /// Markdown syntax highlighting colors.
    pub syntax: EditorSyntax,
}

/// Markdown syntax colors for the source editor.
#[derive(Debug, Clone, Deserialize)]
pub struct EditorSyntax {
    /// Headings.
    pub heading: String,
    /// Emphasis (italic).
    pub emphasis: String,
    /// Strong emphasis (bold).
    pub strong: String,
    /// Link text and URLs.
    pub link: String,
    /// Inline code spans.
    pub inline_code: String,
    /// Block quotes.
    pub block_quote: String,
    /// List markers.
    pub list_marker: String,
    /// Thematic breaks (horizontal rules).
    pub thematic_break: String,
    /// Comments (for example HTML comments).
    pub comment: String,
}

/// Rendered-preview colors.
#[derive(Debug, Clone, Deserialize)]
pub struct Preview {
    /// Page background.
    pub background: String,
    /// Body text.
    pub foreground: String,
    /// Heading text.
    pub heading: String,
    /// Link color.
    pub link: String,
    /// De-emphasized text.
    pub muted: String,
    /// Inline and block code background.
    pub code_background: String,
    /// Block quote left border.
    pub quote_border: String,
    /// Horizontal rule color.
    pub rule: String,
    /// Table border color.
    pub table_border: String,
}

/// Fenced code block colors.
#[derive(Debug, Clone, Deserialize)]
pub struct Code {
    /// Code block background.
    pub background: String,
    /// Default code foreground.
    pub foreground: String,
    /// Keywords.
    pub keyword: String,
    /// String literals.
    pub string: String,
    /// Comments.
    pub comment: String,
    /// Function names.
    pub function: String,
    /// Numeric literals.
    pub number: String,
    /// Type names.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Variables and identifiers.
    pub variable: String,
}

/// Fonts and metrics, shared across both modes.
#[derive(Debug, Clone, Deserialize)]
pub struct Typography {
    /// UI font family stack.
    pub ui_font: String,
    /// Source editor font family stack.
    pub editor_font: String,
    /// Preview body font family stack.
    pub preview_font: String,
    /// Monospace font family stack.
    pub mono_font: String,
    /// Base font size in pixels.
    pub font_size: u32,
    /// Body line height multiplier.
    pub line_height: f32,
}

/// A theme resolved for one [`Mode`]: every color field is now a concrete
/// `#RRGGBB` value, ready for the generators.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Source file id.
    pub id: String,
    /// Human-readable name.
    pub label: String,
    /// The mode this theme was resolved for.
    pub mode: Mode,
    /// Resolved chrome colors and geometry.
    pub chrome: Chrome,
    /// Resolved editor colors.
    pub editor: Editor,
    /// Resolved preview colors.
    pub preview: Preview,
    /// Resolved code colors.
    pub code: Code,
    /// Fonts and metrics.
    pub typography: Typography,
}

impl Theme {
    /// Scheme id combining the theme id and mode, for example `retro82-dark`.
    #[must_use]
    pub fn scheme_id(&self) -> String {
        format!("{}-{}", self.id, self.mode.as_str())
    }

    /// Human-readable scheme name, for example `retro82 dark`.
    #[must_use]
    pub fn scheme_name(&self) -> String {
        format!("{} {}", self.label, self.mode.as_str())
    }
}
