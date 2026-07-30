#![warn(clippy::pedantic)]
//! jotter markdown source editor: a `GtkSourceView` view and buffer wearing the
//! generated theme style scheme, with markdown highlighting and editor settings.
//!
//! [`Editor::new`] must be called from a running GTK application (for example
//! inside `connect_activate`) because it constructs GTK widgets, which needs an
//! initialized display.

use std::cell::RefCell;
use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use sourceview5::prelude::*;

use jotter_theming::Theme;

/// A markdown source editor: a `GtkSourceView` view and buffer wearing the theme scheme.
pub struct Editor {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    // The View is wrapped in a ScrolledWindow so long documents scroll; this is what widget() returns.
    scroller: gtk::ScrolledWindow,
    /// Tag worn by a wikilink that resolves to an existing note.
    link_tag: gtk::TextTag,
    /// Tag worn by a wikilink with no note behind it.
    broken_link_tag: gtk::TextTag,
    /// Tag that flattens wikilink lookalikes the markdown grammar half-styles.
    inert_tag: gtk::TextTag,
    /// Byte ranges of the followable links, for the Ctrl+hover cursor.
    link_ranges: Rc<RefCell<Vec<Range<usize>>>>,
    /// The font rule, reloaded in place when the theme or the size changes.
    font_provider: gtk::CssProvider,
}

/// Maps byte offsets in a buffer to the (line, byte-in-line) pairs GTK wants.
///
/// Offsets arrive in document order, so the scan never restarts.
struct LineIndex<'a> {
    text: &'a str,
    line: i32,
    line_start: usize,
}

impl<'a> LineIndex<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            line: 0,
            line_start: 0,
        }
    }

    /// Line and byte-within-line for `offset`, or `None` if it is out of range.
    fn locate(&mut self, offset: usize) -> Option<(i32, i32)> {
        if offset > self.text.len() {
            return None;
        }
        while let Some(next) = self.text[self.line_start..offset].find('\n') {
            self.line_start += next + 1;
            self.line += 1;
        }
        i32::try_from(offset - self.line_start)
            .ok()
            .map(|column| (self.line, column))
    }
}

impl Editor {
    /// Build the editor, register the theme style scheme, and apply editor settings.
    ///
    /// Must run with an initialized GTK display (call from `connect_activate`).
    #[must_use]
    pub fn new(theme: &Theme) -> Self {
        let buffer = sourceview5::Buffer::new(None);

        // Markdown highlighting; if the language spec is missing we edit as plain text.
        if let Some(md) = sourceview5::LanguageManager::default().language("markdown") {
            buffer.set_language(Some(&md));
        }

        register_and_apply_scheme(&buffer, theme);
        buffer.set_highlight_matching_brackets(true);

        let view = sourceview5::View::with_buffer(&buffer);
        view.set_highlight_current_line(true);
        view.set_show_right_margin(true);
        view.set_right_margin_position(80);
        view.set_show_line_numbers(false);
        view.set_monospace(true);
        view.set_wrap_mode(gtk::WrapMode::WordChar);

        let font_provider = gtk::CssProvider::new();
        install_font(&view, &font_provider, theme);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&view)
            .build();

        let link_tag = gtk::TextTag::builder()
            .name(LINK_TAG)
            .foreground(&theme.editor.syntax.link)
            .underline(gtk::pango::Underline::Single)
            .build();
        let broken_link_tag = gtk::TextTag::builder()
            .name(BROKEN_LINK_TAG)
            .foreground(&theme.preview.muted)
            .underline(gtk::pango::Underline::Error)
            .build();
        let inert_tag = gtk::TextTag::builder()
            .name(INERT_TAG)
            .foreground(&theme.editor.foreground)
            .underline(gtk::pango::Underline::None)
            .build();
        let table = buffer.tag_table();
        table.add(&inert_tag);
        table.add(&link_tag);
        table.add(&broken_link_tag);

        // The highlight engine adds its tags lazily, outranking ours on a fresh
        // load, so take the top back every time it finishes a region.
        {
            let tags = [inert_tag.clone(), broken_link_tag.clone(), link_tag.clone()];
            buffer.connect_highlight_updated(move |buffer, _, _| {
                raise_tags(&buffer.tag_table(), &tags);
            });
        }

        let link_ranges: Rc<RefCell<Vec<Range<usize>>>> = Rc::new(RefCell::new(Vec::new()));
        wire_link_cursor(&view, &buffer, &link_ranges);

