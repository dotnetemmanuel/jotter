#![warn(clippy::pedantic)]
//! jotter preview: a `WebKit` web view that renders parsed markdown HTML with the
//! active theme CSS injected as a user style sheet. Page JavaScript is off.
//!
//! To scroll to a heading anchor with JavaScript disabled, the rendered document
//! is written to a temp file and loaded by `file://` uri, then re-loaded with a
//! `#anchor` fragment so `WebKit` does the scrolling itself on load.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk::prelude::*;
use webkit6::prelude::*;
use webkit6::{
    Settings, UserContentInjectedFrames, UserContentManager, UserStyleLevel, UserStyleSheet,
    WebView,
};

/// The rendered-markdown preview: a `WebKit` web view with theme CSS injected.
pub struct Preview {
    view: WebView,
    /// Path of the temp file the last render was written to, if the write worked.
    base_path: RefCell<Option<PathBuf>>,
}

impl Preview {
    /// Build the preview. Page JavaScript is disabled. The theme preview CSS is
    /// injected as a `UserStyleSheet`.
    #[must_use]
    pub fn new(theme: &jotter_theming::Theme) -> Self {
        let ucm = UserContentManager::new();
        let style = UserStyleSheet::new(
            &theme.to_preview_css(),
            UserContentInjectedFrames::AllFrames,
            UserStyleLevel::User,
            &[],
            &[],
        );
        ucm.add_style_sheet(&style);

        let settings = Settings::new();
        settings.set_enable_javascript(false);
        settings.set_enable_javascript_markup(false);

        let view = WebView::builder()
            .user_content_manager(&ucm)
            .settings(&settings)
            .build();

        Self {
            view,
            base_path: RefCell::new(None),
        }
    }

    /// The widget to pack into a container (the `WebView` upcast to `gtk::Widget`).
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.view.clone().upcast()
    }

    /// Load an HTML fragment (wrap it in a minimal HTML document, utf-8).
    pub fn render(&self, html_fragment: &str) {
        let document = wrap_document(html_fragment);

        if let Some(path) = write_document(&document) {
            let uri = file_uri(&path);
            *self.base_path.borrow_mut() = Some(path);
            self.view.load_uri(&uri);
        } else {
            // Temp write failed: load inline, losing only fragment scrolling.
            *self.base_path.borrow_mut() = None;
            self.view.load_html(&document, None);
        }
    }

    /// Scroll to a heading anchor id. Best-effort. Called right after `render()`
    /// on toggle.
    pub fn scroll_to_anchor(&self, anchor: &str) {
        if let Some(path) = self.base_path.borrow().as_ref() {
            let uri = format!("{}#{anchor}", file_uri(path));
            self.view.load_uri(&uri);
        }
    }

    /// Set the zoom level proportional to the monitor scale factor to avoid blurry
    /// text under Wayland fractional scaling.
    pub fn set_zoom(&self, zoom: f64) {
        self.view.set_zoom_level(zoom);
    }
}

/// Write the document to `{user_cache_dir}/jotter/preview.html`, returning the
/// path on success and `None` on any IO error (caller falls back to inline).
fn write_document(document: &str) -> Option<PathBuf> {
    let dir = gtk::glib::user_cache_dir().join("jotter");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("preview.html");
    std::fs::write(&path, document).ok()?;
    Some(path)
}

/// Wrap an HTML body fragment in a minimal utf-8 document.
fn wrap_document(html_fragment: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{html_fragment}</body></html>"
    )
}

/// The `file://` uri for a filesystem path.
fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

#[cfg(test)]
mod tests {
    use super::{file_uri, wrap_document};
    use std::path::Path;

    #[test]
    fn wraps_fragment_in_utf8_document() {
        let doc = wrap_document("<h1>hi</h1>");
        assert!(doc.starts_with("<!doctype html>"));
        assert!(doc.contains("<meta charset=\"utf-8\">"));
        assert!(doc.contains("<body><h1>hi</h1></body>"));
    }

    #[test]
    fn builds_file_uri() {
        assert_eq!(
            file_uri(Path::new("/tmp/preview.html")),
            "file:///tmp/preview.html"
        );
    }
}
