#![warn(clippy::pedantic)]
//! Glue crate for jotter. Owns the GTK application, the shared UI state, the
//! edit-preview toggle loop, and (in vault mode) the file tree, background index,
//! and filesystem watcher. The binary stays thin and only calls `run`.

mod backlinks;
mod commands;
mod complete;
mod config;
mod drill;
mod git_panel;
mod git_status;
mod links;
mod picker;
mod results;
mod search;
mod search_panel;
mod switcher;
mod title;
mod tree;
mod vault_session;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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

/// Sidebar page names: the file tree, search, tags, and the broken-link report.
const PAGE_TREE: &str = "tree";
const PAGE_SEARCH: &str = "search";
const PAGE_TAGS: &str = "tags";
const PAGE_REPORT: &str = "report";
const PAGE_GIT: &str = "git";

/// Debounce before a typed query hits the index, in milliseconds.
const SEARCH_DEBOUNCE_MS: u64 = 120;

/// How many notes a search returns, and how many lines are shown per note.
const MAX_SEARCH_NOTES: usize = 50;
const MAX_SEARCH_LINES: usize = 3;

/// Re-render debounce for an already-open preview, in milliseconds.
const DEBOUNCE_MS: u64 = 150;

/// How long a message the user asked for holds the status bar against the
/// background chatter of note counts.
const STATUS_HOLD_SECONDS: u32 = 6;

/// How often the GTK loop drains the watcher receiver and index progress channel.
const DRAIN_MS: u64 = 200;

/// How many near matches to offer when a wikilink target does not exist.
const MAX_SUGGESTIONS: usize = 5;

/// How many rows the picker shows at most, however many notes matched.
const MAX_PICKER_ROWS: usize = 50;

/// How many suggestions the `[[` completion popup offers.
const MAX_COMPLETION_ROWS: usize = 8;

/// Actions the command palette offers, in the order it lists them.
const PALETTE_COMMANDS: [(&str, &str); 8] = [
    ("quick-open", "Go to note"),
    ("save", "Save note"),
    ("toggle-mode", "Toggle edit and preview"),
    ("toggle-theme", "Toggle light and dark theme"),
    ("toggle-sidebar", "Toggle sidebar"),
    ("tags", "Browse tags"),
    ("search", "Search notes"),
    ("broken-links", "Show broken links"),
];

/// Fallback document shown when no path is given or a read fails.
const SAMPLE_MARKDOWN: &str = "# jotter\n\nA native GTK4 markdown vault.\n\n## Toggle\n\nPress Ctrl+E to switch between edit and preview.\n\n## Code\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n";

/// What a git worker thread sends back to the UI.
enum GitNews {
    /// A fresh status reading.
    Status(jotter_git::Status),
    /// A sync finished, with what it did or why it could not.
    Synced(Result<jotter_git::SyncReport, String>),
}

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
    /// Note path to display title, so tree rows need no query per bind.
    titles: RefCell<HashMap<String, String>>,
    /// Whether this vault is its own git repository. Absent means no git at all.
    is_git: bool,
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
    /// The broken-link count at the right of the status bar, hidden when zero.
    broken: gtk::Button,
    /// The git segment at the far right, hidden for a vault with no repository.
    git: gtk::Button,
    /// The active vault session, absent in single-file mode.
    session: RefCell<Option<VaultSession>>,
    /// Persisted global config (recent vaults, last-active note per vault).
    config: RefCell<Config>,
    /// Heading anchor a followed wikilink asked for, consumed by the next render.
    pending_anchor: RefCell<Option<String>>,
    /// Whether the buffer has edits not yet written to disk.
    dirty: Cell<bool>,
    /// Path open in single-file mode, absent in vault mode and for the sample.
    single_file: RefCell<Option<PathBuf>>,
    /// Set while the app selects a tree row itself, so it does not open a note.
    quiet_selection: Cell<bool>,
    /// Tree row the app wants selected, reapplied after any rebuild.
    wanted_row: RefCell<Option<PathBuf>>,
    /// Layer the picker panel is added to, above the editor and preview.
    overlay: gtk::Overlay,
    /// The application, for activating an action the command palette chose.
    app: Application,
    /// The picker while it is open, so its key can toggle it shut again.
    picker: RefCell<Option<picker::Handle>>,
    /// The `[[` completion popup, parented on the editor view.
    completion: complete::Popup,
    /// The backlinks strip under the editor and preview.
    backlinks: Rc<backlinks::Strip>,
    /// Sidebar pages: the file tree, and full-text search.
    sidebar_stack: Stack,
    /// The full-text search page.
    search_panel: Rc<search_panel::Panel>,
    /// The tag page.
    tags_panel: Rc<drill::Panel>,
    /// The broken-link report page.
    report_panel: Rc<drill::Panel>,
    /// The git page: what changed since the last commit.
    git_panel: Rc<git_panel::Panel>,
    /// Pending debounced search, replaced on each keystroke.
    search_pending: RefCell<Option<SourceId>>,
    /// Long-lived sender for git news, kept so the channel outlives a worker.
    git_tx: RefCell<Option<std::sync::mpsc::Sender<GitNews>>>,
    /// Bumped on every vault change, so timers from an old vault stop themselves.
    git_generation: Cell<u64>,
    /// The last git status read, for the status bar and the actions behind it.
    git_last: RefCell<Option<jotter_git::Status>>,
    /// Bumped per explicit message, so only the newest one clears itself.
    status_message: Cell<u64>,
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

/// Put the chrome CSS on a display-level provider, returned so a theme toggle can
/// restyle it in place.
fn install_chrome_css(theme: &Theme) -> CssProvider {
    let provider = CssProvider::new();
    apply_chrome_css(&provider, theme);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    provider
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

    let chrome_provider = install_chrome_css(&theme);

    let editor = Editor::new(&theme);
    let completion = complete::Popup::new(&editor.text_view());
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
    // The stack carries `.sidebar`, so styling it here would draw a second border.

    let (status_bar, status, broken, git) = build_status_bar();

    let (pages, opened) = build_sidebar(&sidebar, &theme.chrome.accent);
    let sidebar_stack = pages.stack.clone();

    let backlinks = build_backlinks(&theme.chrome.accent, &opened);

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
        broken,
        git: git.clone(),
        session: RefCell::new(None),
        config: RefCell::new(config),
        pending_anchor: RefCell::new(None),
        dirty: Cell::new(false),
        single_file: RefCell::new(None),
        quiet_selection: Cell::new(false),
        wanted_row: RefCell::new(None),
        overlay: gtk::Overlay::new(),
        app: app.clone(),
        picker: RefCell::new(None),
        completion,
        backlinks: Rc::clone(&backlinks),
        sidebar_stack: sidebar_stack.clone(),
        search_panel: pages.search,
        tags_panel: pages.tags,
        report_panel: pages.report,
        git_panel: pages.git,
        search_pending: RefCell::new(None),
        git_tx: RefCell::new(None),
        git_generation: Cell::new(0),
        git_last: RefCell::new(None),
        status_message: Cell::new(0),
    });
    opened.replace(Some(Rc::clone(&state)));

    // Load content per the resolved startup target, opening a vault if requested.
    match startup {
        Startup::Vault { root, note } => open_vault(&state, &root, note.as_deref()),
        Startup::File(path) => open_single_file(&state, path.as_deref()),
    }

    wire_actions(app, &state);

    // The strip sits under both editor and preview, so it shows in either mode.
    let main_area = gtk::Box::new(Orientation::Vertical, 0);
    main_area.append(&stack);
    main_area.append(&backlinks.widget());

    let paned = Paned::builder()
        .orientation(Orientation::Horizontal)
        .start_child(&sidebar_stack)
        .end_child(&main_area)
        .resize_end_child(true)
        .shrink_start_child(false)
        .position(240)
        .build();

    let root_box = gtk::Box::new(Orientation::Vertical, 0);
    root_box.append(&paned);
    root_box.append(&gtk::Separator::new(Orientation::Horizontal));
    root_box.append(&status_bar);
    paned.set_vexpand(true);
    state.overlay.set_child(Some(&root_box));

    present_window(app, &state);
}

