//! The settings window: a small dialog that changes the app as you touch it.
//!
//! No OK and no Cancel. Every change applies to the window behind immediately
//! and is written to the config, which is the whole reason this is a dialog
//! rather than a page: you watch your own notes change font while you choose.

use std::rc::Rc;

use std::cell::{Cell, RefCell};

use gtk::prelude::*;
use gtk::{Orientation, gdk, glib};

use jotter_theming::{Mode, Theme};

/// What the window asks the app to change.
pub enum Change {
    /// Switch to this theme id.
    Theme(String),
    /// Switch to light or dark.
    Mode(Mode),
}

/// A live settings window, plus the means to keep it honest when the app
/// changes theme behind its back (`Ctrl+T`, say).
pub struct Handle {
    /// The window itself, for closing and raising.
    pub window: gtk::Window,
    /// Reflects the app's current mode back into the controls.
    sync: Rc<dyn Fn(Mode)>,
}

impl Handle {
    /// Updates the controls to match `mode` without reporting a change back.
    pub fn show_mode(&self, mode: Mode) {
        (self.sync)(mode);
    }
}

/// Opens the settings window over `parent`, reporting changes as they happen.
pub fn open<F: Fn(Change) + 'static>(
    parent: &gtk::Window,
    themes: &[String],
    current_theme: &str,
    current_mode: Mode,
    on_change: F,
) -> Handle {
    let on_change = Rc::new(on_change);

    let mode_now = Rc::new(Cell::new(current_mode));
    let swatches: Rc<RefCell<Vec<gtk::DrawingArea>>> = Rc::new(RefCell::new(Vec::new()));
    let (themes_row, theme_picker) =
        theme_buttons(themes, current_theme, &mode_now, &swatches, &on_change);

    let (modes, light, dark, quiet) = mode_buttons(current_mode, &mode_now, &swatches, &on_change);

    let grid = gtk::Grid::builder()
        .row_spacing(10)
        .column_spacing(14)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    grid.attach(&row_label("Theme"), 0, 0, 1, 1);
    // Capped and scrollable: a themes folder can hold any number of files, and
    // an uncapped row of buttons would grow the window past the screen.
    let themes_scroller = gtk::ScrolledWindow::builder()
        .max_content_height(200)
        .propagate_natural_height(true)
        // Without this the scroller asks for its minimum width and the row wraps
        // to one button per line however much room the window has.
        .propagate_natural_width(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&themes_row)
        .build();
    grid.attach(&themes_scroller, 1, 0, 1, 1);
    grid.attach(&row_label("Mode"), 0, 1, 1, 1);
    grid.attach(&modes, 1, 1, 1, 1);

    let window = gtk::Window::builder()
        .title("Settings")
        .transient_for(parent)
        // Modal, so there is only ever one state to be in: open or closed. The
        // main window cannot take a key while it is up, which is what makes
        // Escape reliable whatever was focused when it opened.
        .modal(true)
        .default_width(480)
        .resizable(false)
        .child(&grid)
        .build();
    window.add_css_class("settings");

    // An explicit close button, rather than relying on whatever decoration the
    // compositor happens to draw around a dialog.
    let bar = gtk::HeaderBar::new();
    bar.set_show_title_buttons(false);
    let close = gtk::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("settings-close");
    close.set_tooltip_text(Some("Close (Esc)"));
    let closing = window.clone();
    close.connect_clicked(move |_| closing.close());
    bar.pack_end(&close);
    window.set_titlebar(Some(&bar));

    let keys = gtk::EventControllerKey::new();
    // Capture, so a focused dropdown or toggle cannot swallow the key first.
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let closing = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            closing.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    // A dropdown popover takes the keyboard while it is up. When it goes away
    // GTK does not always hand focus back, which leaves the window looking open
    // but answering nothing, Escape included.
    let refocus = theme_picker.clone();
    window.connect_notify_local(Some("is-active"), move |window, _| {
        if window.is_active() && gtk::prelude::GtkWindowExt::focus(window).is_none() {
            refocus.grab_focus();
        }
    });

    window.present();

    let sync: Rc<dyn Fn(Mode)> = {
        let light = light.clone();
        let dark = dark.clone();
        let mode_now = Rc::clone(&mode_now);
        let swatches = Rc::clone(&swatches);
        let quiet = Rc::clone(&quiet);
        Rc::new(move |mode| {
            quiet.set(true);
            match mode {
                Mode::Light => light.set_active(true),
                Mode::Dark => dark.set_active(true),
            }
            quiet.set(false);
            mode_now.set(mode);
            for area in swatches.borrow().iter() {
                area.queue_draw();
            }
        })
    };

    Handle { window, sync }
}

