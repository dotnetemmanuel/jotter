//! The settings window: a small dialog that changes the app as you touch it.
//!
//! No OK and no Cancel. Every change applies to the window behind immediately
//! and is written to the config, which is the whole reason this is a dialog
//! rather than a page: you watch your own notes change font while you choose.

use std::rc::Rc;

use std::cell::{Cell, RefCell};

use gtk::prelude::*;
use gtk::{Orientation, gdk, glib};

use jotter_theming::{Mode, Style, Theme};

/// What the window asks the app to change.
pub enum Change {
    /// Switch to this theme id.
    Theme(String),
    /// Switch to light or dark.
    Mode(Mode),
    /// Switch between the classic look and the TUI one.
    Style(Style),
    /// Set the editor font family, or clear it back to the theme's own.
    EditorFont(Option<String>),
    /// Set the font size, which the editor and the preview share.
    Size(u32),
    /// Set the preview's font family, or clear it back to the theme's own.
    PreviewFont(Option<String>),
}

/// A font as the window needs to show it.
pub struct Font {
    /// What the theme itself asks for, which may be a stack like `Inter, sans-serif`.
    pub theme: String,
    /// What the user chose instead, if they chose anything.
    pub chosen: Option<String>,
}

/// What the window shows when it opens.
pub struct Current {
    /// Theme id.
    pub theme: String,
    /// Light or dark.
    pub mode: Mode,
    /// The classic look or the TUI one.
    pub style: Style,
    /// Editor font: what the theme asks for, and the user's override if any.
    pub editor_font: Font,
    /// Preview font: what the theme asks for, and the user's override if any.
    pub preview_font: Font,
    /// Font size, shared by both.
    pub size: u32,
}

/// A live settings window, plus the means to keep it honest when the app
/// changes theme behind its back (`Ctrl+T`, say).
pub struct Handle {
    /// The window itself, for closing and raising.
    pub window: gtk::Window,
    /// Reflects the app's current mode back into the controls.
    sync: Rc<dyn Fn(Mode)>,
    /// Reflects the app's current style back into the controls.
    sync_style: Rc<dyn Fn(Style)>,
    /// Reflects a font size changed from outside, by Ctrl+plus or the wheel.
    sync_size: Rc<dyn Fn(u32)>,
}

impl Handle {
    /// Updates the controls to match `mode` without reporting a change back.
    pub fn show_mode(&self, mode: Mode) {
        (self.sync)(mode);
    }

    /// Updates the controls to match `style` without reporting a change back.
    pub fn show_style(&self, style: Style) {
        (self.sync_style)(style);
    }

    /// Updates the size control without reporting a change back.
    pub fn show_size(&self, size: u32) {
        (self.sync_size)(size);
    }
}

/// Opens the settings window over `parent`, reporting changes as they happen.
pub fn open<F: Fn(Change) + 'static>(
    parent: &gtk::Window,
    themes: &[String],
    current: &Current,
    on_change: F,
) -> Handle {
    let current_theme = current.theme.as_str();
    let current_mode = current.mode;
    let on_change = Rc::new(on_change);

    // Set while the app is reflecting its own state, so a programmatic change
    // does not echo back as a request to change it again.
    let quiet = Rc::new(Cell::new(false));
    let mode_now = Rc::new(Cell::new(current_mode));
    let swatches: Rc<RefCell<Vec<gtk::DrawingArea>>> = Rc::new(RefCell::new(Vec::new()));
    let (themes_row, theme_picker) =
        theme_buttons(themes, current_theme, &mode_now, &swatches, &on_change);

    let (modes, light, dark) =
        mode_buttons(current.style, current_mode, &mode_now, &swatches, &quiet, &on_change);
    let (styles, classic, tui) = style_buttons(current.style, &quiet, &on_change);

    let grid = gtk::Grid::builder()
        .row_spacing(10)
        .column_spacing(14)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    grid.attach(&row_label("Style"), 0, 0, 1, 1);
    grid.attach(&styles, 1, 0, 1, 1);
    grid.attach(&row_label("Theme"), 0, 1, 1, 1);
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
    grid.attach(&themes_scroller, 1, 1, 1, 1);
    grid.attach(&row_label("Mode"), 0, 2, 1, 1);
    grid.attach(&modes, 1, 2, 1, 1);

    let (size, size_faces) = size_row(&grid, current.style, current.size, &on_change);

    attach_font_rows(&grid, current, &on_change);

    let window = gtk::Window::builder()
        .title("Settings")
        .transient_for(parent)
        // Modal, so there is only ever one state to be in: open or closed. The
        // main window cannot take a key while it is up, which is what makes
        // Escape reliable whatever was focused when it opened.
        .modal(true)
        .default_width(560)
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

    let sync_style = style_sync(&classic, &tui, &light, &dark, size_faces, &quiet);

    let sync_size: Rc<dyn Fn(u32)> = {
        let size = size.clone();
        Rc::new(move |value| size.set_label(&value.to_string()))
    };

    Handle {
        window,
        sync,
        sync_style,
        sync_size,
    }
}

