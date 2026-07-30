#![warn(clippy::pedantic)]
//! jotter preview: a `WebKit` web view that renders parsed markdown HTML with the
//! active theme CSS embedded as an author `<style>` in each document. JavaScript
//! in the document markup is off (embedded `<script>` and inline handlers are
//! stripped), but host-initiated JavaScript is on so we can drive scrolling. The
//! `<style>` route cascades predictably where injected `UserStyleSheet`s did not
//! (they dropped cell padding or the body background).
//!
//! To scroll to a heading anchor, the rendered document is written to a temp file
//! and loaded by `file://` uri; once that load finishes we run a small host script
//! that calls `scrollIntoView` on the anchor element. Scrolling via script rather
//! than a `#anchor` fragment keeps the current uri fragment-free, so a later
//! `reload_bypass_cache` (used by the in-place theme recolor) truly preserves the
//! reader scroll instead of snapping back to the fragment.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use webkit6::prelude::*;
use webkit6::{
    LoadEvent, NavigationPolicyDecision, NavigationType, PolicyDecisionType, Settings, WebView,
};

/// The rendered-markdown preview: a `WebKit` web view with theme CSS injected.
pub struct Preview {
    view: WebView,
    /// Theme CSS embedded as an author `<style>` in each rendered document;
    /// swapped on a light/dark switch and picked up by the next render.
    css: RefCell<String>,
    /// `WebKit` zoom, kept so the body size can be divided back out of it: the
    /// zoom exists to render crisply on a scaled display, not to make the
    /// preview's text bigger than the editor's.
    zoom: RefCell<f64>,
    /// The theme's own CSS, kept apart from the zoom correction so recomposing
    /// replaces that correction instead of stacking another one on top.
    theme_css: RefCell<String>,
    /// Path of the temp file the current render was written to; shared with the
    /// load-finished handler so it can scroll the loaded page to the anchor.
    base_path: Rc<RefCell<Option<PathBuf>>>,
    /// Heading anchor to scroll to once the current fresh load finishes, if any.
    pending_anchor: Rc<RefCell<Option<String>>>,
    /// Per-render counter so each render loads a fresh `URI` `WebKit` cannot cache.
    counter: RefCell<u64>,
}

impl Preview {
    /// Build the preview. JavaScript in the document markup is disabled (embedded
    /// scripts are stripped) while host-initiated JavaScript stays on so we can
    /// scroll the page. The theme preview CSS is embedded as an author `<style>`
    /// in each rendered document.
    #[must_use]
    pub fn new(theme: &jotter_theming::Theme) -> Self {
        let settings = Settings::new();
        settings.set_enable_javascript(true);
        settings.set_enable_javascript_markup(false);

        let view = WebView::builder().settings(&settings).build();

        let base_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let pending_anchor: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        // Scroll to the heading anchor once the fresh page finishes loading, using
        // a host script so the current uri stays fragment-free (a fragment baked
        // into the current uri would re-snap on the theme recolor reload).
        {
            let pending_anchor = Rc::clone(&pending_anchor);
            view.connect_load_changed(move |view, event| {
                if event != LoadEvent::Finished {
                    return;
                }
                if let Some(anchor) = pending_anchor.borrow_mut().take() {
                    view.evaluate_javascript(
                        &scroll_to_anchor_js(&anchor),
                        None,
                        None,
                        None::<&gtk::gio::Cancellable>,
                        |_| {},
                    );
                }
            });
        }

        Self {
            view,
            css: RefCell::new(theme.to_preview_css()),
            zoom: RefCell::new(1.0),
            theme_css: RefCell::new(theme.to_preview_css()),
            base_path,
            pending_anchor,
            counter: RefCell::new(0),
        }
    }

    /// Call `f` with the target uri whenever the reader clicks a link, and stop
    /// the preview from navigating there itself.
    ///
    /// Only user link clicks are intercepted; our own document loads are not.
    pub fn connect_link_clicked<F: Fn(&str) + 'static>(&self, f: F) {
        self.view.connect_decide_policy(move |_, decision, kind| {
            if kind != PolicyDecisionType::NavigationAction {
                return false;
            }
            let Ok(navigation) = decision.clone().downcast::<NavigationPolicyDecision>() else {
                return false;
            };
            let Some(action) = navigation.navigation_action() else {
                return false;
            };
            if action.navigation_type() != NavigationType::LinkClicked {
                return false;
            }
            let Some(uri) = action.request().and_then(|request| request.uri()) else {
                return false;
            };
            decision.ignore();
            f(&uri);
            true
        });
    }

    /// Swap the theme CSS (for example after a light/dark switch). The change is
    /// applied on the next `render`; the caller re-renders if the preview shows.
    pub fn set_theme(&self, theme: &jotter_theming::Theme) {
        *self.theme_css.borrow_mut() = theme.to_preview_css();
        self.rescale_body();
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
    /// Returns whether this differs from the zoom already in use, so a caller can
    /// repaint: a page rendered before the real scale was known carries the
    /// uncorrected size until something redraws it.
    pub fn set_zoom(&self, zoom: f64) -> bool {
        let changed = (*self.zoom.borrow() - zoom).abs() > 0.001;
        self.zoom.replace(zoom);
        self.view.set_zoom_level(zoom);
        self.rescale_body();
        changed
    }

    /// Appends a body rule that divides the zoom back out of the base size.
    ///
    /// The zoom is there so glyphs render crisply on a fractionally scaled
    /// display; without this the preview's body text would also come out that
    /// much larger than the editor's, which is the same document at the same
    /// size and should look it. Headings keep their `em` multiples, so they
    /// still scale from the body rather than from a fixed number.
    fn rescale_body(&self) {
        *self.css.borrow_mut() = compose_css(&self.theme_css.borrow(), *self.zoom.borrow());
    }
}