/// Builds the bottom bar: the status message, and the broken-link count that
/// sits at its right until there is nothing broken to report.
fn build_status_bar() -> (gtk::Box, Label, gtk::Button, gtk::Button) {
    let status = Label::builder()
        .halign(gtk::Align::Start)
        .margin_start(8)
        .margin_end(8)
        .margin_top(2)
        .margin_bottom(2)
        .build();

    let broken = gtk::Button::builder()
        .has_frame(false)
        .halign(gtk::Align::End)
        .visible(false)
        .tooltip_text("Show broken links")
        .build();
    broken.add_css_class("status-broken");

    let git = gtk::Button::builder()
        .has_frame(false)
        .halign(gtk::Align::End)
        .visible(false)
        .build();
    git.add_css_class("status-git");

    // An explicit spacer, because the right-hand widgets come and go and a
    // hidden one cannot hold the gap open.
    let spacer = gtk::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    let bar = gtk::Box::new(Orientation::Horizontal, 0);
    bar.append(&status);
    bar.append(&spacer);
    bar.append(&git);
    bar.append(&broken);
    (bar, status, broken, git)
}

/// Builds the window around the already-assembled layout and shows it.
fn present_window(app: &Application, state: &Rc<State>) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("jotter")
        .default_width(1400)
        .default_height(900)
        .child(&state.overlay)
        .build();
    window.set_titlebar(Some(&HeaderBar::new()));
    wire_preview_zoom(&window, state);
    wire_save_on_close(&window, state);
    window.present();

    // The note opened before the window existed, so the title is set once it does.
    refresh_window_title(state);
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
    state.single_file.replace(path.map(Path::to_path_buf));
    state.dirty.set(false);
    // Single-file mode has an empty, non-interactive sidebar.
    state.status.set_text("no vault open");
    stop_git(state);
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
    restore_expanded(&tree_model, &state.config.borrow().expanded_folders_for(root));

    state.session.replace(Some(VaultSession {
        vault,
        index,
        _watch: watch,
        tree_model: RefCell::new(tree_model),
        current: RefCell::new(None),
        resolver: RefCell::new(Resolver::default()),
        titles: RefCell::new(HashMap::new()),
        is_git: jotter_git::Repo::discover(root).is_some(),
    }));

    // Written whether or not the vault is a repo: one that becomes one later is
    // covered without jotter having to notice.
    if let Err(err) = jotter_git::write_ignores(root) {
        eprintln!("jotter: could not write ignore files: {err}");
    }
    start_git_polling(state, root);

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
    // The outgoing note is still `current`, so this writes the right file.
    save_if_dirty(state);

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
    state.dirty.set(false);
    refresh_editor_links(state);
    // If preview is showing, re-render so the switch does not show a stale page.
    if state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW) {
        render_into_preview(state);
    }

    if let Some(session) = state.session.borrow().as_ref() {
        session.current.replace(Some(rel.to_path_buf()));
        let root = session.vault.root().to_path_buf();
        let mut config = state.config.borrow_mut();
        config.set_last_active(&root, rel);
        config.push_recent_note(&root, rel);
    }
    // Opening a note is the routine moment to write layout too, so a crash or a
    // kill does not cost more than the folders opened since the last note switch.
    remember_layout(state);
    refresh_backlinks(state);
    refresh_window_title(state);
}

/// Retitles the window for the open note and whether it has unsaved edits.
fn refresh_window_title(state: &Rc<State>) {
    let name = state
        .session
        .borrow()
        .as_ref()
        .and_then(|session| session.current.borrow().clone())
        .or_else(|| state.single_file.borrow().clone())
        .map(|rel| stem_of(&rel));
    if let Some(window) = state.overlay.root().and_downcast::<gtk::Window>() {
        window.set_title(Some(&title::window_title(name.as_deref(), state.dirty.get())));
    }
}

/// How many linking lines the strip shows per note.
const MAX_BACKLINK_LINES: usize = 3;

/// Refills the backlinks strip for the note now open.
fn refresh_backlinks(state: &Rc<State>) {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    let Some(rel) = session.current.borrow().clone() else {
        state.backlinks.set_hits(&[]);
        return;
    };
    let target = vault_session::rel_to_key(&rel);
    let linkers = session.index.linking_notes(&target).unwrap_or_default();

    let resolver = session.resolver.borrow();
    let resolve = |written: &str| resolver.lookup(written);
    let hits: Vec<results::Hit> = linkers
        .into_iter()
        .filter_map(|note| {
            let text = session.vault.read_note(Path::new(&note.path)).ok()?;
            let snippets =
                backlinks::linking_lines(&text, &target, &resolve, MAX_BACKLINK_LINES);
            (!snippets.is_empty()).then_some(results::Hit {
                path: note.path,
                snippets,
                badge: None,
            })
        })
        .collect();
    state.backlinks.set_hits(&hits);
}

