# 8bitdo-micro-focus-switch

Turn your **8BitDo Micro** into the ultimate one-handed shortcut remote on Linux. Automatically switches button mappings based on which application is currently in focus — no phone required.

---

## Why this exists

The **8BitDo Micro** is a featherlight ($25, 24-gram) controller with 16 buttons. In Keyboard (`K`) mode, it is hands-down the best one-handed macro pad for:
- Terminal coding & AI prompt workflows
- Relaxing back while watching YouTube or movies
- Reviewing Anki flashcards
- Smooth article and web browsing

**The catch with the official setup:** The official 8BitDo mobile app only lets you store one custom profile on the controller at a time. If you want to switch from Anki to YouTube, you normally have to unpair, open an Android app, re-flash the profile, and pair back to your computer.

**With `8bitdo-micro-focus-switch`:**
- **Zero-touch automatic switching**: Your controller rebinds itself as you switch windows on your desktop.
- **Visual Keymap HUD**: Forget which button does what? One command or hotkey pops up a visual diagram showing all 16 buttons with glowing pointers and label badges.
- **No phone required**: Reads and flashes hardware profiles directly from your Linux machine over Bluetooth.
- **Instant response**: Sub-millisecond virtual keyboard remapping with multi-key chords and shell command support.

---

## Visual Keymap Diagram & HUD

Pressing buttons shouldn't feel like a memory test. You can display your active keymap at any time:

```bash
# Auto-detects your active window and pops up its controller keymap:
8bitdo-micro-focus-switch show

# View a specific profile diagram (e.g. YouTube, AGY, Anki):
8bitdo-micro-focus-switch show youtube
8bitdo-micro-focus-switch show agy

# Open an interactive viewer in your browser with tabs for every profile:
8bitdo-micro-focus-switch show --interactive

# Save as a self-contained SVG or PNG:
8bitdo-micro-focus-switch show youtube -o youtube_keymap.svg
8bitdo-micro-focus-switch show agy --format png -o agy_keymap.png
```

### In-Game Style HUD on Hyprland

Add this to your `~/.config/hypr/hyprland.conf` to pop up a floating keymap HUD whenever you press `Super+M`:

```hyprlang
# Floating overlay window for the keymap diagram
windowrulev2 = float, class:^(imv)$, title:^(8bitdo-micro.*)
bind = $mainMod, M, exec, 8bitdo-micro-focus-switch show --no-wait
```

---

## Built-in Profiles

Out of the box, `8bitdo-micro-focus-switch` comes configured for popular apps:

| Profile | Trigger Window | What the buttons do |
| :--- | :--- | :--- |
| **`agy`** *(default)* | Terminal / Ghostty | `Shift+Enter` (new line), `Enter`, `@` (files), `/` (commands), `Backspace`, `PageUp/Down` |
| **`youtube`** | Browser watching YouTube | `Space` (play/pause), `M` (mute), `F` (fullscreen), `C` (captions), `J`/`L` (±10s seek), `Shift+,`/`Shift+.` (speed), `Shift+N` (next video) |
| **`anki`** | Anki Flashcards | `1` (Again), `2` (Good), `4` (Easy), `Space` (Show answer), `Ctrl+Shift+D`, `Ctrl+Alt+N`, `Ctrl+Z` |
| **`browser`** | Firefox, Chromium, Zen, Brave | `PageUp/Down`, `Space`, `Ctrl+T` (new tab), `Ctrl+W` (close), `Ctrl+Shift+T` (reopen), Tab switching |
| **`media`** | VLC, MPV | `Space` (play/pause), `F` (fullscreen), `M` (mute), `[` / `]` (speed), `9` / `0` (volume) |
| **`factory`** | Any | Clean single scancodes (`c` through `t`) on all 16 buttons |

---

## Quick Start

### 1. Installation

