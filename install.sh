#!/usr/bin/env bash
# Installs (or updates) jotter for the current user: a real copy of the release
# binary, a desktop entry, and an icon, so it launches from the app launcher and
# never dies with a terminal.
#
# Development keeps running ./target/release/jotter-gui straight from the repo;
# this script is the promotion step that moves the current build into daily use.
set -euo pipefail
cd "$(dirname "$0")"

BIN="$HOME/.local/bin/jotter-gui"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/scalable/apps"
APP_ID="dev.jotter.Jotter"
# The retro82 dark text color, matching the ink the headerbar gives the same
# mark: currentColor has no meaning in a launcher, where it would fall back to
# black.
ICON_COLOR="#f6dcac"

cargo build --release

install -Dm755 target/release/jotter-gui "$BIN"

mkdir -p "$ICONS"
sed "s/currentColor/$ICON_COLOR/g" resources/icons/jotter.svg > "$ICONS/$APP_ID.svg"

# Raster sizes too: some launchers only look at fixed-size directories, and a
# long-running launcher reads the theme once, so refresh the cache it checks.
if command -v rsvg-convert >/dev/null; then
    for size in 48 128 256; do
        dir="$HOME/.local/share/icons/hicolor/${size}x${size}/apps"
        mkdir -p "$dir"
        # Width only, then pad to square: the mark is wider than tall, and
        # forcing both dimensions would squash it.
        rsvg-convert -w "$size" "$ICONS/$APP_ID.svg" | magick - -background none -gravity center -extent "${size}x${size}" "$dir/$APP_ID.png"
    done
fi
command -v gtk-update-icon-cache >/dev/null \
    && gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

mkdir -p "$APPS"
# Exec gets the absolute path: the launcher spawns through the systemd user
# session, whose PATH does not include ~/.local/bin.
cat > "$APPS/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=jotter
Comment=A native GTK4 markdown vault
Exec=$BIN %f
Icon=$APP_ID
Terminal=false
Categories=Utility;TextEditor;
MimeType=text/markdown;
StartupNotify=true
EOF

command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" || true

echo "installed $(du -h "$BIN" | cut -f1) binary to $BIN"
echo "launch: from the app launcher (jotter), or plain \"jotter-gui\" in a shell"
echo "a running jotter-gui keeps its old build until it is restarted"