/// The light and dark pair, and the flag that keeps a programmatic toggle from
/// echoing back as a request to change the mode.
fn mode_buttons(
    style: Style,
    current_mode: Mode,
    mode_now: &Rc<Cell<Mode>>,
    swatches: &Rc<RefCell<Vec<gtk::DrawingArea>>>,
    quiet: &Rc<Cell<bool>>,
    on_change: &Rc<impl Fn(Change) + 'static>,
) -> (gtk::Box, gtk::ToggleButton, gtk::ToggleButton) {
    let light = gtk::ToggleButton::with_label(&crate::style::button(style, "Light"));
    let dark = gtk::ToggleButton::with_label(&crate::style::button(style, "Dark"));
    dark.set_group(Some(&light));
    match current_mode {
        Mode::Light => light.set_active(true),
        Mode::Dark => dark.set_active(true),
    }
    let switched = Rc::clone(on_change);
    let repaint = Rc::clone(mode_now);
    let areas = Rc::clone(swatches);
    let hushed = Rc::clone(quiet);
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
    let hushed = Rc::clone(quiet);
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

    (modes, light, dark)
}

/// The classic and TUI pair, and the flag that keeps a programmatic toggle from
/// echoing back as a request to change the style.
fn style_buttons(
    current: Style,
    quiet: &Rc<Cell<bool>>,
    on_change: &Rc<impl Fn(Change) + 'static>,
) -> (gtk::Box, gtk::ToggleButton, gtk::ToggleButton) {
    let classic = gtk::ToggleButton::with_label(&crate::style::button(current, "Classic"));
    let tui = gtk::ToggleButton::with_label(&crate::style::button(current, "TUI"));
    tui.set_group(Some(&classic));
    match current {
        Style::Classic => classic.set_active(true),
        Style::Tui => tui.set_active(true),
    }
    for (button, style) in [(&classic, Style::Classic), (&tui, Style::Tui)] {
        let switched = Rc::clone(on_change);
        let hushed = Rc::clone(quiet);
        button.connect_toggled(move |button| {
            if button.is_active() && !hushed.get() {
                switched(Change::Style(style));
            }
        });
    }
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.append(&classic);
    row.append(&tui);
    (row, classic, tui)
}

