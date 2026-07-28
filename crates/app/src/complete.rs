//! Deciding whether the caret sits inside a wikilink that can be completed.

use std::cell::RefCell;
use std::ops::Range;

use gtk::prelude::*;

use crate::picker::Row;

/// A live `[[` the caret is typing inside.
#[derive(Debug, PartialEq, Eq)]
pub struct Context {
    /// Text typed since the `[[`, matched against note stems and paths.
    pub query: String,
    /// Byte range of the target text to replace, starting just after the `[[`.
    pub range: Range<usize>,
    /// Whether a `]]` already closes the link.
    pub closed: bool,
}

/// The completable link at `caret`, or `None` when there is nothing to complete.
///
/// `inert` holds the code and frontmatter ranges the wikilink scanner reports,
/// where a `[[` means nothing.
pub fn context(text: &str, caret: usize, inert: &[Range<usize>]) -> Option<Context> {
    let open = text.get(..caret)?.rfind("[[")?;
    if inert.iter().any(|range| range.contains(&open)) {
        return None;
    }

    let start = open + 2;
    let typed = text.get(start..caret)?;
    if typed.contains(['\n', '[', ']', '#', '|']) {
        return None;
    }

    // A caret inside an existing target replaces all of it, not just the prefix.
    let tail = text.get(caret..)?;
    let end = match tail.find("]]") {
        Some(offset) if !tail[..offset].contains(['\n', '[']) => caret + offset,
        _ => caret,
    };

    Some(Context {
        query: typed.to_string(),
        range: start..end,
        closed: end != caret || tail.starts_with("]]"),
    })
}

/// The text that replaces the target, and where the caret lands after it.
///
/// The caret always ends up past the `]]`, whether it was already there or not.
pub fn insertion(target: &str, closed: bool) -> (String, usize) {
    let caret = target.len() + "]]".len();
    let text = if closed {
        target.to_string()
    } else {
        format!("{target}]]")
    };
    (text, caret)
}

/// Rows for the completion popup: the link targets that match `query`.
///
/// `targets` are the shortest form of each note, so a row shows exactly the text
/// that accepting it writes.
pub fn rows(query: &str, targets: &[String], limit: usize) -> Vec<Row> {
    jotter_search::rank(query, targets, String::as_str)
        .into_iter()
        .take(limit)
        .map(|(target, hit)| Row {
            key: target.clone(),
            label: target.clone(),
            detail: String::new(),
            positions: hit.positions,
        })
        .collect()
}

/// The completion list, shown at the caret without taking focus from the editor.
pub struct Popup {
    popover: gtk::Popover,
    list: gtk::ListBox,
    keys: RefCell<Vec<String>>,
}

impl Popup {
    /// Builds the popover parented on `anchor`, hidden until `show`.
    pub fn new(anchor: &gtk::Widget) -> Self {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Browse);
        list.add_css_class("picker-list");

        let scroller = gtk::ScrolledWindow::builder()
            .max_content_height(220)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();

        let popover = gtk::Popover::builder()
            .autohide(false)
            .can_focus(false)
            .has_arrow(false)
            .position(gtk::PositionType::Bottom)
            .child(&scroller)
            .build();
        popover.add_css_class("completion");
        popover.set_parent(anchor);