/// The indexed title of a note, when it says more than the filename does.
fn note_title(state: &Rc<State>, rel: &str) -> Option<String> {
    let session = state.session.borrow();
    let title = session.as_ref()?.titles.borrow().get(rel).cloned()?;
    (!title.is_empty()).then_some(title)
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
        let line = gtk::Box::new(Orientation::Horizontal, 6);
        line.append(&Label::builder().halign(gtk::Align::Start).build());
        let title = Label::builder()
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("tree-title");
        line.append(&title);
        let expander = TreeExpander::new();
        expander.set_child(Some(&line));
        item.set_child(Some(&expander));
    });
    let bind_state = Rc::clone(state);
    factory.connect_bind(move |_, item| {
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
        let Some(node) = row.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let Some(line) = expander.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let (Some(name), Some(title)) = (
            line.first_child().and_downcast::<Label>(),
            line.last_child().and_downcast::<Label>(),
        ) else {
            return;
        };
        let rel = node.string();
        let label = tree::label_for(&rel);
        name.set_text(label);
        let stem = label.strip_suffix(".md").unwrap_or(label);
        let shown = note_title(&bind_state, &rel).filter(|found| found != stem);
        title.set_text(shown.as_deref().unwrap_or_default());
        title.set_visible(shown.is_some());
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
        // A selection the app made itself is a highlight, not a request to open.
        if select_state.quiet_selection.get() {
            return;
        }
        let Some(row) = sel.selected_item().and_downcast::<TreeListRow>() else {
            return;
        };
        if let Some(node) = row.item().and_downcast::<gtk::StringObject>() {
            let rel = PathBuf::from(node.string().as_str());
            // Remembered so a rebuild puts the highlight back where the user left it.
            select_state.wanted_row.replace(Some(rel.clone()));
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
    rebuild_tree(state);
    update_note_count(state);
    refresh_links(state);
}

/// Rebuilds the tree rows in place, keeping open folders open.
fn rebuild_tree(state: &Rc<State>) {
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
    reselect_wanted(state);
}

/// Re-reads titles after a reindex, redrawing the tree only when one changed.
fn refresh_titles(state: &Rc<State>) {
    let changed = {
        let session = state.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        let Ok(notes) = session.index.all_notes() else {
            return;
        };
        let fresh: HashMap<String, String> = notes
            .into_iter()
            .map(|note| (note.path, note.title))
            .collect();
        let mut titles = session.titles.borrow_mut();
        let changed = *titles != fresh;
        if changed {
            *titles = fresh;
        }
        changed
    };
    if !changed {
        return;
    }
    rebuild_tree(state);
    let current = state
        .session
        .borrow()
        .as_ref()
        .and_then(|session| session.current.borrow().clone());
    if let Some(rel) = current {
        select_in_tree(state, &rel);
    }
}

/// Re-tag the wikilinks in the editor buffer so they match what the preview links.
fn refresh_editor_links(state: &Rc<State>) {
    let text = state.editor.text();
    let session = state.session.borrow();
    let spans: Vec<(std::ops::Range<usize>, bool)> = jotter_parser::wikilink::scan(&text)
        .into_iter()
        .map(|link| {
            let resolved = session
                .as_ref()
                .is_some_and(|s| s.resolver.borrow().lookup(&link.target).is_some());
            (link.range, resolved)
        })
        .collect();
    drop(session);
    state
        .editor
        .set_link_spans(&spans, &jotter_parser::wikilink::scan_inert(&text));
}

/// Rebuilds wikilink resolution from the index and re-resolves the links table.
///
/// Runs on every structural change, so a note that appears or disappears flips
/// the links pointing at it without a full reindex.
fn refresh_links(state: &Rc<State>) {
    {
        let session = state.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        match session.index.all_notes() {
            Ok(notes) => {
                *session.titles.borrow_mut() = notes
                    .iter()
                    .map(|note| (note.path.clone(), note.title.clone()))
                    .collect();
                let paths = notes.into_iter().map(|note| note.path);
                *session.resolver.borrow_mut() = Resolver::new(paths);
            }
            Err(err) => eprintln!("jotter: could not read note paths: {err}"),
        }
        if let Err(err) = vault_session::resolve_links(&session.index) {
            eprintln!("jotter: could not resolve links: {err}");
        }
    }
    // A note appearing or vanishing flips whether open links are broken.
    refresh_editor_links(state);
    refresh_backlinks(state);
    refresh_broken(state);
}

/// Starts the git status poll for the vault at `root`.
///
/// A vault with no repository starts nothing at all: no timer, no thread, and no
/// segment in the status bar. Git is a thing you have, not a thing jotter adds.
fn start_git_polling(state: &Rc<State>, root: &Path) {
    stop_git(state);
    if !state.session.borrow().as_ref().is_some_and(|s| s.is_git) {
        return;
    }

    let generation = state.git_generation.get();
    let (tx, rx) = std::sync::mpsc::channel::<GitNews>();
    state.git_tx.replace(Some(tx));

    let draining = Rc::clone(state);
    gtk::glib::timeout_add_local(Duration::from_millis(DRAIN_MS), move || {
        if draining.git_generation.get() != generation {
            return gtk::glib::ControlFlow::Break;
        }
        while let Ok(news) = rx.try_recv() {
            match news {
                GitNews::Status(status) => show_git_status(&draining, status),
                GitNews::Synced(outcome) => show_sync_outcome(&draining, outcome),
            }
        }
        gtk::glib::ControlFlow::Continue
    });

    let polling = Rc::clone(state);
    gtk::glib::timeout_add_seconds_local(git_status::POLL_SECONDS, move || {
        if polling.git_generation.get() != generation {
            return gtk::glib::ControlFlow::Break;
        }
        refresh_git(&polling);
        gtk::glib::ControlFlow::Continue
    });

    let _ = root;
    refresh_git(state);
}

/// Retires the current poll and hides the segment, for a vault change or a
/// single file.
fn stop_git(state: &Rc<State>) {
    state.git_generation.set(state.git_generation.get().wrapping_add(1));
    state.git_tx.replace(None);
    state.git_last.replace(None);
    state.git.set_visible(false);
}

/// Reads git status on a worker thread and sends it back to the UI.
fn refresh_git(state: &Rc<State>) {
    let Some(root) = state
        .session
        .borrow()
        .as_ref()
        .filter(|session| session.is_git)
        .map(|session| session.vault.root().to_path_buf())
    else {
        return;
    };
    let Some(tx) = state.git_tx.borrow().clone() else {
        return;
    };
    std::thread::spawn(move || {
        if let Some(status) = git_status::read(&root) {
            // A closed vault drops the receiver; nothing to report to.
            let _ = tx.send(GitNews::Status(status));
        }
    });
}

/// Saves, then commits, pulls, and pushes on a worker thread.
///
/// The buffer is written first: committing what is on disk while the editor
/// holds something newer would quietly commit the wrong thing.
fn sync_vault(state: &Rc<State>) {
    if state.dirty.get() {
        save_note(state);
    }
    let Some((root, changed)) = state
        .session
        .borrow()
        .as_ref()
        .filter(|session| session.is_git)
        .map(|session| session.vault.root().to_path_buf())
        .map(|root| {
            let changed = state
                .git_last
                .borrow()
                .as_ref()
                .map_or(0, |status| status.changed.len());
            (root, changed)
        })
    else {
        return;
    };
    let Some(tx) = state.git_tx.borrow().clone() else {
        return;
    };

    let when = gtk::glib::DateTime::now_local()
        .and_then(|now| now.format("%Y-%m-%d %H:%M"))
        .map_or_else(|_| String::from("jotter"), |stamp| stamp.to_string());
    let message = git_status::commit_message(changed.max(1), &when);

    say(state, "Syncing...");
    std::thread::spawn(move || {
        let outcome = match jotter_git::Repo::discover(&root) {
            Some(repo) => repo.sync(&message).map_err(|err| err.to_string()),
            None => Err("this vault is not a git repository".to_string()),
        };
        let _ = tx.send(GitNews::Synced(outcome));
    });
}

/// Reports what a finished sync did, and refreshes everything it touched.
fn show_sync_outcome(state: &Rc<State>, outcome: Result<jotter_git::SyncReport, String>) {
    // A rebase can rewrite notes under the editor, so the tree and index follow.
    // This runs first because refreshing the tree writes the note count to the
    // status bar, which would otherwise wipe the sentence below.
    refresh_tree(state);
    refresh_git(state);

    match outcome {
        Ok(report) => {
            say(state, &git_status::sync_summary(&report));
            if !report.conflicts.is_empty() {
                open_git_page(state);
            }
        }
        Err(err) => {
            eprintln!("jotter: sync failed: {err}");
            say(state, &format!("Sync failed: {err}"));
        }
    }
}

/// Toggles the git page: the action and the status-bar segment both land here.
fn toggle_git_page(state: &Rc<State>) {
    let showing = state.sidebar_stack.get_visible()
        && state.sidebar_stack.visible_child_name().as_deref() == Some(PAGE_GIT);
    if showing && state.git_panel.has_focus() {
        close_git_page(state);
        return;
    }
    open_git_page(state);
}

/// Reveals the git page, whatever is showing now.
///
/// Separate from the toggle because a conflict must open the page, and a toggle
/// would close it whenever the user happened to be looking at it already.
fn open_git_page(state: &Rc<State>) {
    if !state.session.borrow().as_ref().is_some_and(|s| s.is_git) {
        return;
    }
    let changed = state
        .git_last
        .borrow()
        .as_ref()
        .map(|status| status.changed.clone())
        .unwrap_or_default();
    state.git_panel.show(&changed);
    state.sidebar_stack.set_visible(true);
    state.sidebar_stack.set_visible_child_name(PAGE_GIT);
    state.git_panel.focus_list();
}

/// Applies a status that arrived from the worker.
fn show_git_status(state: &Rc<State>, status: jotter_git::Status) {
    state.git.set_label(&git_status::label(&status));
    state.git.set_tooltip_text(Some(&git_status::tooltip(&status)));
    state.git.set_visible(true);
    if state.sidebar_stack.visible_child_name().as_deref() == Some(PAGE_GIT) {
        state.git_panel.show(&status.changed);
    }
    state.git_last.replace(Some(status));
}

/// Says something in the status bar and holds it there for a few seconds.
///
/// Without the hold, a watcher event or a reindex writes the note count over the
/// answer the user was waiting for, which is exactly when they are looking.
fn say(state: &Rc<State>, text: &str) {
    state.status.set_text(text);
    let generation = state.status_message.get().wrapping_add(1);
    state.status_message.set(generation);

    let clearing = Rc::clone(state);
    gtk::glib::timeout_add_seconds_local_once(STATUS_HOLD_SECONDS, move || {
        if clearing.status_message.get() == generation {
            clearing.status_message.set(0);
            update_note_count(&clearing);
        }
    });
}

/// Refreshes the status bar note count from the index after a structural change,
/// so an in-app or external create/delete is reflected without a full reconcile.
fn update_note_count(state: &Rc<State>) {
    if state.status_message.get() != 0 {
        return;
    }
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    match session.index.count_notes() {
        Ok(count) => state.status.set_text(&indexed_text(count)),
        Err(err) => eprintln!("jotter: could not count notes: {err}"),
    }
}

/// The idle status-bar text: how many notes the index holds.
fn indexed_text(count: i64) -> String {
    match count {
        1 => "Indexed 1 note".to_string(),
        many => format!("Indexed {many} notes"),
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
    // Remembered so a later rebuild, including one the watcher triggers after an
    // in-app rename, does not drop the selection back to the first row.
    state.wanted_row.replace(Some(rel.to_path_buf()));
    reselect_wanted(state);
}

/// Reapplies the wanted row once the tree has settled.
///
/// A freshly installed `ListView` ignores focus until it is laid out, and then
/// parks focus on its first row, so this always waits for the next idle turn.
fn reselect_wanted(state: &Rc<State>) {
    let state = Rc::clone(state);
    gtk::glib::idle_add_local_once(move || {
        let wanted = state.wanted_row.borrow().clone();
        if let Some(rel) = wanted {
            select_in_tree_now(&state, &rel);
        }
    });
}

/// Selects and focuses the row for `rel`, expanding its ancestors first.
fn select_in_tree_now(state: &Rc<State>, rel: &Path) {
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
            state.quiet_selection.set(true);
            selection.set_selected(i);
            list_view.scroll_to(
                i,
                gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT,
                None,
            );
            state.quiet_selection.set(false);
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

/// Install every accelerator, signal, and controller the window needs.
fn wire_actions(app: &Application, state: &Rc<State>) {
    wire_toggle(app, state);
    wire_theme_toggle(app, state);
    wire_sidebar_toggle(app, state);
    wire_debounce(state);
    wire_preview_links(state);
    wire_editor_links(state);
    wire_save(app, state);
    wire_quick_open(app, state);
    wire_command_palette(app, state);
    wire_completion(state);
    wire_search(app, state);
    wire_tags(app, state);
    wire_report(app, state);
    wire_git(app, state);
    wire_editor_escape(state);
}

/// Install the git actions and the status-bar segment behind them.
fn wire_git(app: &Application, state: &Rc<State>) {
    let refresh = gio::SimpleAction::new("git-refresh", None);
    let refreshing = Rc::clone(state);
    refresh.connect_activate(move |_, _| refresh_git(&refreshing));
    app.add_action(&refresh);

    let untrack = gio::SimpleAction::new("git-untrack", None);
    let untracking = Rc::clone(state);
    untrack.connect_activate(move |_, _| untrack_jotter(&untracking));
    app.add_action(&untrack);

    let sync = gio::SimpleAction::new("git-sync", None);
    let syncing = Rc::clone(state);
    sync.connect_activate(move |_, _| sync_vault(&syncing));
    app.add_action(&sync);
    // Shift as well as Control: sync rewrites history, so it should not sit
    // one slip away from a common key.
    app.set_accels_for_action("app.git-sync", &["<Primary><Shift>g"]);

    let page = gio::SimpleAction::new("git-changes", None);
    let showing = Rc::clone(state);
    page.connect_activate(move |_, _| toggle_git_page(&showing));
    app.add_action(&page);

    // The segment is the way into the page, the way the broken-link count is.
    let clicked = Rc::clone(state);
    state.git.connect_clicked(move |_| toggle_git_page(&clicked));

    let escaping = Rc::clone(state);
    state.git_panel.connect_escape(move || close_git_page(&escaping));
    let going_back = Rc::clone(state);
    state.git_panel.connect_back(move || close_git_page(&going_back));
}

/// Puts the tree back and returns focus to the editor.
fn close_git_page(state: &Rc<State>) {
    state.sidebar_stack.set_visible_child_name(PAGE_TREE);
    state.editor.grab_focus();
}

/// Stops the repository tracking `.jotter`, reporting either way.
fn untrack_jotter(state: &Rc<State>) {
    let Some(root) = state
        .session
        .borrow()
        .as_ref()
        .map(|session| session.vault.root().to_path_buf())
    else {
        return;
    };
    let Some(repo) = jotter_git::Repo::discover(&root) else {
        return;
    };
    match repo.untrack_jotter() {
        Ok(()) => say(state, "The jotter index is no longer tracked. Commit to record it."),
        Err(err) => {
            eprintln!("jotter: could not untrack the index: {err}");
            say(state, &format!("Could not untrack: {err}"));
        }
    }
    refresh_git(state);
}

/// Install the broken-link report: its action, its status-bar count, and the
/// back steps out of it.
fn wire_report(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("broken-links", None);
    let open_state = Rc::clone(state);
    action.connect_activate(move |_, _| show_report(&open_state));
    app.add_action(&action);

    let clicked = Rc::clone(state);
    state.broken.connect_clicked(move |_| show_report(&clicked));

    let escaping = Rc::clone(state);
    state.report_panel.connect_escape(move || report_back(&escaping));
    let going_back = Rc::clone(state);
    state.report_panel.connect_back(move || report_back(&going_back));
}

/// Steps the report back one level, leaving the sidebar when already at the top.
fn report_back(state: &Rc<State>) {
    if matches!(state.report_panel.view(), drill::View::Notes(_)) {
        show_report(state);
        return;
    }
    state.sidebar_stack.set_visible_child_name(PAGE_TREE);
    state.editor.grab_focus();
}

/// Reveals the broken-link report, or closes it when it is already showing.
fn show_report(state: &Rc<State>) {
    let showing = state.sidebar_stack.get_visible()
        && state.sidebar_stack.visible_child_name().as_deref() == Some(PAGE_REPORT);
    if showing
        && state.report_panel.has_focus()
        && matches!(state.report_panel.view(), drill::View::Top)
    {
        state.sidebar_stack.set_visible_child_name(PAGE_TREE);
        state.editor.grab_focus();
        return;
    }

    let Some(missing) = broken_targets(state) else {
        return;
    };
    state.sidebar_stack.set_visible(true);
    state.sidebar_stack.set_visible_child_name(PAGE_REPORT);
    state.report_panel.show_top(&missing);
    state.report_panel.focus_list();
}

/// Every missing link target with how many notes point at it, or `None` outside
/// a vault.
fn broken_targets(state: &Rc<State>) -> Option<Vec<(String, i64)>> {
    let session = state.session.borrow();
    let session = session.as_ref()?;
    Some(session.index.broken_links().unwrap_or_default())
}

/// Updates the status-bar count, which stays hidden while nothing is broken.
///
/// Also refreshes the report when it is the page on screen, so fixing a link
/// while it is open does not leave a stale list behind.
fn refresh_broken(state: &Rc<State>) {
    let Some(missing) = broken_targets(state) else {
        state.broken.set_visible(false);
        return;
    };
    state.broken.set_label(&broken_label(missing.len()));
    state.broken.set_visible(!missing.is_empty());

    if state.sidebar_stack.visible_child_name().as_deref() != Some(PAGE_REPORT) {
        return;
    }
    // A target that is no longer broken has no page left to show, so the report
    // steps back to the list rather than sitting on an empty one. The lists keep
    // their own selection and focus across the redraw.
    match state.report_panel.view() {
        drill::View::Notes(target) if missing.iter().any(|(name, _)| *name == target) => {
            show_broken_linkers(state, &target);
        }
        _ => state.report_panel.show_top(&missing),
    }
}

/// The status-bar text for `count` missing targets.
fn broken_label(count: usize) -> String {
    match count {
        1 => "1 broken link".to_string(),
        many => format!("{many} broken links"),
    }
}

/// Shows the notes pointing at `missing`, with the lines the dead links sit on.
fn show_broken_linkers(state: &Rc<State>, missing: &str) {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    // A target in the report resolves nowhere, so a link matches it by its
    // written form alone.
    let written = |target: &str| Some(target.to_string());
    let hits: Vec<results::Hit> = session
        .index
        .broken_link_notes(missing)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|note| {
            let text = session.vault.read_note(Path::new(&note.path)).ok()?;
            let snippets =
                backlinks::linking_lines(&text, missing, &written, MAX_BACKLINK_LINES);
            (!snippets.is_empty()).then_some(results::Hit {
                path: note.path,
                snippets,
                badge: None,
            })
        })
        .collect();
    state.report_panel.show_notes(missing, &hits);
}

/// Install Ctrl+Shift+T, and the tag page behavior behind it.
fn wire_tags(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("tags", None);
    let open_state = Rc::clone(state);
    action.connect_activate(move |_, _| show_tags(&open_state));
    app.add_action(&action);
    app.set_accels_for_action("app.tags", &["<Primary><Shift>t"]);

    // Escape and the back arrow both step one level: notes to tags, tags to tree.
    let escaping = Rc::clone(state);
    state.tags_panel.connect_escape(move || tags_back(&escaping));
    let going_back = Rc::clone(state);
    state.tags_panel.connect_back(move || tags_back(&going_back));
}

/// Steps the tag page back one level, leaving the sidebar when already at the top.
fn tags_back(state: &Rc<State>) {
    if matches!(state.tags_panel.view(), drill::View::Notes(_)) {
        show_tags(state);
        return;
    }
    state.sidebar_stack.set_visible_child_name(PAGE_TREE);
    state.editor.grab_focus();
}

/// Escape in the editor hands focus back to the sidebar page it came from.
fn wire_editor_escape(state: &Rc<State>) {
    let escaping = Rc::clone(state);
    state.editor.connect_key_capture(move |key| {
        if key != gdk::Key::Escape || escaping.completion.is_open() {
            return false;
        }
        if !escaping.sidebar_stack.get_visible() {
            return false;
        }
        match escaping.sidebar_stack.visible_child_name().as_deref() {
            Some(PAGE_TAGS) => {
                escaping.tags_panel.focus_list();
                true
            }
            Some(PAGE_REPORT) => {
                escaping.report_panel.focus_list();
                true
            }
            Some(PAGE_GIT) => {
                escaping.git_panel.focus_list();
                true
            }
            Some(PAGE_SEARCH) => {
                escaping.search_panel.focus();
                true
            }
            _ => false,
        }
    });
}

/// Reveals the tag list, or closes the page when it is already showing.
fn show_tags(state: &Rc<State>) {
    let showing = state.sidebar_stack.get_visible()
        && state.sidebar_stack.visible_child_name().as_deref() == Some(PAGE_TAGS);
    if showing
        && state.tags_panel.has_focus()
        && matches!(state.tags_panel.view(), drill::View::Top)
    {
        state.sidebar_stack.set_visible_child_name(PAGE_TREE);
        state.editor.grab_focus();
        return;
    }

    let counts = {
        let session = state.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        session.index.tag_counts().unwrap_or_default()
    };
    state.sidebar_stack.set_visible(true);
    state.sidebar_stack.set_visible_child_name(PAGE_TAGS);
    state.tags_panel.show_top(&counts);
    state.tags_panel.focus_list();
}

/// Shows the notes carrying `tag`, with the lines the tag appears on.
fn show_tagged_notes(state: &Rc<State>, tag: &str) {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    let notes = session.index.tagged_notes(tag).unwrap_or_default();
    let terms = [format!("#{tag}")];
    let hits: Vec<results::Hit> = notes
        .into_iter()
        .map(|note| {
            let text = session
                .vault
                .read_note(Path::new(&note.path))
                .unwrap_or_default();
            results::Hit {
                snippets: search::snippets(&text, &terms, MAX_SEARCH_LINES),
                path: note.path,
                badge: None,
            }
        })
        .collect();
    state.tags_panel.show_notes(tag, &hits);
}

/// The state a search row activates through, filled in once the state exists.
type LateState = Rc<RefCell<Option<Rc<State>>>>;

/// Builds the four-page sidebar and the cell its rows activate through.
fn build_sidebar(tree: &ScrolledWindow, accent: &str) -> (Sidebar, LateState) {
    let opened: LateState = Rc::new(RefCell::new(None));

    let target = Rc::clone(&opened);
    let search = search_panel::Panel::new(accent, move |path, line| {
        let state = target.borrow().clone();
        if let Some(state) = state {
            open_search_hit(&state, path, line);
        }
    });

    let tag_target = Rc::clone(&opened);
    let tags = drill::Panel::new(
        drill::Kind::Tags,
        accent,
        move |tag| {
            let state = tag_target.borrow().clone();
            if let Some(state) = state {
                show_tagged_notes(&state, tag);
                state.tags_panel.focus_list();
            }
        },
        open_through(&opened),
    );

    let broken_target = Rc::clone(&opened);
    let report = drill::Panel::new(
        drill::Kind::Broken,
        accent,
        move |missing| {
            let state = broken_target.borrow().clone();
            if let Some(state) = state {
                show_broken_linkers(&state, missing);
                state.report_panel.focus_list();
            }
        },
        open_through(&opened),
    );

    let git = git_panel::Panel::new(accent, open_through(&opened));

    let stack = Stack::new();
    stack.add_named(tree, Some(PAGE_TREE));
    stack.add_named(&search.widget(), Some(PAGE_SEARCH));
    stack.add_named(&tags.widget(), Some(PAGE_TAGS));
    stack.add_named(&report.widget(), Some(PAGE_REPORT));
    stack.add_named(&git.widget(), Some(PAGE_GIT));
    stack.set_visible_child_name(PAGE_TREE);
    stack.add_css_class("sidebar");

    (
        Sidebar {
            search,
            tags,
            report,
            git,
            stack,
        },
        opened,
    )
}

/// A row callback that opens a note at a line, once the state exists.
fn open_through(opened: &LateState) -> impl Fn(&str, i32) + 'static {
    let target = Rc::clone(opened);
    move |path, line| {
        let state = target.borrow().clone();
        if let Some(state) = state {
            open_search_hit(&state, path, line);
        }
    }
}

/// The sidebar pages, built together and handed to the state.
struct Sidebar {
    search: Rc<search_panel::Panel>,
    tags: Rc<drill::Panel>,
    report: Rc<drill::Panel>,
    git: Rc<git_panel::Panel>,
    stack: Stack,
}

/// Builds the backlinks strip, opening notes through the state once it exists.
fn build_backlinks(accent: &str, opened: &LateState) -> Rc<backlinks::Strip> {
    backlinks::Strip::new(
        accent,
        Config::load().backlinks_expanded,
        open_through(opened),
    )
}

/// Install Ctrl+Shift+F, and the panel behavior behind it.
fn wire_search(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("search", None);
    let open_state = Rc::clone(state);
    action.connect_activate(move |_, _| show_search(&open_state));
    app.add_action(&action);
    app.set_accels_for_action("app.search", &["<Primary><Shift>f"]);

    let typing = Rc::clone(state);
    state.search_panel.connect_query(move |query| {
        if let Some(pending) = typing.search_pending.borrow_mut().take() {
            pending.remove();
        }
        let query = query.to_string();
        let fire = Rc::clone(&typing);
        let source = gtk::glib::timeout_add_local_once(
            Duration::from_millis(SEARCH_DEBOUNCE_MS),
            move || {
                fire.search_pending.replace(None);
                run_search(&fire, &query);
            },
        );
        typing.search_pending.replace(Some(source));
    });

    let escaping = Rc::clone(state);
    state.search_panel.connect_escape(move || close_search(&escaping));
    let going_back = Rc::clone(state);
    state.search_panel.connect_back(move || close_search(&going_back));
}

/// Reveals the search page and puts the caret in its entry.
///
/// Pressing the key again from the search box closes it; pressing it from the
/// editor pulls focus back to the query instead, which is the more useful move
/// when the results are still on screen.
fn show_search(state: &Rc<State>) {
    if state.session.borrow().is_none() {
        return;
    }
    let showing = state.sidebar_stack.get_visible()
        && state.sidebar_stack.visible_child_name().as_deref() == Some(PAGE_SEARCH);
    if showing && state.search_panel.has_focus() {
        close_search(state);
        return;
    }
    state.sidebar_stack.set_visible(true);
    state.sidebar_stack.set_visible_child_name(PAGE_SEARCH);
    state.search_panel.focus();
}

/// Puts the file tree back and returns focus to the editor.
fn close_search(state: &Rc<State>) {
    state.sidebar_stack.set_visible_child_name(PAGE_TREE);
    state.editor.grab_focus();
}

/// Runs `query` against the index and fills the panel with matching lines.
fn run_search(state: &Rc<State>, query: &str) {
    let fts = search::fts_query(query);
    if fts.is_empty() {
        state.search_panel.set_hits(&[], false);
        return;
    }
    let terms = search::terms(query);

    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return;
    };
    let notes = session
        .index
        .search_notes(&fts, MAX_SEARCH_NOTES)
        .unwrap_or_default();

    // Snippets come from the file: notes_fts is contentless, so it has no text to quote.
    let hits: Vec<results::Hit> = notes
        .into_iter()
        .map(|note| {
            let text = session
                .vault
                .read_note(Path::new(&note.path))
                .unwrap_or_default();
            results::Hit {
                snippets: search::snippets(&text, &terms, MAX_SEARCH_LINES),
                path: note.path,
                badge: None,
            }
        })
        .collect();
    state.search_panel.set_hits(&hits, true);
}

/// Opens a search result, putting the caret on the line that matched.
fn open_search_hit(state: &Rc<State>, path: &str, line: i32) {
    let rel = PathBuf::from(path);
    load_note(state, &rel);
    select_in_tree(state, &rel);
    state.stack.set_visible_child_name(PAGE_EDIT);
    state.editor.focus_line(line);
    state.editor.grab_focus();
}

/// Offer note targets while the caret sits inside an unclosed `[[`.
fn wire_completion(state: &Rc<State>) {
    let moved = Rc::clone(state);
    state
        .editor
        .connect_caret_moved(move || refresh_completion(&moved));

    let keys = Rc::clone(state);
    state.editor.connect_key_capture(move |key| {
        if !keys.completion.is_open() {
            return false;
        }
        match key {
            gdk::Key::Down => keys.completion.step(1),
            gdk::Key::Up => keys.completion.step(-1),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::Tab => accept_completion(&keys),
            gdk::Key::Escape => keys.completion.hide(),
            _ => return false,
        }
        true
    });
}

/// Re-decide whether the completion popup belongs on screen, and with what.
fn refresh_completion(state: &Rc<State>) {
    let text = state.editor.text();
    let caret = state.editor.caret_byte();

    // Cheap check first: parsing for code ranges is only worth it inside a link.
    let Some(context) = complete::context(&text, caret, &[]) else {
        state.completion.hide();
        return;
    };
    let dead = jotter_parser::wikilink::dead_ranges(&text);
    if complete::context(&text, caret, &dead).is_none() {
        state.completion.hide();
        return;
    }

    let targets = completion_targets(state);
    let rows = complete::rows(&context.query, &targets, MAX_COMPLETION_ROWS);
    state.completion.show(state.editor.caret_rect(), &rows);
}

/// Writes the selected target into the link the caret is in.
fn accept_completion(state: &Rc<State>) {
    let Some(target) = state.completion.selected() else {
        return;
    };
    let text = state.editor.text();
    let caret = state.editor.caret_byte();
    let Some(context) = complete::context(&text, caret, &[]) else {
        return;
    };
    let (replacement, offset) = complete::insertion(&target, context.closed);
    state.completion.hide();
    state.editor.replace_range(context.range, &replacement, offset);
}

/// Every note as the shortest link target that reaches it.
fn completion_targets(state: &Rc<State>) -> Vec<String> {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return Vec::new();
    };
    let resolver = session.resolver.borrow();
    session
        .index
        .all_notes()
        .unwrap_or_default()
        .iter()
        .map(|note| resolver.shortest_target(&note.path))
        .collect()
}

