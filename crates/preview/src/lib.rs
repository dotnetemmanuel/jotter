#![warn(clippy::pedantic)]
//! jotter preview: a `WebKit` web view that renders parsed markdown HTML with the
//! active theme CSS embedded as an author `<style>` in each document. Page
//! JavaScript is off. The `<style>` route cascades predictably where injected
//! `UserStyleSheet`s did not (they dropped cell padding or the body background).
//!
//! To scroll to a heading anchor with JavaScript disabled, the rendered document
//! is written to a temp file and loaded by `file://` uri; once that load finishes
//! the same uri is re-loaded with a `#anchor` fragment, which `WebKit` treats as
//! a same-document scroll. Scrolling only after the fresh load has committed is
//! what makes it reliable.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use webkit6::prelude::*;
use webkit6::{LoadEvent, Settings, WebView};

/// The rendered-markdown preview: a `WebKit` web view with theme CSS injected.
pub struct Preview {
    view: WebView,
    /// Theme CSS embedded as an author `<style>` in each rendered document;
    /// swapped on a light/dark switch and picked up by the next render.
    css: RefCell<String>,
    /// Path of the temp file the current render was written to; shared with the
    /// load-finished handler so it can scroll the loaded page to the anchor.
    base_path: Rc<RefCell<Option<PathBuf>>>,
    /// Heading anchor to scroll to once the current fresh load finishes, if any.
    pending_anchor: Rc<RefCell<Option<String>>>,
    /// Per-render counter so each render loads a fresh `URI` `WebKit` cannot cache.
    counter: RefCell<u64>,
}

impl Preview {
    /// Build the preview. Page JavaScript is disabled. The theme preview CSS is
    /// embedded as an author `<style>` in each rendered document.
    #[must_use]
    pub fn new(theme: &jotter_theming::Theme) -> Self {
        let settings = Settings::new();
        settings.set_enable_javascript(false);
        settings.set_enable_javascript_markup(false);

        let view = WebView::builder().settings(&settings).build();

        let base_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let pending_anchor: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        // Scroll to the heading anchor only once the fresh page finishes loading:
        // a fragment navigation on an already-loaded document is a reliable
        // same-document scroll, unlike a fragment baked into the fresh load.
        {
            let base_path = Rc::clone(&base_path);
            let pending_anchor = Rc::clone(&pending_anchor);
            view.connect_load_changed(move |view, event| {
                if event != LoadEvent::Finished {
                    return;
                }
                let anchor = pending_anchor.borrow_mut().take();
                if let Some(anchor) = anchor
                    && let Some(path) = base_path.borrow().as_ref()
                {
                    view.load_uri(&format!("{}#{anchor}", file_uri(path)));
                }
            });
        }

        Self {
            view,
            css: RefCell::new(theme.to_preview_css()),
            base_path,
            pending_anchor,
            counter: RefCell::new(0),
        }
    }

    /// Swap the theme CSS (for example after a light/dark switch). The change is
    /// applied on the next `render`; the caller re-renders if the preview shows.
    pub fn set_theme(&self, theme: &jotter_theming::Theme) {
        *self.css.borrow_mut() = theme.to_preview_css();
    }

    /// The widget to pack into a container (the `WebView` upcast to `gtk::Widget`).
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.view.clone().upcast()
    }

    /// Load an HTML fragment (wrap it in a minimal HTML document, utf-8).
    pub fn render(&self, html_fragment: &str, anchor: Option<&str>) {
        let document = wrap_document(html_fragment, &self.css.borrow());
        let n = {
            let mut c = self.counter.borrow_mut();
            *c += 1;
            *c
        };

        if let Some(path) = write_document(&document, n) {
            let uri = file_uri(&path);
            // Load the fresh file with no fragment; the load-finished handler then
            // scrolls to the anchor as a reliable same-document navigation.
            *self.pending_anchor.borrow_mut() = anchor.map(str::to_owned);
            // Swap in the new file, then delete the previous render (WebKit already loaded it).
            let previous = self.base_path.borrow_mut().replace(path);
            self.view.load_uri(&uri);
            if let Some(old) = previous {
                let _ = std::fs::remove_file(old);
            }
        } else {
            // Temp write failed: load inline, losing only fragment scrolling.
            *self.pending_anchor.borrow_mut() = None;
            if let Some(old) = self.base_path.borrow_mut().take() {
                let _ = std::fs::remove_file(old);
            }
            self.view.load_html(&document, None);
        }
    }

    /// Re-render in place preserving the current scroll position, used on a theme
    /// switch. Overwriting the current file and reloading past the cache keeps the
    /// scroll offset (a reload ignores the url fragment), unlike loading a fresh
    /// file which resets to the top and flashes before scrolling back.
    pub fn rerender_preserving_scroll(&self, html_fragment: &str) {
        let document = wrap_document(html_fragment, &self.css.borrow());
        let path = self.base_path.borrow().clone();
        if let Some(path) = path
            && std::fs::write(&path, &document).is_ok()
        {
            self.view.reload_bypass_cache();
        } else {
            // No current file yet (or the write failed): fall back to a fresh load.
            self.render(html_fragment, None);
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
fn write_document(document: &str, n: u64) -> Option<PathBuf> {
    let dir = gtk::glib::user_cache_dir().join("jotter");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(preview_file_name(n));
    std::fs::write(&path, document).ok()?;
    Some(path)
}

/// Unique per-render file name so each load has a distinct `URI` `WebKit` will
/// not serve from its `file://` cache.
fn preview_file_name(n: u64) -> String {
    format!("preview-{n}.html")
}

/// Wrap an HTML body fragment in a minimal utf-8 document with the theme CSS in
/// an author `<style>`, so it cascades over `WebKit` defaults predictably.
fn wrap_document(html_fragment: &str, css: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>{css}</style></head><body>{html_fragment}</body></html>"
    )
}

/// The `file://` uri for a filesystem path.
fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

#[cfg(test)]
mod tests {
    use super::{file_uri, preview_file_name, wrap_document};
    use std::path::Path;

    #[test]
    fn wraps_fragment_in_utf8_document() {
        let doc = wrap_document("<h1>hi</h1>", "body { color: red; }");
        assert!(doc.starts_with("<!doctype html>"));
        assert!(doc.contains("<meta charset=\"utf-8\">"));
        assert!(doc.contains("<style>body { color: red; }</style>"));
        assert!(doc.contains("<body><h1>hi</h1></body>"));
    }

    #[test]
    fn builds_file_uri() {
        assert_eq!(
            file_uri(Path::new("/tmp/preview.html")),
            "file:///tmp/preview.html"
        );
    }

    #[test]
    fn preview_file_name_is_unique_per_counter() {
        assert_eq!(preview_file_name(0), "preview-0.html");
        assert_ne!(preview_file_name(1), preview_file_name(2));
    }
}
