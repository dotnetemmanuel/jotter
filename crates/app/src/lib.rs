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
use jotter_theming::Theme;

/// Reverse-DNS application id, stable for GTK settings and Wayland app matching.
const APP_ID: &str = "se.mindfulstack.jotter";

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
    theme: Theme,
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

    app.connect_startup(|_| apply_theme_css());
    app.connect_activate(move |app| build_ui(app, file_arg.as_ref().as_deref()));

    // GTK owns args itself; passing an empty slice avoids double-parsing our path.
    app.run_with_args::<&str>(&[])
}

/// Resolve the bundled default theme and apply its chrome CSS to the display.
///
/// A resolve failure is logged to stderr and skipped so the window still opens.
fn apply_theme_css() {
    let theme = match jotter_theming::bundled::default_theme() {
        Ok(theme) => theme,
        Err(err) => {
            eprintln!("jotter: could not resolve default theme: {err}");
            return;
        }
    };
    let provider = CssProvider::new();
    provider.load_from_string(&theme.to_gtk_css());

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Build the main window: editor and preview in a stack, with the toggle wired.
fn build_ui(app: &Application, file_arg: Option<&str>) {
    let theme = match jotter_theming::bundled::default_theme() {
        Ok(theme) => theme,
        Err(err) => {
            eprintln!("jotter: could not resolve default theme: {err}");
            return;
        }
    };

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
    editor.set_text(&initial);

    let state = Rc::new(State {
        editor,
        preview,
        stack: stack.clone(),
        theme,
        cached_caret: RefCell::new(0),
        pending: RefCell::new(None),
    });

    wire_toggle(app, &state);
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
    window.present();

    state.editor.grab_focus();
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

/// Parse the editor buffer, load it into the preview, and scroll to the heading
/// nearest the cached caret line.
fn render_into_preview(state: &Rc<State>) {
    let text = state.editor.text();
    let rendered = jotter_parser::render(&text, &state.theme.code);
    state.preview.render(&rendered.html);

    // Caret is 0-based, heading source lines are 1-based, so compare in 1-based.
    let caret_1based = *state.cached_caret.borrow() + 1;
    let anchor = nearest_heading(&rendered.headings, caret_1based);
    if let Some(anchor) = anchor {
        state.preview.scroll_to_anchor(anchor);
    }
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
