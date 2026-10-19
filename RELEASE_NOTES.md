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

* **Standalone GUI App (Linux x86_64)**:
  Download and extract `eightbitdo-micro-gui-linux-x86_64.tar.gz`, then run `./eightbitdo-micro-gui`.
* **Python CLI Daemon**:
  Install via `pip` or `pipx`:
  ```bash
  pip install 8bitdo_micro_focus_switch-0.1.0-py3-none-any.whl
  ```
