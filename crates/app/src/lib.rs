#![warn(clippy::pedantic)]
//! Glue crate for jotter. Owns the GTK application, the shared UI state, and the
//! edit-preview toggle loop. The binary stays thin and only calls `run`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib::SourceId;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, CssProvider, HeaderBar, Stack, StackTransitionType, gdk,
};

use jotter_editor::Editor;
use jotter_preview::Preview;
use jotter_theming::{Mode, Theme, ThemeFile};

/// Reverse-DNS application id, stable for GTK settings and Wayland app matching.
const APP_ID: &str = "dev.jotter.Jotter";

/// Stack page name for the editor.
const PAGE_EDIT: &str = "edit";
/// Stack page name for the preview.
const PAGE_PREVIEW: &str = "preview";

/// Re-render debounce for an already-open preview, in milliseconds.
const DEBOUNCE_MS: u64 = 150;

/// Fallback document shown when no path is given or a read fails.
const SAMPLE_MARKDOWN: &str = "# jotter\n\nA native GTK4 markdown vault.\n\n## Toggle\n\nPress Ctrl+E to switch between edit and preview.\n\n## Code\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n";

/// Shared, single-threaded application state cloned into GTK closures.
struct State {
    editor: Editor,
    preview: Preview,
    stack: Stack,
    /// Active resolved theme; swapped in place on a light/dark toggle.
    theme: RefCell<Theme>,
    /// Parsed theme source, re-resolved for the other mode on toggle.
    theme_file: ThemeFile,
    /// Display-level chrome CSS provider, restyled in place on toggle.
    chrome_provider: CssProvider,
    /// Caret line (0-based) cached when leaving the editor, restored on return.
    cached_caret: RefCell<i32>,
    /// Pending debounced re-render, removed and replaced on each buffer change.
    pending: RefCell<Option<SourceId>>,
}

/// Build the application, wire the theme and UI, and run the GTK loop.
///
/// Returns the process exit code from `gtk::Application::run`.
#[must_use]
pub fn run() -> gtk::glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    // First positional arg after the program name, if any, is a markdown path.
    let file_arg: Rc<Option<String>> = Rc::new(std::env::args().nth(1));

    app.connect_activate(move |app| build_ui(app, file_arg.as_ref().as_deref()));

    // GTK owns args itself; passing an empty slice avoids double-parsing our path.
    app.run_with_args::<&str>(&[])
}

/// Apply a theme's chrome CSS to the display through `provider`, creating the
/// display association the first time and restyling in place on later calls.
fn apply_chrome_css(provider: &CssProvider, theme: &Theme) {
    provider.load_from_string(&theme.to_gtk_css());
}

/// Build the main window: editor and preview in a stack, with the toggle wired.
fn build_ui(app: &Application, file_arg: Option<&str>) {
    let theme_file = match jotter_theming::bundled::default_theme_file() {
        Ok(file) => file,
        Err(err) => {
            eprintln!("jotter: could not load default theme: {err}");
            return;
        }
    };
    let theme = match theme_file.resolve(jotter_theming::bundled::DEFAULT_MODE) {
        Ok(theme) => theme,
        Err(err) => {
            eprintln!("jotter: could not resolve default theme: {err}");
            return;
        }
    };

    // Chrome CSS lives on a display-level provider we keep so a toggle restyles it.
    let chrome_provider = CssProvider::new();
    apply_chrome_css(&chrome_provider, &theme);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &chrome_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let editor = Editor::new(&theme);
    let preview = Preview::new(&theme);

    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::Crossfade);
    stack.set_transition_duration(80);
    stack.add_named(&editor.widget(), Some(PAGE_EDIT));
    stack.add_named(&preview.widget(), Some(PAGE_PREVIEW));
    stack.set_visible_child_name(PAGE_EDIT);

    // Load the initial document: path arg (utf-8) or a built-in sample on any miss.
    let initial = match file_arg {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) => {
                eprintln!("jotter: could not read {path}: {err}");
                SAMPLE_MARKDOWN.to_owned()
            }
        },
        None => SAMPLE_MARKDOWN.to_owned(),
    };
    editor.set_initial_text(&initial);

    let state = Rc::new(State {
        editor,
        preview,
        stack: stack.clone(),
        theme: RefCell::new(theme),
        theme_file,
        chrome_provider,
        cached_caret: RefCell::new(0),
        pending: RefCell::new(None),
    });

    wire_toggle(app, &state);
    wire_theme_toggle(app, &state);
    wire_debounce(&state);

    let header = HeaderBar::new();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("jotter")
        .default_width(1400)
        .default_height(900)
        .child(&stack)
        .build();
    window.set_titlebar(Some(&header));
    wire_preview_zoom(&window, &state);
    window.present();

    state.editor.grab_focus();
}

/// Match the `WebKit` preview zoom to the monitor scale so its text stays crisp
/// under Wayland fractional scaling instead of being upscaled from a 1x render.
fn wire_preview_zoom(window: &ApplicationWindow, state: &Rc<State>) {
    let zoom_state = Rc::clone(state);
    let apply = move |window: &ApplicationWindow| {
        // The surface carries the true fractional scale; fall back to the integer scale factor.
        let scale = window
            .surface()
            .map_or_else(|| f64::from(window.scale_factor()), |s| s.scale());
        if scale > 0.0 {
            zoom_state.preview.set_zoom(scale);
        }
    };

    // The surface (and its fractional scale) only exists once the window is realized.
    let on_realize = apply.clone();
    window.connect_realize(move |window| {
        on_realize(window);
        // Track later fractional-scale changes on the same monitor.
        if let Some(surface) = window.surface() {
            let on_scale = on_realize.clone();
            let window = window.clone();
            surface.connect_scale_notify(move |_| on_scale(&window));
        }
    });

    // Track moves to a monitor with a different integer scale factor.
    window.connect_scale_factor_notify(move |window| apply(window));
}