/// Install a Ctrl+O accelerator that opens the picker on notes.
fn wire_quick_open(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("quick-open", None);
    let open_state = Rc::clone(state);
    action.connect_activate(move |_, _| open_picker(&open_state, ""));
    app.add_action(&action);
    app.set_accels_for_action("app.quick-open", &["<Primary>o"]);
}

/// Install a Ctrl+P accelerator that opens the picker already in command mode.
fn wire_command_palette(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("command-palette", None);
    let open_state = Rc::clone(state);
    let prefix = commands::PREFIX.to_string();
    action.connect_activate(move |_, _| open_picker(&open_state, &prefix));
    app.add_action(&action);
    app.set_accels_for_action("app.command-palette", &["<Primary>p"]);
}

/// Opens the picker: notes by default, commands while the query starts with `>`.
///
/// Recents fill the empty note list; the whole command list fills the empty
/// command one.
fn open_picker(state: &Rc<State>, initial_query: &str) {
    // Its own key toggles the picker shut; the other key switches mode instead.
    let open = state.picker.borrow().clone();
    if let Some(handle) = open {
        if commands::same_mode(&handle.query(), initial_query) {
            handle.close();
        } else {
            handle.set_query(initial_query);
        }
        return;
    }

    let notes = switcher_candidates(state).unwrap_or_default();
    let recents = recent_notes(state);
    let command_list = command_list(state);

    // Set by the source on every keystroke so activation knows which list it chose from.
    let in_command_mode = Rc::new(Cell::new(false));

    let source_mode = Rc::clone(&in_command_mode);
    let activate = Rc::clone(state);
    let restore = Rc::clone(state);
    let handle = picker::open(
        &state.overlay,
        "Go to note, or > for commands",
        initial_query,
        move |query| {
            if let Some(rest) = commands::command_query(query) {
                source_mode.set(true);
                return commands::rows(rest, &command_list, MAX_PICKER_ROWS);
            }
            source_mode.set(false);
            switcher::rows(query, &notes, &recents, MAX_PICKER_ROWS)
        },
        move |key| {
            if in_command_mode.get() {
                activate.app.activate_action(key, None);
                return;
            }
            let rel = PathBuf::from(key);
            load_note(&activate, &rel);
            select_in_tree(&activate, &rel);
        },
        move || {
            restore.picker.replace(None);
            restore.editor.grab_focus();
        },
    );
    state.picker.replace(Some(handle));
}

