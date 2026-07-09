//! jotter binary. Phase 0 builds a bare GTK4 window; later phases move the real
//! wiring into the `jotter-app` crate and keep this entry point thin.

use gtk::prelude::*;
use gtk::{Align, Application, ApplicationWindow, HeaderBar, Image, Label, Orientation, gio};

/// Reverse-DNS application id, stable for GTK settings and Wayland app matching.
const APP_ID: &str = "se.mindfulstack.jotter";

/// The logo as a symbolic icon so its `currentColor` fill follows the theme
/// foreground: light on a dark theme, dark on a light theme.
const LOGO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/icons/jotter-symbolic.svg"
);

fn main() -> gtk::glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let logo = Image::from_gicon(&gio::FileIcon::new(&gio::File::for_path(LOGO)));
    logo.set_pixel_size(200);

    let label = Label::builder().label("jotter").build();

    let body = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .valign(Align::Center)
        .halign(Align::Center)
        .build();
    body.append(&logo);
    body.append(&label);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("jotter")
        .default_width(1400)
        .default_height(900)
        .child(&body)
        .build();

    window.set_titlebar(Some(&HeaderBar::new()));
    window.present();
}
