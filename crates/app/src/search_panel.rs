//! The full-text search panel that takes over the sidebar: a query entry above
//! a list of matching notes, each with the lines that matched.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow, gdk, glib};

use crate::search::Snippet;

/// One matching note and the lines worth showing from it.
#[derive(Clone)]
pub struct Hit {
    /// Vault-relative path of the note.
    pub path: String,
    /// Matching lines, already capped by the caller.
    pub snippets: Vec<Snippet>,
}

/// The search sidebar page.
pub struct Panel {
    root: gtk::Box,
    entry: gtk::Entry,
    list: gtk::ListBox,
    status: gtk::Label,
    /// Note path and line behind each list row, parallel to the rows.
    targets: RefCell<Vec<(String, i32)>>,
    /// Color the matched words take, refreshed on a theme change.
    accent: RefCell<String>,
    /// The current results, kept so a theme change can redraw them.
    hits: RefCell<Vec<Hit>>,
    /// Whether a query is in effect, which decides the status line.
    searching: Cell<bool>,
}

impl Panel {
    /// Builds the panel. Rows activate through `on_activate(path, line)`.
    pub fn new<A: Fn(&str, i32) + 'static>(accent: &str, on_activate: A) -> Rc<Self> {
        let entry = gtk::Entry::builder()
            .placeholder_text("Search notes")
            .margin_top(8)
            .margin_bottom(4)
            .margin_start(8)
            .margin_end(8)
            .build();

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("search-results");

        let status = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(10)
            .margin_bottom(4)
            .build();
        status.add_css_class("picker-detail");

        let scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.append(&entry);
        root.append(&status);
        root.append(&scroller);

        let panel = Rc::new(Self {
            root,
            entry,
            list,
            status,
            targets: RefCell::new(Vec::new()),
            accent: RefCell::new(accent.to_string()),
            hits: RefCell::new(Vec::new()),
            searching: Cell::new(false),
        });

        let activated = Rc::clone(&panel);
        panel.list.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            let target = activated.targets.borrow().get(index).cloned();
            if let Some((path, line)) = target {
                on_activate(&path, line);
            }
        });

        panel
    }

    /// The widget to place in the sidebar.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Recolors the matched words after a theme change, redrawing the results.
    pub fn set_accent(&self, accent: &str) {
        self.accent.replace(accent.to_string());
        let hits = self.hits.borrow().clone();
        self.set_hits(&hits, self.searching.get());
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
        self.entry.connect_changed(move |entry| f(entry.text().as_str()));
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
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let mut targets = Vec::new();

        for hit in hits {
            self.list.append(&heading_row(&hit.path, hit.snippets.len()));
            targets.push((hit.path.clone(), 0));
            for snippet in &hit.snippets {
                self.list.append(&snippet_row(snippet, &self.accent.borrow()));
                targets.push((hit.path.clone(), snippet.line));
            }
        }

        self.status.set_text(&status_text(hits, searching));
        *self.targets.borrow_mut() = targets;
        self.searching.set(searching);
        *self.hits.borrow_mut() = hits.to_vec();
    }
}

/// Folder and bare name of a note path, for the two-tone result heading.
fn split_path(path: &str) -> (String, String) {
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.strip_suffix(".md").unwrap_or(file);
    (folder.to_string(), stem.to_string())
}

/// The note line of a result group.
fn heading_row(path: &str, matches: usize) -> gtk::Box {
    let (folder, stem) = split_path(path);

    let row = gtk::Box::new(Orientation::Horizontal, 6);
    row.add_css_class("search-heading");

    let name = gtk::Label::builder().xalign(0.0).label(&stem).build();
    name.add_css_class("search-name");
    row.append(&name);

    if !folder.is_empty() {
        let where_it_lives = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Start)
            .label(folder)
            .build();
        where_it_lives.add_css_class("search-folder");
        row.append(&where_it_lives);
    }

    let count = gtk::Label::builder()
        .xalign(1.0)
        .hexpand(true)
        .label(matches.to_string())
        .build();
    count.add_css_class("search-count");
    row.append(&count);
    row
}

/// One matching line, with the query terms marked in the accent color.
fn snippet_row(snippet: &Snippet, accent: &str) -> gtk::Label {
    let positions = crate::search::highlight_positions(&snippet.text, &snippet.spans);
    let label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .use_markup(true)
        .label(crate::picker::highlight_colored(
            &snippet.text,
            &positions,
            accent,
        ))
        .build();
    label.add_css_class("search-snippet");
    label
}

/// The line under the entry: how many notes matched, or why none did.
fn status_text(hits: &[Hit], searching: bool) -> String {
    if !searching {
        return String::new();
    }
    match hits.len() {
        0 => "No matches".to_string(),
        1 => "1 note".to_string(),
        count => format!("{count} notes"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Hit, split_path, status_text};

    #[test]
    fn a_nested_note_splits_into_folder_and_name() {
        assert_eq!(
            split_path("notes/phase3-plan.md"),
            ("notes".to_string(), "phase3-plan".to_string())
        );
    }

    #[test]
    fn a_root_note_has_no_folder() {
        assert_eq!(
            split_path("plan.md"),
            (String::new(), "plan".to_string())
        );
    }

    #[test]
    fn a_deep_note_keeps_its_whole_folder_path() {
        assert_eq!(
            split_path("a/b/c.md"),
            ("a/b".to_string(), "c".to_string())
        );
    }

    fn hit(path: &str) -> Hit {
        Hit {
            path: path.to_string(),
            snippets: Vec::new(),
        }
    }

    #[test]
    fn an_idle_panel_says_nothing() {
        assert_eq!(status_text(&[], false), "");
    }

    #[test]
    fn an_empty_result_says_so() {
        assert_eq!(status_text(&[], true), "No matches");
    }

    #[test]
    fn one_match_is_singular() {
        assert_eq!(status_text(&[hit("a.md")], true), "1 note");
    }

    #[test]
    fn several_matches_are_counted() {
        assert_eq!(status_text(&[hit("a.md"), hit("b.md")], true), "2 notes");
    }
}
