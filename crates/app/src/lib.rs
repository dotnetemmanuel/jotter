#![warn(clippy::pedantic)]
//! Glue crate for jotter. Owns the GTK application, the shared UI state, the
//! edit-preview toggle loop, and (in vault mode) the file tree, background index,
//! and filesystem watcher. The binary stays thin and only calls `run`.

mod config;
mod links;
mod title;
mod tree;
mod vault_session;

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gtk::gio;
use gtk::glib::SourceId;
use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, CssProvider, HeaderBar, Label, ListView, Orientation, Paned,
    ScrolledWindow, SignalListItemFactory, SingleSelection, Stack, StackTransitionType,
    TreeExpander, TreeListModel, TreeListRow, gdk,
};

use jotter_editor::Editor;
use jotter_index::Index;
use jotter_preview::Preview;
use jotter_theming::{Mode, Theme, ThemeFile};
use jotter_vault::{Vault, VaultChange, WatchGuard};

use config::Config;
use links::{LinkTarget, Resolver};
use vault_session::IndexProgress;

/// Reverse-DNS application id, stable for GTK settings and Wayland app matching.
const APP_ID: &str = "dev.jotter.Jotter";

/// Stack page name for the editor.
const PAGE_EDIT: &str = "edit";
/// Stack page name for the preview.
const PAGE_PREVIEW: &str = "preview";

/// Re-render debounce for an already-open preview, in milliseconds.
const DEBOUNCE_MS: u64 = 150;

/// How often the GTK loop drains the watcher receiver and index progress channel.
const DRAIN_MS: u64 = 200;

/// How many near matches to offer when a wikilink target does not exist.
const MAX_SUGGESTIONS: usize = 5;

/// Fallback document shown when no path is given or a read fails.
const SAMPLE_MARKDOWN: &str = "# jotter\n\nA native GTK4 markdown vault.\n\n## Toggle\n\nPress Ctrl+E to switch between edit and preview.\n\n## Code\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n";

/// The active vault: filesystem handle, UI-thread index, and watcher guard.
///
/// Grouped so the whole session can be optional (single-file mode has none).
struct VaultSession {
    /// Filesystem layer for note IO and enumeration.
    vault: Vault,
    /// UI-thread index connection (single-note reindex on watcher events).
    index: Index,
    /// Kept alive to keep the watch thread running; dropping it stops watching.
    _watch: WatchGuard,
    /// The tree model backing the sidebar `ListView`, rebuilt on structure change.
    tree_model: RefCell<TreeListModel>,
    /// Currently loaded note, vault-relative, if any.
    current: RefCell<Option<PathBuf>>,
    /// Wikilink target lookup, rebuilt from the index on structural change.
    resolver: RefCell<Resolver>,
}

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
    /// The left sidebar container, whose visibility `Ctrl+B` toggles.
    sidebar: ScrolledWindow,
    /// The bottom status label (indexing progress, note counts).
    status: Label,
    /// The active vault session, absent in single-file mode.
    session: RefCell<Option<VaultSession>>,
    /// Persisted global config (recent vaults, last-active note per vault).
    config: RefCell<Config>,
    /// Heading anchor a followed wikilink asked for, consumed by the next render.
    pending_anchor: RefCell<Option<String>>,
}

/// Build the application, wire the theme and UI, and run the GTK loop.
///
/// Returns the process exit code from `gtk::Application::run`.
#[must_use]
pub fn run() -> gtk::glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    // First positional arg after the program name, if any, is a vault or file path.
    let path_arg: Rc<Option<String>> = Rc::new(std::env::args().nth(1));

    app.connect_activate(move |app| build_ui(app, path_arg.as_ref().as_deref()));

    // GTK owns args itself; passing an empty slice avoids double-parsing our path.
    app.run_with_args::<&str>(&[])
}

/// What the app should open, resolved from the CLI arg and saved config.
enum Startup {
    /// Open `root` as a vault, restoring `note` (vault-relative) if present.
    Vault { root: PathBuf, note: Option<PathBuf> },
    /// Open a single markdown file (no sidebar). `None` means the built-in sample.
    File(Option<PathBuf>),
}

/// Resolves the CLI arg plus config into a concrete startup target.
///
/// A directory arg opens a vault. A file arg opens that file. No arg reopens the
/// most-recent vault (with its last-active note) if config has one, else the
/// built-in sample.
fn resolve_startup(arg: Option<&str>, config: &Config) -> Startup {
    if let Some(arg) = arg {
        let path = PathBuf::from(arg);
        if path.is_dir() {
            let note = config.last_active_for(&path);
            return Startup::Vault { root: path, note };
        }
        return Startup::File(Some(path));
    }
    if let Some(root) = config.most_recent_vault()
        && root.is_dir()
    {
        let note = config.last_active_for(&root);
        return Startup::Vault { root, note };
    }
    Startup::File(None)
}

/// Apply a theme's chrome CSS to the display through `provider`, creating the
/// display association the first time and restyling in place on later calls.
fn apply_chrome_css(provider: &CssProvider, theme: &Theme) {
    provider.load_from_string(&theme.to_gtk_css());
}