/// Every command the palette offers, labelled with its current accelerator.
///
/// Built per vault rather than from a fixed list: the git entries exist only
/// where there is a repository to run them against.
fn command_list(state: &Rc<State>) -> Vec<commands::Command> {
    let app = &state.app;
    let mut commands: Vec<commands::Command> = PALETTE_COMMANDS
        .iter()
        .map(|(action, title)| commands::Command {
            action: (*action).to_string(),
            title: (*title).to_string(),
            accel: accel_label(app, action),
        })
        .collect();

    for (action, title) in git_commands(state) {
        commands.push(commands::Command {
            action: action.to_string(),
            title: title.to_string(),
            accel: accel_label(app, action),
        });
    }
    commands
}

/// The git entries this vault has earned, in palette order.
fn git_commands(state: &Rc<State>) -> Vec<(&'static str, &'static str)> {
    let Some(root) = state
        .session
        .borrow()
        .as_ref()
        .filter(|session| session.is_git)
        .map(|session| session.vault.root().to_path_buf())
    else {
        return Vec::new();
    };

    let mut commands = vec![
        ("git-sync", "Sync vault"),
        ("git-changes", "Show changed notes"),
        ("git-refresh", "Refresh git status"),
    ];
    // Only worth offering where it would do something: a vault committed before
    // jotter wrote its ignore files.
    if jotter_git::Repo::discover(&root).is_some_and(|repo| repo.tracks_jotter()) {
        commands.push(("git-untrack", "Stop tracking the jotter index"));
    }
    commands
}

