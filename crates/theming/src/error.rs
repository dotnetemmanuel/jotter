//! Typed errors for loading and resolving themes.

use thiserror::Error;

/// Everything that can go wrong turning theme JSON into a resolved theme.
#[derive(Debug, Error)]
pub enum ThemeError {
    /// The JSON (after comment stripping) failed to parse or did not match the schema.
    #[error("theme JSON is invalid: {0}")]
    Parse(#[from] serde_json::Error),

    /// A palette token holds a `$ref` instead of a concrete color. References may
    /// only appear in the structural sections, never in the palette itself.
    #[error("palette token `{token}` in the {mode} palette must be a color, got `{value}`")]
    PaletteRefNotAllowed {
        /// The mode whose palette holds the offending token (`dark` or `light`).
        mode: &'static str,
        /// The palette token name.
        token: String,
        /// The offending value.
        value: String,
    },

    /// A palette value is not a `#RRGGBB` color.
    #[error("palette token `{token}` in the {mode} palette is not a #RRGGBB color, got `{value}`")]
    InvalidPaletteColor {
        /// The mode whose palette holds the offending token (`dark` or `light`).
        mode: &'static str,
        /// The palette token name.
        token: String,
        /// The offending value.
        value: String,
    },

    /// A structural field references a palette token that does not exist.
    #[error("field `{field}` references undefined palette token `${token}`")]
    UndefinedRef {
        /// The dotted path of the field that holds the bad reference.
        field: String,
        /// The token name that could not be resolved.
        token: String,
    },
}
