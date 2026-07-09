# Arcglyph

Elegant mouse-gesture daemon for Wayland (KDE Plasma).

Hold **right mouse button**, draw a shape, release. Arcglyph recognizes the
gesture, forwards the mapped keyboard shortcut to the focused app, and
draws a smooth blue trail while you move.

- Gesture recognition via evdev grab + uinput injection
- KWin-native focused-window detection through the Scripting DBus API
- Per-app gesture groups — one group can cover multiple apps
- Configurable via YAML or an inline GUI editor
- Config hot-reload — save in GUI and changes apply instantly
- Wayland-first: overlay drawn through `wlr-layer-shell`, no X11 involved
- Single binary, ships with a system-tray control (StatusNotifierItem)
- Fullscreen windows are auto-bypassed — games and video players stay untouched
- Smart bypass: apps without configured gestures get normal right-click behavior

## Quick start

```
# 1. Grant read access to /dev/input/event* and /dev/uinput (one time)
./setup-perms.sh
# --- log out, log back in ---

# 2. Build
cargo build --release

# 3. Run
./target/release/arcglyph
```

Enable "Autostart" from the GUI header to launch arcglyph with your session.

## Gestures

Directions use the numpad convention (4-way cardinal):

```
    8         ↑
  4 . 6  =>  ← . →
    2         ↓
```

A pattern is the sequence of direction changes. `26` = down then right
(an L-shape). Each direction has a 90° sector giving ±45° tolerance,
so imprecise drawing is handled gracefully.

## Configuration

`~/.config/arcglyph/arcglyph.yaml` (override with `ARCGLYPH_CONFIG=`).

Gestures are organized into **groups**. Each group associates a set of
apps with a list of gestures:

```yaml
enabled: true
lang: en
groups:
  - name: Browser
    apps: [google-chrome, chromium, microsoft-edge]
    gestures:
      - {label: Back, pattern: "4", keys: [LEFTALT, LEFT]}
      - {label: Forward, pattern: "6", keys: [LEFTALT, RIGHT]}
      - {label: Scroll to top, pattern: "8", keys: [HOME]}
      - {label: Scroll to bottom, pattern: "2", keys: [END]}
      - {label: Close tab, pattern: "26", keys: [LEFTCTRL, W]}
      - {label: Reopen closed tab, pattern: "24", keys: [LEFTCTRL, LEFTSHIFT, T]}
      - {label: Reload, pattern: "46", keys: [F5]}
  - name: Global
    apps: []
    gestures:
      - {label: Switch window, pattern: "46", keys: [LEFTALT, TAB]}
```

Fields:

- `lang` — GUI language: `en` (default) or `zh`. Toggle it from the header button.
- `groups[*].name` — display name shown in the GUI.
- `groups[*].apps` — list of app_id substrings (case-insensitive). Empty means "every window".
- `groups[*].gestures[*].pattern` — numpad-direction sequence.
- `groups[*].gestures[*].keys` — evdev key names pressed as a chord.
- `groups[*].gestures[*].label` — description shown in the GUI.
- `groups[*].gestures[*].enabled` — per-gesture switch.

Config changes are detected automatically via inotify — no restart needed.

## GUI

Left-click the tray icon or pick **Preferences…** to open the editor.

- Sidebar lists gesture groups; select one to edit its gestures
- Each group can associate multiple apps via chips or the **Pick window** button
- **Pick window**: click the button, then click any window — its app_id is automatically added to the group
- Set a shortcut by clicking **Record** and pressing the key combo directly
- Switch the interface language (English / 中文) from the header button
- Closing the window keeps the daemon running; **Quit** stops everything

## Notes

- The daemon grabs every pointing device with a right-button + relative axes.
  Motion, wheel and other buttons are replayed through a virtual mouse so
  nothing else changes.
- Focused-window detection uses KWin's Scripting DBus API. A background
  thread refreshes the cache every 300 ms so the right-click path never blocks.
- If a fullscreen window is focused, arcglyph bypasses completely.
- If the cursor is outside the focused window, right-click passes through unchanged.
- Apps without any configured gestures get native right-click with zero delay.
