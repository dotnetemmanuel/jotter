//! Themes shipped inside the binary via `include_str!`.
//!
//! These are the fallback set. At runtime jotter also scans the user themes
//! folder; a user theme with the same id overrides the bundled one.

use crate::error::ThemeError;
use crate::model::{Mode, Theme, ThemeFile};

/// A theme bundled into the binary: its id and raw JSONC source.
pub struct BundledTheme {
    /// The theme id, matching the `id` field inside the source.
    pub id: &'static str,
    /// The raw JSONC source, ready for [`ThemeFile::from_jsonc`].
    pub source: &'static str,
}

/// Every theme shipped with jotter, in display order.
pub const BUNDLED: &[BundledTheme] = &[
    BundledTheme {
        id: "retro82",
        source: include_str!("../../../resources/themes/retro82.json"),
    },
    BundledTheme {
        id: "event-horizon",
        source: include_str!("../../../resources/themes/event-horizon.json"),
    },
];

/// The id of the default theme.
pub const DEFAULT_ID: &str = "retro82";

/// The mode of the default theme.
pub const DEFAULT_MODE: Mode = Mode::Dark;

/// Load and resolve the default theme (retro82, dark).
///
/// # Errors
/// Returns a [`ThemeError`] if the bundled default theme fails to parse or
/// resolve, which would be a build-time regression in the shipped JSON.
pub fn default_theme() -> Result<Theme, ThemeError> {
    default_theme_file()?.resolve(DEFAULT_MODE)
}

/// Parse the bundled default theme file (retro82) without resolving a mode, so
/// the caller can re-resolve the other mode on a light/dark switch.
///
/// # Errors
/// Returns a [`ThemeError`] if the bundled default theme fails to parse, which
/// would be a build-time regression in the shipped JSON.
pub fn default_theme_file() -> Result<ThemeFile, ThemeError> {
    ThemeFile::from_jsonc(BUNDLED[0].source)
}
