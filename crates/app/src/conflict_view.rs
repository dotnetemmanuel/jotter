//! The conflict resolver: one block at a time, with both sides beside it.
//!
//! It takes over the main pane rather than living in the sidebar, because
//! choosing between two versions of a paragraph means reading both of them.
//! Modelled on the resolver in Cairn, down to the vocabulary: the sides are
//! **incoming** and **yours**, never ours and theirs, which flip under rebase.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Orientation, ScrolledWindow, gdk, glib};

use jotter_git::conflict::{Choice, Region};

use crate::conflict_model::{Model, heading, progress_text};

/// Below this width the three panes stop being readable and stack instead.
const NARROW: i32 = 900;

/// What the resolver asks the app to do.
pub enum Request {
    /// The answers changed: write them and refresh.
    Answered,
    /// Finish the rebase.
    Continue,
    /// Undo the sync that conflicted.
    Abort,
    /// Leave the resolver, conflict still pending.
    Leave,
}

/// The resolver page.
pub struct View {
    root: gtk::Box,
    heading: gtk::Label,
    progress: gtk::Label,
    panes: gtk::Box,
    incoming: Pane,
    resolution: Pane,
    yours: Pane,
    edit: gtk::Button,
    save: gtk::Button,
    proceed: gtk::Button,
    model: RefCell<Model>,
    editing: std::cell::Cell<bool>,
    /// Set while a rebase is pending, so Continue works even once every block
    /// has been answered and staged and the model has nothing left to count.
    pending: std::cell::Cell<bool>,
}

/// One column: a title over a monospaced body.
struct Pane {
    root: gtk::Box,
    title: gtk::Label,
    body: gtk::TextView,
}

impl Pane {
    fn new(title: &str, class: &str) -> Self {
        let label = gtk::Label::builder().xalign(0.0).label(title).build();
        label.add_css_class("conflict-title");
        label.add_css_class(class);

        let body = gtk::TextView::new();
        body.set_editable(false);
        body.set_cursor_visible(false);
        body.set_monospace(true);
        body.set_wrap_mode(gtk::WrapMode::WordChar);
        body.add_css_class("conflict-body");
        body.add_css_class(class);

        let scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&body)
            .build();

        let root = gtk::Box::new(Orientation::Vertical, 4);
        root.set_hexpand(true);
        root.append(&label);
        root.append(&scroller);
        Self { root, title: label, body }
    }

    fn set_text(&self, text: &str) {
        self.body.buffer().set_text(text);
    }

    fn set_title(&self, text: &str) {
        self.title.set_label(text);
    }

    fn text(&self) -> String {
        let buffer = self.body.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }
}

impl View {
    /// Builds the page. `on_request` runs when the user asks for something.
    pub fn new<R: Fn(Request) + 'static>(on_request: R) -> Rc<Self> {
        let heading = gtk::Label::builder().xalign(0.0).build();
        heading.add_css_class("conflict-heading");
        let progress = gtk::Label::builder().xalign(1.0).hexpand(true).build();
        progress.add_css_class("conflict-progress");

        let header = gtk::Box::new(Orientation::Horizontal, 12);
        header.add_css_class("conflict-header");
        header.append(&heading);
        header.append(&progress);

        let incoming = Pane::new("Incoming", "conflict-incoming");
        let resolution = Pane::new("Resolution", "conflict-resolution");
        let yours = Pane::new("Yours", "conflict-yours");

        let panes = gtk::Box::new(Orientation::Horizontal, 10);
        panes.set_vexpand(true);
        panes.append(&incoming.root);
        panes.append(&resolution.root);
        panes.append(&yours.root);

        let take_incoming = action("Take incoming", "A");
        let take_yours = action("Take yours", "D");
        let keep_both = action("Take both", "B");
        let edit = action("Edit by hand", "E");
        let save = action("Save edit", "Ctrl+S");
        save.set_visible(false);
        let proceed = gtk::Button::with_label("Continue");
        proceed.add_css_class("suggested-action");
        proceed.set_sensitive(false);
        let abort = gtk::Button::with_label("Abort sync");
        abort.add_css_class("destructive-action");

