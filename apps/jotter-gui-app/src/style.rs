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

/// The tree's left gutter, always two columns so names stay aligned down the
/// tree: a folder shows which way it points, a file shows the row cursor. A
/// folder says nothing about selection, since its own marker holds the column.
#[must_use]
pub fn tree_gutter(style: Style, expandable: bool, expanded: bool, selected: bool) -> String {
    if style != Style::Tui {
        return String::new();
    }
    match (expandable, expanded) {
        (true, true) => " \u{25be}".to_string(),
        (true, false) => " \u{25b8}".to_string(),
        (false, _) => format!("{} ", cursor(style, selected)),
    }
}

/// The dot joining two shown status bar items.
pub const JOINER: &str = "\u{b7}";

/// Which joiners of a status bar are on, given which items are shown.
///
/// One after every shown item that still has a shown item after it, so a joiner
/// never leads, trails, or doubles up over a hidden item. Classic joins nothing.
#[must_use]
pub fn joiners(style: Style, shown: &[bool]) -> Vec<bool> {
    let mut on = vec![false; shown.len().saturating_sub(1)];
    if style != Style::Tui {
        return on;
    }
    for (index, slot) in on.iter_mut().enumerate() {
        *slot = shown[index] && shown[index + 1..].iter().any(|later| *later);
    }
    on
}

/// The status bar's broken-link item: terse in TUI, a sentence in classic.
#[must_use]
pub fn broken_links(style: Style, count: usize) -> String {
    match (style, count) {
        (Style::Tui, count) => format!("! {count}"),
        (Style::Classic, 1) => "1 broken link".to_string(),
        (Style::Classic, many) => format!("{many} broken links"),
    }
}

#[cfg(test)]
mod tests {
    use super::{broken_links, button, cursor, heading, joiners, segment, tree_gutter};
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
    fn a_selected_tui_file_carries_the_cursor() {
        assert_eq!(tree_gutter(Tui, false, false, true), "> ");
    }

    #[test]
    fn a_selected_tui_folder_keeps_its_marker_alone() {
        assert_eq!(tree_gutter(Tui, true, true, true), " \u{25be}");
        assert_eq!(tree_gutter(Tui, true, false, true), " \u{25b8}");
    }

    #[test]
    fn every_tui_gutter_is_two_columns_wide() {
        for expandable in [true, false] {
            for expanded in [true, false] {
                for selected in [true, false] {
                    let gutter = tree_gutter(Tui, expandable, expanded, selected);
                    assert_eq!(gutter.chars().count(), 2, "{gutter:?}");
                }
            }
        }
    }

    #[test]
    fn classic_joins_nothing() {
        assert_eq!(joiners(Classic, &[true, true, true]), [false, false]);
    }

    #[test]
    fn a_joiner_sits_between_two_shown_items() {
        assert_eq!(joiners(Tui, &[true, true, true]), [true, true]);
    }

    #[test]
    fn a_joiner_never_leads_or_trails() {
        assert_eq!(joiners(Tui, &[false, true, false]), [false, false]);
        assert_eq!(joiners(Tui, &[false, true, true]), [false, true]);
        assert_eq!(joiners(Tui, &[true, true, false]), [true, false]);
    }

    #[test]
    fn a_hidden_middle_item_does_not_double_the_joiner() {
        assert_eq!(joiners(Tui, &[true, false, true]), [true, false]);
    }

    #[test]
    fn a_lone_item_is_joined_to_nothing() {
        assert!(joiners(Tui, &[true]).is_empty());
        assert!(joiners(Tui, &[]).is_empty());
    }

    #[test]
    fn classic_counts_broken_links_in_words() {
        assert_eq!(broken_links(Classic, 1), "1 broken link");
        assert_eq!(broken_links(Classic, 4), "4 broken links");
    }

    #[test]
    fn a_tui_broken_count_is_terse() {
        assert_eq!(broken_links(Tui, 1), "! 1");
        assert_eq!(segment(Tui, &broken_links(Tui, 3)), "[ ! 3 ]");
    }
}
