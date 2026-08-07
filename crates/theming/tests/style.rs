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
fn the_tui_sheet_squares_the_window_decoration() {
    let css = tui("retro82", Mode::Dark).to_gtk_css();
    assert!(
        css.contains("window.csd {\n  border-radius: 0;\n}"),
        "the toplevel's own CSD frame should lose its rounded corners too"
    );
}

#[test]
fn the_tui_sheet_draws_its_structure_in_the_focus_color() {
    let theme = tui("event-horizon", Mode::Dark);
    let focus = theme.chrome.focus.clone();
    let css = theme.to_gtk_css();
    assert!(
        css.contains(&format!("border-bottom: 2px solid {focus}")),
        "the headerbar rule should be a structural focus line"
    );
    assert!(
        css.contains(".sidebar {"),
        "the sidebar block should still be styled"
    );
}

#[test]
fn the_tui_sheet_divides_its_regions_more_heavily_than_it_rules_their_contents() {
    let theme = tui("event-horizon", Mode::Dark);
    let focus = theme.chrome.focus.clone();
    let css = theme.to_gtk_css();
    for region in [
        format!("border-bottom: 2px solid {focus};\n  box-shadow: none;\n  padding-left: 0;"),
        format!(
            "separator {{\n  background-color: {focus};\n  min-height: 2px;\n  min-width: 2px;"
        ),
        format!("paned > separator {{\n  background-color: {focus};"),
        format!("  padding: 6px 8px 4px 8px;\n  border-bottom: 2px solid {focus};"),
        format!(".panel-bar {{\n  padding-bottom: 6px;\n  border-bottom: 2px solid {focus};"),
    ] {
        assert!(
            css.contains(&region),
            "a structural rule lost its weight: {region}"
        );
    }
    for fine in [
        ".search-heading",
        ".search-snippet",
        ".conflict-title",
        ".conflict-header",
        ".picker-title",
    ] {
        let block = css
            .split(&format!("\n{fine} {{"))
            .nth(1)
            .unwrap_or_else(|| panic!("{fine} is styled"));
        let block = &block[..block.find('}').expect("the block closes")];
        for line in block.lines().map(str::trim) {
            let Some(width) = line
                .strip_prefix("border-bottom:")
                .or_else(|| line.strip_prefix("border-left:"))
            else {
                continue;
            };
            assert!(
                width.trim().starts_with("1px"),
                "{fine} rules content, so it should stay a hairline: {line}"
            );
        }
    }
}

#[test]
fn the_tui_picker_lifts_off_the_window_background() {
    let theme = tui("retro82", Mode::Dark);
    let surface = theme.chrome.surface.clone();
    let bg = theme.chrome.background.clone();
    let css = theme.to_gtk_css();
    assert_ne!(surface, bg, "the fixture theme should give the two apart");
    assert!(
        css.contains(&format!(".picker {{\n  background-color: {surface};")),
        "the picker panel should sit on the surface color, not the window background"
    );
    assert!(
        !css.contains(&format!(".picker {{\n  background-color: {bg};")),
        "the picker should not paint itself in the window background"
    );
    assert!(
        css.contains(".picker entry {\n  background-color: transparent;"),
        "the query entry inherits the general entry background unless it is cleared"
    );
    assert!(
        css.contains(&format!(
            ".picker-scrim {{\n  background-color: alpha({bg}, 0.78);\n}}"
        )),
        "the scrim should be dark enough to lift the panel off the page"
    );
}

#[test]
fn the_tui_picker_title_carries_a_rule() {
    let theme = tui("retro82", Mode::Dark);
    let focus = theme.chrome.focus.clone();
    let css = theme.to_gtk_css();
    assert!(
        css.contains(&format!(
            ".picker-title {{\n  color: {focus};\n  font-family: {font};\n  font-size: {small}px;\n  padding: 2px 6px 2px 6px;\n  border-bottom: 1px solid alpha({focus}, 0.45);\n}}",
            font = theme.typography.ui_font,
            small = theme.typography.font_size.saturating_sub(1),
        )),
        "the picker title should run a hairline out under itself"
    );
}

#[test]
fn the_tui_search_results_breathe_between_files() {
    let css = tui("retro82", Mode::Dark).to_gtk_css();
    assert!(
        css.contains(".search-heading {\n  margin-top: 18px;"),
        "each file heading needs air above it"
    );
    assert!(
        css.contains(".search-results > row:first-child .search-heading {\n  margin-top: 0;\n}"),
        "the first heading must still sit flush under the bar"
    );
    assert!(
        css.contains(".search-snippet {\n  color:"),
        "the per-match lines stay tight, so their rule is unchanged"
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
    assert!(
        css.contains("border-radius: 3px"),
        "classic keeps its corners"
    );
}

#[test]
fn the_tui_sheet_dresses_every_widget_class_the_app_uses() {
    let css = tui("retro82", Mode::Dark).to_gtk_css();
    for class in [
        ".rail",
        ".rail-button",
        ".font-list",
        ".settings",
        ".settings-label",
        ".settings-close",
        ".keysheet-heading",
        ".keysheet-keys",
        ".theme-button",
        ".theme-name",
        ".conflict",
        ".conflict-header",
        ".conflict-title",
        ".conflict-body",
        ".conflict-actions",
        ".status-size",
        ".status-git",
        ".status-broken",
        ".status-joiner",
        ".backlinks",
        ".backlinks-header",
        ".search-results",
        ".panel-back",
        ".tags-heading",
        ".tag-row",
        ".search-heading",
        ".search-name",
        ".search-folder",
        ".search-count",
        ".search-snippet",
        ".completion",
        ".picker",
        ".picker-detail",
        ".panel-bar",
        ".picker-title",
        ".row-cursor",
    ] {
        assert!(
            css.contains(class),
            "the TUI sheet says nothing about {class}"
        );
    }
}

#[test]
fn the_tui_row_cursor_inverts_on_the_selected_row() {
    let theme = tui("retro82", Mode::Dark);
    let css = theme.to_gtk_css();
    assert!(css.contains(".search-results > row:selected .row-cursor"));
}

#[test]
fn the_tui_sheet_dresses_the_completion_popup_surface() {
    let theme = tui("retro82", Mode::Dark);
    let accent = theme.chrome.accent.clone();
    let bg = theme.chrome.background.clone();
    let css = theme.to_gtk_css();
    assert!(
        css.contains(".completion list {"),
        "the completion rows should be styled by list, the ListBox's real CSS node name"
    );
    assert!(
        css.contains(&format!(
            ".completion list > row:selected {{\n  background-color: {accent};\n  color: {bg};\n}}"
        )),
        "the selected completion row should use the theme accent, not GTK's default blue"
    );
}