        let bar = gtk::Box::new(Orientation::Horizontal, 8);
        bar.add_css_class("conflict-actions");
        for button in [&take_incoming, &take_yours, &keep_both, &edit, &save] {
            bar.append(button);
        }
        let spacer = gtk::Box::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        bar.append(&spacer);
        bar.append(&proceed);
        bar.append(&abort);

        let root = gtk::Box::new(Orientation::Vertical, 8);
        root.add_css_class("conflict");
        // A Box takes no focus of its own, so the key controller below would
        // never see a keystroke: the page has to be focusable to be driven.
        root.set_focusable(true);
        root.append(&header);
        root.append(&panes);
        root.append(&bar);

        let view = Rc::new(Self {
            root,
            heading,
            progress,
            panes,
            incoming,
            resolution,
            yours,
            edit,
            save,
            proceed: proceed.clone(),
            model: RefCell::new(Model::new(Vec::new())),
            editing: std::cell::Cell::new(false),
            pending: std::cell::Cell::new(false),
        });

        let on_request = Rc::new(on_request);
        view.wire(&take_incoming, Choice::Incoming, &on_request);
        view.wire(&take_yours, Choice::Yours, &on_request);
        view.wire(&keep_both, Choice::Both, &on_request);

        let editing = Rc::clone(&view);
        view.edit.connect_clicked(move |_| editing.begin_edit());
        let saving = Rc::clone(&view);
        let saved = Rc::clone(&on_request);
        view.save.connect_clicked(move |_| {
            saving.commit_edit();
            saved(Request::Answered);
        });

        let asked = Rc::clone(&on_request);
        proceed.connect_clicked(move |_| asked(Request::Continue));
        let asked = Rc::clone(&on_request);
        abort.connect_clicked(move |_| asked(Request::Abort));

