//! The text the TUI style writes that classic does not.
//!
//! Pure string work, kept out of the widgets so the idioms can be tested
//! without a display.

use jotter_theming::Style;

/// A command button's face: bracketed in TUI, bare in classic.
#[must_use]
pub fn button(style: Style, label: &str) -> String {
    if style != Style::Tui || label.starts_with("[ ") {
        return label.to_string();
    }
    format!("[ {label} ]")
}

/// A panel heading: upper case in TUI, as written in classic.
#[must_use]
pub fn heading(style: Style, text: &str) -> String {
    match style {
        Style::Tui => text.to_uppercase(),
        Style::Classic => text.to_string(),
    }
}

/// A status bar item: bracketed in TUI, bare in classic. An empty item stays
/// empty, since the bar hides those rather than showing empty brackets.
#[must_use]
pub fn segment(style: Style, text: &str) -> String {
    if style != Style::Tui || text.is_empty() {
        return text.to_string();
    }
    format!("[ {text} ]")
}

/// The cursor a list row wears: only the selected row gets it, and only in TUI.
#[must_use]
pub fn cursor(style: Style, selected: bool) -> &'static str {
    match (style, selected) {
        (Style::Classic, _) => "",
        (Style::Tui, true) => ">",
        (Style::Tui, false) => " ",
    }
}

/// The tree's left gutter: the row cursor, then the folder marker. Files keep
/// the column so names stay aligned down the tree.
#[must_use]
pub fn tree_gutter(style: Style, expandable: bool, expanded: bool, selected: bool) -> String {
    if style != Style::Tui {
        return String::new();
    }
    let marker = match (expandable, expanded) {
        (true, true) => "\u{25be}",
        (true, false) => "\u{25b8}",
        (false, _) => " ",
    };
    format!("{}{marker}", cursor(style, selected))
}

#[cfg(test)]
mod tests {
    use super::{button, cursor, heading, segment, tree_gutter};
    use jotter_theming::Style::{Classic, Tui};

    #[test]
    fn a_classic_button_keeps_its_bare_label() {
        assert_eq!(button(Classic, "Continue"), "Continue");
    }

    #[test]
    fn a_tui_button_wears_brackets() {
        assert_eq!(button(Tui, "Continue"), "[ Continue ]");
    }

    #[test]
    fn a_tui_button_is_not_bracketed_twice() {
        assert_eq!(button(Tui, "[ Continue ]"), "[ Continue ]");
    }

    #[test]
    fn a_classic_heading_is_written_as_given() {
        assert_eq!(heading(Classic, "12 tags"), "12 tags");
    }

    #[test]
    fn a_tui_heading_is_upper_case() {
        assert_eq!(heading(Tui, "12 tags"), "12 TAGS");
    }

    #[test]
    fn a_classic_status_segment_is_bare() {
        assert_eq!(segment(Classic, "15px \u{21ba}"), "15px \u{21ba}");
    }

    #[test]
    fn a_tui_status_segment_is_bracketed() {
        assert_eq!(segment(Tui, "15px \u{21ba}"), "[ 15px \u{21ba} ]");
    }

    #[test]
    fn an_empty_segment_stays_empty() {
        assert_eq!(segment(Tui, ""), "");
    }

    #[test]
    fn classic_has_no_row_cursor() {
        assert_eq!(cursor(Classic, true), "");
        assert_eq!(cursor(Classic, false), "");
    }

    #[test]
    fn the_tui_cursor_marks_only_the_selected_row() {
        assert_eq!(cursor(Tui, true), ">");
        assert_eq!(cursor(Tui, false), " ");
    }

    #[test]
    fn a_classic_tree_gutter_is_empty() {
        assert_eq!(tree_gutter(Classic, true, false, false), "");
    }

    #[test]
    fn a_tui_folder_shows_which_way_it_points() {
        assert_eq!(tree_gutter(Tui, true, false, false), " \u{25b8}");
        assert_eq!(tree_gutter(Tui, true, true, false), " \u{25be}");
    }

    #[test]
    fn a_tui_file_has_no_marker_but_keeps_the_column() {
        assert_eq!(tree_gutter(Tui, false, false, false), "  ");
    }

    #[test]
    fn a_selected_tui_row_carries_the_cursor_ahead_of_the_marker() {
        assert_eq!(tree_gutter(Tui, true, true, true), ">\u{25be}");
        assert_eq!(tree_gutter(Tui, false, false, true), "> ");
    }
}