/// Build the main window: sidebar, editor/preview stack, and status bar, wired up.
fn build_ui(app: &Application, path_arg: Option<&str>) {
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
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let sidebar = ScrolledWindow::builder()
        .width_request(240)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    // The theme stylesheet targets `.sidebar` for the surface color and tree rows.
    sidebar.add_css_class("sidebar");

    let status = Label::builder()
        .halign(gtk::Align::Start)
        .margin_start(8)
        .margin_end(8)
        .margin_top(2)
        .margin_bottom(2)
        .build();

    let config = Config::load();
    let startup = resolve_startup(path_arg, &config);

    let state = Rc::new(State {
        editor,
        preview,
        stack: stack.clone(),
        theme: RefCell::new(theme),
        theme_file,
        chrome_provider,
        cached_caret: RefCell::new(0),
        pending: RefCell::new(None),
        sidebar: sidebar.clone(),
        status: status.clone(),
        session: RefCell::new(None),
        config: RefCell::new(config),
        pending_anchor: RefCell::new(None),
    });

    // Load content per the resolved startup target, opening a vault if requested.
    match startup {
        Startup::Vault { root, note } => open_vault(&state, &root, note.as_deref()),
        Startup::File(path) => open_single_file(&state, path.as_deref()),
    }

    wire_toggle(app, &state);
    wire_theme_toggle(app, &state);
    wire_sidebar_toggle(app, &state);
    wire_debounce(&state);
    wire_preview_links(&state);
    wire_editor_links(&state);

    let paned = Paned::builder()
        .orientation(Orientation::Horizontal)
        .start_child(&sidebar)
        .end_child(&stack)
        .resize_end_child(true)
        .shrink_start_child(false)
        .position(240)
        .build();

    let root_box = gtk::Box::new(Orientation::Vertical, 0);
    root_box.append(&paned);
    root_box.append(&gtk::Separator::new(Orientation::Horizontal));
    root_box.append(&status);
    paned.set_vexpand(true);

    let header = HeaderBar::new();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("jotter")
        .default_width(1400)
        .default_height(900)
        .child(&root_box)
        .build();
    window.set_titlebar(Some(&header));
    wire_preview_zoom(&window, &state);
    window.present();

    state.editor.grab_focus();
}

/// Loads a single markdown file (or the built-in sample) with no vault session.
fn open_single_file(state: &Rc<State>, path: Option<&Path>) {
    let initial = match path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) => {
                eprintln!("jotter: could not read {}: {err}", path.display());
                SAMPLE_MARKDOWN.to_owned()
            }
        },
        None => SAMPLE_MARKDOWN.to_owned(),
    };
    state.editor.set_initial_text(&initial);
    // Single-file mode has an empty, non-interactive sidebar.
    state.status.set_text("no vault open");
}

/// Opens `root` as a vault: builds the tree, records it in recents, starts the
/// background index and the watcher, and loads the requested (or first) note.
fn open_vault(state: &Rc<State>, root: &Path, note: Option<&Path>) {
    let vault = match Vault::open(root) {
        Ok(vault) => vault,
        Err(err) => {
            eprintln!("jotter: could not open vault {}: {err}", root.display());
            open_single_file(state, None);
            return;
        }
    };
    let index = match vault_session::open_index(root) {
        Ok(index) => index,
        Err(err) => {
            eprintln!("jotter: could not open index for {}: {err}", root.display());
            open_single_file(state, None);
            return;
        }
    };

    // Start the watcher before enumerating so no external change is missed for long.
    let watch = match jotter_vault::watch(vault.root()) {
        Ok((rx, guard)) => {
            drain_watcher(state, rx);
            guard
        }
        Err(err) => {
            eprintln!("jotter: could not watch {}: {err}", root.display());
            open_single_file(state, None);
            return;
        }
    };

    let tree_model = build_tree(state, root);

    state.session.replace(Some(VaultSession {
        vault,
        index,
        _watch: watch,
        tree_model: RefCell::new(tree_model),
        current: RefCell::new(None),
        resolver: RefCell::new(Resolver::default()),
    }));

    // The index persists across runs, so links can resolve before reindexing runs.
    refresh_links(state);

    // Record the vault in recents and persist right away.
    {
        let mut config = state.config.borrow_mut();
        config.push_recent(root);
        config.save();
    }

    // Choose the note to open: the requested one, else the first tree note.
    let to_open = note
        .map(Path::to_path_buf)
        .filter(|rel| root.join(rel).is_file())
        .or_else(|| first_note(state));
    if let Some(rel) = to_open {
        load_note(state, &rel);
    } else {
        state.editor.set_initial_text(SAMPLE_MARKDOWN);
    }

    start_indexing(state, root);
}

/// The first `.md` note in the vault (path order), for a default open target.
fn first_note(state: &Rc<State>) -> Option<PathBuf> {
    let session = state.session.borrow();
    let vault = &session.as_ref()?.vault;
    let mut notes = vault.notes().ok()?;
    notes.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    notes.into_iter().next().map(|n| n.rel_path)
}

/// Loads the note at vault-relative `rel` into the editor and updates state.
///
/// Uses `set_initial_text` so the caret lands at the top of the freshly loaded
/// note. Records it as current and as the vault's last-active note in config.
fn load_note(state: &Rc<State>, rel: &Path) {
    let text = {
        let session = state.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        match session.vault.read_note(rel) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("jotter: could not read note {}: {err}", rel.display());
                return;
            }
        }
    };

    state.editor.set_initial_text(&text);
    *state.cached_caret.borrow_mut() = 0;
    // If preview is showing, re-render so the switch does not show a stale page.
    if state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW) {
        render_into_preview(state);
    }

    if let Some(session) = state.session.borrow().as_ref() {
        session.current.replace(Some(rel.to_path_buf()));
        let root = session.vault.root().to_path_buf();
        let mut config = state.config.borrow_mut();
        config.set_last_active(&root, rel);
        config.save();
    }
}

