//! The full-text search page: a query entry above the shared results list.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow, gdk, glib};

use crate::results::{Hit, List};

/// The search sidebar page.
pub struct Panel {
    root: gtk::Box,
    back: gtk::Button,
    entry: gtk::Entry,
    status: gtk::Label,
    results: Rc<List>,
    /// Whether a query is in effect, which decides the status line.
    searching: Cell<bool>,
}

impl Panel {
    /// Builds the panel. Rows activate through `on_activate(path, line)`.
    pub fn new<A: Fn(&str, i32) + 'static>(accent: &str, on_activate: A) -> Rc<Self> {
        let entry = gtk::Entry::builder()
            .placeholder_text("Search notes")
            .hexpand(true)
            .build();

        // The icon themes here only have chevrons, so the arrow is a Nerd Font
        // glyph, which is centered on the em box rather than sitting on the baseline.
        let back = gtk::Button::builder()
            .label("\u{f060}")
            .has_frame(false)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Start)
            .tooltip_text("Back to files")
            .build();
        back.add_css_class("panel-back");

        let bar = gtk::Box::new(Orientation::Horizontal, 4);
        bar.set_margin_start(6);
        bar.set_margin_end(8);
        bar.set_margin_top(6);
        bar.set_margin_bottom(4);
        bar.append(&back);
        bar.append(&entry);

        let status = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(10)
            .margin_bottom(4)
            .build();
        status.add_css_class("picker-detail");

        let results = List::new(accent, on_activate);
        let scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&results.widget())
            .build();

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.append(&bar);
        root.append(&status);
        root.append(&scroller);

        Rc::new(Self {
            root,
            back,
            entry,
            status,
            results,
            searching: Cell::new(false),
        })
    }

    /// The widget to place in the sidebar.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Recolors the matched words after a theme change.
    pub fn set_accent(&self, accent: &str) {
        self.results.set_accent(accent);
    }

    /// Whether focus is anywhere in the panel, entry or results.
    ///
    /// Focus sits on a child node (the entry's internal text, or a result row),
    /// never on the panel itself, so this asks about focus-within.
    #[must_use]
    pub fn has_focus(&self) -> bool {
        self.root
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    /// Focuses the query entry and selects what is in it.
    pub fn focus(&self) {
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    /// Runs `f` on every change to the query text.
    pub fn connect_query<F: Fn(&str) + 'static>(&self, f: F) {
        self.entry
            .connect_changed(move |entry| f(entry.text().as_str()));
    }

    /// Runs `f` when the back arrow is clicked.
    pub fn connect_back<F: Fn() + 'static>(&self, f: F) {
        self.back.connect_clicked(move |_| f());
    }

    /// Runs `f` when Escape is pressed in the query entry.
    pub fn connect_escape<F: Fn() + 'static>(&self, f: F) {
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                f();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.entry.add_controller(keys);
    }

    /// Replaces the results with `hits`, or shows why there are none.
    pub fn set_hits(&self, hits: &[Hit], searching: bool) {
        self.results.set_hits(hits);
        self.searching.set(searching);
        self.status.set_text(&status_text(hits.len(), searching));
    }
}

/// The line under the entry: how many notes matched, or why none did.
fn status_text(matches: usize, searching: bool) -> String {
    if !searching {
        return String::new();
    }
    match matches {
        0 => "No matches".to_string(),
        1 => "1 note".to_string(),
        count => format!("{count} notes"),
    }
}

#[cfg(test)]
mod tests {
    use super::status_text;

    #[test]
    fn an_idle_panel_says_nothing() {
        assert_eq!(status_text(0, false), "");
    }

    #[test]
    fn an_empty_result_says_so() {
        assert_eq!(status_text(0, true), "No matches");
    }

    #[test]
    fn one_match_is_singular() {
        assert_eq!(status_text(1, true), "1 note");
    }

    #[test]
    fn several_matches_are_counted() {
        assert_eq!(status_text(2, true), "2 notes");
    }
}
