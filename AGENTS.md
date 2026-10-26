# Agent Instructions for 8BitDo Micro Focus Switch

This guide instructs AI agents (Antigravity, Claude, Cursor, Copilot, etc.) on how to interview users, design intuitive profiles, preserve cross-application muscle memory, and manage configurations for the 8BitDo Micro controller.

---

## 1. Project Anatomy

The codebase is written in pure Rust (with a Tauri v2 desktop GUI and web frontend):

- **`~/.config/8bitdo-micro/profiles.toml`**: The user's active configuration file. Edit this file to add or adjust app mappings.
- **`src/core/config.rs`**: Configuration loader, profile matcher, TOML parser/serializer, and default template.
- **`src/core/protocol.rs`**: Controller button constants, evdev keycode tables, D-pad rotation math, and BLE GATT payload encoders.
- **`src/core/daemon.rs`**: Focus-aware background remapper engine (Hyprland IPC + evdev grab + `/dev/uinput` virtual keyboard).
- **`src/core/window_watcher.rs`**: Real-time Hyprland `.socket2.sock` event listener and window query client.
- **`src/core/recorder.rs`**: Interactive physical key capture engine and TOML binding editor (`bind`).
- **`src/core/visualizer.rs`**: Visual keymap generator (interactive HTML visualizer and vector SVG diagrams).
- **`src/cli/mod.rs`**: Terminal command parser and CLI entry point.
- **`src/gui/mod.rs`**: Tauri desktop configurator window, system tray, and IPC backend.
- **`frontend/`**: Vite + TypeScript visual configurator webview.

---

## 2. Profile Structure (`profiles.toml`)

Profiles live in `~/.config/8bitdo-micro/profiles.toml`. Each profile represents an application context:

```toml
[profiles.spotify]
description = "Spotify Music Controller"
match_class = "spotify"                    # Case-insensitive regex tested against window class
match_title = ".*"                         # Optional case-insensitive regex for window title
turn = 0                                   # D-pad orientation: 0 (horizontal), 90 (vertical wand), 180, or 270

[profiles.spotify.buttons]
A = "Space"                                # Play / Pause
B = "M"                                    # Mute
X = "Ctrl+Right"                           # Next Track
Y = "Ctrl+Left"                            # Previous Track
L = "Ctrl+Down"                            # Volume Down
R = "Ctrl+Up"                              # Volume Up
L2 = "None"                                # Reserved for Omarchy Global Layer
R2 = "Ctrl+F"                              # Search in playlist
SELECT = "Esc"                             # Cancel / Unfocus
START = "F9"                               # Whisper voice dictation
STAR = "exec:notify-send 'Spotify'"        # Direct shell command
HOME = "Super"
UP = "Up"
DOWN = "Down"
LEFT = "Left"
RIGHT = "Right"

[profiles.spotify.combos]
"L+R" = ["Super", "K"]                     # Command palette
```

### 2.1 Canonical Physical Button Names

Always use these exact button names (or their aliases):

