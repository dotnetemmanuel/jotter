//! The overlay picker: an entry with a filtered list under it. It knows nothing
//! about notes or commands; callers supply the rows and handle activation.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Align, ListView, Orientation, ScrolledWindow, SignalListItemFactory, SingleSelection, gdk, glib,
};
use jotter_theming::Style;

/// A live picker, so the caller can close it or switch it to another mode.
#[derive(Clone)]
pub struct Handle {
    entry: gtk::Entry,
    close: Rc<dyn Fn()>,
}

impl Handle {
    /// The text currently in the entry.
    #[must_use]
    pub fn query(&self) -> String {
        self.entry.text().to_string()
    }

    /// Replaces the query, refilling the list and leaving the caret at the end.
    pub fn set_query(&self, text: &str) {
        self.entry.set_text(text);
        self.entry.set_position(-1);
        self.entry.grab_focus();
    }

    /// Dismisses the picker as Escape would.
    pub fn close(&self) {
        (self.close)();
    }
}

/// One line in the picker list.
pub struct Row {
    /// Opaque identifier handed back on activation.
    pub key: String,
    /// Primary text, the one the query matched against.
    pub label: String,
    /// Secondary text shown dimmed after the label.
    pub detail: String,
    /// Byte offsets into `label` to highlight, ascending.
    pub positions: Vec<usize>,
}

