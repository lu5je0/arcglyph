#!/usr/bin/env bash
# Launch arcglyph as root while preserving the Wayland / D-Bus session so the
# overlay, tray and GUI can still talk to the user's compositor.
set -euo pipefail

BIN="$(cd "$(dirname "$0")" && pwd)/target/release/arcglyph"
if [[ ! -x "$BIN" ]]; then
    echo "not built yet: $BIN" >&2
    echo "run: cargo build --release" >&2
    exit 1
fi

exec sudo -E \
    XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}" \
    WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}" \
    DISPLAY="${DISPLAY:-}" \
    DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/bus}" \
    HOME="$HOME" \
    "$BIN" "$@"