        Self {
            view,
            buffer,
            scroller,
            link_tag,
            broken_link_tag,
            inert_tag,
            link_ranges,
            font_provider,
        }
    }

    /// The widget to pack into a container: the `ScrolledWindow` wrapping the view.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.scroller.clone().upcast()
    }

    /// Current buffer text.
    #[must_use]
    pub fn text(&self) -> String {
        let (start, end) = self.buffer.bounds();
        self.buffer.text(&start, &end, true).to_string()
    }

    /// Load the initial document. Bracketed as an irreversible action so it stays
    /// out of the undo history and does not collide with `GtkSourceView`s own
    /// irreversible bracketing of the first content load (a user action there
    /// triggers the "cannot begin irreversible action while in user action" warning).
    pub fn set_initial_text(&self, text: &str) {
        self.buffer.begin_irreversible_action();
        self.buffer.set_text(text);
        self.buffer.end_irreversible_action();
        // set_text leaves the caret at the end; move it to the top so the first
        // edit -> preview toggle anchors at the document top rather than the last heading.
        self.buffer.place_cursor(&self.buffer.start_iter());
    }

    /// Replace the buffer text for later programmatic edits. Wrapped in
    /// `begin_user_action`/`end_user_action` so the undo history stays coherent.
    pub fn set_text(&self, text: &str) {
        self.buffer.begin_user_action();
        self.buffer.set_text(text);
        self.buffer.end_user_action();
    }

    /// Style byte ranges as wikilinks, replacing any previous ones.
    ///
    /// Each span is `(range, resolved)`. The ranges come from the wikilink
    /// scanner, so what lights up here is exactly what the preview links: a
    /// `[[x]]` inside code or after a backslash is not in the list and stays
    /// plain, which the markdown grammar alone cannot manage.
    pub fn set_link_spans(&self, spans: &[(Range<usize>, bool)], inert: &[Range<usize>]) {
        let (start, end) = self.buffer.bounds();
        for tag in [&self.link_tag, &self.broken_link_tag, &self.inert_tag] {
            self.buffer.remove_tag(tag, &start, &end);
        }

        let text = self.buffer.text(&start, &end, true);
        let mut lines = LineIndex::new(&text);
        // One shared cursor over the text means the spans must arrive in order.
        let tagged = spans
            .iter()
            .map(|(range, resolved)| {
                let tag = if *resolved {
                    &self.link_tag
                } else {
                    &self.broken_link_tag
                };
                (range, tag)
            })
            .chain(inert.iter().map(|range| (range, &self.inert_tag)));
        let mut tagged: Vec<_> = tagged.collect();
        tagged.sort_by_key(|(range, _)| range.start);

        for (range, tag) in tagged {
            let (Some(from), Some(to)) = (lines.locate(range.start), lines.locate(range.end)) else {
                continue;
            };
            let (Some(from), Some(to)) = (
                self.buffer.iter_at_line_index(from.0, from.1),
                self.buffer.iter_at_line_index(to.0, to.1),
            ) else {
                continue;
            };
            self.buffer.apply_tag(tag, &from, &to);
        }

        *self.link_ranges.borrow_mut() = spans.iter().map(|(range, _)| range.clone()).collect();
        raise_tags(
            &self.buffer.tag_table(),
            &[
                self.inert_tag.clone(),
                self.broken_link_tag.clone(),
                self.link_tag.clone(),
            ],
        );
    }

    /// Call `f` with the clicked byte offset in the buffer on a Ctrl+Click.
    ///
    /// The offset is in bytes, matching what the wikilink scanner reports.
    pub fn connect_ctrl_click<F: Fn(usize) + 'static>(&self, f: F) {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_PRIMARY);

        let view = self.view.clone();
        let buffer = self.buffer.clone();
        gesture.connect_pressed(move |gesture, _, x, y| {
            if !gesture
                .current_event_state()
                .contains(gtk::gdk::ModifierType::CONTROL_MASK)
            {
                return;
            }
            let Some(offset) = offset_at(&view, &buffer, x, y) else {
                return;
            };
            f(offset);
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });

        self.view.add_controller(gesture);
    }

    /// 0-based line of the caret.
    #[must_use]
    pub fn caret_line(&self) -> i32 {
        let mark = self.buffer.get_insert();
        let iter = self.buffer.iter_at_mark(&mark);
        iter.line()
    }

    /// Move the caret to a 0-based line (clamped to the buffer).
    pub fn set_caret_line(&self, line: i32) {
        let last = self.buffer.line_count() - 1;
        let target = line.clamp(0, last.max(0));
        if let Some(iter) = self.buffer.iter_at_line(target) {
            self.buffer.place_cursor(&iter);
        }
    }

    /// Put the caret on a 0-based line and scroll it into view.
    pub fn focus_line(&self, line: i32) {
        let last = self.buffer.line_count() - 1;
        let Some(iter) = self.buffer.iter_at_line(line.clamp(0, last.max(0))) else {
            return;
        };
        self.buffer.place_cursor(&iter);
        self.view
            .scroll_to_mark(&self.buffer.get_insert(), 0.0, true, 0.0, 0.3);
    }

    /// Re-apply a theme: the buffer's style scheme, the link tags, and the font.
    ///
    /// The font rule is reloaded on the provider installed at construction, so a
    /// size change lands live without registering another provider.
    pub fn set_theme(&self, theme: &Theme) {
        register_and_apply_scheme(&self.buffer, theme);
        apply_font(&self.font_provider, theme);
        self.link_tag.set_foreground(Some(&theme.editor.syntax.link));
        self.broken_link_tag
            .set_foreground(Some(&theme.preview.muted));
        self.inert_tag
            .set_foreground(Some(&theme.editor.foreground));
    }

    /// Give keyboard focus to the editor view.
    pub fn grab_focus(&self) {
        self.view.grab_focus();
    }

    /// Register a callback fired whenever the buffer text changes (used for debounce).
    pub fn connect_changed<F: Fn() + 'static>(&self, f: F) {
        self.buffer.connect_changed(move |_| f());
    }

    /// Byte offset of the caret, in the same units the wikilink scanner reports.
    #[must_use]
    pub fn caret_byte(&self) -> usize {
        let iter = self.buffer.iter_at_mark(&self.buffer.get_insert());
        self.buffer.text(&self.buffer.start_iter(), &iter, true).len()
    }

    /// Replace the buffer bytes in `range` with `text`, then put the caret
    /// `caret_offset` bytes past the start of what was replaced.
    pub fn replace_range(&self, range: Range<usize>, text: &str, caret_offset: usize) {
        let Some((from, to)) = self.iters_for(range.clone()) else {
            return;
        };
        self.buffer.delete(&mut from.clone(), &mut to.clone());
        let Some(mut at) = self.iter_at_byte(range.start) else {
            return;
        };
        self.buffer.insert(&mut at, text);
        if let Some(caret) = self.iter_at_byte(range.start + caret_offset) {
            self.buffer.place_cursor(&caret);
        }
    }

    /// Where the caret is on screen, in view coordinates, for anchoring a popover.
    #[must_use]
    pub fn caret_rect(&self) -> gtk::gdk::Rectangle {
        let iter = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let strong = self.view.cursor_locations(Some(&iter)).0;
        let (x, y) = self.view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            strong.x(),
            strong.y(),
        );
        gtk::gdk::Rectangle::new(x, y, strong.width(), strong.height())
    }

    /// The text widget itself, so a popover can parent onto it.
    #[must_use]
    pub fn text_view(&self) -> gtk::Widget {
        self.view.clone().upcast()
    }

    /// Run `f` whenever the caret moves, including moves caused by editing.
    pub fn connect_caret_moved<F: Fn() + 'static>(&self, f: F) {
        self.buffer
            .connect_cursor_position_notify(move |_| f());
    }

    /// Intercept a key press before the view handles it; `f` returns true to consume.
    pub fn connect_key_capture<F: Fn(gtk::gdk::Key) -> bool + 'static>(&self, f: F) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            if f(key) {
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        self.view.add_controller(keys);
    }

    fn iter_at_byte(&self, offset: usize) -> Option<gtk::TextIter> {
        let (start, end) = self.buffer.bounds();
        let text = self.buffer.text(&start, &end, true);
        let (line, column) = LineIndex::new(&text).locate(offset)?;
        self.buffer.iter_at_line_index(line, column)
    }

    fn iters_for(&self, range: Range<usize>) -> Option<(gtk::TextIter, gtk::TextIter)> {
        let (start, end) = self.buffer.bounds();
        let text = self.buffer.text(&start, &end, true);
        let mut lines = LineIndex::new(&text);
        let (from_line, from_col) = lines.locate(range.start)?;
        let (to_line, to_col) = lines.locate(range.end)?;
        Some((
            self.buffer.iter_at_line_index(from_line, from_col)?,
            self.buffer.iter_at_line_index(to_line, to_col)?,
        ))
    }
}

