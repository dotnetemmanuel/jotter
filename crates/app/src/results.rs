//! The shared results list: notes with the lines that matched, clickable to open
//! a note at one of them. Used by search, backlinks, tags, and the broken-link
//! report.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::Orientation;

use crate::search::Snippet;

/// One matching note and the lines worth showing from it.
#[derive(Clone)]
pub struct Hit {
    /// Vault-relative path of the note.
    pub path: String,
    /// Matching lines, already capped by the caller.
    pub snippets: Vec<Snippet>,
}

/// A list of hits. Rows activate through the callback given at construction.
pub struct List {
    rows: gtk::ListBox,
    /// Note path and line behind each row, parallel to the rows.
    targets: RefCell<Vec<(String, i32)>>,
    /// Color the matched words take, refreshed on a theme change.
    accent: RefCell<String>,
    /// The current hits, kept so a theme change can redraw them.
    hits: RefCell<Vec<Hit>>,
}

impl List {
    /// Builds the list. Rows activate through `on_activate(path, line)`.
    pub fn new<A: Fn(&str, i32) + 'static>(accent: &str, on_activate: A) -> Rc<Self> {
        let rows = gtk::ListBox::new();
        rows.set_selection_mode(gtk::SelectionMode::Single);
        rows.add_css_class("search-results");

        let shared = Rc::new(Self {
            rows,
            targets: RefCell::new(Vec::new()),
            accent: RefCell::new(accent.to_string()),
            hits: RefCell::new(Vec::new()),
        });

        let activated = Rc::clone(&shared);
        shared.rows.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            let target = activated.targets.borrow().get(index).cloned();
            if let Some((path, line)) = target {
                on_activate(&path, line);
            }
        });

        shared
    }

    /// The list widget, for placing in a container.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.rows.clone().upcast()
    }

    /// How many notes are currently listed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.hits.borrow().len()
    }

    /// Recolors the matched words after a theme change, redrawing the rows.
    pub fn set_accent(&self, accent: &str) {
        self.accent.replace(accent.to_string());
        let hits = self.hits.borrow().clone();
        self.set_hits(&hits);
    }

    /// Replaces the rows with `hits`.
    pub fn set_hits(&self, hits: &[Hit]) {
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        let mut targets = Vec::new();

        for hit in hits {
            self.rows.append(&heading_row(&hit.path, hit.snippets.len()));
            targets.push((hit.path.clone(), 0));
            for snippet in &hit.snippets {
                self.rows.append(&snippet_row(snippet, &self.accent.borrow()));
                targets.push((hit.path.clone(), snippet.line));
            }
        }

        *self.targets.borrow_mut() = targets;
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

#[cfg(test)]
mod tests {
    use super::split_path;

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
}
