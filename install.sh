#!/usr/bin/env bash
# Installs (or updates) jotter for the current user: a real copy of the release
# binary, a desktop entry, and an icon, so it launches from the app launcher and
# never dies with a terminal.
#
# Development keeps running ./target/release/jotter straight from the repo; this
# script is the promotion step that moves the current build into daily use.
set -euo pipefail
cd "$(dirname "$0")"

BIN="$HOME/.local/bin/jotter"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/scalable/apps"
APP_ID="dev.jotter.Jotter"
# The retro82 accent, stamped into the icon: currentColor has no meaning in a
# launcher, where it would fall back to black.
ICON_COLOR="#d9762b"

cargo build --release

install -Dm755 target/release/jotter "$BIN"

mkdir -p "$ICONS"
sed "s/currentColor/$ICON_COLOR/g" resources/icons/jotter.svg > "$ICONS/$APP_ID.svg"

mkdir -p "$APPS"
cat > "$APPS/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=jotter
Comment=A native GTK4 markdown vault
Exec=jotter %f
Icon=$APP_ID
Terminal=false
Categories=Utility;TextEditor;
MimeType=text/markdown;
StartupNotify=true
EOF

command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" || true

echo "installed $(du -h "$BIN" | cut -f1) binary to $BIN"
echo "launch: from the app launcher (jotter), or plain \"jotter\" in a shell"
echo "a running jotter keeps its old build until it is restarted"
