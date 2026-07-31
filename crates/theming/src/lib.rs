#![warn(clippy::pedantic)]
//! jotter theming: load JSONC theme files, resolve palette references for a
//! mode, and generate the three outputs the app needs.
//!
//! One theme file carries a dark and a light palette plus structural sections
//! whose colors are `$token` references. [`ThemeFile::from_jsonc`] parses a file,
//! [`ThemeFile::resolve`] turns it into a concrete [`Theme`] for one [`Mode`],
//! and the [`Theme`] generators produce:
//! - [`Theme::to_gtk_css`] for the application chrome,
//! - [`Theme::to_sourceview_scheme_xml`] for the editor,
//! - [`Theme::to_preview_css`] for the rendered preview.
//!
//! The crate is pure logic with no GTK dependency, so all of it is unit-testable.

pub mod bundled;
mod error;
mod generate;
mod model;
mod parse;
mod resolve;

pub use error::ThemeError;
pub use model::{
    Chrome, Code, Editor, EditorSyntax, Mode, Palettes, Preview, Style, Theme, ThemeFile,
    Typography,
};
