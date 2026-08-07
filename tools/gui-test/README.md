# Driving jotter without a human

jotter is a GTK4 Wayland app, so testing it used to mean asking someone to click.
These two pieces remove that: a headless compositor to run it in, and a virtual
pointer to drive it. Everything happens off screen, on its own Wayland socket, so
it never touches the desktop you are working on.

Nothing here is part of the app. It is excluded from the workspace so
`cargo build --workspace` does not pull Wayland client crates into normal builds.

## Running it

```sh
cargo build --release
tools/gui-test/cage-run.sh ./target/release/jotter-gui /path/to/test-vault
```

The script prints the socket cage took, usually `wayland-0` when your desktop is
on `wayland-1`. Point everything else at it:

```sh
export WAYLAND_DISPLAY=wayland-0
grim shot.png                                   # screenshot
wtype -M ctrl -k h -m ctrl                      # keys
tools/gui-test/wlpoint/target/release/wlpoint "m:120:200,w:200,d,w:100,u"
```

GTK actions can also be fired over D-Bus, which needs no display at all:

```sh
busctl --user call dev.jotter.Jotter /dev/jotter/Jotter \
  org.gtk.Actions Activate 'sava{sv}' keys 0 0
```

## wlpoint

A pointer over `zwlr_virtual_pointer_v1`, which cage supports. The script is a
comma-separated list of steps:

| step | does |
|---|---|
| `m:X:Y` | move to absolute pixel X,Y |
| `d` / `u` | press / release the left button |
| `R` | click the right button |
| `w:MS` | wait MS milliseconds |

Coordinates are pixels against the output size, 1280x720 by default. Override
with `WLPOINT_EXTENT=1920x1080` if you start cage with a different output.

Build it once: `cd tools/gui-test/wlpoint && cargo build --release`.

## A drag has to look human

This is the part that cost an afternoon. Motions sent back to back register as a
plain click and `GtkDragSource` never fires. A drag that works looks like:

```
m:128:215,w:300,d,w:250,m:128:222,w:60,m:128:235,w:60,m:127:250,w:60,m:128:273,w:1200,u
```

Press, wait a beat, then several small motions about 60 ms apart, hover the target
long enough for the drop highlight to settle, then release.

## Traps

- **Never launch while a jotter-gui is already running.** It is a single-instance
  app, so the launch is handed to the running process and your headless one exits
  immediately. `cage-run.sh` refuses to start in that case.
- **Isolate the config.** `XDG_CONFIG_HOME=$(mktemp -d)` keeps test runs out of
  your real `~/.config/jotter/config.toml`, which otherwise collects test vaults
  in its recents and inherits your theme.
- **Pre-create `.jotter/`** in a test vault to skip the adopt gate. The gate keeps
  focus on Cancel, so Enter refuses rather than confirms.
- Screenshot after every step that changes the tree: row positions move, and a
  drag aimed at a stale y lands on the wrong row.