/// Builds (or rebuilds) the sidebar tree model and installs it on the `ListView`.
///
/// Returns the new `TreeListModel` so the session can hold it for later rebuilds.
fn build_tree(state: &Rc<State>, root: &Path) -> TreeListModel {
    let root_store = tree::root_store(root);
    let root_owned = root.to_path_buf();

    // Lazily produce a folder node's children; a file node returns None.
    let tree_model = TreeListModel::new(root_store, false, false, move |item| {
        let node = item.downcast_ref::<gtk::StringObject>()?;
        let rel = node.string();
        tree::child_store(&root_owned, &rel).map(Cast::upcast)
    });

    let selection = SingleSelection::new(Some(tree_model.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);

    let factory = SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = Label::builder().halign(gtk::Align::Start).build();
        let expander = TreeExpander::new();
        expander.set_child(Some(&label));
        item.set_child(Some(&expander));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(row) = item.item().and_downcast::<TreeListRow>() else {
            return;
        };
        let Some(expander) = item.child().and_downcast::<TreeExpander>() else {
            return;
        };
        expander.set_list_row(Some(&row));
        if let Some(node) = row.item().and_downcast::<gtk::StringObject>()
            && let Some(label) = expander.child().and_downcast::<Label>()
        {
            label.set_text(tree::label_for(&node.string()));
        }
    });

    let list_view = ListView::new(Some(selection.clone()), Some(factory));

    // Activating a row (Enter or double-click) opens a file, or toggles a folder.
    let activate_state = Rc::clone(state);
    let activate_sel = selection.clone();
    list_view.connect_activate(move |_, position| {
        let Some(row) = activate_sel
            .item(position)
            .and_downcast::<TreeListRow>()
        else {
            return;
        };
        activate_row(&activate_state, &row);
    });

    // Single-selection change also opens a file, so a single click is enough.
    let select_state = Rc::clone(state);
    selection.connect_selection_changed(move |sel, _, _| {
        let Some(row) = sel.selected_item().and_downcast::<TreeListRow>() else {
            return;
        };
        if let Some(node) = row.item().and_downcast::<gtk::StringObject>() {
            let rel = PathBuf::from(node.string().as_str());
            if is_file_node(&select_state, &node.string()) {
                load_note(&select_state, &rel);
            }
        }
    });

    wire_tree_context_menu(state, &list_view);

    state.sidebar.set_child(Some(&list_view));
    tree_model
}

/// Opens a file row or expands/collapses a folder row on activation.
fn activate_row(state: &Rc<State>, row: &TreeListRow) {
    let Some(node) = row.item().and_downcast::<gtk::StringObject>() else {
        return;
    };
    let rel = node.string();
    if is_file_node(state, &rel) {
        load_note(state, &PathBuf::from(rel.as_str()));
    } else {
        row.set_expanded(!row.is_expanded());
    }
}

/// True if the vault-relative `rel` names an existing file (not a folder).
fn is_file_node(state: &Rc<State>, rel: &str) -> bool {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return false;
    };
    !tree::is_dir_path(session.vault.root(), rel)
}

/// Rebuilds the tree model to reflect a structural change (create/rename/delete).
fn refresh_tree(state: &Rc<State>) {
    let (root, expanded) = {
        let session = state.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        // Capture which folders are open so the rebuild does not collapse the tree.
        let expanded = expanded_paths(&session.tree_model.borrow());
        (session.vault.root().to_path_buf(), expanded)
    };
    let model = build_tree(state, &root);
    restore_expanded(&model, &expanded);
    if let Some(session) = state.session.borrow().as_ref() {
        *session.tree_model.borrow_mut() = model;
    }
    update_note_count(state);
    refresh_links(state);
}

/// Rebuilds wikilink resolution from the index and re-resolves the links table.
///
/// Runs on every structural change, so a note that appears or disappears flips
/// the links pointing at it without a full reindex.
fn refresh_links(state: &Rc<State>) {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    match session.index.all_notes() {
        Ok(notes) => {
            let paths = notes.into_iter().map(|note| note.path);
            *session.resolver.borrow_mut() = Resolver::new(paths);
        }
        Err(err) => eprintln!("jotter: could not read note paths: {err}"),
    }
    if let Err(err) = vault_session::resolve_links(&session.index) {
        eprintln!("jotter: could not resolve links: {err}");
    }
}

/// Refreshes the status bar note count from the index after a structural change,
/// so an in-app or external create/delete is reflected without a full reconcile.
fn update_note_count(state: &Rc<State>) {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    match session.index.count_notes() {
        Ok(count) => state.status.set_text(&format!("Indexed {count} notes")),
        Err(err) => eprintln!("jotter: could not count notes: {err}"),
    }
}

/// Collects the vault-relative paths of every currently expanded folder row.
fn expanded_paths(model: &TreeListModel) -> HashSet<String> {
    let mut set = HashSet::new();
    for i in 0..model.n_items() {
        if let Some(row) = model.item(i).and_downcast::<TreeListRow>()
            && row.is_expanded()
            && let Some(node) = row.item().and_downcast::<gtk::StringObject>()
        {
            set.insert(node.string().to_string());
        }
    }
    set
}

