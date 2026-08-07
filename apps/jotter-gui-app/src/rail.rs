//! The icon rail: the one column that is always there.
//!
//! Modes, not panels. Search, tags, git, and the broken-link report are ways of
//! interacting with the notes rather than places to be, so they stay on their
//! keys and in the palette. That leaves two buttons: the notes toggle, which is
//! the mouse equivalent of `Ctrl+B`, and settings.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::Orientation;

/// Nerd Font glyphs, so the rail looks the same wherever jotter runs rather than
/// depending on whichever icon theme is installed. Both come from the Material
/// Design range, which is drawn on one grid: the older ranges mix side bearings,
/// which left the cogwheel visibly right of the notes icon.
const NOTES: &str = "\u{f0219}";
const SETTINGS: &str = "\u{f0493}";

/// One icon. The active marker is an underline on the label itself, so it is as
/// wide as the glyph rather than as wide as the button.
fn glyph(icon: &str) -> gtk::Label {
    // Centre-aligned rather than filling: a label that fills its button carries
    // an underline as wide as the button, not as wide as the icon.
    gtk::Label::builder()
        .label(icon)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build()
}

/// The rail.
pub struct Rail {
    root: gtk::Box,
    notes: gtk::ToggleButton,
    /// Set while the app is reflecting state, so a programmatic toggle does not
    /// echo back as a user request.
    quiet: std::cell::Cell<bool>,
}

impl Rail {
    /// Builds the rail. `on_notes` toggles the tree, `on_settings` opens settings.
    pub fn new<N: Fn(bool) + 'static, S: Fn() + 'static>(on_notes: N, on_settings: S) -> Rc<Self> {
        let notes = gtk::ToggleButton::builder()
            .active(true)
            .has_frame(false)
            .tooltip_text("Notes (Ctrl+B)")
            .child(&glyph(NOTES))
            .build();
        notes.add_css_class("rail-button");

        let settings = gtk::Button::builder()
            .has_frame(false)
            .tooltip_text("Settings")
            .child(&glyph(SETTINGS))
            .build();
        settings.add_css_class("rail-button");
        // The cogwheel is a wider glyph than the document, drawn from the same
        // origin, so centring by advance leaves its ink 1.5px right of the
        // square. This centres what you actually see.
        settings.add_css_class("rail-settings");
        settings.connect_clicked(move |_| on_settings());

        let spacer = gtk::Box::new(Orientation::Vertical, 0);
        spacer.set_vexpand(true);

        let root = gtk::Box::new(Orientation::Vertical, 4);
        root.add_css_class("rail");
        // Minimal, but wide enough to read as a column rather than a sliver.
        root.set_size_request(60, -1);
        root.append(&notes);
        root.append(&spacer);
        root.append(&settings);

        let rail = Rc::new(Self {
            root,
            notes,
            quiet: std::cell::Cell::new(false),
        });

        let toggling = Rc::clone(&rail);
        rail.notes.connect_toggled(move |button| {
            // Set_active from the Ctrl+B path lands here too, so the callback has
            // to be idempotent: it is given the state, not asked to flip one.
            if !toggling.quiet.get() {
                on_notes(button.is_active());
            }
        });

        rail
    }

    /// The widget to place at the left edge.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Reflects the tree's visibility without running the toggle callback.
    pub fn set_notes_active(&self, active: bool) {
        if self.notes.is_active() == active {
            return;
        }
        self.quiet.set(true);
        self.notes.set_active(active);
        self.quiet.set(false);
    }
}
