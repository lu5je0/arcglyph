# Arcglyph

Elegant mouse-gesture daemon for Wayland (KDE Plasma).

Hold **right mouse button**, draw a shape, release. Arcglyph recognizes the
gesture, forwards the mapped keyboard shortcut to the focused app, and
draws a smooth blue trail while you move.

- Gesture recognition via evdev grab + uinput injection
- KWin-native focused-window detection through the Scripting DBus API
- Per-app bindings (Chrome / Firefox / anything with an `app_id`)
- Configurable via YAML or an inline GUI editor
- Wayland-first: overlay drawn through `wlr-layer-shell`, no X11 involved
- Single binary, ships with a system-tray control (StatusNotifierItem)
- Fullscreen windows are auto-bypassed — games and video players stay untouched

## Quick start

```
# 1. Grant read access to /dev/input/event* and /dev/uinput (one time)
./setup-perms.sh
# --- log out, log back in ---

# 2. Build
cargo build --release

# 3. Install a desktop entry + KDE autostart (optional)
./install-desktop.sh

# 4. Run
./target/release/arcglyph
# or, if you installed the desktop entry, pick "Arcglyph" from the launcher.
```

## Gestures

Directions use the numpad convention:

```
7 8 9      ↖ ↑ ↗
4 . 6      ← . →
1 2 3      ↙ ↓ ↘
```

A pattern is the sequence of direction changes. `26` = down, then right
(a right-angle L). Small drifts within 90° of the current direction are
absorbed, so diagonals are only recorded when the motion actually goes
diagonally.

Defaults (scoped to Chromium-family browsers):

| Pattern | Action |
|---|---|
| `4` | Back (Alt+←) |
| `6` | Forward (Alt+→) |
| `8` | Scroll to top (Home) |
| `2` | Scroll to bottom (End) |
| `26` | Close tab (Ctrl+W) |
| `24` | Reopen closed tab (Ctrl+Shift+T) |
| `46` | Reload (F5) |

Every other app sees the right mouse button unchanged — including its
context menu, since arcglyph only intercepts when a real drag happens.

## Configuration

`~/.config/arcglyph/arcglyph.yaml` (override with `ARCGLYPH_CONFIG=`).

One YAML flow-style entry per binding:

```yaml
- { label: 关闭标签页, pattern: "26", keys: [LEFTCTRL, W], apps: [google-chrome] }
```

Fields:

- `pattern` — numpad-direction sequence.
- `keys` — evdev key names pressed as a chord (`linux/input-event-codes.h`).
- `apps` — optional. Case-insensitive substrings matched against the focused
  window's `app_id`. Empty means "every window".
- `label` — description shown in the GUI.

## GUI

Left-click the tray icon or pick **Preferences…** to open the editor.
Closing the window keeps the daemon running; **Quit** stops everything.

## Notes

- The daemon grabs every pointing device with a right-button + relative axes.
  Motion, wheel and other buttons are replayed through a virtual mouse so
  nothing else changes.
- Focused-window detection uses KWin's Scripting DBus API (same trick as
  [kdotool](https://github.com/jinliu/kdotool)). One round-trip per press,
  typically <20 ms.
- If a fullscreen window is focused when you press the right button,
  arcglyph bypasses the whole cycle so games and video players don't lose
  input.
