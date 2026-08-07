//! Snapshot tests for the three generator outputs.

use jotter_theming::{Mode, Style, Theme, ThemeFile};

fn resolve(id: &str, mode: Mode) -> Theme {
    let src = jotter_theming::bundled::BUNDLED
        .iter()
        .find(|b| b.id == id)
        .unwrap_or_else(|| panic!("theme {id} is bundled"))
        .source;
    ThemeFile::from_jsonc(src)
        .expect("theme parses")
        .resolve(mode)
        .expect("theme resolves")
}

#[test]
fn retro82_dark_gtk_css() {
    insta::assert_snapshot!(resolve("retro82", Mode::Dark).to_gtk_css());
}

#[test]
fn retro82_dark_sourceview_scheme() {
    insta::assert_snapshot!(resolve("retro82", Mode::Dark).to_sourceview_scheme_xml());
}

#[test]
fn retro82_dark_preview_css() {
    insta::assert_snapshot!(resolve("retro82", Mode::Dark).to_preview_css());
}

#[test]
fn retro82_light_gtk_css() {
    insta::assert_snapshot!(resolve("retro82", Mode::Light).to_gtk_css());
}

#[test]
fn retro82_light_preview_css() {
    insta::assert_snapshot!(resolve("retro82", Mode::Light).to_preview_css());
}

#[test]
fn event_horizon_dark_gtk_css() {
    insta::assert_snapshot!(resolve("event-horizon", Mode::Dark).to_gtk_css());
}

#[test]
fn event_horizon_dark_sourceview_scheme() {
    insta::assert_snapshot!(resolve("event-horizon", Mode::Dark).to_sourceview_scheme_xml());
}

#[test]
fn event_horizon_dark_preview_css() {
    insta::assert_snapshot!(resolve("event-horizon", Mode::Dark).to_preview_css());
}

#[test]
fn retro82_dark_tui_gtk_css() {
    insta::assert_snapshot!(
        resolve("retro82", Mode::Dark)
            .with_style(Style::Tui)
            .to_gtk_css()
    );
}

#[test]
fn event_horizon_dark_tui_gtk_css() {
    insta::assert_snapshot!(
        resolve("event-horizon", Mode::Dark)
            .with_style(Style::Tui)
            .to_gtk_css()
    );
}
