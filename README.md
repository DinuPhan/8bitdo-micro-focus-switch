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

## Quick Usage

```bash
# Inspect Hyprland focus, /dev/input status, and live BLE GATT hardware profile:
8bitdo-micro-focus-switch status --ble

# Read and back up the controller's current 180-byte hardware profile:
8bitdo-micro-focus-switch ble read --save-bin ~/.config/8bitdo-micro/backup.bin

# Flash a profile ('supermemo', 'anki', 'factory', or any profile in profiles.toml) to hardware:
8bitdo-micro-focus-switch ble write anki
8bitdo-micro-focus-switch ble write supermemo --turn 90

# Reset controller hardware to factory K-mode keys (c..t) for <1ms uinput daemon mode:
8bitdo-micro-focus-switch ble reset

# Run the focus-aware daemon:
8bitdo-micro-focus-switch daemon --backend uinput --ensure-factory-ble
# Or run in BLE GATT hardware-reflash mode (no /dev/uinput permissions required):
8bitdo-micro-focus-switch daemon --backend ble
```

## One-Time `/dev/uinput` Permission Setup (for `--backend uinput`)

```bash
echo 'KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/99-8bitdo-micro-uinput.rules
sudo udevadm control --reload-rules && sudo udevadm trigger /dev/uinput
```
