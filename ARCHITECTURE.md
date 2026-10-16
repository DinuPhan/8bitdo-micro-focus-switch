# Architecture & Protocol Reference

This document details the internal design, reverse-engineered BLE GATT protocol, and system architecture of `8bitdo-micro-focus-switch`.

---

## 1. System Overview

```
                        ┌─────────────────────────────────────┐
                        │   8BitDo Micro (Keyboard 'K' Mode)   │
                        └──────────┬────────────────┬─────────┘
                                   │                │
            Bluetooth Classic HID  │                │ BLE GATT ("80EL")
            (/dev/input/event*)    │                │ (Service 0xFF10 / Char 0xFF13)
                                   ▼                ▼
            ┌──────────────────────────────┐  ┌──────────────────────────────┐
            │       evdev / uinput         │  │     Bleak Async BLE Client   │
            │   (<1ms Instant Remapping)   │  │   (Direct Hardware Flasher)  │
            └──────────────┬───────────────┘  └──────────────┬───────────────┘
                           │                                 │
                           └────────────────┬────────────────┘
                                            │
                                            ▼
                           ┌─────────────────────────────────┐
                           │      FocusSwitchDaemon Engine   │
                           │   - Profile Resolver            │
                           │   - Key Chord Tracker & Safety  │
                           │   - D-Pad Rotator (0/90/180/270)│
                           └────────────────▲────────────────┘
                                            │
                                            │ IPC Stream (activewindow>>)
                                            │
                               ┌────────────┴────────────┐
                               │ Hyprland (.socket2.sock)│
                               └─────────────────────────┘
```

The 8BitDo Micro operates on two wireless interfaces concurrently:
1. **Bluetooth Classic HID**: Exposes a standard keyboard (`Vendor=0x2dc8, Product=0x9021`). In factory mode, pressing the 16 buttons emits simple ASCII keystrokes (`c` through `t`).
2. **Bluetooth Low Energy (BLE) GATT**: Broadcasts as `"80EL"` with custom proprietary characteristics used by the 8BitDo Android application to query firmware, read keymaps, and flash profiles into non-volatile memory.

---

## 2. Reverse-Engineered BLE GATT Protocol

The protocol was reverse-engineered by disassembling the official Android companion app (`8BitDo-Ultimate-Software-for-Android.apk`) and analyzing the native shared library `libadvance-lib.so`.

### 2.1 BLE Identifiers
- **Device Advertised Name**: `80EL`
- **GATT Service UUID**: `0000FF10-0000-1000-8000-00805F9B34FB`
- **GATT Characteristic UUID**: `0000FF13-0000-1000-8000-00805F9B34FB` (Read, Write, Notify)

### 2.2 Memory Structure: `record_micro` (180 bytes / `0xB4`)
Configuration memory on the controller is represented by a 180-byte struct:

| Offset | Length | Description |
| :--- | :--- | :--- |
| `+0x00` | 3 bytes | Header Prefix / Magic (typically `d5 08 00`) |
| `+0x03` | 1 byte | Enable Profile Flag (`0x00` = Disabled / Factory, `0x01` = Custom Active) |
| `+0x04` | 4 bytes | Protocol Flag 1 (e.g. `0x20200911` on v1.05 firmware) |
| `+0x08` | 4 bytes | Protocol Flag 2 (e.g. `0x20200911` on v1.05 firmware) |
| `+0x0C` | 72 bytes | 18 Button Mapping Slots (4 bytes each: USB HID usage IDs) |
| `+0x54` | 1 byte | D-pad rotation angle (`0` = 0°, `1` = 90° CW, `2` = 180°, `3` = 270° CW) |
| `+0x5C` | 88 bytes | Reserved / zero padding (total 180 bytes) |

#### Button Slot Indices
```
 0: A          1: B          2: X          3: Y
 4: L1 (L)     5: R1 (R)     6: L2         7: R2
 8: L3 (n/a)   9: R3 (n/a)  10: Select (-) 11: Start (+)
12: Star      13: Home/Heart14: Up         15: Down
16: Left      17: Right
```
Each slot holds up to four 1-byte USB HID Usage codes: one primary key (e.g., `0x04` = 'A') plus up to three modifier keys (e.g., `0xE0` = LeftCtrl, `0xE1` = LeftShift).

### 2.3 Checksum Algorithm: CRC-16/ARC
Every payload chunk uses standard **CRC-16/ARC**:
- **Polynomial**: `0xA001` (reflected `0x8005`)
- **Initial Value**: `0xFFFF`
- **Input Reflected**: True
- **Result Reflected**: True
- **Final XOR**: `0x0000`

