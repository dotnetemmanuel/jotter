//! jotter binary. Phase 0 built a bare GTK4 window; this adds a phase-1 preview
//! that applies the generated theme CSS so the chrome can be seen live. The real
//! wiring moves into the `jotter-app` crate in phase 1b.

use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Entry, HeaderBar,
    Image, Label, Orientation, gdk, gio,
};

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
    app.connect_startup(|_| load_theme());
    app.connect_activate(build_ui);
    app.run()
}

/// Generate the default theme (retro82, dark) and apply its chrome CSS.
fn load_theme() {
    let theme = jotter_theming::bundled::default_theme().expect("bundled default theme resolves");
    let provider = CssProvider::new();
    provider.load_from_string(&theme.to_gtk_css());

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_ui(app: &Application) {
    let logo = Image::from_gicon(&gio::FileIcon::new(&gio::File::for_path(LOGO)));
    logo.set_pixel_size(200);

    let label = Label::builder().label("jotter").build();

    // Sample controls so the neo-brutalist chrome (borders, hard shadows,
    // rounded corners) is visible while previewing the theme.
    let entry = Entry::builder().placeholder_text("Search notes").build();

    let new_note = Button::with_label("New note");
    new_note.add_css_class("suggested-action");
    let delete = Button::with_label("Delete");
    delete.add_css_class("destructive-action");
    let plain = Button::with_label("Preview");

    let buttons = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Center)
        .build();
    buttons.append(&new_note);
    buttons.append(&plain);
    buttons.append(&delete);

    let body = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .valign(Align::Center)
        .halign(Align::Center)
        .build();
    body.append(&logo);
    body.append(&label);
    body.append(&entry);
    body.append(&buttons);

    let header = HeaderBar::new();
    header.pack_start(&Button::with_label("Vault"));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("jotter")
        .default_width(1400)
        .default_height(900)
        .child(&body)
        .build();

    window.set_titlebar(Some(&header));
    window.present();
}
