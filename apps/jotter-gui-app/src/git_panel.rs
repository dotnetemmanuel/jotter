//! The git sidebar page: what changed since the last commit.
//!
//! Read-only by design. Sync commits everything, so a staging control here would
//! be a switch that changes nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow, gdk, glib};

use jotter_theming::Style;

use crate::results::{Hit, List};

/// The page.
pub struct Panel {
    root: gtk::Box,
    back: gtk::Button,
    heading: gtk::Label,
    changed: Rc<List>,
    /// The visual language the page is drawn in.
    style: Cell<Style>,
    /// The heading text as last computed, unstyled, so a style change can redo it.
    last_heading: RefCell<String>,
}

impl Panel {
    /// Builds the page. Rows open the note they name.
    pub fn new<A: Fn(&str, i32) + 'static>(accent: &str, on_open: A) -> Rc<Self> {
        let initial_heading = heading_text(0);
        let heading = gtk::Label::builder()
            .xalign(0.0)
            .label(&initial_heading)
            .build();
        heading.add_css_class("tags-heading");

        let back = gtk::Button::builder()
            .label("\u{f060}")
            .has_frame(false)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Start)
            .tooltip_text("Back")
            .build();
        back.add_css_class("panel-back");

        let bar = gtk::Box::new(Orientation::Horizontal, 4);
        bar.add_css_class("panel-bar");
        bar.set_margin_start(6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(4);
        bar.append(&back);
        bar.append(&heading);

        let changed = List::new(accent, on_open);
        let scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&changed.widget())
            .build();

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.append(&bar);
        root.append(&scroller);

        Rc::new(Self {
            root,
            back,
            heading,
            changed,
            style: Cell::new(Style::Classic),
            last_heading: RefCell::new(initial_heading),
        })
    }

    /// The widget to place in the sidebar.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Recolors after a theme change.
    pub fn set_accent(&self, accent: &str) {
        self.changed.set_accent(accent);
    }

    /// Redraws the page in `style`.
    pub fn set_style(&self, style: Style) {
        self.style.set(style);
        self.changed.set_style(style);
        let heading = self.last_heading.borrow().clone();
        self.write_heading(heading);
    }

    /// Whether focus is anywhere in the page.
    #[must_use]
    pub fn has_focus(&self) -> bool {
        self.root
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    /// Lists `changed`, each row carrying what happened to that note.
    pub fn show(&self, changed: &[jotter_git::Change]) {
        let hits: Vec<Hit> = changed
            .iter()
            .map(|change| Hit {
                path: change.path.clone(),
                snippets: Vec::new(),
                badge: Some(change.kind.label().to_string()),
            })
            .collect();
        self.changed.set_hits(&hits);
        self.write_heading(heading_text(changed.len()));
    }

    /// Lists the conflicted notes and how far each one has been answered.
    pub fn show_conflicts(&self, files: &[(String, usize, usize)]) {
        let hits: Vec<Hit> = files
            .iter()
            .map(|(path, answered, total)| Hit {
                path: path.clone(),
                snippets: Vec::new(),
                badge: Some(if answered == total {
                    "\u{2713}".to_string()
                } else {
                    format!("{answered}/{total}")
                }),
            })
            .collect();
        self.changed.set_hits(&hits);
        self.write_heading(conflict_heading(files.len()));
    }

    /// Puts focus on the list so the arrows work straight away.
    pub fn focus_list(&self) {
        self.changed.focus_first();
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

    /// Sets the heading label styled for the active dress, and remembers it unstyled.
    fn write_heading(&self, text: String) {
        self.heading.set_text(&crate::style::heading(self.style.get(), &text));
        *self.last_heading.borrow_mut() = text;
    }
}

/// The line above the list while a rebase is stuck.
fn conflict_heading(count: usize) -> String {
    match count {
        1 => "1 note conflicts".to_string(),
        many => format!("{many} notes conflict"),
    }
}

/// The line above the list.
fn heading_text(count: usize) -> String {
    match count {
        0 => "Nothing changed".to_string(),
        1 => "1 changed".to_string(),
        many => format!("{many} changed"),
    }
}

#[cfg(test)]
mod tests {
    use super::{conflict_heading, heading_text};

    #[test]
    fn one_conflicted_note_reads_singular() {
        assert_eq!(conflict_heading(1), "1 note conflicts");
        assert_eq!(conflict_heading(3), "3 notes conflict");
    }

    #[test]
    fn a_committed_vault_says_nothing_changed() {
        assert_eq!(heading_text(0), "Nothing changed");
    }

    #[test]
    fn one_change_reads_singular() {
        assert_eq!(heading_text(1), "1 changed");
    }

    #[test]
    fn changes_are_counted() {
        assert_eq!(heading_text(5), "5 changed");
    }
}