        view.wire_keys(&on_request);
        view.wire_layout();
        view
    }

    /// Hangs a choice off a button.
    fn wire(self: &Rc<Self>, button: &gtk::Button, choice: Choice, on_request: &Rc<impl Fn(Request) + 'static>) {
        let view = Rc::clone(self);
        let on_request = Rc::clone(on_request);
        button.connect_clicked(move |_| {
            view.answer(choice.clone());
            on_request(Request::Answered);
        });
    }

    /// The widget to place in the main stack.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Replaces the conflicts being resolved, keeping the cursor where it can be.
    pub fn set_model(&self, model: Model) {
        let (file, block) = {
            let old = self.model.borrow();
            (old.file, old.block)
        };
        let mut model = model;
        if model.files.get(file).is_some_and(|held| block < held.blocks()) {
            model.file = file;
            model.block = block;
        }
        *self.model.borrow_mut() = model;
        self.editing.set(false);
        self.redraw();
    }

    /// Records that a rebase is waiting to be finished.
    pub fn allow_continue(&self, pending: bool) {
        self.pending.set(pending);
        self.redraw();
    }

    /// Per-note progress for the rail: path, answered, and total blocks.
    #[must_use]
    pub fn file_progress(&self) -> Vec<(String, usize, usize)> {
        self.model
            .borrow()
            .files
            .iter()
            .map(|file| (file.path.clone(), file.answered(), file.blocks()))
            .collect()
    }

    /// The answers as they stand, per file, for writing back to disk.
    #[must_use]
    pub fn answers(&self) -> Vec<(String, Vec<Choice>)> {
        self.model
            .borrow()
            .files
            .iter()
            .map(|file| (file.path.clone(), file.choices.clone()))
            .collect()
    }

    /// Puts the cursor on the first block of `path`.
    pub fn focus_file(&self, path: &str) {
        {
            let mut model = self.model.borrow_mut();
            if let Some(index) = model.files.iter().position(|file| file.path == path) {
                model.file = index;
                model.block = 0;
            }
        }
        self.redraw();
    }

    fn answer(&self, choice: Choice) {
        self.model.borrow_mut().answer(choice);
        self.redraw();
    }

    fn step(&self, forward: bool) {
        self.model.borrow_mut().step(forward);
        self.editing.set(false);
        self.redraw();
    }

    fn jump_file(&self, forward: bool) {
        self.model.borrow_mut().jump_file(forward);
        self.editing.set(false);
        self.redraw();
    }

    /// Opens the resolution pane for typing, seeded with both sides.
    fn begin_edit(&self) {
        let seed = match self.model.borrow().region() {
            Some(region) => both_sides(region),
            None => return,
        };
        self.editing.set(true);
        self.resolution.set_text(&seed);
        self.resolution.body.set_editable(true);
        self.resolution.body.set_cursor_visible(true);
        self.resolution.body.grab_focus();
        self.edit.set_visible(false);
        self.save.set_visible(true);
    }

    /// Takes what was typed as the answer.
    fn commit_edit(&self) {
        if !self.editing.get() {
            return;
        }
        let text = self.resolution.text();
        self.editing.set(false);
        self.model.borrow_mut().answer(Choice::Custom(text));
        self.redraw();
    }

    /// Repaints everything from the model.
    fn redraw(&self) {
        let model = self.model.borrow();
        self.heading.set_label(&heading(&model));
        self.progress.set_label(&progress_text(&model));
        self.proceed
            .set_sensitive(model.all_answered() || (self.pending.get() && model.files.is_empty()));

        let Some(region) = model.region() else {
            self.incoming.set_text("");
            self.yours.set_text("");
            self.resolution.set_text("");
            return;
        };
        self.incoming.set_text(&lines(&region.incoming));
        self.yours.set_text(&lines(&region.yours));

        let choice = model.choice();
        self.resolution.set_title(match choice {
            Choice::Unresolved => "Resolution",
            _ => "Resolution \u{2713}",
        });
        if self.editing.get() {
            return;
        }
        self.resolution.body.set_editable(false);
        self.resolution.body.set_cursor_visible(false);
        self.edit.set_visible(true);
        self.save.set_visible(false);
        self.resolution.set_text(&resolution_text(region, &choice));
    }

    /// Keys, in Cairn's vocabulary so the two apps do not disagree.
    ///
    /// Attached to the window rather than the page: the resolver owns the whole
    /// main area while it is up, and focus may legitimately be sitting in the
    /// sidebar rail. Gated on the page being on screen, and on focus not being
    /// in someone else's text widget.
    fn wire_keys(self: &Rc<Self>, on_request: &Rc<impl Fn(Request) + 'static>) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let view = Rc::clone(self);
        let on_request = Rc::clone(on_request);
        keys.connect_key_pressed(move |_, key, _, state| {
            if !view.root.is_mapped() || view.someone_else_is_typing() {
                return glib::Propagation::Proceed;
            }
            let control = state.contains(gdk::ModifierType::CONTROL_MASK);
            if view.editing.get() {
                return match key {
                    gdk::Key::Escape => {
                        view.editing.set(false);
                        view.redraw();
                        glib::Propagation::Stop
                    }
                    gdk::Key::s if control => {
                        view.commit_edit();
                        on_request(Request::Answered);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                };
            }
            let answered = |choice: Choice| {
                view.answer(choice);
                on_request(Request::Answered);
                glib::Propagation::Stop
            };
            match key {
                gdk::Key::a => answered(Choice::Incoming),
                gdk::Key::d => answered(Choice::Yours),
                gdk::Key::b => answered(Choice::Both),
                gdk::Key::e => {
                    view.begin_edit();
                    glib::Propagation::Stop
                }
                gdk::Key::n | gdk::Key::Tab => {
                    view.step(true);
                    glib::Propagation::Stop
                }
                gdk::Key::N | gdk::Key::ISO_Left_Tab => {
                    view.step(false);
                    glib::Propagation::Stop
                }
                gdk::Key::bracketright => {
                    view.jump_file(true);
                    glib::Propagation::Stop
                }
                gdk::Key::bracketleft => {
                    view.jump_file(false);
                    glib::Propagation::Stop
                }
                gdk::Key::Return if view.proceed.is_sensitive() => {
                    on_request(Request::Continue);
                    glib::Propagation::Stop
                }
                gdk::Key::Escape => {
                    on_request(Request::Leave);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        // Attached on map, because the window does not exist at construction.
        let attaching = Rc::clone(self);
        let keys = std::cell::RefCell::new(Some(keys));
        self.root.connect_map(move |root| {
            let Some(keys) = keys.borrow_mut().take() else {
                return;
            };
            match root.root().and_downcast::<gtk::Window>() {
                Some(window) => window.add_controller(keys),
                None => attaching.root.add_controller(keys),
            }
        });
    }

    /// Whether focus sits in a text widget that is not the resolution pane, so
    /// typing in the sidebar is never mistaken for answering a conflict.
    fn someone_else_is_typing(&self) -> bool {
        let Some(window) = self.root.root().and_downcast::<gtk::Window>() else {
            return false;
        };
        let Some(focused) = gtk::prelude::GtkWindowExt::focus(&window) else {
            return false;
        };
        if focused.is_ancestor(&self.resolution.body) || focused == self.resolution.body.clone().upcast::<gtk::Widget>() {
            return false;
        }
        focused.is::<gtk::Entry>() || focused.is::<gtk::TextView>() || focused.is::<gtk::Text>()
    }

    /// Three columns while there is room, stacked when there is not.
    fn wire_layout(self: &Rc<Self>) {
        let view = Rc::clone(self);
        self.root.connect_map(move |root| view.reflow(root.width()));
        let view = Rc::clone(self);
        self.root
            .connect_notify_local(Some("width-request"), move |root, _| view.reflow(root.width()));

        let view = Rc::clone(self);
        let resize = gtk::EventControllerMotion::new();
        resize.connect_enter(move |controller, _, _| {
            if let Some(widget) = controller.widget() {
                view.reflow(widget.width());
            }
        });
        self.root.add_controller(resize);
    }

    /// Applies the layout for `width`.
    fn reflow(&self, width: i32) {
        let narrow = width > 0 && width < NARROW;
        let wanted = if narrow {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        };
        if self.panes.orientation() != wanted {
            self.panes.set_orientation(wanted);
        }
    }
}

/// A button labelled with its key, the way the palette shows accelerators.
fn action(label: &str, key: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_tooltip_text(Some(&format!("{label} ({key})")));
    button.add_css_class("conflict-action");
    button
}

/// Both sides, one after the other, as the seed for a hand-written answer.
fn both_sides(region: &Region) -> String {
    let mut text = lines(&region.incoming);
    text.push_str(&lines(&region.yours));
    text
}

/// Joins lines for display, one per line.
fn lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    format!("{}\n", lines.join("\n"))
}

/// What the resolution pane shows for a choice.
fn resolution_text(region: &Region, choice: &Choice) -> String {
    match choice {
        Choice::Unresolved => {
            "Not resolved yet.\n\n\
             a  take incoming\n\
             d  take yours\n\
             b  take both\n\
             e  edit by hand\n\n\
             n and N step through the conflicts, [ and ] jump between notes."
                .to_string()
        }
        Choice::Incoming => lines(&region.incoming),
        Choice::Yours => lines(&region.yours),
        Choice::Both => both_sides(region),
        Choice::Custom(text) => text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{both_sides, lines, resolution_text};
    use jotter_git::conflict::{Choice, Region};

    fn region() -> Region {
        Region {
            incoming: vec!["from the remote".to_string()],
            base: None,
            yours: vec!["what I wrote".to_string()],
        }
    }

    #[test]
    fn lines_end_with_a_newline() {
        assert_eq!(lines(&["one".to_string(), "two".to_string()]), "one\ntwo\n");
    }

    #[test]
    fn an_empty_side_shows_nothing() {
        assert_eq!(lines(&[]), "");
    }

    #[test]
    fn both_sides_put_incoming_first() {
        assert_eq!(both_sides(&region()), "from the remote\nwhat I wrote\n");
    }

    #[test]
    fn an_unanswered_block_shows_the_choices() {
        let text = resolution_text(&region(), &Choice::Unresolved);
        assert!(text.contains("take incoming"));
        assert!(text.contains("edit by hand"));
    }

    #[test]
    fn an_answered_block_shows_what_was_chosen() {
        assert_eq!(
            resolution_text(&region(), &Choice::Yours),
            "what I wrote\n"
        );
        assert_eq!(
            resolution_text(&region(), &Choice::Custom("mine, edited".to_string())),
            "mine, edited"
        );
    }
}
