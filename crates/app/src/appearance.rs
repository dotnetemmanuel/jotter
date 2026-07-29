//! The user's look, laid over a resolved theme.
//!
//! `crates/theming` stays pure: it knows what a theme file says, not what this
//! user overrode. Settings changes are applied here instead, so an untouched
//! config produces exactly the theme the file describes.

use jotter_theming::{Mode, Theme, ThemeFile};

use crate::config::Appearance;

/// Smallest and largest font size the settings window offers.
pub const MIN_SIZE: u32 = 8;
pub const MAX_SIZE: u32 = 32;

/// Applies the user's choices to `theme`, leaving untouched fields alone.
pub fn apply(theme: &mut Theme, appearance: &Appearance) {
    if let Some(font) = font_of(appearance.editor_font.as_deref()) {
        theme.typography.editor_font = font;
    }
    if let Some(font) = font_of(appearance.preview_font.as_deref()) {
        theme.typography.preview_font = font;
    }
    if let Some(size) = size_of(appearance.editor_size) {
        theme.typography.font_size = size;
    }
}

/// A font name worth applying: a blank one means the user cleared the field.
fn font_of(name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// A size worth applying, clamped to what the window can offer.
fn size_of(size: Option<u32>) -> Option<u32> {
    Some(size?.clamp(MIN_SIZE, MAX_SIZE))
}

/// The mode the config asks for, defaulting to the theme's own default.
#[must_use]
pub fn mode_of(appearance: &Appearance) -> Mode {
    match appearance.mode.as_deref() {
        Some("light") => Mode::Light,
        Some("dark") => Mode::Dark,
        _ => jotter_theming::bundled::DEFAULT_MODE,
    }
}

/// How a mode is written in the config.
#[must_use]
pub fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    }
}

/// Resolves `file` for `appearance`, then applies the user's overrides.
///
/// # Errors
/// Returns the theming error if the file cannot be resolved for that mode.
pub fn resolve(file: &ThemeFile, appearance: &Appearance) -> Result<Theme, jotter_theming::ThemeError> {
    let mut theme = file.resolve(mode_of(appearance))?;
    apply(&mut theme, appearance);
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::{MAX_SIZE, MIN_SIZE, apply, mode_name, mode_of};
    use crate::config::Appearance;
    use jotter_theming::Mode;

    fn theme() -> jotter_theming::Theme {
        jotter_theming::bundled::default_theme().unwrap()
    }

    #[test]
    fn an_empty_config_changes_nothing() {
        let untouched = theme();
        let mut applied = theme();
        apply(&mut applied, &Appearance::default());
        assert_eq!(applied.typography.editor_font, untouched.typography.editor_font);
        assert_eq!(applied.typography.font_size, untouched.typography.font_size);
    }

    #[test]
    fn a_chosen_font_wins_over_the_theme() {
        let mut applied = theme();
        apply(
            &mut applied,
            &Appearance {
                editor_font: Some("Iosevka".to_string()),
                preview_font: Some("Inter".to_string()),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.typography.editor_font, "Iosevka");
        assert_eq!(applied.typography.preview_font, "Inter");
    }

    #[test]
    fn a_cleared_font_falls_back_to_the_theme() {
        let untouched = theme();
        let mut applied = theme();
        apply(
            &mut applied,
            &Appearance {
                editor_font: Some("   ".to_string()),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.typography.editor_font, untouched.typography.editor_font);
    }

    #[test]
    fn a_size_outside_the_range_is_clamped_rather_than_obeyed() {
        let mut applied = theme();
        apply(
            &mut applied,
            &Appearance {
                editor_size: Some(500),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.typography.font_size, MAX_SIZE);

        let mut tiny = theme();
        apply(
            &mut tiny,
            &Appearance {
                editor_size: Some(1),
                ..Appearance::default()
            },
        );
        assert_eq!(tiny.typography.font_size, MIN_SIZE);
    }

    #[test]
    fn the_mode_round_trips_through_the_config() {
        let light = Appearance {
            mode: Some(mode_name(Mode::Light).to_string()),
            ..Appearance::default()
        };
        assert_eq!(mode_of(&light), Mode::Light);
        assert_eq!(mode_of(&Appearance::default()), jotter_theming::bundled::DEFAULT_MODE);
    }

    #[test]
    fn an_unknown_mode_falls_back_rather_than_failing() {
        let nonsense = Appearance {
            mode: Some("sepia".to_string()),
            ..Appearance::default()
        };
        assert_eq!(mode_of(&nonsense), jotter_theming::bundled::DEFAULT_MODE);
    }
}