/// The theme CSS with the zoom divided back out of the body size.
///
/// Composed from the theme each time rather than appended to what is already
/// there: appending left one stale rule per zoom change in the document, and the
/// last one to land is not necessarily the one matching the current zoom.
fn compose_css(theme_css: &str, zoom: f64) -> String {
    let Some(base) = base_font_size(theme_css).filter(|_| zoom > 0.0) else {
        return theme_css.to_owned();
    };
    let corrected = f64::from(base) / zoom;
    format!("{theme_css}\nbody {{ font-size: {corrected:.3}px; }}\n")
}

/// The `body` font size the theme's preview CSS asks for, in pixels.
fn base_font_size(css: &str) -> Option<u32> {
    let body = css.find("body {")?;
    let size = css[body..].find("font-size:")? + body + "font-size:".len();
    let rest = &css[size..];
    let end = rest.find("px")?;
    rest[..end].trim().parse().ok()
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

/// Host script that scrolls the anchor element to the top of the viewport, or
/// does nothing when no element has that id. The `true` argument aligns the
/// element to the top, matching the old `#anchor` fragment behaviour.
fn scroll_to_anchor_js(anchor: &str) -> String {
    format!(
        "(function(){{var e=document.getElementById({});if(e)e.scrollIntoView(true);}})();",
        js_string_literal(anchor)
    )
}

/// Encode `s` as a double-quoted JavaScript string literal, escaping the few
/// characters that could otherwise break out of the literal (a heading slug is
/// unlikely to contain them, but the injection must be safe regardless).
fn js_string_literal(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{file_uri, js_string_literal, preview_file_name, scroll_to_anchor_js, wrap_document};
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

    #[test]
    fn scroll_script_targets_the_anchor_id() {
        let js = scroll_to_anchor_js("my-heading");
        assert!(js.contains("getElementById(\"my-heading\")"));
        assert!(js.contains("scrollIntoView(true)"));
    }

    #[test]
    fn js_string_literal_escapes_quotes_and_backslashes() {
        assert_eq!(js_string_literal("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(js_string_literal("plain"), "\"plain\"");
    }
}

#[cfg(test)]
mod zoom_correction {
    use super::compose_css;

    const THEME: &str = "body {\n  margin: 0;\n  font-size: 15px;\n}\n\nh1 { font-size: 2.4em; }\n";

    #[test]
    fn the_body_size_divides_the_zoom_back_out() {
        let css = compose_css(THEME, 1.25);
        assert!(css.contains("body { font-size: 12.000px; }"));
    }

    /// The correction the browser actually applies: the last body rule wins.
    fn effective(css: &str) -> f64 {
        let rule = css.rmatch_indices("body { font-size:").next().expect("a correction");
        let rest = &css[rule.0 + rule.1.len()..];
        rest[..rest.find("px").expect("px")].trim().parse().expect("a number")
    }

    #[test]
    fn each_zoom_leaves_exactly_one_correction() {
        // Every zoom change used to append, leaving one stale rule per change.
        for zoom in [1.0, 1.25, 2.0] {
            let css = compose_css(THEME, zoom);
            assert_eq!(css.matches("body { font-size:").count(), 1, "zoom {zoom}");
        }
    }

    #[test]
    fn the_correction_always_matches_the_current_zoom() {
        for zoom in [1.0, 1.2, 1.25, 1.5, 2.0] {
            let css = compose_css(THEME, zoom);
            // What the reader sees: the corrected size scaled back up by the zoom.
            let rendered = effective(&css) * zoom;
            assert!((rendered - 15.0).abs() < 0.01, "zoom {zoom} rendered {rendered}");
        }
    }

    #[test]
    fn a_nonsense_zoom_leaves_the_theme_alone() {
        assert_eq!(compose_css(THEME, 0.0), THEME);
        assert_eq!(compose_css("p { color: red; }", 1.25), "p { color: red; }");
    }
}
