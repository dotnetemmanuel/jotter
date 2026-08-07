//! The keybinding sheet behind `Ctrl+H`: what the app is actually bound to.
//!
//! Accelerators are never written down here. Each row names an app action and
//! what it does, and the label comes back from GTK at display time, so a binding
//! that moved cannot leave a stale sheet behind. An action with no accelerator is
//! dropped rather than shown blank: the palette is where those live.

use gtk::prelude::*;
use gtk::{Orientation, gdk, glib};

use jotter_theming::Style;

/// Actions the sheet lists, grouped the way it shows them.
const SECTIONS: [(&str, &[(&str, &str)]); 4] = [
    (
        "Notes",
        &[
            ("quick-open", "Go to note"),
            ("save", "Save note"),
            ("toggle-mode", "Switch between edit and preview"),
        ],
    ),
    (
        "Finding things",
        &[
            ("command-palette", "Command palette"),
            ("search", "Search every note"),
            ("tags", "Browse tags"),
        ],
    ),
    (
        "Appearance",
        &[
            ("toggle-theme", "Light and dark"),
            ("toggle-sidebar", "Show and hide the tree"),
            ("font-bigger", "Bigger text"),
            ("font-smaller", "Smaller text"),
            ("font-reset", "Text size back to the theme"),
        ],
    ),
    (
        "The app",
        &[
            ("settings", "Settings"),
            ("keys", "This sheet"),
            ("open-vault", "Open another vault"),
            ("git-sync", "Sync the vault"),
        ],
    ),
];

/// Keys a widget owns rather than an action, so GTK has no accelerator to read.
const WIDGET_KEYS: [(&str, &[(&str, &str)]); 2] = [
    (
        "Wherever you are",
        &[
            ("Escape", "Close the panel, popup, or dialog"),
            ("Enter", "Open what is selected"),
            ("Ctrl+scroll", "Text size"),
        ],
    ),
    (
        "In the conflict resolver",
        &[
            ("a", "Take incoming"),
            ("d", "Take yours"),
            ("b", "Take both"),
            ("e", "Edit the block by hand, Ctrl+S to keep it"),
            ("n / Shift+N", "Next and previous block"),
            ("] / [", "Next and previous note"),
        ],
    ),
];

/// One line of the sheet: the keys, and what they do.
#[derive(Debug, PartialEq, Eq)]
pub struct Binding {
    /// Accelerator label, such as `Ctrl+S`.
    pub keys: String,
    /// What pressing it does.
    pub what: String,
}

/// A titled block of bindings.
#[derive(Debug, PartialEq, Eq)]
pub struct Section {
    /// Heading shown above the block.
    pub name: String,
    /// The lines, in the order the table names them.
    pub bindings: Vec<Binding>,
}

/// Every section of the sheet, with `accel` asked for each action's label.
///
/// An action `accel` has nothing for is left out, and a section left empty by
/// that goes with it.
#[must_use]
pub fn sections(accel: &dyn Fn(&str) -> String) -> Vec<Section> {
    let bound = SECTIONS.iter().map(|(name, rows)| Section {
        name: (*name).to_owned(),
        bindings: rows
            .iter()
            .filter_map(|(action, what)| {
                let keys = accel(action);
                (!keys.is_empty()).then(|| Binding {
                    keys,
                    what: (*what).to_owned(),
                })
            })
            .collect(),
    });
    let fixed = WIDGET_KEYS.iter().map(|(name, rows)| Section {
        name: (*name).to_owned(),
        bindings: rows
            .iter()
            .map(|(keys, what)| Binding {
                keys: (*keys).to_owned(),
                what: (*what).to_owned(),
            })
            .collect(),
    });
    bound
        .chain(fixed)
        .filter(|section| !section.bindings.is_empty())
        .collect()
}