/// Re-expands the folders named in `want` after a rebuild. Expanding a folder
/// appends its children, so this repeats until the flattened list stops growing.
fn restore_expanded(model: &TreeListModel, want: &HashSet<String>) {
    if want.is_empty() {
        return;
    }
    loop {
        let mut changed = false;
        for i in 0..model.n_items() {
            if let Some(row) = model.item(i).and_downcast::<TreeListRow>()
                && !row.is_expanded()
                && let Some(node) = row.item().and_downcast::<gtk::StringObject>()
                && want.contains(node.string().as_str())
            {
                row.set_expanded(true);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Reveals and selects the row for vault-relative `rel` after a tree rebuild.
///
/// Expands every ancestor folder so the target row exists in the flattened list,
/// then selects it. Used to highlight a freshly created or renamed note.
fn select_in_tree(state: &Rc<State>, rel: &Path) {
    // Expand every ancestor folder so the target row is present after the rebuild.
    let mut ancestors: HashSet<String> = HashSet::new();
    let mut cur = rel.parent();
    while let Some(dir) = cur {
        let key = dir.to_string_lossy();
        if !key.is_empty() {
            ancestors.insert(key.into_owned());
        }
        cur = dir.parent();
    }

    let want = rel.to_string_lossy();
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    let model = session.tree_model.borrow();
    restore_expanded(&model, &ancestors);

    let Some(list_view) = state.sidebar.child().and_downcast::<ListView>() else {
        return;
    };
    let Some(selection) = list_view.model().and_downcast::<SingleSelection>() else {
        return;
    };
    for i in 0..model.n_items() {
        if let Some(row) = model.item(i).and_downcast::<TreeListRow>()
            && let Some(node) = row.item().and_downcast::<gtk::StringObject>()
            && node.string().as_str() == want
        {
            selection.set_selected(i);
            break;
        }
    }
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
    let action = gio::SimpleAction::new("toggle-mode", None);
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

/// Install a Ctrl+B accelerator that toggles the sidebar visibility.
fn wire_sidebar_toggle(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("toggle-sidebar", None);
    let sidebar_state = Rc::clone(state);
    action.connect_activate(move |_, _| {
        let visible = sidebar_state.sidebar.get_visible();
        sidebar_state.sidebar.set_visible(!visible);
    });
    app.add_action(&action);
    app.set_accels_for_action("app.toggle-sidebar", &["<Primary>b"]);
}

/// Install a Ctrl+T accelerator that switches the active theme between light and dark.
fn wire_theme_toggle(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("toggle-theme", None);
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

    // Swapping the provider CSS does not always invalidate the sidebar
    // ScrolledWindow background, so it can keep the old mode color until the next
    // unrelated redraw. Force a full repaint so every chrome surface recolors now.
    if let Some(root) = state.sidebar.root() {
        root.queue_draw();
    }

    // The loaded preview page keeps the old CSS and code colors, so re-render it if
    // it is showing. Preserve scroll so the recolor does not jump the reader.
    if state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW) {
        let text = state.editor.text();
        let rendered = render_markdown(state, &text);
        state.preview.rerender_preserving_scroll(&rendered.html);
    }
}

/// Render the editor text with the active theme and the vault's link resolver.
///
/// Single-file mode has no vault, so every wikilink there renders as broken.
fn render_markdown(state: &Rc<State>, text: &str) -> jotter_parser::Rendered {
    let code = &state.theme.borrow().code;
    match state.session.borrow().as_ref() {
        Some(session) => jotter_parser::render(text, code, &*session.resolver.borrow()),
        None => jotter_parser::render(text, code, &Resolver::default()),
    }
}

/// Parse the editor buffer, load it into the preview, and scroll to the heading
/// nearest the cached caret line.
///
/// An anchor requested by a followed wikilink wins over the caret heading and is
/// consumed here, so it applies to exactly one render.
fn render_into_preview(state: &Rc<State>) {
    let text = state.editor.text();
    let rendered = render_markdown(state, &text);

    let requested = state.pending_anchor.borrow_mut().take();
    // Caret is 0-based, heading source lines are 1-based, so compare in 1-based.
    let caret_1based = *state.cached_caret.borrow() + 1;
    let anchor = requested
        .as_deref()
        .or_else(|| nearest_heading(&rendered.headings, caret_1based));
    state.preview.render(&rendered.html, anchor);
}

/// Route links clicked in the preview: notes open here, unresolved ones prompt,
/// and anything else goes to the system browser rather than hijacking the pane.
fn wire_preview_links(state: &Rc<State>) {
    let link_state = Rc::clone(state);
    state.preview.connect_link_clicked(move |uri| match links::parse_uri(uri) {
        LinkTarget::Note { path, anchor } => open_note_link(&link_state, &path, anchor),
        LinkTarget::New(target) => follow_broken_link(&link_state, &target),
        LinkTarget::External(uri) => launch_external(&uri),
    });
}

/// Ctrl+Click in the editor follows the wikilink under the pointer.
fn wire_editor_links(state: &Rc<State>) {
    let link_state = Rc::clone(state);
    state.editor.connect_ctrl_click(move |offset| {
        let text = link_state.editor.text();
        let Some(link) = jotter_parser::wikilink::scan(&text)
            .into_iter()
            .find(|link| link.range.contains(&offset))
        else {
            return;
        };
        let resolved = link_state
            .session
            .borrow()
            .as_ref()
            .and_then(|session| session.resolver.borrow().lookup(&link.target));
        match resolved {
            Some(path) => {
                let anchor = link.heading.as_deref().map(jotter_parser::wikilink::anchor_slug);
                open_note_link(&link_state, Path::new(&path), anchor);
            }
            None => follow_broken_link(&link_state, &link.target),
        }
    });
}

/// Open a wikilink target, scrolling the preview to `anchor` if it named one.
fn open_note_link(state: &Rc<State>, rel: &Path, anchor: Option<String>) {
    let exists = state
        .session
        .borrow()
        .as_ref()
        .is_some_and(|session| session.vault.root().join(rel).is_file());
    if !exists {
        // The index outran the filesystem; treat it as broken so the user can act.
        follow_broken_link(state, &rel.to_string_lossy());
        return;
    }
    *state.pending_anchor.borrow_mut() = anchor;
    load_note(state, rel);
    select_in_tree(state, rel);
}

/// Handle a click on a link with no note behind it: offer near matches when the
/// target looks like a typo, otherwise create the note straight away.
fn follow_broken_link(state: &Rc<State>, target: &str) {
    let suggestions = state
        .session
        .borrow()
        .as_ref()
        .map(|session| session.resolver.borrow().suggestions(target, MAX_SUGGESTIONS))
        .unwrap_or_default();

    if suggestions.is_empty() {
        create_note_for_link(state, target);
    } else {
        choose_link_target(state, target, &suggestions);
    }
}

/// Create the note a broken link points at, then open it.
fn create_note_for_link(state: &Rc<State>, target: &str) {
    let source = state
        .session
        .borrow()
        .as_ref()
        .and_then(|session| session.current.borrow().clone());
    let rel = links::new_note_path(target, source.as_deref());
    create_note_at(state, &rel);
}

/// Ask which existing note a mistyped link meant, with creating it as the last
/// option. Choosing a note also fixes the link text in the source.
fn choose_link_target(state: &Rc<State>, target: &str, suggestions: &[String]) {
    let parent = state.sidebar.root().and_downcast::<gtk::Window>();
    let dialog = gtk::Window::builder()
        .title(format!("No note named \"{target}\""))
        .modal(true)
        .default_width(420)
        .build();
    if let Some(parent) = parent.as_ref() {
        dialog.set_transient_for(Some(parent));
    }

    let content = gtk::Box::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    for path in suggestions {
        list.append(&gtk::Label::builder().label(path).xalign(0.0).build());
    }
    let create_label = format!("Create \"{target}\"");
    list.append(&gtk::Label::builder().label(&create_label).xalign(0.0).build());
    content.append(&list);
    dialog.set_child(Some(&content));

    // Row index maps back to a suggestion, or past the end to the create action.
    let choices: Vec<String> = suggestions.to_vec();
    let chooser = Rc::clone(state);
    let target = target.to_owned();
    let chosen_dialog = dialog.clone();
    list.connect_row_activated(move |_, row| {
        let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
        chosen_dialog.close();
        match choices.get(index) {
            Some(path) => {
                rewrite_link_target(&chooser, &target, path);
                open_note_link(&chooser, Path::new(path), None);
            }
            None => create_note_for_link(&chooser, &target),
        }
    });

    let escape = gtk::EventControllerKey::new();
    let escape_dialog = dialog.clone();
    escape.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            escape_dialog.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    dialog.add_controller(escape);

    dialog.present();
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
        first.grab_focus();
    }
}

/// Point every `[[from]]` in the open note at `chosen` instead, then save.
///
/// The whole buffer is written back, which is also the only way an edit reaches
/// disk today: there is no save command yet.
fn rewrite_link_target(state: &Rc<State>, from: &str, chosen: &str) {
    let replacement = state
        .session
        .borrow()
        .as_ref()
        .map(|session| session.resolver.borrow().shortest_target(chosen));
    let Some(replacement) = replacement else {
        return;
    };

    let text = state.editor.text();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for link in jotter_parser::wikilink::scan(&text) {
        if link.target != from {
            continue;
        }
        out.push_str(&text[cursor..link.range.start]);
        out.push_str(&links::format_wikilink(
            &replacement,
            link.heading.as_deref(),
            link.alias.as_deref(),
        ));
        cursor = link.range.end;
    }
    if cursor == 0 {
        return;
    }
    out.push_str(&text[cursor..]);

    let caret = state.editor.caret_line();
    state.editor.set_text(&out);
    state.editor.set_caret_line(caret);
    save_current_note(state, &out);
}

/// Write `text` to the open note and reindex it.
fn save_current_note(state: &Rc<State>, text: &str) {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    let current = session.current.borrow().clone();
    let Some(rel) = current else {
        return;
    };
    if let Err(err) = session.vault.write_note(&rel, text) {
        eprintln!("jotter: could not save {}: {err}", rel.display());
        return;
    }
    if let Err(err) = vault_session::reindex_note(&session.vault, &session.index, &rel) {
        eprintln!("jotter: could not reindex {}: {err}", rel.display());
    }
}

/// Hand a non-note uri to the desktop, so the preview never navigates away.
fn launch_external(uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    launcher.launch(
        None::<&gtk::Window>,
        None::<&gio::Cancellable>,
        |result| {
            if let Err(err) = result {
                eprintln!("jotter: could not open link: {err}");
            }
        },
    );
}

/// Pick the anchor for the heading nearest at or above `caret_1based`.
///
/// Returns the greatest `source_line <= caret_1based`, else `None` so the preview
/// stays at the top of the document (the caret sits above the first heading, or
/// there are no headings at all).
fn nearest_heading(headings: &[jotter_parser::HeadingAnchor], caret_1based: i32) -> Option<&str> {
    let caret = usize::try_from(caret_1based.max(1)).unwrap_or(1);
    let mut chosen: Option<&jotter_parser::HeadingAnchor> = None;
    for heading in headings {
        if heading.source_line <= caret {
            chosen = Some(heading);
        }
    }
    chosen.map(|h| h.anchor.as_str())
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
        let id = gtk::glib::timeout_add_local(Duration::from_millis(DEBOUNCE_MS), move || {
            // The caret in preview mode is stale; re-read it before rendering.
            *timeout_state.cached_caret.borrow_mut() = timeout_state.editor.caret_line();
            render_into_preview(&timeout_state);
            *timeout_state.pending.borrow_mut() = None;
            gtk::glib::ControlFlow::Break
        });
        *changed_state.pending.borrow_mut() = Some(id);
    });
}

/// Spawns the background reconcile thread and drains its progress into the status
/// bar. The thread opens its own index connection (`SQLite` is not `Send`).
fn start_indexing(state: &Rc<State>, root: &Path) {
    let (tx, rx) = std::sync::mpsc::channel::<IndexProgress>();
    let thread_root = root.to_path_buf();
    // A detached thread: it reports through the channel and needs no join handle.
    std::thread::spawn(move || {
        let report = |progress: IndexProgress| {
            // If the UI dropped the receiver, indexing progress no longer matters.
            let _ = tx.send(progress);
        };
        if let Err(err) = vault_session::reconcile_in_thread(&thread_root, &report) {
            let _ = tx.send(IndexProgress::Failed(err.to_string()));
        }
    });

    let status = state.status.clone();
    let index_state = Rc::clone(state);
    gtk::glib::timeout_add_local(Duration::from_millis(DRAIN_MS), move || {
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(IndexProgress::Working { done, total }) => {
                    status.set_text(&format!("Indexing {done}/{total}"));
                }
                Ok(IndexProgress::Done { total }) => {
                    status.set_text(&format!("Indexed {total} notes"));
                    // Links that looked broken against a partial index may resolve now.
                    refresh_links(&index_state);
                    if index_state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW) {
                        render_into_preview(&index_state);
                    }
                }
                Ok(IndexProgress::Failed(msg)) => {
                    eprintln!("jotter: indexing failed: {msg}");
                    status.set_text("Indexing failed");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

/// Drains the watcher receiver on a timer, applying each change incrementally.
fn drain_watcher(state: &Rc<State>, rx: Receiver<VaultChange>) {
    let drain_state = Rc::clone(state);
    gtk::glib::timeout_add_local(Duration::from_millis(DRAIN_MS), move || {
        let mut structural = false;
        loop {
            match rx.try_recv() {
                Ok(change) => {
                    if apply_change(&drain_state, &change) {
                        structural = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // The guard was dropped (vault closed); stop draining.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        if structural {
            refresh_tree(&drain_state);
        }
        gtk::glib::ControlFlow::Continue
    });
}

/// Applies one watcher change to the index. Returns true if the tree structure
/// changed (create/remove/rename) so the caller can refresh the tree once.
fn apply_change(state: &Rc<State>, change: &VaultChange) -> bool {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return false;
    };
    match change {
        VaultChange::Created(rel) | VaultChange::Modified(rel) => {
            // The debouncer can report a brand-new file as Modified, so decide whether
            // the tree needs a refresh by whether the index already knows the path, not
            // by the event kind. A path the index has never seen is a new file.
            let is_new = !matches!(
                session.index.mtime_by_path(&vault_session::rel_to_key(rel)),
                Ok(Some(_))
            );
            if let Err(err) = vault_session::reindex_note(&session.vault, &session.index, rel) {
                eprintln!("jotter: reindex on change failed for {}: {err}", rel.display());
            }
            is_new
        }
        VaultChange::Removed(rel) => {
            if let Err(err) = vault_session::deindex_note(&session.index, rel) {
                eprintln!("jotter: deindex failed for {}: {err}", rel.display());
            }
            true
        }
        VaultChange::Renamed { from, to } => {
            if let Err(err) = vault_session::deindex_note(&session.index, from) {
                eprintln!("jotter: deindex on rename failed for {}: {err}", from.display());
            }
            if let Err(err) = vault_session::reindex_note(&session.vault, &session.index, to) {
                eprintln!("jotter: reindex on rename failed for {}: {err}", to.display());
            }
            true
        }
    }
}

/// Wires a right-click context menu on the tree with note/folder operations.
///
/// The right-clicked row (not the selection) is the operation target, resolved by
/// hit-testing the click and stashed in `target` for the actions to read. A click
/// in empty space leaves `target` as `None`, meaning the vault root.
fn wire_tree_context_menu(state: &Rc<State>, list_view: &ListView) {
    let menu = gio::Menu::new();
    menu.append(Some("New note"), Some("app.tree-new-note"));
    menu.append(Some("New folder"), Some("app.tree-new-folder"));
    menu.append(Some("Rename"), Some("app.tree-rename"));
    menu.append(Some("Delete to trash"), Some("app.tree-delete"));

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(list_view);
    popover.set_has_arrow(false);

    // Shared between the gesture (which sets it) and the actions (which read it).
    let target: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    let popover_click = popover.clone();
    let list_view_hit = list_view.clone();
    let target_press = Rc::clone(&target);
    gesture.connect_pressed(move |_, _, x, y| {
        // Resolve the target from the row under the pointer, not the prior selection.
        *target_press.borrow_mut() = row_at(&list_view_hit, x, y);
        // Point the popover at the click; widget coordinates never exceed i32 range.
        let rect = gdk::Rectangle::new(px_to_i32(x), px_to_i32(y), 1, 1);
        popover_click.set_pointing_to(Some(&rect));
        popover_click.popup();
    });
    list_view.add_controller(gesture);

    install_tree_actions(state, &target);
}

/// The vault-relative path of the tree row under widget-local (`x`, `y`), if any.
///
/// Walks up from the picked widget to the row's `TreeExpander`, whose bound
/// `TreeListRow` carries the node path. A click in empty space yields `None`.
fn row_at(list_view: &ListView, x: f64, y: f64) -> Option<PathBuf> {
    let list_view_widget: &gtk::Widget = list_view.upcast_ref();
    let mut widget = list_view.pick(x, y, gtk::PickFlags::DEFAULT)?;
    loop {
        if let Some(expander) = widget.downcast_ref::<TreeExpander>() {
            let row = expander.list_row()?;
            let node = row.item().and_downcast::<gtk::StringObject>()?;
            return Some(PathBuf::from(node.string().as_str()));
        }
        if &widget == list_view_widget {
            return None;
        }
        widget = widget.parent()?;
    }
}

/// The vault-relative directory new items land in for a right-clicked `target`.
///
/// A folder target is that folder; a file target is its parent; `None` (empty
/// space) is the vault root (empty path).
fn target_dir(state: &Rc<State>, target: Option<&Path>) -> PathBuf {
    match target {
        None => PathBuf::new(),
        Some(rel) => {
            if is_file_node(state, &rel.to_string_lossy()) {
                rel.parent().map(Path::to_path_buf).unwrap_or_default()
            } else {
                rel.to_path_buf()
            }
        }
    }
}

/// Installs the four `app.tree-*` actions the context menu invokes.
///
/// Each reads the shared `target` (the right-clicked node) rather than the tree
/// selection, so the operation acts on the row the user actually clicked.
fn install_tree_actions(state: &Rc<State>, target: &Rc<RefCell<Option<PathBuf>>>) {
    let Some(app) = gio::Application::default().and_downcast::<Application>() else {
        return;
    };

    // New note: prompt for a name and create it under the target directory.
    let new_note = gio::SimpleAction::new("tree-new-note", None);
    let s = Rc::clone(state);
    let t = Rc::clone(target);
    new_note.connect_activate(move |_, _| {
        let dir = target_dir(&s, t.borrow().as_deref());
        let s2 = Rc::clone(&s);
        prompt(&s, "New note", "name.md", move |name| {
            create_note(&s2, &dir, &name);
        });
    });
    app.add_action(&new_note);

    // New folder: create a directory under the target (no vault folder API).
    let new_folder = gio::SimpleAction::new("tree-new-folder", None);
    let s = Rc::clone(state);
    let t = Rc::clone(target);
    new_folder.connect_activate(move |_, _| {
        let dir = target_dir(&s, t.borrow().as_deref());
        let s2 = Rc::clone(&s);
        prompt(&s, "New folder", "folder", move |name| {
            create_folder(&s2, &dir, &name);
        });
    });
    app.add_action(&new_folder);

    // Rename: only meaningful for a file target.
    let rename = gio::SimpleAction::new("tree-rename", None);
    let s = Rc::clone(state);
    let t = Rc::clone(target);
    rename.connect_activate(move |_, _| {
        let Some(rel) = t.borrow().clone() else {
            return;
        };
        if !is_file_node(&s, &rel.to_string_lossy()) {
            return;
        }
        let default = rel
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("note.md")
            .to_owned();
        let s2 = Rc::clone(&s);
        prompt(&s, "Rename note", &default, move |name| {
            rename_note(&s2, &rel, &name);
        });
    });
    app.add_action(&rename);

    // Delete to trash: only for a file target.
    let delete = gio::SimpleAction::new("tree-delete", None);
    let s = Rc::clone(state);
    let t = Rc::clone(target);
    delete.connect_activate(move |_, _| {
        let Some(rel) = t.borrow().clone() else {
            return;
        };
        if is_file_node(&s, &rel.to_string_lossy()) {
            delete_note(&s, &rel);
        }
    });
    app.add_action(&delete);
}

/// Creates a note named `name` (defaulting the `.md` extension) under `dir`.
fn create_note(state: &Rc<State>, dir: &Path, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let mut file = dir.join(name);
    if file.extension().is_none() {
        file.set_extension("md");
    }
    create_note_at(state, &file);
}

/// Creates a note at a vault-relative path, then opens and reveals it.
///
/// A path that already exists is opened rather than treated as an error, so a
/// link followed against a stale index still lands somewhere sensible.
fn create_note_at(state: &Rc<State>, rel: &Path) {
    // Scope the session borrow so refresh_tree can borrow it again below.
    let created = {
        let session_ref = state.session.borrow();
        let Some(session) = session_ref.as_ref() else {
            return;
        };
        if session.vault.root().join(rel).is_file() {
            true
        } else {
            match session
                .vault
                .create_note(rel, format!("# {}\n\n", stem_of(rel)))
            {
                Ok(()) => {
                    let _ = vault_session::reindex_note(&session.vault, &session.index, rel);
                    true
                }
                Err(err) => {
                    eprintln!("jotter: could not create note {}: {err}", rel.display());
                    false
                }
            }
        }
    };
    refresh_tree(state);
    // Open and reveal the new note so the user lands straight in it.
    if created {
        load_note(state, rel);
        select_in_tree(state, rel);
    }
}

/// Creates an empty folder named `name` under `dir` (there is no vault folder API,
/// so this uses the filesystem directly under the verified vault root).
fn create_folder(state: &Rc<State>, dir: &Path, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    {
        let session_ref = state.session.borrow();
        let Some(session) = session_ref.as_ref() else {
            return;
        };
        let abs = session.vault.root().join(dir).join(name);
        if let Err(err) = std::fs::create_dir_all(&abs) {
            eprintln!("jotter: could not create folder {}: {err}", abs.display());
        }
    }
    refresh_tree(state);
}

/// Renames the note at `rel` to `name` within the same directory.
fn rename_note(state: &Rc<State>, rel: &Path, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let parent = rel.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut to = parent.join(name);
    if to.extension().is_none() {
        to.set_extension("md");
    }
    let was_current;
    {
        let session_ref = state.session.borrow();
        let Some(session) = session_ref.as_ref() else {
            return;
        };
        match session.vault.rename_note(rel, &to) {
            Ok(()) => {
                let _ = vault_session::deindex_note(&session.index, rel);
                let _ = vault_session::reindex_note(&session.vault, &session.index, &to);
            }
            Err(err) => {
                eprintln!("jotter: could not rename {}: {err}", rel.display());
                return;
            }
        }
        // If the renamed note was open, follow it to its new path.
        was_current = session.current.borrow().as_deref() == Some(rel);
    }
    if was_current {
        load_note(state, &to);
    }
    refresh_tree(state);
    if was_current {
        select_in_tree(state, &to);
    }
}

/// Moves the note at `rel` to the vault trash and drops it from the index.
fn delete_note(state: &Rc<State>, rel: &Path) {
    {
        let session_ref = state.session.borrow();
        let Some(session) = session_ref.as_ref() else {
            return;
        };
        match session.vault.delete_to_trash(rel) {
            Ok(()) => {
                let _ = vault_session::deindex_note(&session.index, rel);
            }
            Err(err) => {
                eprintln!("jotter: could not delete {}: {err}", rel.display());
                return;
            }
        }
    }
    refresh_tree(state);
}

/// Rounds a widget-local coordinate to a pixel, clamped to the `i32` range.
fn px_to_i32(value: f64) -> i32 {
    let rounded = value.round();
    if rounded >= f64::from(i32::MAX) {
        i32::MAX
    } else if rounded <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        // Clamped into i32 range above, so this conversion is exact and lossless.
        #[allow(clippy::cast_possible_truncation, reason = "value is clamped to i32 range")]
        {
            rounded as i32
        }
    }
}

/// The filename stem of `path`, for a new note's default H1, or "note".
fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note")
        .to_owned()
}

/// Shows a modal single-entry prompt; runs `on_ok(text)` when the user confirms.
///
/// Uses a plain transient `gtk::Window` (the old `gtk::Dialog` is deprecated since
/// GTK 4.10). Enter or the OK button confirms, Escape or Cancel dismisses.
fn prompt<F: Fn(String) + 'static>(state: &Rc<State>, title: &str, default: &str, on_ok: F) {
    let parent = state.sidebar.root().and_downcast::<gtk::Window>();
    let dialog = gtk::Window::builder()
        .title(title)
        .modal(true)
        .default_width(360)
        .build();
    if let Some(parent) = parent.as_ref() {
        dialog.set_transient_for(Some(parent));
    }

    let content = gtk::Box::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let entry = gtk::Entry::builder().text(default).build();
    content.append(&entry);

    let buttons = gtk::Box::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let ok = gtk::Button::with_label("OK");
    ok.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&ok);
    content.append(&buttons);
    dialog.set_child(Some(&content));

    // OK and Enter run the callback once, then close; Cancel and Escape just close.
    let on_ok = Rc::new(on_ok);
    let confirm = {
        let dialog = dialog.clone();
        let entry = entry.clone();
        let on_ok = Rc::clone(&on_ok);
        move || {
            on_ok(entry.text().to_string());
            dialog.close();
        }
    };
    let confirm_ok = confirm.clone();
    ok.connect_clicked(move |_| confirm_ok());
    entry.connect_activate(move |_| confirm());
    let dialog_cancel = dialog.clone();
    cancel.connect_clicked(move |_| dialog_cancel.close());

    dialog.present();
    entry.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::{Startup, resolve_startup};
    use crate::config::Config;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn directory_arg_opens_vault() {
        let tmp = TempDir::new().unwrap();
        let config = Config::default();
        match resolve_startup(Some(&tmp.path().to_string_lossy()), &config) {
            Startup::Vault { root, note } => {
                assert_eq!(root, tmp.path());
                assert!(note.is_none());
            }
            Startup::File(_) => panic!("expected vault startup for a directory arg"),
        }
    }

    #[test]
    fn file_arg_opens_single_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        let config = Config::default();
        match resolve_startup(Some(&file.to_string_lossy()), &config) {
            Startup::File(Some(path)) => assert_eq!(path, file),
            Startup::File(None) => panic!("expected the file path, got the sample fallback"),
            Startup::Vault { .. } => panic!("expected single-file startup, got a vault"),
        }
    }

    #[test]
    fn no_arg_no_config_uses_sample() {
        let config = Config::default();
        assert!(matches!(resolve_startup(None, &config), Startup::File(None)));
    }

    #[test]
    fn no_arg_reopens_recent_vault_with_note() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "x").unwrap();
        let mut config = Config::default();
        config.push_recent(tmp.path());
        config.set_last_active(tmp.path(), Path::new("a.md"));
        match resolve_startup(None, &config) {
            Startup::Vault { root, note } => {
                assert_eq!(root, tmp.path());
                assert_eq!(note.as_deref(), Some(Path::new("a.md")));
            }
            Startup::File(_) => panic!("expected a recent vault, got single-file startup"),
        }
    }
}
