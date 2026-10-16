# 8bitdo-micro-focus-switch

Focus-aware input remapper daemon (`Hyprland` + `evdev`/`uinput`) and direct BLE GATT (`"80EL"`) hardware profile reader/flasher for the **8BitDo Micro** controller in Keyboard (`K`) mode.

## Features

1. **Direct BLE GATT Hardware Reader & Flasher (`8bitdo-micro-focus-switch ble`)**:
   - Reverse-engineered from `8BitDo-Ultimate-Software-for-Android.apk` (`libadvance-lib.so` `record_micro` @ `0x1694ac`, `Generate_CustomDataCmd` @ `0x12be58`, `crc16` @ `0x12c678`).
   - Connects directly to the controller's simultaneous BLE GATT configuration service (`"80EL"`, service `0000FF10-...`, characteristic `0000FF13-...`) **while the controller remains paired and connected to Linux as a Classic HID Keyboard**.
   - Reads firmware version and the 180-byte `record_micro` mapping table, or flashes/saves any profile (`supermemo`, `anki`, `factory`, or custom profiles from `~/.config/8bitdo-micro/profiles.toml`) without needing an Android phone.

2. **Instant `<1ms` Focus-Aware Remapper Daemon (`8bitdo-micro-focus-switch daemon`)**:
   - Listens to Hyprland's `.socket2.sock` (`activewindow>>`) with zero polling.
   - Grabs `/dev/input/event*` (`Vendor=0x2dc8, Product=0x9021`) via `EVIOCGRAB` and emits per-application mappings, up to 4-key chords, shell commands (`exec`), and D-pad rotation (`0°`, `90°`, `180°`, `270°`) through a virtual `uinput` keyboard (`--backend uinput`), or automatically reflashes the hardware over BLE GATT on focus change (`--backend ble`).

## Profiles & Presets

`8bitdo-micro-focus-switch` comes pre-configured with rules in `~/.config/8bitdo-micro/profiles.toml`:

| Profile | Target / Focus | Key Mappings & Actions |
| :--- | :--- | :--- |
| **`agy`** *(default)* | Ghostty / Terminal (`agy`) | `Shift+Enter` (newline), `Enter`, `@` (mention files), `/` (slash commands), `Backspace`, `PageUp/Down` |
| **`youtube`** | Browser watching YouTube | `Space` (play/pause), `M` (mute), `F` (fullscreen), `C` (captions), `J`/`L` (±10s seek), `Shift+,`/`Shift+.` (speed), `Shift+N` (next) |
| **`anki`** | Anki Flashcards | `1` (Again), `2` (Good), `4` (Easy), `Space` (Show answer), `Ctrl+Shift+D`, `Ctrl+Alt+N`, `Ctrl+Del`, `Ctrl+Z` |
| **`supermemo`** | SuperMemo / Reading | Matches official 8BitDo SuperMemo hardware mapping |
| **`browser`** | Web Browsing (Firefox/Brave/Zen) | `PageUp/Down`, `Space`, `Ctrl+T` (new tab), `Ctrl+W` (close), `Ctrl+Shift+T` (reopen), `F5`, Tab switching |
| **`media`** | VLC, MPV | `Space` (play/pause), `F` (fullscreen), `M` (mute), `[` / `]` (speed), `9` / `0` (volume) |
| **`factory`** | Factory K-mode | Restores clean single scancodes (`c` through `t`) on all 16 buttons |

## Quick Usage

```bash
# Inspect Hyprland focus, /dev/input status, and live BLE GATT hardware profile:
8bitdo-micro-focus-switch status --ble

# Read and back up the controller's current 180-byte hardware profile:
8bitdo-micro-focus-switch ble read --save-bin ~/.config/8bitdo-micro/backup.bin

# Flash any profile directly to the controller hardware over BLE (no phone needed):
8bitdo-micro-focus-switch ble write agy
8bitdo-micro-focus-switch ble write youtube
8bitdo-micro-focus-switch ble write anki
8bitdo-micro-focus-switch ble write supermemo --turn 90

# Reset controller hardware to factory K-mode keys (c..t) for <1ms uinput daemon mode:
8bitdo-micro-focus-switch ble reset

# Run the focus-aware daemon:
8bitdo-micro-focus-switch daemon --backend uinput --ensure-factory-ble
# Or run in BLE GATT hardware-reflash mode (zero root / uinput permissions required):
8bitdo-micro-focus-switch daemon --backend ble
```

## Running as a Background Service (`systemd --user`)

```bash
# Generate and register user systemd unit:
8bitdo-micro-focus-switch install-service --backend ble
systemctl --user daemon-reload
systemctl --user enable --now 8bitdo-micro-focus-switch.service

# View live focus-switch logs:
journalctl --user -u 8bitdo-micro-focus-switch.service -f
```

## Optional: `<1ms` Virtual Keyboard Mode (`--backend uinput`)

By default, `--backend ble` works out of the box with zero permissions. To enable ultra-low latency (`<1ms`) virtual keyboard remapping via `evdev` + `uinput`:

```bash
echo 'KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/99-8bitdo-micro-uinput.rules
sudo udevadm control --reload-rules && sudo udevadm trigger /dev/uinput
```

## License

[MIT](LICENSE) © Nguyen Dinh Phan