/// Tag name for a resolved wikilink in the source buffer.
const LINK_TAG: &str = "jotter-wikilink";
/// Tag name for a wikilink with no note behind it.
const BROKEN_LINK_TAG: &str = "jotter-wikilink-broken";
/// Tag name for a wikilink lookalike that is not a link.
const INERT_TAG: &str = "jotter-wikilink-inert";

/// Move `tags` above every syntax tag, last one highest.
fn raise_tags(table: &gtk::TextTagTable, tags: &[gtk::TextTag]) {
    let top = table.size() - 1;
    for tag in tags {
        tag.set_priority(top);
    }
}

/// Show a pointer cursor while Ctrl is held over a link, so it reads as clickable.
fn wire_link_cursor(
    view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    ranges: &Rc<RefCell<Vec<Range<usize>>>>,
) {
    let pointer = Rc::new(RefCell::new((0.0_f64, 0.0_f64)));

    let motion = gtk::EventControllerMotion::new();
    {
        let (view, buffer, ranges, pointer) = (
            view.clone(),
            buffer.clone(),
            Rc::clone(ranges),
            Rc::clone(&pointer),
        );
        motion.connect_motion(move |controller, x, y| {
            *pointer.borrow_mut() = (x, y);
            let held = controller
                .current_event_state()
                .contains(gtk::gdk::ModifierType::CONTROL_MASK);
            apply_link_cursor(&view, &buffer, &ranges, held, (x, y));
        });
    }
    {
        let view = view.clone();
        motion.connect_leave(move |_| view.set_cursor(None));
    }
    view.add_controller(motion);

    // Pressing or releasing Ctrl changes the cursor without the pointer moving.
    let keys = gtk::EventControllerKey::new();
    {
        let (view, buffer, ranges, pointer) = (
            view.clone(),
            buffer.clone(),
            Rc::clone(ranges),
            Rc::clone(&pointer),
        );
        keys.connect_modifiers(move |_, state| {
            let held = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            apply_link_cursor(&view, &buffer, &ranges, held, *pointer.borrow());
            gtk::glib::Propagation::Proceed
        });
    }
    view.add_controller(keys);
}

