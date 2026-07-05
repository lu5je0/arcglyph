#!/usr/bin/env bash
# Install a XDG .desktop entry so arcglyph appears in the app launcher
# and can be started/autostarted from KDE's normal application menu.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/target/release/arcglyph"
if [[ ! -x "$BIN" ]]; then
    echo "not built yet: $BIN" >&2
    echo "run: cargo build --release" >&2
    exit 1
fi

APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APPS_DIR"

FILE="$APPS_DIR/arcglyph.desktop"
cat > "$FILE" <<EOF
[Desktop Entry]
Type=Application
Name=Arcglyph
GenericName=Mouse Gestures
Comment=Right-click drag mouse gesture daemon for Wayland
Exec=$BIN
Icon=input-mouse
Terminal=false
Categories=Utility;Settings;
Keywords=gesture;mouse;shortcut;
StartupNotify=false
X-GNOME-Autostart-enabled=true
X-KDE-autostart-after=panel
X-KDE-StartupNotify=false
EOF

echo "installed $FILE"

# Also expose it as an autostart entry so ges launches with the session.
AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
mkdir -p "$AUTOSTART_DIR"
cp -f "$FILE" "$AUTOSTART_DIR/arcglyph.desktop"
echo "installed $AUTOSTART_DIR/arcglyph.desktop"

echo
echo "Done. Log out and back in (or run '$BIN') to start."
