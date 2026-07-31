//! What the TUI stylesheet must say, beyond what a snapshot pins down.

use jotter_theming::{Mode, Style, Theme, ThemeFile};

fn tui(id: &str, mode: Mode) -> Theme {
    let src = jotter_theming::bundled::BUNDLED
        .iter()
        .find(|b| b.id == id)
        .unwrap_or_else(|| panic!("theme {id} is bundled"))
        .source;
    ThemeFile::from_jsonc(src)
        .expect("theme parses")
        .resolve(mode)
        .expect("theme resolves")
        .with_style(Style::Tui)
}

#[test]
fn a_resolved_theme_is_classic_until_asked_otherwise() {
    let theme = tui("retro82", Mode::Dark);
    assert_eq!(theme.style, Style::Tui);
    let plain = ThemeFile::from_jsonc(
        jotter_theming::bundled::BUNDLED
            .iter()
            .find(|b| b.id == "retro82")
            .unwrap()
            .source,
    )
    .unwrap()
    .resolve(Mode::Dark)
    .unwrap();
    assert_eq!(plain.style, Style::Classic);
}

#[test]
fn the_tui_sheet_has_no_rounded_corners() {
    let css = tui("retro82", Mode::Dark).to_gtk_css();
    for line in css.lines() {
        let Some(radius) = line.trim().strip_prefix("border-radius:") else {
            continue;
        };
        assert_eq!(
            radius.trim().trim_end_matches(';'),
            "0",
            "a TUI corner is not square: {line}"
        );
    }
}

#[test]
fn the_tui_sheet_draws_its_structure_in_the_focus_color() {
    let theme = tui("event-horizon", Mode::Dark);
    let focus = theme.chrome.focus.clone();
    let css = theme.to_gtk_css();
    assert!(
        css.contains(&format!("border-bottom: 1px solid {focus}")),
        "the headerbar rule should be a focus hairline"
    );
    assert!(
        css.contains(".sidebar {"),
        "the sidebar block should still be styled"
    );
}

#[test]
fn the_tui_sheet_sets_the_ui_font_the_theme_was_given() {
    let mut theme = tui("retro82", Mode::Dark);
    theme.typography.ui_font = "\"CaskaydiaMono Nerd Font\", monospace".to_string();
    let css = theme.to_gtk_css();
    assert!(css.contains("font-family: \"CaskaydiaMono Nerd Font\", monospace;"));
}

#[test]
fn the_classic_sheet_is_untouched_by_the_new_arm() {
    let theme = tui("retro82", Mode::Dark).with_style(Style::Classic);
    let css = theme.to_gtk_css();
    assert!(css.contains("border-radius: 3px"), "classic keeps its corners");
}
