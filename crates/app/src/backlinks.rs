//! Backlinks: the notes pointing at the open note, and where they point from.
//!
//! The strip lives under the editor and preview, so it is there in both modes.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow};

use jotter_theming::Style;

use crate::results::{Hit, List};
use crate::search::Snippet;

/// The collapsible strip of notes linking to the open one.
pub struct Strip {
    root: gtk::Box,
    header: gtk::Button,
    revealer: gtk::Revealer,
    results: Rc<List>,
    /// What the user wants, which survives a note that has nothing to show.
    wanted: Cell<bool>,
    /// The visual language the strip is drawn in.
    style: Cell<Style>,
}

impl Strip {
    /// Builds the strip, collapsed or not per `expanded`. Rows activate through
    /// `on_activate(path, line)`.
    pub fn new<A: Fn(&str, i32) + 'static>(
        accent: &str,
        expanded: bool,
        on_activate: A,
    ) -> Rc<Self> {
        let header = gtk::Button::builder()
            .label(header_text(0, expanded))
            .has_frame(false)
            .halign(gtk::Align::Start)
            .build();
        header.add_css_class("backlinks-header");

        let results = List::new(accent, on_activate);
        let scroller = ScrolledWindow::builder()
            .max_content_height(180)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&results.widget())
            .build();

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(120)
            .reveal_child(expanded)
            .child(&scroller)
            .build();

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.add_css_class("backlinks");
        root.append(&gtk::Separator::new(Orientation::Horizontal));
        root.append(&header);
        root.append(&revealer);

        let strip = Rc::new(Self {
            root,
            header,
            revealer,
            results,
            wanted: Cell::new(expanded),
            style: Cell::new(Style::Classic),
        });

        let toggled = Rc::clone(&strip);
        strip.header.connect_clicked(move |_| {
            toggled.set_expanded(!toggled.wanted.get());
        });

        strip
    }

    /// The widget to place under the editor.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Whether the user wants the list open, for persisting between runs.
    ///
    /// Not the same as visible: a note with no backlinks shows nothing without
    /// forgetting that the strip should reopen on a note that has some.
    #[must_use]
    pub fn is_expanded(&self) -> bool {
        self.wanted.get()
    }

    /// Records whether the list should be open, and shows it if there is anything.
    pub fn set_expanded(&self, expanded: bool) {
        self.wanted.set(expanded);
        self.redraw();
    }

    /// Applies the wanted state against what there is to show.
    fn redraw(&self) {
        let count = self.results.len();
        let showing = self.wanted.get() && count > 0;
        self.revealer.set_reveal_child(showing);
        self.header
            .set_label(&crate::style::heading(self.style.get(), &header_text(count, showing)));
    }

    /// Recolors the matched words after a theme change.
    pub fn set_accent(&self, accent: &str) {
        self.results.set_accent(accent);
    }

    /// Redraws the strip in `style`.
    pub fn set_style(&self, style: Style) {
        self.style.set(style);
        self.results.set_style(style);
        self.redraw();
    }

    /// Replaces the listed backlinks, keeping the strip open if it was.
    pub fn set_hits(&self, hits: &[Hit]) {
        self.results.set_hits(hits);
        self.redraw();
    }
}

/// The header label: how many notes link here, and which way the arrow points.
fn header_text(count: usize, expanded: bool) -> String {
    let arrow = if expanded { "\u{25be}" } else { "\u{25b8}" };
    match count {
        0 => "No backlinks".to_string(),
        1 => format!("{arrow} 1 backlink"),
        many => format!("{arrow} {many} backlinks"),
    }
}

/// Lines of `text` holding a wikilink that resolves to `target`, at most `limit`.
///
/// `resolve` answers where a link target points, the same question the preview
/// asks, so a link written as a bare stem and one written as a path both count.
pub fn linking_lines(
    text: &str,
    target: &str,
    resolve: &dyn Fn(&str) -> Option<String>,
    limit: usize,
) -> Vec<Snippet> {
    let starts = line_starts(text);
    let mut found: Vec<Snippet> = Vec::new();

    for link in jotter_parser::wikilink::scan(text) {
        if resolve(&link.target).as_deref() != Some(target) {
            continue;
        }
        let line = starts.partition_point(|start| *start <= link.range.start) - 1;
        let raw = text.lines().nth(line).unwrap_or_default();
        let indent = raw.len() - raw.trim_start().len();
        let span = link.range.start - starts[line] - indent..link.range.end - starts[line] - indent;

        if let Some(existing) = found.iter_mut().find(|found| found.line == line_number(line)) {
            existing.spans.push(span);
            continue;
        }
        if found.len() == limit {
            break;
        }
        found.push(Snippet {
            line: line_number(line),
            text: raw.trim().to_string(),
            spans: vec![span],
        });
    }
    found
}

/// Byte offset where each line starts.
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    starts
}

fn line_number(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{header_text, linking_lines};

    #[test]
    fn nothing_linking_here_says_so() {
        assert_eq!(header_text(0, false), "No backlinks");
        assert_eq!(header_text(0, true), "No backlinks");
    }

    #[test]
    fn one_backlink_is_singular() {
        assert_eq!(header_text(1, false), "\u{25b8} 1 backlink");
    }

    #[test]
    fn the_arrow_follows_the_state() {
        assert_eq!(header_text(3, true), "\u{25be} 3 backlinks");
        assert_eq!(header_text(3, false), "\u{25b8} 3 backlinks");
    }

    fn resolver(target: &str) -> Option<String> {
        match target {
            "plan" | "notes/plan" => Some("notes/plan.md".to_string()),
            "other" => Some("other.md".to_string()),
            _ => None,
        }
    }

    fn lines(text: &str, limit: usize) -> Vec<(i32, String)> {
        linking_lines(text, "notes/plan.md", &resolver, limit)
            .into_iter()
            .map(|snippet| (snippet.line, snippet.text))
            .collect()
    }

    #[test]
    fn the_line_holding_the_link_is_returned() {
        let text = "intro\nsee [[plan]] for details\ntail";
        assert_eq!(lines(text, 5), [(1, "see [[plan]] for details".to_string())]);
    }

    #[test]
    fn a_link_written_as_a_path_counts_too() {
        let text = "see [[notes/plan]] here";
        assert_eq!(lines(text, 5).len(), 1);
    }

    #[test]
    fn a_link_to_another_note_is_ignored() {
        assert!(lines("see [[other]] here", 5).is_empty());
    }

    #[test]
    fn a_broken_link_is_ignored() {
        assert!(lines("see [[ghost]] here", 5).is_empty());
    }

    #[test]
    fn a_link_inside_code_is_ignored() {
        assert!(lines("use `[[plan]]` in text", 5).is_empty());
    }

    #[test]
    fn two_lines_are_both_reported() {
        let text = "[[plan]] one\nmiddle\n[[plan]] two";
        assert_eq!(lines(text, 5).len(), 2);
    }

    #[test]
    fn one_line_with_two_links_is_reported_once() {
        assert_eq!(lines("[[plan]] and [[notes/plan]]", 5).len(), 1);
    }

    #[test]
    fn the_limit_caps_the_lines() {
        let text = "[[plan]] a\n[[plan]] b\n[[plan]] c";
        assert_eq!(lines(text, 2).len(), 2);
    }

    #[test]
    fn the_line_is_trimmed_and_the_link_span_follows() {
        let found = linking_lines("   see [[plan]] x", "notes/plan.md", &resolver, 5);
        assert_eq!(found[0].text, "see [[plan]] x");
        assert_eq!(found[0].spans, vec![4..12]);
    }
}
