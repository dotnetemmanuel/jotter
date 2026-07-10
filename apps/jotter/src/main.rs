//! jotter binary. Phase 1b: a thin entry point that hands off to `jotter-app`,
//! which owns the GTK application, the theme, and the edit-preview toggle loop.

fn main() -> gtk::glib::ExitCode {
    jotter_app::run()
}