/// The first accelerator bound to `action`, as a display label like `Ctrl+S`.
fn accel_label(app: &Application, action: &str) -> String {
    let accels = app.accels_for_action(&format!("app.{action}"));
    let Some(first) = accels.first() else {
        return String::new();
    };
    let Some((key, mods)) = gtk::accelerator_parse(first) else {
        return String::new();
    };
    commands::tidy_accel(&gtk::accelerator_get_label(key, mods))
}

/// Every note the switcher can offer, from the index, falling back to the vault
/// while a first index build is still running.
fn switcher_candidates(state: &Rc<State>) -> Option<Vec<switcher::Candidate>> {
    let session = state.session.borrow();
    let session = session.as_ref()?;
    let indexed: Vec<switcher::Candidate> = session
        .index
        .all_notes()
        .unwrap_or_default()
        .into_iter()
        .map(|note| switcher::Candidate {
            path: note.path,
            title: note.title,
        })
        .collect();
    if !indexed.is_empty() {
        return Some(indexed);
    }
    Some(
        session
            .vault
            .notes()
            .unwrap_or_default()
            .into_iter()
            .map(|note| switcher::Candidate {
                title: stem_of(&note.rel_path),
                path: note.rel_path.to_string_lossy().into_owned(),
            })
            .collect(),
    )
}

/// Recently opened notes of the active vault, most-recent-first.
fn recent_notes(state: &Rc<State>) -> Vec<String> {
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return Vec::new();
    };
    state
        .config
        .borrow()
        .recent_notes_for(session.vault.root())
        .to_vec()
}