/// Opens the picker over `overlay`, seeded with `initial_query`.
///
/// `source` re-runs on every keystroke and returns the rows to show. `title`
/// re-runs on every keystroke too, so a picker whose mode can change as the
/// user types (the switcher turning into the palette) keeps an honest
/// heading. `activate` receives the key of the chosen row, `on_close` runs
/// after the panel goes away either way, so callers can restore focus.
#[allow(clippy::too_many_arguments, reason = "each argument is a distinct collaborator, not a bundle waiting to happen")]
pub fn open<T, S, A, C>(
    overlay: &gtk::Overlay,
    style: Style,
    title: T,
    placeholder: &str,
    initial_query: &str,
    source: S,
    activate: A,
    on_close: C,
) -> Handle
where
    T: Fn(&str) -> String + 'static,
    S: Fn(&str) -> Vec<Row> + 'static,
    A: Fn(&str) + 'static,
    C: Fn() + 'static,
{
    let rows: Rc<RefCell<Vec<Row>>> = Rc::new(RefCell::new(Vec::new()));
    let source = Rc::new(source);
    let activate = Rc::new(activate);
    let on_close = Rc::new(on_close);

    let (panel, entry, selection, list_view) =
        build_panel(style, title, placeholder, initial_query, &rows);

    // Covers the window so a click anywhere outside the panel dismisses it.
    let scrim = gtk::Box::new(Orientation::Vertical, 0);
    scrim.add_css_class("picker-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    overlay.add_overlay(&scrim);
    overlay.add_overlay(&panel);

    let refill = {
        let rows = Rc::clone(&rows);
        let source = Rc::clone(&source);
        let selection = selection.clone();
        let list_view = list_view.clone();
        move |query: &str| {
            let found = source(query);
            let keys: Vec<&str> = found.iter().map(|row| row.key.as_str()).collect();
            let model = gtk::StringList::new(&keys);
            drop(keys);
            *rows.borrow_mut() = found;
            selection.set_model(Some(&model));
            if selection.n_items() > 0 {
                selection.set_selected(0);
                list_view.scroll_to(0, gtk::ListScrollFlags::empty(), None);
            }
        }
    };
    refill(initial_query);
    let refill = Rc::new(refill);

    let close = {
        let overlay = overlay.clone();
        let panel = panel.clone();
        let scrim = scrim.clone();
        let on_close = Rc::clone(&on_close);
        let close: Rc<dyn Fn()> = Rc::new(move || {
            overlay.remove_overlay(&panel);
            overlay.remove_overlay(&scrim);
            on_close();
        });
        close
    };

    let outside = gtk::GestureClick::new();
    outside.connect_pressed({
        let close = Rc::clone(&close);
        move |_, _, _, _| close()
    });
    scrim.add_controller(outside);

    let choose = {
        let rows = Rc::clone(&rows);
        let selection = selection.clone();
        let activate = Rc::clone(&activate);
        let close = Rc::clone(&close);
        let choose: Rc<dyn Fn()> = Rc::new(move || {
            let position = usize::try_from(selection.selected()).unwrap_or(usize::MAX);
            let key = rows.borrow().get(position).map(|row| row.key.clone());
            close();
            if let Some(key) = key {
                activate(&key);
            }
        });
        choose
    };

    entry.connect_changed({
        let refill = Rc::clone(&refill);
        move |entry| refill(entry.text().as_str())
    });

    list_view.connect_activate({
        let choose = Rc::clone(&choose);
        let selection = selection.clone();
        move |_, position| {
            selection.set_selected(position);
            choose();
        }
    });

    wire_keys(&panel, &selection, &list_view, &choose, &close);

    entry.grab_focus();
    entry.set_position(-1);

    Handle { entry, close }
}

/// The floating panel: query entry above a list, centred near the top.
fn build_panel<T>(
    style: Style,
    title: T,
    placeholder: &str,
    initial_query: &str,
    rows: &Rc<RefCell<Vec<Row>>>,
) -> (gtk::Box, gtk::Entry, SingleSelection, ListView)
where
    T: Fn(&str) -> String + 'static,
{
    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .text(initial_query)
        .build();

    let selection = SingleSelection::new(Some(gtk::StringList::new(&[])));
    let list_view = ListView::new(Some(selection.clone()), Some(row_factory(style, rows)));
    list_view.add_css_class("picker-list");
    // A ListView activates on double click by default. In a palette you have
    // already chosen by the time you reach for the mouse.
    list_view.set_single_click_activate(true);

    let scroller = ScrolledWindow::builder()
        .max_content_height(360)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list_view)
        .build();

    let panel = gtk::Box::new(Orientation::Vertical, 0);
    panel.add_css_class("picker");
    panel.set_halign(Align::Center);
    panel.set_valign(Align::Start);
    panel.set_margin_top(56);
    panel.set_size_request(560, -1);

    if style == Style::Tui {
        let heading = gtk::Label::builder()
            .xalign(0.0)
            .label(crate::style::heading(style, &title(initial_query)))
            .build();
        heading.add_css_class("picker-title");
        panel.append(&heading);

        let line = gtk::Box::new(Orientation::Horizontal, 0);
        let prompt = gtk::Label::new(Some(">"));
        prompt.add_css_class("picker-prompt");
        // A typed leading > is its own prompt; the chrome supplies one otherwise.
        prompt.set_visible(!initial_query.starts_with('>'));
        line.append(&prompt);
        entry.set_hexpand(true);
        line.append(&entry);
        panel.append(&line);

        entry.connect_changed(move |entry| {
            let text = entry.text();
            heading.set_text(&crate::style::heading(style, &title(&text)));
            prompt.set_visible(!text.starts_with('>'));
        });
    } else {
        panel.append(&entry);
    }
    panel.append(&scroller);

    (panel, entry, selection, list_view)
}
/// Builds the row factory, reading each row out of `rows` on bind.
fn row_factory(style: Style, rows: &Rc<RefCell<Vec<Row>>>) -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let line = gtk::Box::new(Orientation::Horizontal, 8);
        let cursor = gtk::Label::builder().xalign(0.0).build();
        cursor.add_css_class("row-cursor");
        cursor.set_visible(style == Style::Tui);
        line.append(&cursor);
        let label = gtk::Label::builder().xalign(0.0).use_markup(true).build();
        let detail = gtk::Label::builder().xalign(0.0).build();
        detail.add_css_class("picker-detail");
        line.append(&label);
        line.append(&detail);
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        item.set_child(Some(&line));

        item.connect_notify_local(Some("selected"), move |item, _| {
            cursor.set_text(crate::style::cursor(style, item.is_selected()));
        });
    });
    factory.connect_bind({
        let rows = Rc::clone(rows);
        move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            let Some(line) = item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let Some(cursor) = line.first_child().and_downcast::<gtk::Label>() else {
                return;
            };
            let Some(label) = cursor.next_sibling().and_downcast::<gtk::Label>() else {
                return;
            };
            let Some(detail) = label.next_sibling().and_downcast::<gtk::Label>() else {
                return;
            };
            cursor.set_text(crate::style::cursor(style, item.is_selected()));
            let rows = rows.borrow();
            let position = usize::try_from(item.position()).unwrap_or(usize::MAX);
            if let Some(row) = rows.get(position) {
                label.set_markup(&highlight(&row.label, &row.positions));
                detail.set_text(&row.detail);
            }
        }
    });
    factory
}