/// Install a Ctrl+E accelerator that toggles the stack between edit and preview.
fn wire_toggle(app: &Application, state: &Rc<State>) {
    let action = gtk::gio::SimpleAction::new("toggle-mode", None);
    let toggle_state = Rc::clone(state);
    action.connect_activate(move |_, _| toggle_mode(&toggle_state));
    app.add_action(&action);
    app.set_accels_for_action("app.toggle-mode", &["<Primary>e"]);
}

/// Swap the stack page, syncing caret to preview scroll or preview back to caret.
fn toggle_mode(state: &Rc<State>) {
    let showing_preview = state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW);

    if showing_preview {
        // Preview -> edit: restore the cached caret line and focus the editor.
        state.stack.set_visible_child_name(PAGE_EDIT);
        state.editor.set_caret_line(*state.cached_caret.borrow());
        state.editor.grab_focus();
    } else {
        // Edit -> preview: cache the caret, render, and scroll to nearest heading.
        *state.cached_caret.borrow_mut() = state.editor.caret_line();
        render_into_preview(state);
        state.stack.set_visible_child_name(PAGE_PREVIEW);
    }
}

/// Install a Ctrl+T accelerator that switches the active theme between light and dark.
fn wire_theme_toggle(app: &Application, state: &Rc<State>) {
    let action = gtk::gio::SimpleAction::new("toggle-theme", None);
    let theme_state = Rc::clone(state);
    action.connect_activate(move |_, _| toggle_theme_mode(&theme_state));
    app.add_action(&action);
    app.set_accels_for_action("app.toggle-theme", &["<Primary>t"]);
}

/// Re-resolve the theme for the opposite mode and re-apply it live to the chrome,
/// editor scheme, and preview CSS. On a resolve failure the current theme stays.
fn toggle_theme_mode(state: &Rc<State>) {
    let next_mode = match state.theme.borrow().mode {
        Mode::Dark => Mode::Light,
        Mode::Light => Mode::Dark,
    };
    let next = match state.theme_file.resolve(next_mode) {
        Ok(theme) => theme,
        Err(err) => {
            eprintln!("jotter: could not switch theme mode: {err}");
            return;
        }
    };

    apply_chrome_css(&state.chrome_provider, &next);
    state.editor.set_theme(&next);
    state.preview.set_theme(&next);
    *state.theme.borrow_mut() = next;

    // The loaded preview page keeps the old CSS and code colors, so re-render it if
    // it is showing. Preserve scroll so the recolor does not jump the reader.
    if state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW) {
        let text = state.editor.text();
        let rendered = jotter_parser::render(&text, &state.theme.borrow().code);
        state.preview.rerender_preserving_scroll(&rendered.html);
    }
}

/// Parse the editor buffer, load it into the preview, and scroll to the heading
/// nearest the cached caret line.
fn render_into_preview(state: &Rc<State>) {
    let text = state.editor.text();
    let rendered = jotter_parser::render(&text, &state.theme.borrow().code);

    // Caret is 0-based, heading source lines are 1-based, so compare in 1-based.
    let caret_1based = *state.cached_caret.borrow() + 1;
    let anchor = nearest_heading(&rendered.headings, caret_1based);
    state.preview.render(&rendered.html, anchor);
}

/// Pick the anchor for the heading nearest at or above `caret_1based`.
///
/// Returns the greatest `source_line <= caret_1based`, else the first heading,
/// else `None` when there are no headings.
fn nearest_heading(headings: &[jotter_parser::HeadingAnchor], caret_1based: i32) -> Option<&str> {
    let caret = usize::try_from(caret_1based.max(1)).unwrap_or(1);
    let mut chosen: Option<&jotter_parser::HeadingAnchor> = None;
    for heading in headings {
        if heading.source_line <= caret {
            chosen = Some(heading);
        }
    }
    chosen
        .or_else(|| headings.first())
        .map(|h| h.anchor.as_str())
}

/// Re-render the preview 150 ms after a buffer change, but only while the preview
/// is the visible page. Cancels any earlier pending timeout first.
fn wire_debounce(state: &Rc<State>) {
    let changed_state = Rc::clone(state);
    state.editor.connect_changed(move || {
        if changed_state.stack.visible_child_name().as_deref() != Some(PAGE_PREVIEW) {
            return;
        }

        // Cancel a pending re-render so only the latest change fires.
        if let Some(old) = changed_state.pending.borrow_mut().take() {
            old.remove();
        }

        let timeout_state = Rc::clone(&changed_state);
        let id = gtk::glib::timeout_add_local(
            std::time::Duration::from_millis(DEBOUNCE_MS),
            move || {
                // The caret in preview mode is stale; re-read it before rendering.
                *timeout_state.cached_caret.borrow_mut() = timeout_state.editor.caret_line();
                render_into_preview(&timeout_state);
                *timeout_state.pending.borrow_mut() = None;
                gtk::glib::ControlFlow::Break
            },
        );
        *changed_state.pending.borrow_mut() = Some(id);
    });
}
