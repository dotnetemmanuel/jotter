//! The user's look, laid over a resolved theme.
//!
//! `crates/theming` stays pure: it knows what a theme file says, not what this
//! user overrode. Settings changes are applied here instead, so an untouched
//! config produces exactly the theme the file describes.

use jotter_theming::{Mode, Style, Theme, ThemeFile};

use crate::config::Appearance;

/// Smallest and largest font size the settings window offers.
pub const MIN_SIZE: u32 = 8;
pub const MAX_SIZE: u32 = 32;

/// Applies the user's choices to `theme`, leaving untouched fields alone.
pub fn apply(theme: &mut Theme, appearance: &Appearance) {
    theme.style = style_of(appearance);
    if theme.style == Style::Tui {
        // The theme's own font: the rail draws Nerd Font glyphs a chosen font may lack.
        theme.typography.ui_font = theme.typography.editor_font.clone();
    }
    if let Some(font) = font_of(appearance.editor_font.as_deref()) {
        theme.typography.editor_font = font;
    }
    if let Some(font) = font_of(appearance.preview_font.as_deref()) {
        theme.typography.preview_font = font;
    }
    if let Some(size) = size_of(appearance.font_size) {
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

/// How many size steps a scroll gesture has earned, keeping the remainder.
///
/// A wheel notch reports about 1.0, but a touchpad reports a stream of small
/// deltas, so stepping on every event makes Ctrl+scroll wildly sensitive.
/// Accumulating means one step per notch either way. A change of direction
/// drops the remainder, so a nudge back does not fire immediately.
#[must_use]
pub fn scroll_steps(carried: f64, delta: f64) -> (i32, f64) {
    let total = if carried.signum() == delta.signum() {
        carried + delta
    } else {
        delta
    };
    let steps = total.trunc();
    #[allow(clippy::cast_possible_truncation)]
    (steps as i32, total - steps)
}

/// One step up or down from `size`, staying inside the range.
#[must_use]
pub fn step(size: u32, up: bool) -> u32 {
    let next = if up { size + 1 } else { size.saturating_sub(1) };
    next.clamp(MIN_SIZE, MAX_SIZE)
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

/// The style the config asks for, defaulting to classic.
#[must_use]
pub fn style_of(appearance: &Appearance) -> Style {
    match appearance.style.as_deref() {
        Some("tui") => Style::Tui,
        _ => Style::Classic,
    }
}

/// How a style is written in the config.
#[must_use]
pub fn style_name(style: Style) -> &'static str {
    style.as_str()
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
    use jotter_theming::{Mode, Style};

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
                font_size: Some(500),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.typography.font_size, MAX_SIZE);

        let mut tiny = theme();
        apply(
            &mut tiny,
            &Appearance {
                font_size: Some(1),
                ..Appearance::default()
            },
        );
        assert_eq!(tiny.typography.font_size, MIN_SIZE);
    }

    #[test]
    fn one_size_serves_the_editor_and_the_preview() {
        let mut applied = theme();
        apply(
            &mut applied,
            &Appearance {
                font_size: Some(17),
                ..Appearance::default()
            },
        );
        // The preview CSS reads the same field, so they cannot drift apart.
        assert_eq!(applied.typography.font_size, 17);
    }

    #[test]
    fn a_wheel_notch_is_one_step() {
        assert_eq!(super::scroll_steps(0.0, 1.0), (1, 0.0));
        assert_eq!(super::scroll_steps(0.0, -1.0), (-1, 0.0));
    }

    #[test]
    fn touchpad_dribble_adds_up_to_one_step() {
        let (steps, carried) = super::scroll_steps(0.0, 0.3);
        assert_eq!(steps, 0);
        let (steps, carried) = super::scroll_steps(carried, 0.3);
        assert_eq!(steps, 0);
        let (steps, _) = super::scroll_steps(carried, 0.5);
        assert_eq!(steps, 1, "three nudges of a third should be one step");
    }

    #[test]
    fn turning_around_drops_what_was_carried() {
        let (_, carried) = super::scroll_steps(0.0, 0.9);
        let (steps, _) = super::scroll_steps(carried, -0.2);
        assert_eq!(steps, 0, "a nudge back should not fire a step");
    }

    #[test]
    fn a_hard_flick_can_earn_several_steps() {
        assert_eq!(super::scroll_steps(0.0, 3.0), (3, 0.0));
    }

    #[test]
    fn stepping_stops_at_the_ends_of_the_range() {
        assert_eq!(super::step(14, true), 15);
        assert_eq!(super::step(14, false), 13);
        assert_eq!(super::step(MAX_SIZE, true), MAX_SIZE);
        assert_eq!(super::step(MIN_SIZE, false), MIN_SIZE);
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

    #[test]
    fn the_style_round_trips_through_the_config() {
        let tui = Appearance {
            style: Some(super::style_name(Style::Tui).to_string()),
            ..Appearance::default()
        };
        assert_eq!(super::style_of(&tui), Style::Tui);
        assert_eq!(super::style_of(&Appearance::default()), Style::Classic);
    }

    #[test]
    fn an_unknown_style_falls_back_rather_than_failing() {
        let nonsense = Appearance {
            style: Some("ansi-art".to_string()),
            ..Appearance::default()
        };
        assert_eq!(super::style_of(&nonsense), Style::Classic);
    }

    #[test]
    fn the_tui_ui_font_is_the_theme_editor_font_not_the_chosen_one() {
        let mut applied = theme();
        let theme_editor_font = applied.typography.editor_font.clone();
        apply(
            &mut applied,
            &Appearance {
                style: Some("tui".to_string()),
                editor_font: Some("Iosevka".to_string()),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.style, Style::Tui);
        assert_eq!(applied.typography.ui_font, theme_editor_font);
        assert_eq!(applied.typography.editor_font, "Iosevka");
    }

    #[test]
    fn classic_leaves_the_ui_font_alone() {
        let untouched = theme();
        let mut applied = theme();
        apply(
            &mut applied,
            &Appearance {
                editor_font: Some("Iosevka".to_string()),
                ..Appearance::default()
            },
        );
        assert_eq!(applied.style, Style::Classic);
        assert_eq!(applied.typography.ui_font, untouched.typography.ui_font);
    }
}
