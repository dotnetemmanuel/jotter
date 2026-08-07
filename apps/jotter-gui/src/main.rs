#![warn(clippy::pedantic)]
//! jotter-gui binary. Phase 1b: a thin entry point that hands off to `jotter-gui-app`,
//! which owns the GTK application, the theme, and the edit-preview toggle loop.

fn main() -> gtk::glib::ExitCode {
    jotter_gui_app::run()
}
