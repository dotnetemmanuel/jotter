//! The tag sidebar page: every tag with its count, then the notes carrying one.
//!
//! Tags are listed alphabetically, so a tag stays where you last saw it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow, gdk, glib};

use crate::results::{Hit, List};

/// Which level of the page is showing.
#[derive(Clone, PartialEq, Eq)]
pub enum View {
    /// The list of tags.
    Tags,
    /// The notes carrying one tag.
    Notes(String),
}

/// The tag page.
pub struct Panel {
    root: gtk::Box,
    back: gtk::Button,
    heading: gtk::Label,
    tags: gtk::ListBox,
    tag_scroller: ScrolledWindow,
    notes: Rc<List>,
    note_scroller: ScrolledWindow,
    /// Tag behind each row of the tag list, parallel to the rows.
    names: RefCell<Vec<String>>,
    view: RefCell<View>,
}

impl Panel {
    /// Builds the page. `on_tag` runs when a tag is chosen, `on_note` when a note is.
    pub fn new<T: Fn(&str) + 'static, N: Fn(&str, i32) + 'static>(
        accent: &str,
        on_tag: T,
        on_note: N,
    ) -> Rc<Self> {
        let heading = gtk::Label::builder()
            .xalign(0.0)
            .label(heading_text(&View::Tags, 0))
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

        let tags = gtk::ListBox::new();
        tags.set_selection_mode(gtk::SelectionMode::Single);
        tags.add_css_class("search-results");
        let tag_scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&tags)
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
        root.append(&tag_scroller);
        root.append(&note_scroller);

        let panel = Rc::new(Self {
            root,
            back,
            heading,
            tags,
            tag_scroller,
            notes,
            note_scroller,
            names: RefCell::new(Vec::new()),
            view: RefCell::new(View::Tags),
        });

        let chosen = Rc::clone(&panel);
        panel.tags.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            let tag = chosen.names.borrow().get(index).cloned();
            if let Some(tag) = tag {
                on_tag(&tag);
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

    /// Shows the tag list.
    pub fn show_tags(&self, counts: &[(String, i64)]) {
        while let Some(child) = self.tags.first_child() {
            self.tags.remove(&child);
        }
        for (tag, uses) in counts {
            self.tags.append(&tag_row(tag, *uses));
        }
        *self.names.borrow_mut() = counts.iter().map(|(tag, _)| tag.clone()).collect();

        *self.view.borrow_mut() = View::Tags;
        self.heading.set_text(&heading_text(&View::Tags, counts.len()));
        self.tag_scroller.set_visible(true);
        self.note_scroller.set_visible(false);
        self.focus_list();
    }

    /// Shows the notes carrying `tag`.
    pub fn show_notes(&self, tag: &str, hits: &[Hit]) {
        self.notes.set_hits(hits);
        let view = View::Notes(tag.to_string());
        self.heading.set_text(&heading_text(&view, hits.len()));
        *self.view.borrow_mut() = view;
        self.tag_scroller.set_visible(false);
        self.note_scroller.set_visible(true);
        self.focus_list();
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

    /// Puts focus on the visible list so arrows work straight away.
    pub fn focus_list(&self) {
        if matches!(*self.view.borrow(), View::Tags) {
            if let Some(row) = self.tags.row_at_index(0) {
                self.tags.select_row(Some(&row));
                row.grab_focus();
            }
        } else {
            self.notes.widget().grab_focus();
        }
    }
}

/// One tag row: the tag, and how many notes carry it.
fn tag_row(tag: &str, uses: i64) -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.add_css_class("tag-row");
    let name = gtk::Label::builder().xalign(0.0).label(format!("#{tag}")).build();
    let count = gtk::Label::builder()
        .xalign(1.0)
        .hexpand(true)
        .label(uses.to_string())
        .build();
    count.add_css_class("search-count");
    row.append(&name);
    row.append(&count);
    row
}

/// The line above the list, naming the level and what it holds.
fn heading_text(view: &View, count: usize) -> String {
    match view {
        View::Tags => match count {
            0 => "No tags".to_string(),
            1 => "1 tag".to_string(),
            many => format!("{many} tags"),
        },
        View::Notes(tag) => match count {
            0 => format!("#{tag}: no notes"),
            1 => format!("#{tag}: 1 note"),
            many => format!("#{tag}: {many} notes"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{View, heading_text};

    #[test]
    fn an_untagged_vault_says_so() {
        assert_eq!(heading_text(&View::Tags, 0), "No tags");
    }

    #[test]
    fn one_tag_reads_singular() {
        assert_eq!(heading_text(&View::Tags, 1), "1 tag");
    }

    #[test]
    fn tags_are_counted() {
        assert_eq!(heading_text(&View::Tags, 4), "4 tags");
    }

    #[test]
    fn a_tags_notes_are_named_by_the_tag() {
        let view = View::Notes("project".to_string());
        assert_eq!(heading_text(&view, 3), "#project: 3 notes");
        assert_eq!(heading_text(&view, 1), "#project: 1 note");
    }

    #[test]
    fn a_tag_with_nothing_left_says_so() {
        let view = View::Notes("gone".to_string());
        assert_eq!(heading_text(&view, 0), "#gone: no notes");
    }
}