/// Reflects a style changed from outside back into the window's own buttons:
/// the classic/TUI and light/dark toggles, and the size steps.
fn style_sync(
    classic: &gtk::ToggleButton,
    tui: &gtk::ToggleButton,
    light: &gtk::ToggleButton,
    dark: &gtk::ToggleButton,
    size_faces: Vec<(gtk::Button, String)>,
    quiet: &Rc<Cell<bool>>,
) -> Rc<dyn Fn(Style)> {
    let classic = classic.clone();
    let tui = tui.clone();
    let quiet = Rc::clone(quiet);
    let mut faces: Vec<(gtk::Button, String)> = vec![
        (classic.clone().upcast::<gtk::Button>(), "Classic".to_string()),
        (tui.clone().upcast::<gtk::Button>(), "TUI".to_string()),
        (light.clone().upcast::<gtk::Button>(), "Light".to_string()),
        (dark.clone().upcast::<gtk::Button>(), "Dark".to_string()),
    ];
    faces.extend(size_faces);
    Rc::new(move |style| {
        quiet.set(true);
        match style {
            Style::Classic => classic.set_active(true),
            Style::Tui => tui.set_active(true),
        }
        quiet.set(false);
        for (button, label) in &faces {
            button.set_label(&crate::style::button(style, label));
        }
    })
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

/// The size row: minus, the number, plus, since that is the order the hands
/// expect and a spin button puts both arrows on one side.
fn size_row(
    grid: &gtk::Grid,
    style: Style,
    current: u32,
    on_change: &Rc<impl Fn(Change) + 'static>,
) -> (gtk::Label, Vec<(gtk::Button, String)>) {
    let value = Rc::new(Cell::new(current));
    let shown = gtk::Label::new(Some(&current.to_string()));
    shown.add_css_class("size-value");
    shown.set_width_chars(2);

    let row = gtk::Box::new(Orientation::Horizontal, 8);
    let mut faces = Vec::new();
    for (label, up) in [("\u{2212}", false), ("+", true)] {
        let button = gtk::Button::with_label(&crate::style::button(style, label));
        button.add_css_class("size-step");
        faces.push((button.clone(), label.to_string()));
        let value = Rc::clone(&value);
        let stepped = shown.clone();
        let on_change = Rc::clone(on_change);
        button.connect_clicked(move |_| {
            let next = crate::appearance::step(value.get(), up);
            if next == value.get() {
                return;
            }
            value.set(next);
            stepped.set_label(&next.to_string());
            on_change(Change::Size(next));
        });
        if up {
            row.append(&shown);
        }
        row.append(&button);
    }
    row.append(&gtk::Label::new(Some("shared by the editor and the preview")));

    grid.attach(&row_label("Font size"), 0, 3, 1, 1);
    grid.attach(&row, 1, 3, 1, 1);
    (shown, faces)
}

/// The two font rows: the editor's, monospaced only, and the preview's.
fn attach_font_rows(
    grid: &gtk::Grid,
    current: &Current,
    on_change: &Rc<impl Fn(Change) + 'static>,
) {
    let picked = Rc::clone(on_change);
    let editor = font_row(&crate::fonts::families(true), &current.editor_font, move |name| {
        picked(Change::EditorFont(name));
    });
    grid.attach(&row_label("Editor font"), 0, 4, 1, 1);
    grid.attach(&editor, 1, 4, 1, 1);

    let picked = Rc::clone(on_change);
    let preview = font_row(&crate::fonts::families(false), &current.preview_font, move |name| {
        picked(Change::PreviewFont(name));
    });
    grid.attach(&row_label("Preview font"), 0, 5, 1, 1);
    grid.attach(&preview, 1, 5, 1, 1);
}

/// A font row: a filter box over the families installed, and a size beside it.
///
/// A list rather than a dropdown for the same reason the themes are buttons: a
/// popover grabs the keyboard, so Escape closes the list instead of the window.
fn font_row<P: Fn(Option<String>) + 'static>(
    families: &[String],
    current: &Font,
    on_pick: P,
) -> gtk::Box {
    let filter = gtk::SearchEntry::builder()
        .placeholder_text("Filter fonts")
        .build();

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("font-list");
    let scroller = gtk::ScrolledWindow::builder()
        .min_content_height(120)
        .max_content_height(120)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();

    let shown = Rc::new(RefCell::new(Vec::<Option<String>>::new()));
    let fill = {
        let list = list.clone();
        let shown = Rc::clone(&shown);
        let all = families.to_vec();
        let chosen = current.chosen.clone();
        let theme_font = current.theme.clone();
        move |query: &str| {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            // The theme's own font is always the first entry, so there is a way
            // back from any choice; the chosen font, if any, is pinned under it.
            // Both stay put while you filter.
            let mut names: Vec<Option<String>> = vec![None];
            if let Some(chosen) = chosen.clone() {
                names.push(Some(chosen));
            }
            let pinned = chosen.clone();
            names.extend(
                crate::fonts::matching(&all, query)
                    .into_iter()
                    .filter(|name| pinned.as_deref() != Some(name.as_str()))
                    .map(Some),
            );

            let active = usize::from(chosen.is_some());
            for (index, name) in names.iter().enumerate() {
                let row = gtk::Box::new(Orientation::Horizontal, 6);
                let tick = gtk::Label::new(Some(if index == active { "\u{2713}" } else { " " }));
                tick.add_css_class("font-tick");
                row.append(&tick);
                let text = match name {
                    Some(name) => name.clone(),
                    None => format!("Theme default ({theme_font})"),
                };
                row.append(&gtk::Label::builder().xalign(0.0).label(text).build());
                list.append(&row);
            }
            if let Some(row) = list.row_at_index(i32::try_from(active).unwrap_or(0)) {
                list.select_row(Some(&row));
            }
            *shown.borrow_mut() = names;
        }
    };
    fill("");
    let fill = Rc::new(fill);

    let typing = Rc::clone(&fill);
    filter.connect_search_changed(move |entry| typing(entry.text().as_str()));

    let picking = Rc::clone(&shown);
    list.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            return;
        };
        let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
        if let Some(name) = picking.borrow().get(index) {
            on_pick(name.clone());
        }
    });

    filter.set_hexpand(true);
    let row = gtk::Box::new(Orientation::Vertical, 6);
    row.append(&filter);
    row.append(&scroller);
    row
}

/// A left-hand label for one row.
fn row_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder().xalign(0.0).label(text).build();
    label.add_css_class("settings-label");
    label
}
