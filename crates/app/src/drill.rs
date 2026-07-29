//! A two-level sidebar page: names with counts, then the notes behind one.
//!
//! Tags and broken links are the same page with different words, so they share
//! it. Names are listed alphabetically, so one stays where you last saw it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow, gdk, glib};

use crate::results::{Hit, List};

/// What the page is listing, which decides how the rows read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Tags, and the notes carrying one.
    Tags,
    /// Missing link targets, and the notes pointing at one.
    Broken,
}

/// Which level of the page is showing.
#[derive(Clone, PartialEq, Eq)]
pub enum View {
    /// The list of names.
    Top,
    /// The notes behind one name.
    Notes(String),
}

/// The page.
pub struct Panel {
    kind: Kind,
    root: gtk::Box,
    back: gtk::Button,
    heading: gtk::Label,
    top: gtk::ListBox,
    top_scroller: ScrolledWindow,
    notes: Rc<List>,
    note_scroller: ScrolledWindow,
    /// Name behind each row of the top list, parallel to the rows.
    names: RefCell<Vec<String>>,
    view: RefCell<View>,
}

impl Panel {
    /// Builds the page. `on_pick` runs when a name is chosen, `on_note` when a
    /// note is.
    pub fn new<T: Fn(&str) + 'static, N: Fn(&str, i32) + 'static>(
        kind: Kind,
        accent: &str,
        on_pick: T,
        on_note: N,
    ) -> Rc<Self> {
        let heading = gtk::Label::builder()
            .xalign(0.0)
            .label(heading_text(kind, &View::Top, 0))
            .build();
        heading.add_css_class("tags-heading");

        // The icon themes here only have chevrons, so the arrow is a Nerd Font
        // glyph, which is centered on the em box rather than sitting on the baseline.
        let back = gtk::Button::builder()
            .label("\u{f060}")
            .has_frame(false)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Start)
            .tooltip_text("Back")
            .build();
        back.add_css_class("panel-back");

        let bar = gtk::Box::new(Orientation::Horizontal, 4);
        bar.set_margin_start(6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(4);
        bar.append(&back);
        bar.append(&heading);

        let top = gtk::ListBox::new();
        top.set_selection_mode(gtk::SelectionMode::Single);
        top.add_css_class("search-results");
        let top_scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&top)
            .build();

        let notes = List::new(accent, on_note);
        let note_scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&notes.widget())
            .build();
        note_scroller.set_visible(false);

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.append(&bar);
        root.append(&top_scroller);
        root.append(&note_scroller);

        let panel = Rc::new(Self {
            kind,
            root,
            back,
            heading,
            top,
            top_scroller,
            notes,
            note_scroller,
            names: RefCell::new(Vec::new()),
            view: RefCell::new(View::Top),
        });

        let chosen = Rc::clone(&panel);
        panel.top.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            let name = chosen.names.borrow().get(index).cloned();
            if let Some(name) = name {
                on_pick(&name);
            }
        });

        panel
    }

    /// The widget to place in the sidebar.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Recolors matched text after a theme change.
    pub fn set_accent(&self, accent: &str) {
        self.notes.set_accent(accent);
    }

    /// Whether focus is anywhere in the page.
    #[must_use]
    pub fn has_focus(&self) -> bool {
        self.root
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    /// The level currently showing.
    #[must_use]
    pub fn view(&self) -> View {
        self.view.borrow().clone()
    }

    /// Shows the top list. Focus is the caller's to move, so a background
    /// refresh does not pull the caret out of the editor.
    ///
    /// The selected name is kept across the redraw where it survives, so a link
    /// breaking elsewhere does not lose the row being read.
    pub fn show_top(&self, counts: &[(String, i64)]) {
        let was_selected = self.selected_name();
        let had_focus = crate::results::holds_focus_widget(&self.root);
        while let Some(child) = self.top.first_child() {
            self.top.remove(&child);
        }
        for (name, uses) in counts {
            self.top.append(&top_row(self.kind, name, *uses));
        }
        *self.names.borrow_mut() = counts.iter().map(|(name, _)| name.clone()).collect();
        let restored = was_selected.and_then(|name| self.row_named(&name));
        if let Some(row) = &restored {
            self.top.select_row(Some(row));
        }
        if had_focus && let Some(row) = restored.or_else(|| self.top.row_at_index(0)) {
            self.top.select_row(Some(&row));
            crate::results::set_focus_widget(&row);
        }

        *self.view.borrow_mut() = View::Top;
        self.heading
            .set_text(&heading_text(self.kind, &View::Top, counts.len()));
        self.top_scroller.set_visible(true);
        self.note_scroller.set_visible(false);
    }

    /// Shows the notes behind `name`.
    pub fn show_notes(&self, name: &str, hits: &[Hit]) {
        self.notes.set_hits(hits);
        let view = View::Notes(name.to_string());
        self.heading
            .set_text(&heading_text(self.kind, &view, hits.len()));
        *self.view.borrow_mut() = view;
        self.top_scroller.set_visible(false);
        self.note_scroller.set_visible(true);
    }

    /// Runs `f` when the back arrow is clicked.
    pub fn connect_back<F: Fn() + 'static>(&self, f: F) {
        self.back.connect_clicked(move |_| f());
    }

    /// Runs `f` when Escape is pressed anywhere on the page.
    pub fn connect_escape<F: Fn() + 'static>(&self, f: F) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                f();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.root.add_controller(keys);
    }

    /// Puts focus on the visible list so arrows work straight away, on the
    /// selected row where there is one.
    pub fn focus_list(&self) {
        if matches!(*self.view.borrow(), View::Top) {
            let row = self
                .top
                .selected_row()
                .or_else(|| self.top.row_at_index(0));
            if let Some(row) = row {
                self.top.select_row(Some(&row));
                row.grab_focus();
            }
        } else {
            self.notes.focus_first();
        }
    }

    /// The name behind the selected top row.
    fn selected_name(&self) -> Option<String> {
        let index = usize::try_from(self.top.selected_row()?.index()).ok()?;
        self.names.borrow().get(index).cloned()
    }

    /// The top row carrying `name`, once the rows have been rebuilt.
    fn row_named(&self, name: &str) -> Option<gtk::ListBoxRow> {
        let index = self.names.borrow().iter().position(|held| held == name)?;
        self.top.row_at_index(i32::try_from(index).ok()?)
    }
}