| Button | Accepted Aliases | Physical Location on Controller |
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
1. **Single Key String**: `A = "Space"`, `X = "Backspace"`, `L = "PageUp"`
2. **Modifier Chords (`+` syntax)`**: `Y = "Shift+Enter"`, `R = "Ctrl+Shift+D"`, `L2 = "Ctrl+Alt+Delete"`
3. **Array of Keys**: `R2 = ["Ctrl", "Shift", "T"]`
4. **Shell Execution**: `STAR = "exec:notify-send 'Controller' 'Star button pressed'"`
5. **No Action**: `L2 = "None"` (unbound, lets the button serve purely as a combo modifier)

### 2.3 Global Base Template (`[profiles.global]`)

Combos and buttons defined under `[profiles.global]` apply to ALL profiles by default unless specifically overridden by an app profile:

```toml
[profiles.global.combos]
"L+R" = ["Super", "K"]               # App launcher / command palette across all apps
"Select+Start" = "exec:hyprctl dispatch togglespecialworkspace" # Scratchpad
"L2+UP" = ["LEFTMETA", "F"]          # Fullscreen window
"L2+DOWN" = ["LEFTMETA", "T"]        # Toggle floating
"L2+LEFT" = ["LEFTMETA", "LEFT"]     # Focus window left
"L2+RIGHT" = ["LEFTMETA", "RIGHT"]   # Focus window right
"L2+START" = "F9"                    # Universal Whisper voice dictation
```

---

## 3. Cognitive Invariance: The Cross-App Mapping Philosophy

The 8BitDo Micro is a compact 16-button controller. When a user switches between watching YouTube, triaging emails in a web browser, and coding with an AI agent in the terminal, **they must not have to rewire their motor cognition or second-guess what a button does**.

### 3.1 The Golden Rule of Motor Cognition

> **A physical button must represent the same mental intent across all profiles.**
> If button `A` represents "Confirm / Select / Advance" in one app, it must NEVER suddenly become "Delete / Cancel" in another. If `SELECT` is "Escape / Dismiss", it must never trigger an irreversible destructive action.

### 3.2 The Universal Button Functional Anchors

Every profile designed by an agent should adhere to this functional zone architecture:

| Button Zone | Anchor Role | Cognitive Intent | Examples Across Apps |
| :--- | :--- | :--- | :--- |
| **`A`** (East) | **Primary Actuator / Advance** | Affirmative action, execute, play, advance, enter. | **Media**: Play / Pause<br>**Browser**: Open link / Submit form<br>**Email**: Open conversation / Select<br>**Code/Agent**: Accept / Next / Primary action |
| **`B`** (South) | **Secondary / Downward Motion** | Page down, downward reading, secondary action, mute. | **Media**: Mute audio / Toggle fullscreen<br>**Browser**: Page Down<br>**Email**: Archive / Next message<br>**Code/Agent**: Page Down / Reject or Next |
| **`X`** (North) | **Tertiary / Upward Motion** | Page up, upward reading, expand view, inspect. | **Media**: Fullscreen toggle (`F`)<br>**Browser**: Page Up<br>**Email**: Star / Flag message<br>**Code/Agent**: Page Up / Diff view |
| **`Y`** (West) | **Alternate / Input / Sub-Action** | Modality switch, text input focus, subtitles, reply. | **Media**: Toggle subtitles (`C`)<br>**Browser**: Focus address bar (`Ctrl+L`)<br>**Email**: Reply (`R`)<br>**Code/Agent**: Multiline newline (`Shift+Enter`) |
| **`SELECT`** | **Universal Escape / Dismiss** | Stop, unfocus, dismiss modal, leave fullscreen. | **All Apps**: `Esc` or `Ctrl+W` (close tab/modal). **Never assign destructive mutations here.** |
| **`START`** | **Universal Voice / Assist** | System speech dictation or agent prompt trigger. | **All Apps**: `F9` (Whisper Voice) or `Ctrl+K`. |
| **`L` / `R`** | **Step / Scrub / Cycle** | Previous / Next item, tab, or relative scrub. | **Media**: Seek -10s (`J`) / Seek +10s (`L`)<br>**Browser**: Prev link (`Shift+Tab`) / Next link (`Tab`)<br>**Email**: Prev thread (`K`) / Next thread (`J`)<br>**Code/Agent**: Prev file/diff / Next file/diff |
| **`L2`** | **Omarchy Global Layer Modifier** | Hold down to access window management and app launching chords (`L2+A`, `L2+B`, `L2+UP`...). | **All Apps**: Keep `L2 = "None"` or modifier so global chords trigger cleanly without phantom keypresses. |
| **`R2`** | **App-Specific Turbo Modifier / Commit** | Submit prompt, speed up playback, or app chord shift. | **Media**: Speed up playback (`>`)<br>**Browser**: Secondary chord shift (`R2+Tab`, etc.)<br>**Code/Agent**: Submit prompt (`Enter`) |
| **`D-Pad`** | **Continuous Spatial Navigation** | Scroll viewport, adjust level, or fine cursor stepping. | `UP`/`DOWN`: Vertical scroll or Volume up/down.<br>`LEFT`/`RIGHT`: Horizontal pan or 5s fine seek. |
| **`STAR`** | **Contextual Utility / Search** | Focus search bar, cycle secondary feature, or scratch action. | **Media**: Focus search (`/`)<br>**Browser**: Reopen closed tab (`Ctrl+Shift+T`)<br>**Email**: Search mail (`/`)<br>**Code/Agent**: Space / Quick utility |
| **`HOME`** | **Back / Home Navigation** | Navigate back, return to index, or meta overview. | **Media/Browser**: History Back (`Alt+Left`)<br>**Email**: Back to Inbox (`U` / `Esc`)<br>**Code/Agent**: Command prefix (`/`) or `Super` |
| **`L+R`** | **Universal Command Palette** | Bring up search, launcher, or quick switcher. | **All Apps**: `Super+K`, `Ctrl+P`, or `Ctrl+Shift+P`. |

### 3.3 Semantic Mapping Matrix

When creating a new profile, match the application's domain to this reference matrix:

| Functional Intent | Media (YouTube / mpv) | Web / Research (Chrome/Firefox/Zen) | Email (Gmail / Thunderbird) | Coding & Agents (agy / terminal) | Reading / Notes (Obsidian / PDF) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Primary Actuate** | `A`: Space (Play/Pause) | `A`: Enter (Follow link) | `A`: Enter (Open thread) | `A`: Backspace / Approve | `A`: Space / Next card |
| **Downward Step** | `B`: M (Mute audio) | `B`: PageDown | `B`: `E` (Archive thread) | `B`: PageDown | `B`: PageDown |
| **Upward Step** | `X`: `F` (Fullscreen) | `X`: PageUp | `X`: `S` (Star / Flag) | `X`: PageUp | `X`: PageUp |
| **Alt Modality** | `Y`: `C` (Subtitles) | `Y`: Ctrl+L (Address bar) | `Y`: `R` (Reply) | `Y`: Shift+Enter (Newline) | `Y`: Toggle Edit/Preview |
| **Step Backward** | `L`: Seek -10s (`J`) | `L`: Shift+Tab (Prev link) | `L`: Prev thread (`K`) | `L`: Tab (Indent/Prev) | `L`: Prev page (`[`) |
| **Step Forward** | `R`: Seek +10s (`L`) | `R`: Tab (Next link) | `R`: Next thread (`J`) | `R`: Debug / Voice | `R`: Next page (`]`) |
| **Cancel / Dismiss** | `SELECT`: Esc | `SELECT`: Ctrl+W (Close tab) | `SELECT`: Esc / Back | `SELECT`: Esc (Cancel prompt) | `SELECT`: Esc |
| **Universal Voice** | `START`: F9 (Whisper) | `START`: Ctrl+T (New tab) | `START`: `C` (Compose) | `START`: Shift+2 (`@` mention) | `START`: F9 (Voice note) |
| **Quick Search** | `STAR`: `/` (Search) | `STAR`: Ctrl+Shift+T (Reopen) | `STAR`: `/` (Search mail) | `STAR`: Space | `STAR`: Quick switch |
| **Return / Root** | `HOME`: Alt+Left (Back) | `HOME`: Alt+Left (Back) | `HOME`: `U` (Return to list) | `HOME`: `/` (Command prefix) | `HOME`: Home / Outline |
| **Commit / Fire** | `R2`: Playback speed up | `R2`: Secondary chord shift | `R2`: Ctrl+Enter (Send mail) | `R2`: Enter (Submit prompt) | `R2`: Toggle highlight |

### 3.4 Semantic Fallback Principle

If a target application lacks a direct equivalent for a given function (for example, YouTube has no "Delete", or a PDF reader has no "Reply"):

1. **Borrow the Nearest Semantic Analog**:
   - If no "Delete": map to "Dismiss", "Skip", "Close banner", or "Mute".
   - If no "Send": map to "Confirm", "Save", or "Submit".
   - If no "Seek": map to "Next/Previous Item", "Next Chapter", or "Zoom in/out".
2. **Never Invert Polarity**:
   - Never place a destructive command (like close window or purge) onto `A` or `START`.
   - Never place a confirm/commit command onto `SELECT`.
3. **Leave as Harmless / None if Unknown**:
   - If a button has no clear purpose in that app, map it to `"None"` rather than guessing a key that might cause unintended side effects during typing or navigation.

---

## 4. Agent Workflows

### 4.1 Onboarding & Creating Profiles for Users

When a user asks: *"Help me set up profiles for my daily workflow"* or *"Create a profile for app X"*:

#### Step 1: Discover Daily App Stack & Grip Style
Prompt the user to identify their routine:
> "To make your controller feel completely natural across your workflow, tell me:
> 1. What applications do you switch between most often? (e.g., YouTube/media, Gmail/Thunderbird, Ghostty/terminal, Obsidian/notes, browser reading).
> 2. How do you hold the controller? (Horizontal with both hands [`turn = 0`], or vertical one-handed wand like a remote [`turn = 90`])?
> 3. What repetitive micro-actions do you do most in each app (e.g., approving agent edits, archiving emails, skipping 10s, switching tabs)?"

#### Step 2: Detect Exact Window Class and Title
Ask the user to open or focus the target window, then run:
```bash
8bitdo-micro-focus-switch status
```
This inspects Hyprland's active window and returns the exact `class` and `title`. Formulate resilient regexes:
```toml
# Use case-insensitive regex covering both web app and native window variants:
match_class = "^(thunderbird|google-chrome|firefox)$"
match_title = ".*(Mail|Inbox).*"
```

#### Step 3: Apply the Cognitive Invariance Anchors
Synthesize the profile using the Semantic Matrix in Section 3. Ensure:
- `SELECT` is `Esc` / dismiss.
- `A` is the primary actuator.
- `L`/`R` handles relative stepping.
- `L2` preserves the global Omarchy system chords.
- `D-Pad` preserves intuitive directional navigation.

#### Step 4: Write and Register the Profile
Add the new profile block to `~/.config/8bitdo-micro/profiles.toml`. You can do this either by:
1. Direct file edit to `~/.config/8bitdo-micro/profiles.toml`.
2. Using the CLI scaffold:
   ```bash
   8bitdo-micro-focus-switch config <profile_name> --yes --no-edit
   ```
3. Binding individual keys via the CLI:
   ```bash
   8bitdo-micro-focus-switch bind <profile_name> A --key "Space" --description "Play / Pause"
   8bitdo-micro-focus-switch bind <profile_name> "L+R" --key "Super+K" --description "Command Palette"
   ```

#### Step 5: Reload Daemon and Verify Visually
Reload the running daemon (zero downtime, seamless Hyprland focus switch):
```bash
8bitdo-micro-focus-switch reload
```
Generate an SVG or open the visual layout to verify the mapping:
```bash
8bitdo-micro-focus-switch show <profile_name> -o /tmp/keymap.svg
```

---

## 5. Development & Testing Commands

When modifying or verifying the controller codebase:

- **Run all Rust unit and golden tests**:
  ```bash
  cargo test
  ```
- **Check code quality and compiler warnings**:
  ```bash
  cargo check
  ```
- **Inspect active controller status & Hyprland window**:
  ```bash
  8bitdo-micro-focus-switch status
  ```
- **Check available key names**:
  ```bash
  8bitdo-micro-focus-switch keys [query]
  ```
- **Test BLE communication**:
  ```bash
  8bitdo-micro-focus-switch ble read
  ```

---

## 6. Invariants & Safety Rules

1. **Preserve User Backups**: Never overwrite `~/.config/8bitdo-micro/hardware-backup.bin` unless performing an explicit reset.
2. **Zero Polling**: Do not introduce polling loops into the daemon or window watchers. Always use Hyprland IPC events via `.socket2.sock`.
3. **Non-Destructive Config Updates**: When editing `profiles.toml`, keep existing user comments and profiles intact.
4. **POSIX Permission Enforcement**: Always ensure `~/.config/8bitdo-micro/` directory is `0700` and config files are `0600`. Never write keypress streams to world-readable paths.
5. **Fail-Closed Modifier Separation**: Never map a standard single-press action to `L2` if `L2` is intended as a chord modifier; unmapped modifiers must be `"None"` to prevent phantom keystrokes.