/// Install a Ctrl+B accelerator that toggles the sidebar visibility.
fn wire_sidebar_toggle(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("toggle-sidebar", None);
    let sidebar_state = Rc::clone(state);
    action.connect_activate(move |_, _| {
        let visible = sidebar_state.sidebar_stack.get_visible();
        sidebar_state.sidebar_stack.set_visible(!visible);
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
    state.search_panel.set_accent(&next.chrome.accent);
    state.tags_panel.set_accent(&next.chrome.accent);
    state.report_panel.set_accent(&next.chrome.accent);
    state.git_panel.set_accent(&next.chrome.accent);
    state.backlinks.set_accent(&next.chrome.accent);
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
    save_note(state);
}

/// Write out unsaved edits before the window closes, so quitting never drops work.
fn wire_save_on_close(window: &ApplicationWindow, state: &Rc<State>) {
    let close_state = Rc::clone(state);
    window.connect_close_request(move |_| {
        save_if_dirty(&close_state);
        remember_layout(&close_state);
        gtk::glib::Propagation::Proceed
    });
}

/// Persists what the window looked like: the strip, and which folders are open.
fn remember_layout(state: &Rc<State>) {
    let expanded = state.session.borrow().as_ref().map(|session| {
        (
            session.vault.root().to_path_buf(),
            expanded_paths(&session.tree_model.borrow()),
        )
    });
    let mut config = state.config.borrow_mut();
    config.backlinks_expanded = state.backlinks.is_expanded();
    if let Some((root, folders)) = expanded {
        config.set_expanded_folders(&root, &folders);
    }
    config.save();
}

/// Save the buffer if it has unsaved edits, and say nothing when it does not.
///
/// Used where saving is a side effect of something else (switching notes,
/// closing the window) rather than something the user asked for directly.
fn save_if_dirty(state: &Rc<State>) {
    if state.dirty.get() {
        save_note(state);
    }
}

/// Install `Ctrl+S`, which writes the buffer to the file it came from.
fn wire_save(app: &Application, state: &Rc<State>) {
    let action = gio::SimpleAction::new("save", None);
    let save_state = Rc::clone(state);
    action.connect_activate(move |_, _| save_note(&save_state));
    app.add_action(&action);
    app.set_accels_for_action("app.save", &["<Primary>s"]);
}

/// Save the buffer to disk, reporting the outcome in the status bar.
///
/// Works in both modes: a vault note goes through the vault and is reindexed, a
/// single opened file is written directly. The built-in sample has nowhere to go.
fn save_note(state: &Rc<State>) {
    if !state.dirty.get() {
        say(state, "No changes");
        return;
    }
    let text = state.editor.text();

    let saved = if state.session.borrow().is_some() {
        save_current_note(state, &text)
    } else {
        save_single_file(state, &text)
    };

    match saved {
        Some(name) => {
            state.dirty.set(false);
            say(state, &format!("Saved {name}"));
            refresh_titles(state);
            refresh_backlinks(state);
            refresh_broken(state);
            refresh_git(state);
            refresh_window_title(state);
        }
        None => say(state, "Nothing to save"),
    }
}

/// Write `text` to the open note and reindex it. Returns the name saved.
fn save_current_note(state: &Rc<State>, text: &str) -> Option<String> {
    let session = state.session.borrow();
    let session = session.as_ref()?;
    let rel = session.current.borrow().clone()?;

    if let Err(err) = session.vault.write_note(&rel, text) {
        eprintln!("jotter: could not save {}: {err}", rel.display());
        say(state, "Save failed");
        return None;
    }
    if let Err(err) = vault_session::reindex_note_resolved(&session.vault, &session.index, &rel) {
        eprintln!("jotter: could not reindex {}: {err}", rel.display());
    }
    Some(rel.display().to_string())
}

/// Write `text` to the file opened in single-file mode. Returns the name saved.
fn save_single_file(state: &Rc<State>, text: &str) -> Option<String> {
    let path = state.single_file.borrow().clone()?;
    if let Err(err) = std::fs::write(&path, text) {
        eprintln!("jotter: could not save {}: {err}", path.display());
        say(state, "Save failed");
        return None;
    }
    Some(path.display().to_string())
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
        let was_clean = !changed_state.dirty.get();
        changed_state.dirty.set(true);
        if was_clean {
            refresh_window_title(&changed_state);
        }

        // Cancel a pending pass so only the latest change fires.
        if let Some(old) = changed_state.pending.borrow_mut().take() {
            old.remove();
        }

        let timeout_state = Rc::clone(&changed_state);
        let id = gtk::glib::timeout_add_local(Duration::from_millis(DEBOUNCE_MS), move || {
            // Tagging runs in either mode; the preview only re-renders when it shows.
            refresh_editor_links(&timeout_state);
            if timeout_state.dirty.get() {
                timeout_state.status.set_text("Unsaved changes (Ctrl+S)");
            }
            if timeout_state.stack.visible_child_name().as_deref() == Some(PAGE_PREVIEW) {
                // The caret in preview mode is stale; re-read it before rendering.
                *timeout_state.cached_caret.borrow_mut() = timeout_state.editor.caret_line();
                render_into_preview(&timeout_state);
            }
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
                    status.set_text(&indexed_text(i64::try_from(total).unwrap_or(i64::MAX)));
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
        } else {
            // An external edit can rename the note without moving its file.
            refresh_titles(&drain_state);
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
            if let Err(err) = vault_session::reindex_note_resolved(&session.vault, &session.index, rel) {
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
            if let Err(err) = vault_session::reindex_note_resolved(&session.vault, &session.index, to) {
                eprintln!("jotter: reindex on rename failed for {}: {err}", to.display());
            }
            true
        }
    }
}

/// What a delete asks before doing it. `notes` counts what a folder holds.
fn delete_question(name: &str, notes: Option<usize>) -> String {
    match notes {
        None => format!("Delete \"{name}\" to trash?"),
        Some(0) => format!("Delete the empty folder \"{name}\" to trash?"),
        Some(1) => format!("Delete \"{name}\" and the 1 note inside it to trash?"),
        Some(count) => format!("Delete \"{name}\" and the {count} notes inside it to trash?"),
    }
}

/// What a right-clicked tree row is, which decides the menu wording.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Note,
    Folder,
}

/// The context menu for a row of `kind`, or for empty space when there is none.
fn tree_menu(kind: Option<Kind>) -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("New note"), Some("app.tree-new-note"));
    menu.append(Some("New folder"), Some("app.tree-new-folder"));
    let Some(kind) = kind else {
        return menu;
    };
    let (rename, delete) = match kind {
        Kind::Note => ("Rename note", "Delete note to trash"),
        Kind::Folder => ("Rename folder", "Delete folder to trash"),
    };
    menu.append(Some(rename), Some("app.tree-rename"));
    menu.append(Some(delete), Some("app.tree-delete"));
    menu
}