/// Set or clear the pointer cursor for the widget coordinates `at`.
fn apply_link_cursor(
    view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    ranges: &Rc<RefCell<Vec<Range<usize>>>>,
    ctrl_held: bool,
    at: (f64, f64),
) {
    let over_link = ctrl_held
        && offset_at(view, buffer, at.0, at.1)
            .is_some_and(|offset| ranges.borrow().iter().any(|r| r.contains(&offset)));
    if over_link {
        view.set_cursor_from_name(Some("pointer"));
    } else {
        view.set_cursor(None);
    }
}

/// Byte offset in the buffer under the widget coordinates `(x, y)`.
#[allow(clippy::cast_possible_truncation)]
fn offset_at(
    view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    x: f64,
    y: f64,
) -> Option<usize> {
    let (bx, by) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
    let iter = view.iter_at_location(bx, by)?;
    let start = buffer.start_iter();
    Some(buffer.text(&start, &iter, true).len())
}

/// Directory `GtkSourceView` searches for user style scheme XML files.
fn scheme_dir() -> PathBuf {
    let mut dir = gtk::glib::user_data_dir();
    dir.push("gtksourceview-5");
    dir.push("styles");
    dir
}

/// Write the generated scheme XML, register its directory, and set the scheme on
/// the buffer. `GtkSourceView` has no load-from-string, so we go through a file.
/// On any failure we leave the buffer with no scheme rather than panic.
fn register_and_apply_scheme(buffer: &sourceview5::Buffer, theme: &Theme) {
    let dir = scheme_dir();
    let scheme_id = theme.scheme_id();

    if fs::create_dir_all(&dir).is_ok() {
        let path = dir.join(format!("{scheme_id}.xml"));
        let _ = fs::write(&path, theme.to_sourceview_scheme_xml());
    }

    let manager = sourceview5::StyleSchemeManager::default();
    manager.append_search_path(&dir.to_string_lossy());
    // force_rescan picks up a scheme file written after the manager first scanned.
    manager.force_rescan();

    if let Some(scheme) = manager.scheme(&scheme_id) {
        buffer.set_style_scheme(Some(&scheme));
    }
}

/// CSS class the editor view carries so its font CSS targets only this widget.
const EDITOR_CSS_CLASS: &str = "jotter-editor-view";

/// Install the font provider once, scoped to the view by a css class.
///
/// CSS rather than a font property so the theme's `editor_font` and `font_size`
/// win, and one provider reloaded in place rather than a new one per change:
/// the per-widget `style_context` provider is deprecated since GTK 4.10, and
/// stacking display providers would leave every old size still registered.
fn install_font(view: &sourceview5::View, provider: &gtk::CssProvider, theme: &Theme) {
    view.add_css_class(EDITOR_CSS_CLASS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    apply_font(provider, theme);
}

/// Borrow helper, kept so the call above reads in one line.
/// Reload the font rule, which changes the editor's font family and size live.
fn apply_font(provider: &gtk::CssProvider, theme: &Theme) {
    let t = &theme.typography;
    provider.load_from_string(&format!(
        "textview.{class} {{ font-family: {family}; font-size: {size}px; }}",
        class = EDITOR_CSS_CLASS,
        family = t.editor_font,
        size = t.font_size,
    ));
}