Requires Python 3.11+. Install using [`uv`](https://github.com/astral-sh/uv) or `pip`:

```bash
git clone https://github.com/DinuPhan/8bitdo-micro-focus-switch.git
cd 8bitdo-micro-focus-switch
uv pip install -e .
```

### 2. Pair Controller

1. Switch the bottom slider on the 8BitDo Micro to **`K`** (Keyboard mode).
2. Hold the small **Pair** button on the bottom until the LED blinks rapidly.
3. Pair with your Linux machine in your Bluetooth settings. It will connect as an input keyboard.

### 3. Quick Status Check

```bash
# Verify your controller is connected and see the active profile:
8bitdo-micro-focus-switch status

# Or preview your keymap layout visually:
8bitdo-micro-focus-switch show
```

---

## Daily Commands

These are the primary commands you will use daily:

```bash
# 1. Preview your key mappings as an interactive visual HUD:
8bitdo-micro-focus-switch show

# 2. Interactively bind a button by pressing your keyboard (no typing required!):
8bitdo-micro-focus-switch bind
8bitdo-micro-focus-switch bind agy A
8bitdo-micro-focus-switch bind agy "L+R"

# 3. Browse or search bindable keys catalog (like the 8BitDo app):
8bitdo-micro-focus-switch keys
8bitdo-micro-focus-switch keys volume

# 4. Listen and inspect keyboard chords in real-time:
8bitdo-micro-focus-switch record

# 5. Edit or customize an app profile in your editor:
8bitdo-micro-focus-switch config agy
8bitdo-micro-focus-switch config spotify

# 6. Reload profiles into the running daemon (zero downtime, no disconnect):
8bitdo-micro-focus-switch reload

# 7. Check Hyprland focus status and connected controllers:
8bitdo-micro-focus-switch status

# 8. Scan for nearby controllers:
8bitdo-micro-focus-switch discover
```

---

## Interactive Key Binder (`bind`), Recorder (`record`), & Catalog (`keys`)

Never struggle to remember exact key names, scancodes, or modifier spellings:

### 1. Interactive Key Binder (`bind`)
Simply press the shortcut on your physical keyboard:

```bash
# Interactive wizard: detects active window, asks for button, and waits for keypress:
8bitdo-micro-focus-switch bind

# Bind a specific button or combo directly:
8bitdo-micro-focus-switch bind agy A
8bitdo-micro-focus-switch bind agy "L+R"
```
1. It waits: `>> Listening for keyboard input for [agy.A]... Press any key or combination (ESC to cancel):`
2. Press `Ctrl+Shift+P` (or `Space`, `Super+K`, `PageUp`) on your keyboard.
3. It detects the chord, saves it to `~/.config/8bitdo-micro/profiles.toml`, and sends a zero-downtime `SIGHUP` reload signal to the running daemon.
4. **Discard with `ESC`**: Pressing `ESC` at any time immediately cancels the wait without saving any changes.

### 2. Live Chord Recorder (`record`)
Press keys on your keyboard to inspect their exact chord names and TOML representations:
```bash
8bitdo-micro-focus-switch record
```

### 3. Bindable Keys Catalog (`keys`)
Just like the 8BitDo mobile app, browse all supported keys categorized into Modifiers, Navigation, Actions, Media & Volume, Function Keys, and Symbols:
```bash
# View complete categorized catalog:
8bitdo-micro-focus-switch keys

# Search for specific keys:
8bitdo-micro-focus-switch keys media
8bitdo-micro-focus-switch keys volume
```

---

## Customizing Mappings in Editor (`config`)

All profiles live in `~/.config/8bitdo-micro/profiles.toml`. You can manage and edit your profiles effortlessly using the `config` command:

```bash
# 1. Open the configuration file in your editor ($EDITOR or Neovim):
8bitdo-micro-focus-switch config

# 2. Jump directly to an existing profile to tweak its buttons:
8bitdo-micro-focus-switch config youtube
8bitdo-micro-focus-switch config agy

# 3. Add a new app profile (e.g. Spotify, Discord, VS Code):
8bitdo-micro-focus-switch config spotify
# -> If the profile doesn't exist yet, it asks to create it, generates a clean
#    starter template with all 16 buttons, merges it into profiles.toml,
#    and opens your editor directly at the new section!

# 4. List all configured profiles with line numbers:
8bitdo-micro-focus-switch config --list

# 5. Reload the running daemon immediately (zero downtime):
8bitdo-micro-focus-switch reload
```

> **Automatic In-Process Reload**: When you edit profiles via `8bitdo-micro-focus-switch config` or bind keys via `8bitdo-micro-focus-switch bind`, saving automatically validates syntax and sends a `SIGHUP` reload signal to the running daemon. Your controller never disconnects, and virtual keyboard grabs remain active.

### Profile Example

To add or customize an app, a profile looks like this:

```toml
[profiles.spotify]
description = "Spotify Music Controller"
match_class = "spotify"           # Regex matched against window class (from hyprctl)
turn = 0                          # 0 = normal horizontal, 90 = held vertically

# Button mappings (use friendly names or chords with "+"):
A = "Space"                       # Play / Pause
B = "M"                           # Mute
X = "Ctrl+Right"                  # Next Track
Y = "Ctrl+Left"                   # Previous Track
L = "Ctrl+Down"                   # Volume Down
R = "Ctrl+Up"                     # Volume Up
SELECT = "Esc"
START = "exec:playerctl play-pause" # You can even run shell commands!
STAR = "None"
HOME = "Super"
UP = "Up"
DOWN = "Down"
LEFT = "Left"
RIGHT = "Right"
```

> **Inline Descriptions & Tooltips**: Single-line comments (e.g. `# Play / Pause`) written inline with button or combo mappings are automatically parsed and displayed as visual subtitles and interactive hover tooltips in `show` keymap diagrams!

### Button Names Reference

The 16 physical buttons are named:
- **Face buttons**: `A`, `B`, `X`, `Y`
- **Shoulders**: `L` (L1), `R` (R1), `L2`, `R2`
- **Center**: `SELECT` (`-`), `START` (`+`)
- **Bottom**: `STAR` (★), `HOME` (♥)
- **D-pad**: `UP`, `DOWN`, `LEFT`, `RIGHT`

### Holding the Controller Vertically (`turn = 90`)

If you like holding the controller vertically in one hand (e.g. for reading or flashcards), set `turn = 90` in your profile. The D-pad automatically rotates so physical "Up" continues pointing in the direction you expect!

### Multi-Button Combos & Chording (e.g. L1 + R1 -> Super + K)

You can map simultaneous button presses (chords) into hotkeys or shell commands! When defined, pressing both buttons within ~40ms suppresses the individual button actions and fires your combo hotkey instead:

```toml
[profiles.agy.combos]
"L+R" = ["Super", "K"]               # L1 + R1 -> Super + K (Command palette / app launcher)
"Select+Start" = "exec:notify-send 'Micro' 'Hello!'"
"L2+R2" = ["Ctrl", "Shift", "P"]
```

- **Simultaneous & Hold-and-Press**: Supports both simultaneous chording (~50ms) and holding a button as a modifier (e.g. holding `L2` while tapping `LEFT`).
- **Zero-Latency Solo Buttons**: Buttons that are not part of any combo fire immediately with `0ms` delay.
- **Fast Tap Guarantee**: Tapping a combo button quickly will still register its solo key instantly upon release.
- **Visual Diagram Integration**: Configured combos appear as dedicated glowing badges at the bottom of the visual keymap!

> **Fun Fact**: Supporting 2-button combos expands 16 physical buttons into **136 unique actions per profile** ($\binom{16}{1} + \binom{16}{2} = 16 + 120$) — instantly outnumbering a standard 104-key desktop keyboard on a 24-gram keychain controller!

### Global Base Combos (`[profiles.global]` or `[profiles.all]`)

Want a combo to work everywhere across all apps without copy-pasting it into every profile? Define it under `[profiles.global.combos]` (or `[profiles.all.combos]`):

```toml
[profiles.global.combos]
"L+R" = ["Super", "K"]               # L1 + R1 -> Super + K (Works in every app!)
"Select+Start" = "exec:hyprctl dispatch togglespecialworkspace"
```

- **Inherited by Default**: All profiles (`agy`, `anki`, `youtube`, `browser`, etc.) automatically inherit these combos.
- **Profile Overrides Take Precedence**: If an individual profile defines its own combo (e.g., `[profiles.youtube.combos] "L+R" = "F"`), that specific profile's combo takes precedence over the global default.
- **Quick CLI Binding**: You can bind global combos directly:
  ```bash
  8bitdo-micro-focus-switch bind global "L+R"
  ```

> **Hardware vs. Software Mode**: Multi-button combos run seamlessly in software daemon mode (`uinput`). The 8BitDo Micro microcontroller's onboard memory only holds 18 single-button slots, so BLE GATT hardware reflash only stores single button mappings.

---

## Direct Hardware Flasher (No Phone Needed)

If you prefer to flash a profile directly into the controller's onboard memory (so it stays even if you take the controller to another computer):

```bash
# Flash any profile directly over Bluetooth:
8bitdo-micro-focus-switch ble write youtube
8bitdo-micro-focus-switch ble write agy
8bitdo-micro-focus-switch ble write anki --turn 90

# Reset controller back to factory keys:
8bitdo-micro-focus-switch ble reset
```

---

## Situational & Setup Commands

These commands are only needed for one-time environment setup or advanced usage:

### 1. `install-udev` (One-time setup for Combos / uinput mode)

```bash
8bitdo-micro-focus-switch install-udev
```

- **When is this needed?**
  - **REQUIRED** if you use multi-button combos/chords (e.g., `L1 + R1 -> Super + K`).
  - **REQUIRED** if you use shell execution commands (`exec: ...`) or instant `<1ms` software focus remapping.
- **When is this NOT needed?**
  - If you only use direct BLE hardware flashing (`8bitdo-micro-focus-switch ble write <profile>`) to burn profiles to the controller's onboard memory, you do **not** need `install-udev`.

### 2. `install-service` (Auto-start Daemon on Login)

```bash
# Generate the systemd user unit to start the remapper automatically:
8bitdo-micro-focus-switch install-service
systemctl --user daemon-reload
systemctl --user enable --now 8bitdo-micro-focus-switch.service
```

- **When is this needed?**
  - **Optional / Situational**: Use this if you want the focus-aware remapper daemon to run seamlessly in the background as a system service.
  - If you prefer running `8bitdo-micro-focus-switch daemon` manually in a terminal, you do not need this service.

---

## More Resources

- [**ARCHITECTURE.md**](ARCHITECTURE.md): Deep dive into the reverse-engineered 180-byte BLE GATT protocol, packet structures, CRC-16 algorithm, and latency models.
- [**AGENTS.md**](AGENTS.md): Guidelines and tips for AI coding agents to configure and generate new profiles automatically.

---

## License

[MIT](LICENSE) © Nguyen Dinh Phan