/// Wires a right-click context menu on the tree with note/folder operations.
///
/// The right-clicked row (not the selection) is the operation target, resolved by
/// hit-testing the click and stashed in `target` for the actions to read. A click
/// in empty space leaves `target` as `None`, meaning the vault root.
fn wire_tree_context_menu(state: &Rc<State>, list_view: &ListView) {
    let popover = gtk::PopoverMenu::from_model(Some(&tree_menu(None)));
    popover.set_parent(list_view);
    popover.set_has_arrow(false);

    // Shared between the gesture (which sets it) and the actions (which read it).
    let target: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    let popover_click = popover.clone();
    let list_view_hit = list_view.clone();
    let target_press = Rc::clone(&target);
    let menu_state = Rc::clone(state);
    gesture.connect_pressed(move |_, _, x, y| {
        // Resolve the target from the row under the pointer, not the prior selection.
        let hit = row_at(&list_view_hit, x, y);
        let kind = hit.as_ref().map(|rel| {
            if is_file_node(&menu_state, &rel.to_string_lossy()) {
                Kind::Note
            } else {
                Kind::Folder
            }
        });
        popover_click.set_menu_model(Some(&tree_menu(kind)));
        if let Some(rel) = hit.as_ref() {
            select_in_tree_now(&menu_state, rel);
        }
        *target_press.borrow_mut() = hit;
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

    // Rename: a note keeps its .md, a folder is renamed with its whole subtree.
    let rename = gio::SimpleAction::new("tree-rename", None);
    let s = Rc::clone(state);
    let t = Rc::clone(target);
    rename.connect_activate(move |_, _| {
        let Some(rel) = t.borrow().clone() else {
            return;
        };
        let is_file = is_file_node(&s, &rel.to_string_lossy());
        let default = rel
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("note.md")
            .to_owned();
        let title = if is_file { "Rename note" } else { "Rename folder" };
        let s2 = Rc::clone(&s);
        prompt(&s, title, &default, move |name| {
            if is_file {
                rename_note(&s2, &rel, &name);
            } else {
                rename_folder(&s2, &rel, &name);
            }
        });
    });
    app.add_action(&rename);

    // Delete to trash: a folder goes whole, with everything under it.
    let delete = gio::SimpleAction::new("tree-delete", None);
    let s = Rc::clone(state);
    let t = Rc::clone(target);
    delete.connect_activate(move |_, _| {
        let Some(rel) = t.borrow().clone() else {
            return;
        };
        let is_file = is_file_node(&s, &rel.to_string_lossy());
        let name = rel.to_string_lossy().into_owned();
        let inside = (!is_file).then(|| notes_under(&s, &rel).len());
        let s2 = Rc::clone(&s);
        confirm(&s, &delete_question(&name, inside), "Delete", move || {
            if is_file {
                delete_note(&s2, &rel);
            } else {
                delete_folder(&s2, &rel);
            }
        });
    });
    app.add_action(&delete);
}

/// Indexed notes living under `rel`, by vault-relative path.
fn notes_under(state: &Rc<State>, rel: &Path) -> Vec<String> {
    let prefix = format!("{}/", vault_session::rel_to_key(rel));
    let session = state.session.borrow();
    let Some(session) = session.as_ref() else {
        return Vec::new();
    };
    session
        .index
        .all_notes()
        .unwrap_or_default()
        .into_iter()
        .map(|note| note.path)
        .filter(|path| path.starts_with(&prefix))
        .collect()
}

/// Leaves the editor showing another note, or nothing when the vault is empty.
fn close_current_note(state: &Rc<State>) {
    if let Some(session) = state.session.borrow().as_ref() {
        session.current.replace(None);
    }
    if let Some(rel) = first_note(state) {
        load_note(state, &rel);
        select_in_tree(state, &rel);
        return;
    }
    state.editor.set_initial_text("");
    state.dirty.set(false);
    refresh_backlinks(state);
    refresh_window_title(state);
}

/// Renames a folder and every note under it, following the open note if it moved.
fn rename_folder(state: &Rc<State>, rel: &Path, name: &str) {
    let name = name.trim();
    if name.is_empty() || rel.as_os_str().is_empty() {
        return;
    }
    let to = rel.parent().map(Path::to_path_buf).unwrap_or_default().join(name);
    let moved_current = {
        let session_ref = state.session.borrow();
        let Some(session) = session_ref.as_ref() else {
            return;
        };
        if let Err(err) = session.vault.rename_note(rel, &to) {
            eprintln!("jotter: could not rename folder {}: {err}", rel.display());
            return;
        }
        reindex_moved(session, rel, Some(&to));
        session
            .current
            .borrow()
            .as_ref()
            .and_then(|current| current.strip_prefix(rel).ok())
            .map(|tail| to.join(tail))
    };
    refresh_tree(state);
    match moved_current {
        // The open note moved with the folder, so the tree follows the note.
        Some(current) => {
            load_note(state, &current);
            select_in_tree(state, &current);
        }
        None => select_in_tree(state, &to),
    }
}

/// Moves a folder to the trash with everything under it.
fn delete_folder(state: &Rc<State>, rel: &Path) {
    if rel.as_os_str().is_empty() {
        return;
    }
    let held_current;
    {
        let session_ref = state.session.borrow();
        let Some(session) = session_ref.as_ref() else {
            return;
        };
        if let Err(err) = session.vault.delete_to_trash(rel) {
            eprintln!("jotter: could not delete folder {}: {err}", rel.display());
            return;
        }
        reindex_moved(session, rel, None);
        held_current = session
            .current
            .borrow()
            .as_ref()
            .is_some_and(|current| current.starts_with(rel));
    }
    refresh_tree(state);
    if held_current {
        close_current_note(state);
    }
}

/// Moves index rows from under `from` to under `to`, or drops them when the
/// folder went to the trash.
fn reindex_moved(session: &VaultSession, from: &Path, to: Option<&Path>) {
    let prefix = format!("{}/", vault_session::rel_to_key(from));
    let Ok(notes) = session.index.all_notes() else {
        return;
    };
    for note in notes.iter().filter(|note| note.path.starts_with(&prefix)) {
        let old = PathBuf::from(&note.path);
        let _ = vault_session::deindex_note(&session.index, &old);
        if let Some(to) = to
            && let Ok(tail) = old.strip_prefix(from)
        {
            let moved = to.join(tail);
            let _ = vault_session::reindex_note_resolved(&session.vault, &session.index, &moved);
        }
    }
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
                    let _ = vault_session::reindex_note_resolved(&session.vault, &session.index, rel);
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
                let _ = vault_session::reindex_note_resolved(&session.vault, &session.index, &to);
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
    let was_current;
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
        was_current = session.current.borrow().as_deref() == Some(rel);
    }
    refresh_tree(state);
    if was_current {
        close_current_note(state);
    }
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
fn confirm<F: Fn() + 'static>(state: &Rc<State>, question: &str, action: &str, on_yes: F) {
    let parent = state.sidebar.root().and_downcast::<gtk::Window>();
    let dialog = gtk::Window::builder()
        .title("Confirm")
        .modal(true)
        .default_width(380)
        .build();
    if let Some(parent) = parent.as_ref() {
        dialog.set_transient_for(Some(parent));
    }

    let content = gtk::Box::new(Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(
        &gtk::Label::builder()
            .label(question)
            .xalign(0.0)
            .wrap(true)
            .build(),
    );

    let buttons = gtk::Box::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let go = gtk::Button::with_label(action);
    go.add_css_class("destructive-action");
    buttons.append(&cancel);
    buttons.append(&go);
    content.append(&buttons);
    dialog.set_child(Some(&content));

    let go_dialog = dialog.clone();
    go.connect_clicked(move |_| {
        on_yes();
        go_dialog.close();
    });
    let cancel_dialog = dialog.clone();
    cancel.connect_clicked(move |_| cancel_dialog.close());

    let escape = gtk::EventControllerKey::new();
    let escape_dialog = dialog.clone();
    escape.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            escape_dialog.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    dialog.add_controller(escape);

    dialog.present();
    cancel.grab_focus();
}

/// Prompts for one line of text, running `on_ok` with what was typed.
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
    use super::{broken_label, delete_question, indexed_text};

    #[test]
    fn one_broken_link_reads_singular() {
        assert_eq!(broken_label(1), "1 broken link");
    }

    #[test]
    fn a_single_note_is_not_plural() {
        assert_eq!(indexed_text(1), "Indexed 1 note");
        assert_eq!(indexed_text(0), "Indexed 0 notes");
        assert_eq!(indexed_text(42), "Indexed 42 notes");
    }

    #[test]
    fn broken_links_are_counted() {
        assert_eq!(broken_label(4), "4 broken links");
    }

    #[test]
    fn deleting_a_note_names_it() {
        assert_eq!(
            delete_question("notes/plan.md", None),
            "Delete \"notes/plan.md\" to trash?"
        );
    }

    #[test]
    fn deleting_a_folder_counts_what_goes_with_it() {
        assert_eq!(
            delete_question("scratch", Some(3)),
            "Delete \"scratch\" and the 3 notes inside it to trash?"
        );
    }

    #[test]
    fn one_note_inside_reads_singular() {
        assert_eq!(
            delete_question("scratch", Some(1)),
            "Delete \"scratch\" and the 1 note inside it to trash?"
        );
    }

    #[test]
    fn an_empty_folder_says_it_is_empty() {
        assert_eq!(
            delete_question("scratch", Some(0)),
            "Delete the empty folder \"scratch\" to trash?"
        );
    }

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
