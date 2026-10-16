# Agent Instructions for 8BitDo Micro Focus Switch

This guide instructs AI agents (Antigravity, Claude, Cursor, Copilot, etc.) on how to inspect, add, customize, and verify button mapping profiles for the 8BitDo Micro controller.

---

## 1. Project Anatomy

- **`~/.config/8bitdo-micro/profiles.toml`**: The user's active configuration file. Edit this file to add or adjust app mappings.
- **`src/eightbitdo_micro_focus_switch/config.py`**: Configuration parser and default TOML template.
- **`src/eightbitdo_micro_focus_switch/protocol.py`**: Keycode tables, D-pad rotation math, and hardware BLE encoders.
- **`src/eightbitdo_micro_focus_switch/daemon.py`**: Background focus-aware remapper engine (Hyprland IPC + `uinput`/`ble`).
- **`src/eightbitdo_micro_focus_switch/visualizer.py`**: SVG and interactive HTML diagram generator.
- **`src/eightbitdo_micro_focus_switch/cli.py`**: CLI entry point.

---

## 2. Profile Structure (`profiles.toml`)

Profiles live in `~/.config/8bitdo-micro/profiles.toml`. Each profile represents an application context:

```toml
[profiles.spotify]
description = "Spotify Music Controller"
match_class = "spotify"                    # Case-insensitive regex tested against window class
match_title = ".*"                         # Optional case-insensitive regex for window title
turn = 0                                   # D-pad orientation: 0, 90, 180, or 270 degrees

# Button Mappings
A = "Space"                                # Play / Pause
B = "M"                                    # Mute
X = "Ctrl+Right"                           # Next Track
Y = "Ctrl+Left"                            # Previous Track
L = "Ctrl+Down"                            # Volume Down
R = "Ctrl+Up"                              # Volume Up
L2 = "Ctrl+Shift+B"                        # Toggle Sidebar
R2 = "Ctrl+F"                              # Search
SELECT = "Esc"
START = "exec:playerctl play-pause"        # Direct shell command
STAR = "exec:notify-send 'Spotify'"
HOME = "Super"
UP = "Up"
DOWN = "Down"
LEFT = "Left"
RIGHT = "Right"
```

### 2.1 Canonical Physical Button Names

Always use these exact button names (or their aliases):

| Button | Accepted Aliases | Location on Micro |
| :--- | :--- | :--- |
| `A` | `A` | Right face diamond (East) |
| `B` | `B` | Right face diamond (South) |
| `X` | `X` | Right face diamond (North) |
| `Y` | `Y` | Right face diamond (West) |
| `L` | `L1` | Top-left shoulder bumper |
| `R` | `R1` | Top-right shoulder bumper |
| `L2` | `L2` | Top-left shoulder trigger |
| `R2` | `R2` | Top-right shoulder trigger |
| `SELECT` | `MINUS`, `-` | Top-left pill button |
| `START` | `PLUS`, `+` | Top-right pill button |
| `STAR` | `SHARE` | Bottom-left round star button |
| `HOME` | `HEART`, `LOGO` | Bottom-right round heart button |
| `UP` | `UP` | D-pad North |
| `DOWN` | `DOWN` | D-pad South |
| `LEFT` | `LEFT` | D-pad West |
| `RIGHT` | `RIGHT` | D-pad East |

### 2.2 Action Formats

Agents can assign actions in multiple ways:

1. **Single Key String**:
   `A = "Space"`, `X = "Backspace"`, `L = "PageUp"`
2. **Modifier Chords (`+` syntax)**:
   `Y = "Shift+Enter"`
   `R = "Ctrl+Shift+D"`
   `L2 = "Ctrl+Alt+Delete"`
3. **Array of Keys**:
   `R2 = ["Ctrl", "Shift", "T"]`
4. **Shell Execution**:
   `STAR = "exec:notify-send 'Controller' 'Star button pressed'"`
   `START = { exec = "hyprctl dispatch togglespecialworkspace" }`
5. **No Action**:
   `SELECT = "None"`

### 2.3 Supported Key Names

Friendly names are automatically mapped to Linux evdev codes and USB HID codes:
- **Modifiers**: `Ctrl` (`LeftCtrl`), `Shift` (`LeftShift`), `Alt` (`LeftAlt`), `Super` (`LeftMeta`)
- **Navigation**: `Up`, `Down`, `Left`, `Right`, `PageUp`, `PageDown`, `Home`, `End`
- **Editing**: `Enter`, `Backspace`, `Delete`, `Tab`, `Space`, `Esc`, `Insert`
- **Letters & Numbers**: `A` through `Z`, `0` through `9`, `F1` through `F12`
- **Symbols**: `Minus`, `Equal`, `Comma`, `Dot`, `Slash`, `Backslash`, `Semicolon`, `Apostrophe`

---

## 3. Agent Workflows

### 3.1 Adding a New App Profile

When the user asks to support a new app (e.g., *"Make a profile for Obsidian"*):

1. **Find the window's `class`**:
   Ask the user or inspect Hyprland's current focus:
   ```bash
   8bitdo-micro-focus-switch status
   ```
2. **Draft the profile**:
   Identify the app's top 16 shortcuts and choose a sensible D-pad orientation (`turn = 0` horizontal, or `turn = 90` vertical).
3. **Append to `~/.config/8bitdo-micro/profiles.toml`**:
   Add the new `[profiles.<app_name>]` block with `match_class` and button assignments.
4. **Inspect the visual diagram**:
   Verify the layout and chords visually:
   ```bash
   8bitdo-micro-focus-switch show <app_name> -o /tmp/diagram.svg
   ```
5. **Restart or reload the service**:
   If the user has the systemd service running:
   ```bash
   systemctl --user restart 8bitdo-micro-focus-switch.service
   ```

### 3.2 Testing Changes

Run the test suite to verify no regressions in parser or protocol:
```bash
uv run pytest -v
```

### 3.3 Inspecting Hardware State (BLE)

If troubleshooting connection or verifying hardware mapping:
```bash
# Query active BLE hardware state without disconnecting keyboard:
8bitdo-micro-focus-switch ble read

# Test flashing a specific profile directly to controller:
8bitdo-micro-focus-switch ble write <profile_name>
```

---

## 4. Invariants & Safety Rules

1. **Preserve User Backups**: Never overwrite `~/.config/8bitdo-micro/hardware-backup.bin` unless performing an explicit reset.
2. **Zero Polling**: Do not introduce polling loops into daemon or window watchers. Always use Hyprland IPC events via `.socket2.sock`.
3. **Non-Destructive Config Updates**: When editing `profiles.toml`, keep existing user comments and profiles intact.