### 2.4 BLE Packet Framing
Packets sent over characteristic `0xFF13` use a 17-byte header prefix prepended with a `0x04` channel indicator:

```
[0x04] [cmd:1] [subcmd:1] [chunk_len:2] [crc16:2] [total_len:2] [offset:2] [reserved:6] [payload: chunk_len]
```

- **Writing Configuration**: The 180-byte buffer is transmitted in four 45-byte chunks (`cmd=0x09, subcmd=0x09`):
  - Chunk 0: offset `0`, length `45`
  - Chunk 1: offset `45`, length `45`
  - Chunk 2: offset `90`, length `45`
  - Chunk 3: offset `135`, length `45`
- **Saving Configuration**: After all chunks are acknowledged, a save command packet is sent:
  - Header: `cmd=0x05, subcmd=0x05, chunk_len=0, crc16=0xFFFF, total_len=0, offset=0`
  - Payload: None.

---

## 3. Daemon Execution Models

`8bitdo-micro-focus-switch daemon` provides two remapping engines:

### 3.1 Mode A: `uinput` Virtual Keyboard (Recommended for Linux Desktop)
- **Mechanism**: Opens `/dev/input/event*` for the 8BitDo Micro with `EVIOCGRAB` (exclusive grab), suppressing the raw keystrokes from the host OS. Keystrokes are translated in software according to the active profile and reinjected via Linux `/dev/uinput` as a virtual keyboard.
- **Latency**: `< 1ms` (zero BLE radio overhead).
- **Capabilities**:
  - Complex multi-key chords (e.g., `Ctrl+Shift+Alt+Key`).
  - Arbitrary shell commands (`exec: notify-send ...`).
  - D-pad reorientation on the fly.
  - Non-HID Linux keys (e.g. Media Volume, Brightness).
  - **Simultaneous Multi-Button Combos / Chording**:
    - When buttons belonging to a defined combo (e.g. `L1 + R1`) are pressed down, a configurable buffer window (`combo_timeout_ms`, default 40ms) holds the initial button.
    - If a matching companion button arrives before timeout: single button emission is cancelled, and the chord action (e.g. `Super + K`) is fired to `uinput`.
    - If no companion button arrives within 40ms: the buffered single button is immediately flushed.
    - Single buttons not participating in any combo bypass the buffer completely, firing with `0ms` latency.
    - Fast tap safety: releasing a pending button before the timeout expires immediately emits a rapid tap (down + up) so fast keystrokes are never delayed or dropped.
- **Release Safety**: The daemon tracks currently held physical keys and active combo groups. If the focused window changes while keys are physically depressed, releasing any button safely terminates only the original key combination without phantom key repeats.

### 3.2 Mode B: `ble` Direct Hardware Reflash
- **Mechanism**: The daemon monitors Hyprland focus. When a target window gains focus, the daemon connects to the controller's `"80EL"` BLE interface in the background and rewrites the 180-byte hardware mapping directly into the controller's onboard memory.
- **Latency**: ~1.5 to 2.5 seconds per profile switch (BLE connection + 4 packets + commit).
- **Capabilities**:
  - Pure hardware operation — requires no `/dev/uinput` permissions or root access.
  - The controller retains the active profile even if unplugged or connected to another machine.

---

## 4. Window Focus Detection (Hyprland IPC)

Focus detection uses Hyprland's Unix domain socket stream (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`).
- **Zero Polling**: Listens to real-time events emitted by the Hyprland compositor.
- **Event Parsing**: Catches `activewindow>>[windowclass],[windowtitle]`.
- **Regex Resolution**: Iterates through configured profiles in `profiles.toml` testing `match_class` and `match_title` regular expressions. If no profile matches, falls back to `fallback_profile`.

---

## 5. Visualizer Engine

The diagram generator (`eightbitdo_micro_focus_switch/visualizer.py`):
1. **Asset Embedding**: Uses `importlib.resources` to embed a dark-mode vector-styled controller contour PNG directly into self-contained SVG files as a `data:image/png;base64,...` URI.
2. **Anchor Calculations**: 16 geometric anchors map to the physical centers of each button, running leader polylines with rounded elbows to label badges on the left and right margins.
3. **Catppuccin Palette**: Uses Catppuccin Mocha styling (`#11111b` background, `#cdd6f4` text, categorized accent colors for action buttons, triggers, bumpers, and D-pad).
4. **Interactive HTML Viewer**: Embeds generated SVGs for every profile into a tabbed single-page HTML interface with zero external CSS/JS dependencies.
