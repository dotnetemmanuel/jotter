//! The shared results list: notes with the lines that matched, clickable to open
//! a note at one of them. Used by search, backlinks, tags, and the broken-link
//! report.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::Orientation;

use jotter_theming::Style;

use crate::search::Snippet;

/// One matching note and the lines worth showing from it.
#[derive(Clone)]
pub struct Hit {
    /// Vault-relative path of the note.
    pub path: String,
    /// Matching lines, already capped by the caller.
    pub snippets: Vec<Snippet>,
    /// Word shown where the line count usually goes, for lists that count nothing.
    pub badge: Option<String>,
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
    /// The visual language the rows are drawn in.
    style: Cell<Style>,
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
            style: Cell::new(Style::Classic),
        });

        let activated = Rc::clone(&shared);
        shared.rows.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            let target = activated.targets.borrow().get(index).cloned();
            if let Some((path, line)) = target {
                on_activate(&path, line);
            }
        });

        let cursored = Rc::clone(&shared);
        shared.rows.connect_row_selected(move |rows, selected| {
            let style = cursored.style.get();
            if style != Style::Tui {
                return;
            }
            let mut index = 0;
            while let Some(row) = rows.row_at_index(index) {
                let here = selected.is_some_and(|chosen| chosen == &row);
                set_row_cursor(&row, crate::style::cursor(style, here));
                index += 1;
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

    /// Selects a row and focuses it, so the arrows move from there: the selected
    /// one where there is one, else the first.
    ///
    /// Focusing the list alone leaves nothing selected, and the first arrow press
    /// then goes nowhere.
    pub fn focus_first(&self) {
        let row = self
            .rows
            .selected_row()
            .or_else(|| self.rows.row_at_index(0));
        match row {
            Some(row) => {
                self.rows.select_row(Some(&row));
                row.grab_focus();
            }
            None => {
                self.rows.grab_focus();
            }
        }
    }

    /// Recolors the matched words after a theme change, redrawing the rows.
    pub fn set_accent(&self, accent: &str) {
        self.accent.replace(accent.to_string());
        let hits = self.hits.borrow().clone();
        self.set_hits(&hits);
    }

    /// Redraws the rows in `style`, which changes the row cursor.
    pub fn set_style(&self, style: Style) {
        if self.style.get() == style {
            return;
        }
        self.style.set(style);
        let hits = self.hits.borrow().clone();
        self.set_hits(&hits);
    }

    /// Replaces the rows with `hits`, keeping the selected line where it survives.
    pub fn set_hits(&self, hits: &[Hit]) {
        let was_selected = self.selected_target();
        let had_focus = holds_focus_widget(&self.rows);
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        let mut targets = Vec::new();

        for hit in hits {
            self.rows.append(&heading_row(
                self.style.get(),
                &hit.path,
                hit.snippets.len(),
                hit.badge.as_deref(),
            ));
            targets.push((hit.path.clone(), 0));
            for snippet in &hit.snippets {
                self.rows.append(&snippet_row(snippet, &self.accent.borrow()));
                targets.push((hit.path.clone(), snippet.line));
            }
        }

        *self.targets.borrow_mut() = targets;
        *self.hits.borrow_mut() = hits.to_vec();

        let restored = was_selected.and_then(|target| self.row_for(&target));
        if let Some(row) = &restored {
            self.rows.select_row(Some(row));
        }
        if had_focus && let Some(row) = restored.or_else(|| self.rows.row_at_index(0)) {
            self.rows.select_row(Some(&row));
            set_focus_widget(&row);
        }
    }

    /// The note and line behind the selected row.
    fn selected_target(&self) -> Option<(String, i32)> {
        let index = usize::try_from(self.rows.selected_row()?.index()).ok()?;
        self.targets.borrow().get(index).cloned()
    }

    /// The row leading to `target`, once the rows have been rebuilt.
    fn row_for(&self, target: &(String, i32)) -> Option<gtk::ListBoxRow> {
        let index = self.targets.borrow().iter().position(|held| held == target)?;
        self.rows.row_at_index(i32::try_from(index).ok()?)
    }
}

/// Whether the window's focus widget sits inside `parent`.
///
/// Not the same as focus-within: a window that is not active still remembers
/// where the keyboard will land when it comes back, and a redraw that destroys
/// that widget loses it silently.
pub fn holds_focus_widget(parent: &impl IsA<gtk::Widget>) -> bool {
    let Some(window) = parent.as_ref().root().and_downcast::<gtk::Window>() else {
        return false;
    };
    let Some(focused) = GtkWindowExt::focus(&window) else {
        return false;
    };
    focused.is_ancestor(parent)
}

/// Points the window's focus widget at `row`, without activating the window.
pub fn set_focus_widget(row: &gtk::ListBoxRow) {
    if let Some(window) = row.root().and_downcast::<gtk::Window>() {
        GtkWindowExt::set_focus(&window, Some(row));
    }
}

/// Rewrites the cursor label of one result row, where it has one.
pub(crate) fn set_row_cursor(row: &gtk::ListBoxRow, mark: &str) {
    let Some(cursor) = row
        .child()
        .and_downcast::<gtk::Box>()
        .and_then(|line| line.first_child())
        .and_downcast::<gtk::Label>()
    else {
        return;
    };
    if cursor.has_css_class("row-cursor") {
        cursor.set_text(mark);
    }
}

/// Folder and bare name of a note path, for the two-tone result heading.
fn split_path(path: &str) -> (String, String) {
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.strip_suffix(".md").unwrap_or(file);
    (folder.to_string(), stem.to_string())
}

/// The note line of a result group.
fn heading_row(style: Style, path: &str, matches: usize, badge: Option<&str>) -> gtk::Box {
    let (folder, stem) = split_path(path);

    let row = gtk::Box::new(Orientation::Horizontal, 6);
    row.add_css_class("search-heading");

    let mark = crate::style::cursor(style, false);
    if !mark.is_empty() {
        let cursor = gtk::Label::builder().xalign(0.0).label(mark).build();
        cursor.add_css_class("row-cursor");
        row.append(&cursor);
    }

    let name = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .label(&stem)
        .build();
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
        .label(badge.map_or_else(|| matches.to_string(), str::to_string))
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