/// The light and dark pair, and the flag that keeps a programmatic toggle from
/// echoing back as a request to change the mode.
fn mode_buttons(
    current_mode: Mode,
    mode_now: &Rc<Cell<Mode>>,
    swatches: &Rc<RefCell<Vec<gtk::DrawingArea>>>,
    on_change: &Rc<impl Fn(Change) + 'static>,
) -> (gtk::Box, gtk::ToggleButton, gtk::ToggleButton, Rc<Cell<bool>>) {
    let light = gtk::ToggleButton::with_label("Light");
    let dark = gtk::ToggleButton::with_label("Dark");
    dark.set_group(Some(&light));
    match current_mode {
        Mode::Light => light.set_active(true),
        Mode::Dark => dark.set_active(true),
    }
    // Set while the app is reflecting its own state, so a programmatic toggle
    // does not echo back as a request to change it.
    let quiet = Rc::new(Cell::new(false));

    let switched = Rc::clone(on_change);
    let repaint = Rc::clone(mode_now);
    let areas = Rc::clone(swatches);
    let hushed = Rc::clone(&quiet);
    light.connect_toggled(move |button| {
        if button.is_active() && !hushed.get() {
            // The swatches show the mode you are in, so they repaint with it.
            repaint.set(Mode::Light);
            for area in areas.borrow().iter() {
                area.queue_draw();
            }
            switched(Change::Mode(Mode::Light));
        }
    });
    let switched = Rc::clone(on_change);
    let repaint = Rc::clone(mode_now);
    let areas = Rc::clone(swatches);
    let hushed = Rc::clone(&quiet);
    dark.connect_toggled(move |button| {
        if button.is_active() && !hushed.get() {
            // The swatches show the mode you are in, so they repaint with it.
            repaint.set(Mode::Dark);
            for area in areas.borrow().iter() {
                area.queue_draw();
            }
            switched(Change::Mode(Mode::Dark));
        }
    });
    let modes = gtk::Box::new(Orientation::Horizontal, 8);
    modes.append(&light);
    modes.append(&dark);

    (modes, light, dark, quiet)
}

/// How wide and tall one theme swatch is drawn.
const SWATCH: (i32, i32) = (108, 26);

/// The colors a swatch shows, in order, taken from the theme's own chrome.
fn bars(theme: &Theme) -> Vec<gdk::RGBA> {
    [
        &theme.chrome.background,
        &theme.chrome.surface,
        &theme.chrome.accent,
        &theme.chrome.focus,
        &theme.chrome.text,
        &theme.chrome.danger,
    ]
    .into_iter()
    .filter_map(|color| gdk::RGBA::parse(color).ok())
    .collect()
}

/// A theme drawn in its own colors, because an id tells you nothing about the
/// look. Holds both palettes, so it repaints when the mode changes.
fn swatch(dark: Vec<gdk::RGBA>, light: Vec<gdk::RGBA>, mode: &Rc<Cell<Mode>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(SWATCH.0);
    area.set_content_height(SWATCH.1);
    let mode = Rc::clone(mode);
    area.set_draw_func(move |_, cairo, width, height| {
        let colors = match mode.get() {
            Mode::Dark => &dark,
            Mode::Light => &light,
        };
        if colors.is_empty() {
            return;
        }
        let count = f64::from(u32::try_from(colors.len()).unwrap_or(1));
        let bar = f64::from(width) / count;
        for (index, color) in colors.iter().enumerate() {
            cairo.set_source_rgb(
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
            );
            let at = f64::from(u32::try_from(index).unwrap_or(0)) * bar;
            cairo.rectangle(at, 0.0, bar.ceil(), f64::from(height));
            let _ = cairo.fill();
        }
    });
    area
}

/// The theme row: one toggle per theme, grouped so they behave as radios.
///
/// Buttons rather than a dropdown, because a popover takes a keyboard grab: it
/// swallows Escape and does not reliably hand focus back afterwards.
fn theme_buttons(
    themes: &[String],
    current_theme: &str,
    mode: &Rc<Cell<Mode>>,
    swatches: &Rc<RefCell<Vec<gtk::DrawingArea>>>,
    on_change: &Rc<impl Fn(Change) + 'static>,
) -> (gtk::FlowBox, gtk::ToggleButton) {
    let row = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .column_spacing(8)
        .row_spacing(8)
        .build();

    let mut first: Option<gtk::ToggleButton> = None;
    let mut current: Option<gtk::ToggleButton> = None;
    for id in themes {
        let button = gtk::ToggleButton::new();
        let face = gtk::Box::new(Orientation::Vertical, 4);
        if let Ok(file) = crate::themes::load(id) {
            let dark = file.resolve(Mode::Dark).map(|theme| bars(&theme)).unwrap_or_default();
            let light = file.resolve(Mode::Light).map(|theme| bars(&theme)).unwrap_or_default();
            let area = swatch(dark, light, mode);
            swatches.borrow_mut().push(area.clone());
            face.append(&area);
        }
        let name = gtk::Label::new(Some(id));
        name.add_css_class("theme-name");
        face.append(&name);
        button.set_child(Some(&face));
        button.add_css_class("theme-button");
        match &first {
            Some(group) => button.set_group(Some(group)),
            None => first = Some(button.clone()),
        }
        if id == current_theme {
            button.set_active(true);
            current = Some(button.clone());
        }
        let picked = Rc::clone(on_change);
        let id = id.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                picked(Change::Theme(id.clone()));
            }
        });
        row.append(&button);
    }

    let focus = current.or(first).unwrap_or_default();
    (row, focus)
}

/// A left-hand label for one row.
fn row_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder().xalign(0.0).label(text).build();
    label.add_css_class("settings-label");
    label
}