/// Shows the sheet over `parent`, closing on Escape or `Ctrl+H` again.
///
/// Returns the window so the caller can close it when the key comes a second
/// time, the way the settings window works.
pub fn open(parent: &gtk::Window, sections: &[Section], style: Style) -> gtk::Window {
    let columns = gtk::Box::new(Orientation::Horizontal, 28);
    columns.set_margin_top(16);
    columns.set_margin_bottom(16);
    columns.set_margin_start(16);
    columns.set_margin_end(16);
    // Two columns, so the sheet reads at a glance instead of scrolling.
    let (left, right) = (column(), column());
    for (index, section) in sections.iter().enumerate() {
        let side = if index % 2 == 0 { &left } else { &right };
        side.append(&section_block(section, style));
    }
    columns.append(&left);
    columns.append(&right);

    let window = gtk::Window::builder()
        .title("Keyboard shortcuts")
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .child(&columns)
        .build();
    window.add_css_class("settings");

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
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let closing = window.clone();
    keys.connect_key_pressed(move |_, key, _, state| {
        let control = state.contains(gdk::ModifierType::CONTROL_MASK);
        if key == gdk::Key::Escape || (control && key == gdk::Key::h) {
            closing.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    window.present();
    window
}

/// One vertical stack of sections.
fn column() -> gtk::Box {
    let side = gtk::Box::new(Orientation::Vertical, 18);
    side.set_hexpand(true);
    side
}

/// A heading and its bindings, keys on the left and meaning on the right.
fn section_block(section: &Section, style: Style) -> gtk::Box {
    let block = gtk::Box::new(Orientation::Vertical, 6);
    let heading = gtk::Label::builder()
        .label(crate::style::heading(style, &section.name))
        .xalign(0.0)
        .build();
    heading.add_css_class("keysheet-heading");
    block.append(&heading);

    let grid = gtk::Grid::builder()
        .row_spacing(4)
        .column_spacing(14)
        .build();
    for (row, binding) in section.bindings.iter().enumerate() {
        let keys = gtk::Label::builder()
            .label(&binding.keys)
            .xalign(1.0)
            .build();
        keys.add_css_class("keysheet-keys");
        let what = gtk::Label::builder()
            .label(&binding.what)
            .xalign(0.0)
            .build();
        let row = i32::try_from(row).unwrap_or(i32::MAX);
        grid.attach(&keys, 0, row, 1, 1);
        grid.attach(&what, 1, row, 1, 1);
    }
    block.append(&grid);
    block
}

#[cfg(test)]
mod tests {
    use super::{Binding, sections};

    fn all(action: &str) -> String {
        format!(
            "Ctrl+{}",
            action.chars().next().unwrap_or('x').to_uppercase()
        )
    }

    #[test]
    fn every_section_is_present_when_everything_is_bound() {
        let found = sections(&all);
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Notes",
                "Finding things",
                "Appearance",
                "The app",
                "Wherever you are",
                "In the conflict resolver",
            ]
        );
    }

    #[test]
    fn a_row_pairs_the_read_back_label_with_what_it_does() {
        let found = sections(&|_| "Ctrl+S".to_owned());
        assert_eq!(
            found[0].bindings[0],
            Binding {
                keys: "Ctrl+S".to_owned(),
                what: "Go to note".to_owned()
            }
        );
    }

    #[test]
    fn an_unbound_action_is_left_out() {
        let found = sections(&|action| {
            if action == "save" {
                String::new()
            } else {
                all(action)
            }
        });
        let notes: Vec<&str> = found[0].bindings.iter().map(|b| b.what.as_str()).collect();
        assert_eq!(notes, ["Go to note", "Switch between edit and preview"]);
    }

    #[test]
    fn a_section_with_nothing_bound_disappears() {
        let found = sections(&|_| String::new());
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        // Only the widget keys are left: they do not come from an accelerator.
        assert_eq!(names, ["Wherever you are", "In the conflict resolver"]);
    }

    #[test]
    fn the_widget_keys_are_never_dropped() {
        let found = sections(&|_| String::new());
        assert_eq!(found[1].bindings.len(), 6);
    }
}
