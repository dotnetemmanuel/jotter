#![warn(clippy::pedantic)]
//! jotter markdown source editor: a `GtkSourceView` view and buffer wearing the
//! generated theme style scheme, with markdown highlighting and editor settings.
//!
//! [`Editor::new`] must be called from a running GTK application (for example
//! inside `connect_activate`) because it constructs GTK widgets, which needs an
//! initialized display.

use std::fs;
use std::path::PathBuf;

use gtk::prelude::*;
use sourceview5::prelude::*;

use jotter_theming::Theme;

/// A markdown source editor: a `GtkSourceView` view and buffer wearing the theme scheme.
pub struct Editor {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    // The View is wrapped in a ScrolledWindow so long documents scroll; this is what widget() returns.
    scroller: gtk::ScrolledWindow,
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

        apply_font(&view, theme);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&view)
            .build();

        Self {
            view,
            buffer,
            scroller,
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

    /// Re-apply a theme (for example after a light/dark switch): re-register and
    /// set the style scheme on the buffer. The font is mode-independent, so it is
    /// left as applied at construction to avoid stacking display providers.
    pub fn set_theme(&self, theme: &Theme) {
        register_and_apply_scheme(&self.buffer, theme);
    }

    /// Give keyboard focus to the editor view.
    pub fn grab_focus(&self) {
        self.view.grab_focus();
    }

    /// Register a callback fired whenever the buffer text changes (used for debounce).
    pub fn connect_changed<F: Fn() + 'static>(&self, f: F) {
        self.buffer.connect_changed(move |_| f());
    }
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

/// Apply the editor font family and size via a display-wide `CssProvider` scoped
/// by a css class on the view. We use CSS (not just monospace) so the theme
/// `editor_font` and `font_size` win. The per-widget `style_context` provider is
/// deprecated since GTK 4.10, so we register on the display and scope by class.
fn apply_font(view: &sourceview5::View, theme: &Theme) {
    view.add_css_class(EDITOR_CSS_CLASS);
    let t = &theme.typography;
    let css = format!(
        "textview.{class} {{ font-family: {family}; font-size: {size}px; }}",
        class = EDITOR_CSS_CLASS,
        family = t.editor_font,
        size = t.font_size,
    );
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
