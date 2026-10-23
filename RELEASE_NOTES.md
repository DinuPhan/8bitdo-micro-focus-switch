## 8BitDo Micro Focus Switch v0.3.0 🦀 (Rewrite to Pure Rust)

Major milestone release of **8BitDo Micro Focus Switch**: completely rewritten from Python to **100% pure Rust** as a single self-contained, unified binary!

---

### 🌟 Key Highlights in v0.3.0

#### 1. 100% Pure Rust Architecture (Zero Runtime Dependencies)
* **Single Self-Contained Binary**:
  - The CLI (`daemon`, `status`, `bind`, `show`, `ble`, `listen`, `setup`, etc.) and the desktop GUI are now compiled into one single executable (`8bitdo-micro-focus-switch`).
  - No Python, interpreter, or runtime dependencies required. Instant startup with minimal memory footprint (<20MB).

#### 2. Clean Repository Restructure
* **Root Configuration**:
  - `Cargo.toml` and `tauri.conf.json` directly at root.
* **Modular Rust Source Tree**:
  - `src/core/`: Controller protocol, BLE GATT flasher, config parser, async evdev/uinput daemon, keycodes, window watcher, and visualizer.
  - `src/cli/`: Full Clap v4 CLI suite with subcommands and interactive helpers.
  - `src/gui/`: Tauri v2 IPC handlers, system tray, and in-process daemon control.
  - `frontend/`: Web UI visualizer built with Vite and TypeScript.

#### 3. Daemon & Input Engine Hardening
* **Distinct Virtual Keyboard Identity**:
  - Assigned dedicated virtual `InputId` (`0x1209:0x0001`) to eliminate any potential conflict or feedback loop between the virtual device and physical 8BitDo controller (`0x2dc8:0x9021`).
* **Enhanced Evdev & Exclusive Grab Reliability**:
  - Explicit error inspection and diagnostics on Linux `dev.grab()`.
  - Updated udev rule to `MODE="0666"` for direct kernel `/dev/uinput` access without requiring root/group gymnastics.
* **Comprehensive Logging**:
  - Informative daemon lifecycle logs detailing device discovery, grab status, profile transitions, and raw event translations.

#### 4. Automated Cross-Platform CI / Release Flow
* GitHub Actions automatically builds and publishes:
  - **Linux x86_64**: `.deb`, `.AppImage`, and standalone `8bitdo-micro-focus-switch-linux-x86_64.tar.gz`.
  - **macOS (Apple Silicon & Intel)**: `.dmg` bundles and standalone `macos-aarch64` / `macos-x86_64` tarballs.
  - **Windows x86_64**: `.msi` installers and standalone `.zip` executable packages.

---

## 8BitDo Micro Focus Switch & Visual Configurator v0.1.0 🎉

Initial public release of **8BitDo Micro Focus Switch**, a comprehensive focus-aware input remapping daemon and visual configurator for the **8BitDo Micro** controller (Keyboard Mode).

---

### 🌟 Key Highlights

#### 1. Modern Desktop GUI (Tauri v2)
* **System-Tray Integration**: Minimizes cleanly to tray with daemon status indicators and quick-reload actions.
* **Interactive Visual Controller Mapper**:
  - Live vector layout illustrating all 16 controller buttons and callouts.
  - Dynamically displays key bindings, inline TOML descriptions, and tooltips.
  - Supports D-pad rotation for one-handed operation (0°, 90°, 180°, 270°).
  - Floating Zoom Controls (60% – 250% zoom, Fit to screen, and `Ctrl + Wheel` support) covering 85–90% of the screen.
* **Interactive Key Capture Modal**:
  - Live keyboard recorder for single buttons and multi-button combos/chords.
  - Quick modifier chips (Ctrl, Shift, Alt, Super) and function keys.
* **Direct BLE Hardware Flashing**:
  - Read/write hardware profiles over BLE GATT without needing third-party mobile apps.

#### 2. Focus-Aware Remapping Engine & Daemon
* **Low-Latency Window Switching**:
  - Seamless profile switching on active window changes (Hyprland IPC & X11).
* **Robust Combo Buffering & Debouncing**:
  - Zero input lag on standard typing keys.
  - Kernel repeat handling and timer cancellation protection for fast typing.
* **Cross-Platform Architecture**:
  - Modular input backend design supporting Linux (`uinput`/`evdev`), Windows (Low-level hooks), and macOS (Event Tap & Sentinel keymap).

---

### 📦 Assets & Installation

* **Unified Binary & GUI (Linux x86_64, Windows, macOS)**:
  Download the release bundle for your platform (`.deb`, `.AppImage`, `.msi`, `.dmg`, or standalone binary `8bitdo-micro-focus-switch`).
  - Running without arguments or `--gui` opens the desktop GUI with system tray.
  - Running with subcommands (`status`, `bind`, `show`, `ble`) runs in terminal mode.