        Self {
            popover,
            list,
            keys: RefCell::new(Vec::new()),
        }
    }

    /// Shows `rows` at `at`, or hides the popup when there is nothing to offer.
    pub fn show(&self, at: gtk::gdk::Rectangle, rows: &[Row]) {
        if rows.is_empty() {
            self.hide();
            return;
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for row in rows {
            let label = gtk::Label::builder()
                .xalign(0.0)
                .use_markup(true)
                .label(crate::picker::highlight(&row.label, &row.positions))
                .build();
            self.list.append(&label);
        }
        *self.keys.borrow_mut() = rows.iter().map(|row| row.key.clone()).collect();
        self.select(0);
        self.popover.set_pointing_to(Some(&at));
        self.popover.popup();
    }

    pub fn hide(&self) {
        self.popover.popdown();
        self.keys.borrow_mut().clear();
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.keys.borrow().is_empty() && self.popover.is_visible()
    }

    /// Moves the selection by `delta`, clamped to the list.
    pub fn step(&self, delta: i32) {
        let count = i32::try_from(self.keys.borrow().len()).unwrap_or(0);
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(0, |row| row.index());
        self.select((current + delta).clamp(0, count - 1));
    }

    /// The target of the selected row, if any.
    #[must_use]
    pub fn selected(&self) -> Option<String> {
        let index = self.list.selected_row()?.index();
        let index = usize::try_from(index).ok()?;
        self.keys.borrow().get(index).cloned()
    }

    fn select(&self, index: i32) {
        if let Some(row) = self.list.row_at_index(index) {
            self.list.select_row(Some(&row));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{context, insertion, rows};

    fn targets() -> Vec<String> {
        ["phase3-plan", "notes/plan", "groceries"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn an_empty_query_offers_every_target() {
        assert_eq!(rows("", &targets(), 10).len(), 3);
    }

    #[test]
    fn a_query_ranks_the_best_target_first() {
        let found = rows("plan", &targets(), 10);
        assert_eq!(found[0].key, "notes/plan");
    }

    #[test]
    fn a_row_offers_the_text_it_will_insert() {
        let found = rows("groc", &targets(), 10);
        assert_eq!(found[0].key, "groceries");
        assert_eq!(found[0].label, "groceries");
    }

    #[test]
    fn the_limit_caps_the_row_count() {
        assert_eq!(rows("", &targets(), 2).len(), 2);
    }

    #[test]
    fn an_unclosed_link_gains_its_brackets() {
        assert_eq!(
            insertion("notes/plan", false),
            ("notes/plan]]".to_string(), 12)
        );
    }

    #[test]
    fn a_closed_link_keeps_the_brackets_it_has() {
        assert_eq!(insertion("plan", true), ("plan".to_string(), 6));
    }

    fn query_at(text: &str, caret: usize) -> Option<String> {
        context(text, caret, &[]).map(|found| found.query)
    }

    #[test]
    fn the_empty_target_of_a_fresh_link_completes() {
        assert_eq!(query_at("see [[", 6), Some(String::new()));
    }

    #[test]
    fn a_partial_target_completes() {
        assert_eq!(query_at("see [[pl", 8), Some("pl".to_string()));
    }

    #[test]
    fn a_target_with_a_space_completes() {
        assert_eq!(query_at("see [[my pl", 11), Some("my pl".to_string()));
    }

    #[test]
    fn plain_text_does_not_complete() {
        assert_eq!(query_at("just writing", 12), None);
    }

    #[test]
    fn the_caret_after_a_finished_link_does_not_complete() {
        assert_eq!(query_at("see [[plan]] here", 17), None);
    }

    #[test]
    fn a_second_link_completes_after_a_finished_one() {
        assert_eq!(query_at("see [[plan]] and [[gr", 21), Some("gr".to_string()));
    }

    #[test]
    fn a_newline_ends_the_link() {
        assert_eq!(query_at("see [[\nplan", 11), None);
    }

    #[test]
    fn a_heading_part_is_not_completed() {
        assert_eq!(query_at("see [[plan#sec", 14), None);
    }

    #[test]
    fn an_alias_part_is_not_completed() {
        assert_eq!(query_at("see [[plan|the", 14), None);
    }

    #[test]
    fn the_range_covers_the_typed_target() {
        let found = context("see [[pl", 8, &[]).expect("context");
        assert_eq!(found.range, 6..8);
        assert!(!found.closed);
    }

    #[test]
    fn completing_mid_target_replaces_the_whole_target() {
        let found = context("see [[plan]]", 8, &[]).expect("context");
        assert_eq!(found.query, "pl");
        assert_eq!(found.range, 6..10);
        assert!(found.closed);
    }

    #[test]
    fn a_link_inside_code_does_not_complete() {
        let inert = [Range { start: 0, end: 5 }];
        assert_eq!(context("`[[pl", 5, &inert), None);
    }

    #[test]
    fn a_link_outside_an_inert_range_still_completes() {
        let inert = [Range { start: 0, end: 6 }];
        assert_eq!(
            context("`code` [[pl", 11, &inert).map(|found| found.query),
            Some("pl".to_string())
        );
    }
}
