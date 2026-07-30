//! Injects pointer events into a wlroots compositor over `zwlr_virtual_pointer_v1`,
//! so a GTK drag can be driven with no human and no visible desktop.
//!
//! Usage: `wlpoint <script>`, where the script is comma-separated steps:
//!
//! - `m:X:Y` move to absolute pixel X,Y
//! - `d` press the left button, `u` release it
//! - `R` click the right button
//! - `w:MS` wait MS milliseconds
//!
//! Coordinates are pixels against the output size, 1280x720 unless
//! `WLPOINT_EXTENT` says otherwise (`WLPOINT_EXTENT=1920x1080`).
//!
//! A drag needs motions spaced out in time: sent back to back they read as a
//! plain click and `GtkDragSource` never starts. See the README next door.

use std::time::{Duration, Instant};

use wayland_client::protocol::wl_pointer::ButtonState;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
/// Coordinate space the compositor maps motion into, matching cage headless.
const DEFAULT_EXTENT: (u32, u32) = (1280, 720);

struct App {
    manager: Option<ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event
            && interface == "zwlr_virtual_pointer_manager_v1"
        {
            state.manager = Some(registry.bind::<ZwlrVirtualPointerManagerV1, _, _>(
                name,
                version.min(2),
                qh,
                (),
            ));
        }
    }
}

delegate_noop!(App: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(App: ignore ZwlrVirtualPointerV1);

/// The output size to map motion against, from `WLPOINT_EXTENT` or the default.
fn extent() -> (u32, u32) {
    let Ok(spec) = std::env::var("WLPOINT_EXTENT") else {
        return DEFAULT_EXTENT;
    };
    let Some((w, h)) = spec.split_once(['x', 'X']) else {
        return DEFAULT_EXTENT;
    };
    match (w.parse(), h.parse()) {
        (Ok(w), Ok(h)) => (w, h),
        _ => DEFAULT_EXTENT,
    }
}

fn main() {
    let Some(script) = std::env::args().nth(1) else {
        eprintln!("usage: wlpoint <script>, for example \"m:100:200,w:200,d,w:100,u\"");
        std::process::exit(2);
    };

    let conn = Connection::connect_to_env().expect("connect to wayland");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = App { manager: None };
    queue.roundtrip(&mut app).expect("registry roundtrip");
    let manager = app
        .manager
        .clone()
        .expect("compositor does not offer zwlr_virtual_pointer_manager_v1");
    let pointer = manager.create_virtual_pointer(None, &qh, ());

    let (x_extent, y_extent) = extent();
    let start = Instant::now();
    let stamp = || u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);

    for step in script.split(',').filter(|step| !step.is_empty()) {
        match step.split(':').collect::<Vec<_>>().as_slice() {
            ["m", x, y] => {
                let x = x.parse().expect("x is a number");
                let y = y.parse().expect("y is a number");
                pointer.motion_absolute(stamp(), x, y, x_extent, y_extent);
                pointer.frame();
            }
            ["d"] => {
                pointer.button(stamp(), BTN_LEFT, ButtonState::Pressed);
                pointer.frame();
            }
            ["u"] => {
                pointer.button(stamp(), BTN_LEFT, ButtonState::Released);
                pointer.frame();
            }
            ["R"] => {
                pointer.button(stamp(), BTN_RIGHT, ButtonState::Pressed);
                pointer.frame();
                pointer.button(stamp(), BTN_RIGHT, ButtonState::Released);
                pointer.frame();
            }
            ["w", ms] => {
                conn.flush().expect("flush");
                std::thread::sleep(Duration::from_millis(ms.parse().expect("ms is a number")));
            }
            other => {
                eprintln!("wlpoint: unknown step {other:?}");
                std::process::exit(2);
            }
        }
        conn.flush().expect("flush");
        queue.roundtrip(&mut app).expect("roundtrip");
    }

    conn.flush().expect("flush");
    // The pointer dies with the connection, so give the compositor a moment to
    // deliver the last events before the process exits under it.
    std::thread::sleep(Duration::from_millis(200));
}