/// Arrow keys move the selection, Enter chooses, Escape dismisses.
///
/// Capture phase, so the entry cannot swallow a key the picker owns.
fn wire_keys(
    panel: &gtk::Box,
    selection: &SingleSelection,
    list_view: &ListView,
    choose: &Rc<dyn Fn()>,
    close: &Rc<dyn Fn()>,
) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let selection = selection.clone();
        let list_view = list_view.clone();
        let choose = Rc::clone(choose);
        let close = Rc::clone(close);
        move |_, key, _, _| match key {
            gdk::Key::Escape => {
                close();
                glib::Propagation::Stop
            }
            gdk::Key::Down => {
                step(&selection, &list_view, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Up => {
                step(&selection, &list_view, -1);
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                choose();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    panel.add_controller(keys);
}

/// Moves the selection by `delta`, clamped to the list, and scrolls it into view.
fn step(selection: &SingleSelection, list_view: &ListView, delta: i32) {
    let count = selection.n_items();
    if count == 0 {
        return;
    }
    let current = i64::from(selection.selected());
    let last = i64::from(count - 1);
    let next = (current + i64::from(delta)).clamp(0, last);
    let next = u32::try_from(next).unwrap_or(0);
    selection.set_selected(next);
    list_view.scroll_to(next, gtk::ListScrollFlags::empty(), None);
}

/// Wraps the matched characters of `label` in bold, escaping the rest for Pango.
pub fn highlight(label: &str, positions: &[usize]) -> String {
    marked(label, positions, None)
}

/// Bold plus a color on the matched characters, for surfaces where bold alone
/// does not stand out.
pub fn highlight_colored(label: &str, positions: &[usize], color: &str) -> String {
    marked(label, positions, Some(color))
}

fn marked(label: &str, positions: &[usize], color: Option<&str>) -> String {
    let mut out = String::with_capacity(label.len());
    let mut bold = false;
    for (offset, character) in label.char_indices() {
        let matched = positions.binary_search(&offset).is_ok();
        if matched && !bold {
            match color {
                Some(color) => {
                    out.push_str("<b><span foreground=\"");
                    out.push_str(color);
                    out.push_str("\">");
                }
                None => out.push_str("<b>"),
            }
            bold = true;
        } else if !matched && bold {
            out.push_str(close_tag(color));
            bold = false;
        }
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(character),
        }
    }
    if bold {
        out.push_str(close_tag(color));
    }
    out
}

fn close_tag(color: Option<&str>) -> &'static str {
    if color.is_some() { "</span></b>" } else { "</b>" }
}

#[cfg(test)]
mod tests {
    use super::{highlight, highlight_colored};

    #[test]
    fn a_colored_highlight_carries_the_color_and_the_bold() {
        assert_eq!(
            highlight_colored("plan", &[0, 1], "#ff8800"),
            "<b><span foreground=\"#ff8800\">pl</span></b>an"
        );
    }

    #[test]
    fn a_colored_highlight_closes_a_trailing_run() {
        assert_eq!(
            highlight_colored("pl", &[0, 1], "#ff8800"),
            "<b><span foreground=\"#ff8800\">pl</span></b>"
        );
    }

    #[test]
    fn plain_text_when_nothing_matched() {
        assert_eq!(highlight("plan.md", &[]), "plan.md");
    }

    #[test]
    fn bolds_the_matched_characters() {
        assert_eq!(highlight("plan", &[0, 1]), "<b>pl</b>an");
    }

    #[test]
    fn merges_an_adjacent_run_into_one_span() {
        assert_eq!(highlight("plan", &[0, 1, 2, 3]), "<b>plan</b>");
    }

    #[test]
    fn separates_runs_that_are_not_adjacent() {
        assert_eq!(highlight("plan", &[0, 3]), "<b>p</b>la<b>n</b>");
    }

    #[test]
    fn escapes_markup_characters_in_the_label() {
        assert_eq!(highlight("a<b>&c", &[]), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn escapes_a_matched_markup_character() {
        assert_eq!(highlight("<x", &[0]), "<b>&lt;</b>x");
    }

    #[test]
    fn handles_multibyte_labels() {
        assert_eq!(highlight("café", &[2]), "ca<b>f</b>é");
        assert_eq!(highlight("café", &[3]), "caf<b>é</b>");
    }
}