/// One top row: the name, and how many notes are behind it.
fn top_row(kind: Kind, name: &str, uses: i64) -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.add_css_class("tag-row");
    let label = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .label(written(kind, name))
        .build();
    let count = gtk::Label::builder()
        .xalign(1.0)
        .hexpand(true)
        .label(uses.to_string())
        .build();
    count.add_css_class("search-count");
    row.append(&label);
    row.append(&count);
    row
}

/// A name as it appears in a note: a tag with its hash, a target in its brackets.
fn written(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Tags => format!("#{name}"),
        Kind::Broken => format!("[[{name}]]"),
    }
}

/// The line above the list, naming the level and what it holds.
fn heading_text(kind: Kind, view: &View, count: usize) -> String {
    match view {
        View::Top => match (kind, count) {
            (Kind::Tags, 0) => "No tags".to_string(),
            (Kind::Tags, 1) => "1 tag".to_string(),
            (Kind::Tags, many) => format!("{many} tags"),
            (Kind::Broken, 0) => "No broken links".to_string(),
            (Kind::Broken, 1) => "1 broken link".to_string(),
            (Kind::Broken, many) => format!("{many} broken links"),
        },
        View::Notes(name) => {
            let name = written(kind, name);
            match count {
                0 => format!("{name}: no notes"),
                1 => format!("{name}: 1 note"),
                many => format!("{name}: {many} notes"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, View, heading_text};

    #[test]
    fn an_untagged_vault_says_so() {
        assert_eq!(heading_text(Kind::Tags, &View::Top, 0), "No tags");
    }

    #[test]
    fn one_tag_reads_singular() {
        assert_eq!(heading_text(Kind::Tags, &View::Top, 1), "1 tag");
    }

    #[test]
    fn tags_are_counted() {
        assert_eq!(heading_text(Kind::Tags, &View::Top, 4), "4 tags");
    }

    #[test]
    fn a_tags_notes_are_named_by_the_tag() {
        let view = View::Notes("project".to_string());
        assert_eq!(heading_text(Kind::Tags, &view, 3), "#project: 3 notes");
        assert_eq!(heading_text(Kind::Tags, &view, 1), "#project: 1 note");
    }

    #[test]
    fn a_tag_with_nothing_left_says_so() {
        let view = View::Notes("gone".to_string());
        assert_eq!(heading_text(Kind::Tags, &view, 0), "#gone: no notes");
    }

    #[test]
    fn a_vault_with_no_dead_links_says_so() {
        assert_eq!(heading_text(Kind::Broken, &View::Top, 0), "No broken links");
    }

    #[test]
    fn one_broken_link_reads_singular() {
        assert_eq!(heading_text(Kind::Broken, &View::Top, 1), "1 broken link");
    }

    #[test]
    fn broken_links_are_counted() {
        assert_eq!(heading_text(Kind::Broken, &View::Top, 3), "3 broken links");
    }

    #[test]
    fn a_missing_target_is_named_as_it_was_written() {
        let view = View::Notes("ghost".to_string());
        assert_eq!(heading_text(Kind::Broken, &view, 2), "[[ghost]]: 2 notes");
    }
}
