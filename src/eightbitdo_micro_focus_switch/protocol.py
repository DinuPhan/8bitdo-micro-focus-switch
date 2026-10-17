"""Reverse-engineered 8BitDo Micro ('80EL') binary protocol, key tables, and profile structs.

Provenance:
- APK: 8BitDo-Ultimate-Software-for-Android.apk (SHA-256: 2f612ee0cdd8586b1a14e85171d857969b2ad0e4273390fea6f2bd03615e3eb1)
- Native library: lib/arm64-v8a/libadvance-lib.so (SHA-256: e17341b46ec04e4e820296ddae7dd77088cfcec4662f1f491ffe55d31a24e6bf)
  - record_micro @ 0x1694ac (180 bytes / 0xB4)
  - crc16 @ 0x12c678 (CRC-16/ARC, poly=0xA001 reflected, init=0xFFFF)
  - Generate_CustomDataCmd @ 0x12be58
  - getMicroReadCofigChannel @ 0x12f1b0
  - getMicroSaveCustomConfigChannel @ 0x12f200
  - getMicroWriteCofigChannel @ 0x12f24c
  - getVersionChannel_Micro @ 0x12f110
  - getPlatFormChannel_Micro @ 0x12f160
"""

from __future__ import annotations

from dataclasses import dataclass, field
import struct
try:
    from evdev import ecodes
except ImportError:
    # Minimal fallback Linux keycodes for cross-platform support (Windows/macOS)
    _FALLBACK_ECODES: dict[str, int] = {
        "EV_SYN": 0, "EV_KEY": 1,
        "KEY_0": 11, "KEY_1": 2, "KEY_2": 3, "KEY_3": 4, "KEY_4": 5, "KEY_5": 6, "KEY_6": 7, "KEY_7": 8, "KEY_8": 9, "KEY_9": 10,
        "KEY_A": 30, "KEY_APOSTROPHE": 40, "KEY_B": 48, "KEY_BACKSLASH": 43, "KEY_BACKSPACE": 14, "KEY_C": 46, "KEY_CAPSLOCK": 58,
        "KEY_COMMA": 51, "KEY_COMPOSE": 127, "KEY_D": 32, "KEY_DELETE": 111, "KEY_DOT": 52, "KEY_DOWN": 108, "KEY_E": 18,
        "KEY_END": 107, "KEY_ENTER": 28, "KEY_EQUAL": 13, "KEY_ESC": 1, "KEY_F": 33, "KEY_F1": 59, "KEY_F10": 68, "KEY_F11": 87,
        "KEY_F12": 88, "KEY_F13": 183, "KEY_F14": 184, "KEY_F15": 185, "KEY_F16": 186, "KEY_F17": 187, "KEY_F18": 188,
        "KEY_F19": 189, "KEY_F2": 60, "KEY_F20": 190, "KEY_F21": 191, "KEY_F22": 192, "KEY_F23": 193, "KEY_F24": 194,
        "KEY_F3": 61, "KEY_F4": 62, "KEY_F5": 63, "KEY_F6": 64, "KEY_F7": 65, "KEY_F8": 66, "KEY_F9": 67, "KEY_G": 34,
        "KEY_GRAVE": 41, "KEY_H": 35, "KEY_HOME": 102, "KEY_I": 23, "KEY_INSERT": 110, "KEY_J": 36, "KEY_K": 37, "KEY_KP0": 82,
        "KEY_KP1": 79, "KEY_KP2": 80, "KEY_KP3": 81, "KEY_KP4": 75, "KEY_KP5": 76, "KEY_KP6": 77, "KEY_KP7": 71, "KEY_KP8": 72,
        "KEY_KP9": 73, "KEY_KPASTERISK": 55, "KEY_KPDOT": 83, "KEY_KPENTER": 96, "KEY_KPMINUS": 74, "KEY_KPPLUS": 78,
        "KEY_KPSLASH": 98, "KEY_L": 38, "KEY_LEFT": 105, "KEY_LEFTALT": 56, "KEY_LEFTBRACE": 26, "KEY_LEFTCTRL": 29,
        "KEY_LEFTMETA": 125, "KEY_LEFTSHIFT": 42, "KEY_M": 50, "KEY_MINUS": 12, "KEY_N": 49, "KEY_NUMLOCK": 69, "KEY_O": 24,
        "KEY_P": 25, "KEY_PAGEDOWN": 109, "KEY_PAGEUP": 104, "KEY_PAUSE": 119, "KEY_Q": 16, "KEY_R": 19, "KEY_RIGHT": 106,
        "KEY_RIGHTALT": 100, "KEY_RIGHTBRACE": 27, "KEY_RIGHTCTRL": 97, "KEY_RIGHTMETA": 126, "KEY_RIGHTSHIFT": 54, "KEY_S": 31,
        "KEY_SCROLLLOCK": 70, "KEY_SEMICOLON": 39, "KEY_SLASH": 53, "KEY_SPACE": 57, "KEY_SYSRQ": 99, "KEY_T": 20, "KEY_TAB": 15,
        "KEY_U": 22, "KEY_UP": 103, "KEY_V": 47, "KEY_W": 17, "KEY_X": 45, "KEY_Y": 21, "KEY_Z": 44,
    }

    class _EcodesFallback:
        def __getattr__(self, name: str) -> int:
            if name in _FALLBACK_ECODES:
                return _FALLBACK_ECODES[name]
            raise AttributeError(f"ecodes fallback has no attribute {name!r}")

        def __contains__(self, name: object) -> bool:
            return name in _FALLBACK_ECODES

    ecodes = _EcodesFallback()  # type: ignore

# Classic HID Keyboard identifiers (Keyboard mode 'K')
MICRO_VENDOR_ID = 0x2DC8
MICRO_PRODUCT_ID = 0x9021
MICRO_INPUT_NAME_SUBSTRING = "8BitDo Micro"

# BLE GATT identifiers (simultaneous config channel advertised as "80EL")
BLE_DEVICE_NAME = "80EL"
BLE_SERVICE_UUID = "0000ff10-0000-1000-8000-00805f9b34fb"
BLE_CHAR_UUID = "0000ff13-0000-1000-8000-00805f9b34fb"
BLE_CHAR_FALLBACK_UUID = "0000ff11-0000-1000-8000-00805f9b34fb"

# Binary constants from libadvance-lib.so
RECORD_MICRO_SIZE = 0xB4  # 180 bytes
MAX_ATT_MTU = 0x3F  # 63 bytes
MAX_BLE_CHUNK_LEN = MAX_ATT_MTU - 0x12  # 45 (0x2D) bytes per packet
FLAG_ENABLE = 0x20200911  # BasicAdvanceUI.flag_enable (538970385)
CMD_PACKET_PREFIX = 0x04

# BLE Custom Header commands
CMD_WRITE_CONFIG = 0x0001
CMD_READ_CONFIG = 0x0002
CMD_SAVE_CONFIG = 0x0006
CMD_SAVE_SUBCMD = 0x005B
CMD_READ_VERSION = 0x000B
CMD_GET_PLATFORM = 0x005A
VERSION_READ_ADDR = 0x01017040

# 20 button slots in record_micro (+0x0C .. +0x5B, 4 bytes per slot)
SLOT_ORDER: tuple[str, ...] = (
    "A",       # 0 (+0x0C)
    "B",       # 1 (+0x10)
    "X",       # 2 (+0x14)
    "Y",       # 3 (+0x18)
    "L",       # 4 (+0x1C) - KeyMap_L1
    "R",       # 5 (+0x20) - KeyMap_R1
    "L2",      # 6 (+0x24)
    "R2",      # 7 (+0x28)
    "L3",      # 8 (+0x2C) - unused on Micro
    "R3",      # 9 (+0x30) - unused on Micro
    "SELECT",  # 10 (+0x34) - KeyMap_Select (-)
    "START",   # 11 (+0x38) - KeyMap_Start (+)
    "STAR",    # 12 (+0x3C) - KeyMap_Share (Star)
    "HOME",    # 13 (+0x40) - KeyMap_Logo (Heart/Home)
    "UP",      # 14 (+0x44)
    "DOWN",    # 15 (+0x48)
    "LEFT",    # 16 (+0x4C)
    "RIGHT",   # 17 (+0x50)
    "P1",      # 18 (+0x54) - unused on Micro
    "P2",      # 19 (+0x58) - unused on Micro
)

PHYSICAL_BUTTONS: tuple[str, ...] = (
    "A",
    "B",
    "X",
    "Y",
    "L",
    "R",
    "L2",
    "R2",
    "SELECT",
    "START",
    "STAR",
    "HOME",
    "UP",
    "DOWN",
    "LEFT",
    "RIGHT",
)

BUTTON_ALIASES: dict[str, str] = {
    "A": "A",
    "B": "B",
    "X": "X",
    "Y": "Y",
    "L": "L",
    "L1": "L",
    "R": "R",
    "R1": "R",
    "L2": "L2",
    "R2": "R2",
    "L3": "L3",
    "R3": "R3",
    "SELECT": "SELECT",
    "MINUS": "SELECT",
    "-": "SELECT",
    "START": "START",
    "PLUS": "START",
    "+": "START",
    "STAR": "STAR",
    "SHARE": "STAR",
    "HOME": "HOME",
    "HEART": "HOME",
    "LOGO": "HOME",
    "UP": "UP",
    "DOWN": "DOWN",
    "LEFT": "LEFT",
    "RIGHT": "RIGHT",
    "P1": "P1",
    "P2": "P2",
}


def normalize_button_name(name: str) -> str:
    """Normalize a user-provided button name to its canonical SLOT_ORDER name."""
    key = name.strip().upper()
    if key not in BUTTON_ALIASES:
        valid = ", ".join(PHYSICAL_BUTTONS)
        raise ValueError(f"Unknown 8BitDo Micro button {name!r} (valid: {valid})")
    return BUTTON_ALIASES[key]


# Factory default K-mode USB HID Usage IDs (from MicroUI.getS_Mapping when flag != flag_enable)
FACTORY_BUTTON_HID_USAGES: dict[str, tuple[int, int, int, int]] = {
    "A": (0x0A, 0, 0, 0),       # g
    "B": (0x0D, 0, 0, 0),       # j
    "X": (0x0B, 0, 0, 0),       # h
    "Y": (0x0C, 0, 0, 0),       # i
    "L": (0x0E, 0, 0, 0),       # k
    "R": (0x10, 0, 0, 0),       # m
    "L2": (0x0F, 0, 0, 0),      # l
    "R2": (0x15, 0, 0, 0),      # r
    "L3": (0x00, 0, 0, 0),
    "R3": (0x00, 0, 0, 0),
    "SELECT": (0x11, 0, 0, 0),  # n
    "START": (0x12, 0, 0, 0),   # o
    "STAR": (0x17, 0, 0, 0),    # t
    "HOME": (0x16, 0, 0, 0),    # s
    "UP": (0x06, 0, 0, 0),      # c
    "DOWN": (0x07, 0, 0, 0),    # d
    "LEFT": (0x08, 0, 0, 0),    # e
    "RIGHT": (0x09, 0, 0, 0),   # f
    "P1": (0x00, 0, 0, 0),
    "P2": (0x00, 0, 0, 0),
}

# Factory default Linux evdev keycode -> canonical button name (for host-side daemon)
FACTORY_EVDEV_TO_BUTTON: dict[int, str] = {
    ecodes.KEY_G: "A",
    ecodes.KEY_J: "B",
    ecodes.KEY_H: "X",
    ecodes.KEY_I: "Y",
    ecodes.KEY_K: "L",
    ecodes.KEY_M: "R",
    ecodes.KEY_L: "L2",
    ecodes.KEY_R: "R2",
    ecodes.KEY_N: "SELECT",
    ecodes.KEY_O: "START",
    ecodes.KEY_T: "STAR",
    ecodes.KEY_S: "HOME",
    ecodes.KEY_C: "UP",
    ecodes.KEY_D: "DOWN",
    ecodes.KEY_E: "LEFT",
    ecodes.KEY_F: "RIGHT",
}

BUTTON_TO_FACTORY_EVDEV: dict[str, int] = {v: k for k, v in FACTORY_EVDEV_TO_BUTTON.items()}

from .keycodes import (
    EVDEV_TO_NAME,
    HID_TO_EVDEV,
    HID_TO_NAME,
    KEY_BY_EVDEV,
    KEY_BY_HID,
    KEY_BY_MAC_VK,
    KEY_BY_NAME,
    KEY_BY_WIN_VK,
    KEY_SPECS,
    KeySpec,
    NAME_TO_EVDEV,
    NAME_TO_HID,
    NAME_TO_MAC_VK,
    NAME_TO_WIN_VK,
    SENTINEL_BUTTON_MAP,
    SENTINEL_HID_TO_BUTTON,
    SENTINEL_MAC_VK_TO_BUTTON,
    SENTINEL_WIN_VK_TO_BUTTON,
    WIN_VK_TO_NAME,
    normalize_key_name,
)

# USB HID Keyboard/Keypad Page (0x07) Usage ID <-> Canonical Key Name <-> Linux evdev keycode
# Re-exported as a tuple of (hid, name, evdev, aliases) for complete backward compatibility
_HID_KEY_TABLE: tuple[tuple[int, str, int, tuple[str, ...]], ...] = tuple(
    (k.hid, k.name, k.evdev, k.aliases) for k in KEY_SPECS
)


def key_name_to_hid(name: str | int) -> int:
    """Convert a key name (e.g. 'SPACE', 'LCTRL', 'KEY_A') or raw integer to USB HID Usage ID."""
    if isinstance(name, int):
        if 0 <= name <= 0xFF:
            return name
        raise ValueError(f"HID Usage ID out of range: {name}")
    cleaned = name.strip().upper()
    if cleaned.startswith("0X"):
        val = int(cleaned, 16)
        if 0 <= val <= 0xFF:
            return val
    if cleaned in NAME_TO_HID:
        return NAME_TO_HID[cleaned]
    raise ValueError(f"Unknown key name for HID mapping: {name!r}")


def key_name_to_evdev(name: str | int) -> int:
    """Convert a key name (e.g. 'SPACE', 'LCTRL', 'KEY_A', 'XF86AudioMute') to Linux evdev keycode."""
    if isinstance(name, int):
        return name
    cleaned = name.strip().upper()
    if cleaned in NAME_TO_EVDEV:
        return NAME_TO_EVDEV[cleaned]
    ev_attr = cleaned if cleaned.startswith("KEY_") else f"KEY_{cleaned}"
    if hasattr(ecodes, ev_attr):
        return int(getattr(ecodes, ev_attr))
    raise ValueError(f"Unknown evdev key name: {name!r}")


def hid_tuple_to_names(usages: Sequence[int]) -> list[str]:
    """Format a 4-byte HID usage slot into a list of non-zero key names."""
    return [HID_TO_NAME.get(u, f"0x{u:02X}") for u in usages if u != 0]


# D-pad rotation (MicroTurnRecord: 0=0°, 1=90° CW, 2=180°, 3=270° CW)
TURN_DEGREES_TO_CODE: dict[int, int] = {
    0: 0,
    90: 1,
    180: 2,
    270: 3,
}
TURN_CODE_TO_DEGREES: dict[int, int] = {v: k for k, v in TURN_DEGREES_TO_CODE.items()}

# How physical D-pad presses map to logical D-pad directions when rotated
_DPAD_RING: tuple[str, str, str, str] = ("UP", "RIGHT", "DOWN", "LEFT")


def rotate_dpad_button(button: str, turn_degrees_or_code: int) -> str:
    """Rotate a physical D-pad button ('UP', 'DOWN', 'LEFT', 'RIGHT') by 0/90/180/270 degrees."""
    btn = normalize_button_name(button)
    if btn not in _DPAD_RING:
        return btn
    if turn_degrees_or_code in TURN_DEGREES_TO_CODE:
        steps = TURN_DEGREES_TO_CODE[turn_degrees_or_code]
    elif turn_degrees_or_code in TURN_CODE_TO_DEGREES:
        steps = turn_degrees_or_code
    else:
        raise ValueError(f"Invalid D-pad turn value {turn_degrees_or_code!r} (expected 0, 90, 180, 270 or 0..3)")
    idx = _DPAD_RING.index(btn)
    return _DPAD_RING[(idx + steps) % 4]


def crc16_arc(data: bytes | bytearray | memoryview) -> int:
    """Compute CRC-16/ARC (reflected polynomial 0xA001, init 0xFFFF) matching libadvance-lib.so @ 0x12c678."""
    if len(data) < 1:
        return 0xFFFF
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            if crc & 1:
                crc = ((crc >> 1) & 0x7FFF) ^ 0xA001
            else:
                crc = (crc >> 1) & 0x7FFF
    return crc & 0xFFFF


@dataclass(slots=True)
class BlePacketHeader:
    """16-byte cumstom_header_t structure inside BLE GATT packets."""

    cmd: int
    subcmd: int
    chunk_len: int
    crc16: int
    total_len: int
    offset: int


def build_ble_packet(
    cmd: int,
    subcmd: int,
    total_len: int,
    offset: int,
    payload: bytes = b"",
    *,
    header_chunk_len: int | None = None,
) -> bytes:
    """Build a 1 + 16 + len(payload) byte BLE GATT command packet for 0000FF13."""
    c_len = len(payload) if header_chunk_len is None else header_chunk_len
    crc = crc16_arc(payload)
    header = struct.pack("<HHHHII", cmd, subcmd, c_len, crc, total_len, offset)
    return bytes([CMD_PACKET_PREFIX]) + header + payload


def parse_ble_notification(data: bytes) -> tuple[BlePacketHeader, bytes]:
    """Parse an incoming BLE notification from 0000FF13 into (BlePacketHeader, payload)."""
    if len(data) < 16:
        raise ValueError(f"BLE notification too short ({len(data)} bytes < 16): {data.hex()}")
    cmd, subcmd, chunk_len, crc, total_len, offset = struct.unpack("<HHHHII", data[:16])
    payload = data[16 : 16 + chunk_len]
    return BlePacketHeader(cmd, subcmd, chunk_len, crc, total_len, offset), payload


def build_get_platform_packet() -> bytes:
    """Build getPlatFormChannel_Micro packet (cmd=0x005A, total=1, offset=0)."""
    return build_ble_packet(CMD_GET_PLATFORM, 0x0000, 1, 0, b"\x00")


def build_read_version_packet() -> bytes:
    """Build getVersionChannel_Micro packet (cmd=0x000B, total=4, offset=0x01017040)."""
    return build_ble_packet(CMD_READ_VERSION, 0x0000, 4, VERSION_READ_ADDR, b"\x00" * 4)


def build_read_config_packets() -> list[tuple[int, bytes]]:
    """Build the 4 chunked readMicroCustomConfig packets (offsets 0, 45, 90, 135)."""
    packets: list[tuple[int, bytes]] = []
    for offset in range(0, RECORD_MICRO_SIZE, MAX_BLE_CHUNK_LEN):
        chunk_len = min(MAX_BLE_CHUNK_LEN, RECORD_MICRO_SIZE - offset)
        pkt = build_ble_packet(CMD_READ_CONFIG, 0x0000, RECORD_MICRO_SIZE, offset, b"\x00" * chunk_len)
        packets.append((offset, pkt))
    return packets


def build_write_config_packets(record_bytes: bytes) -> list[tuple[int, bytes]]:
    """Build the 4 chunked writeMicroCustomConfig packets (offsets 0, 45, 90, 135) for a 180-byte record_micro."""
    if len(record_bytes) != RECORD_MICRO_SIZE:
        raise ValueError(f"record_micro must be {RECORD_MICRO_SIZE} bytes, got {len(record_bytes)}")
    packets: list[tuple[int, bytes]] = []
    for offset in range(0, RECORD_MICRO_SIZE, MAX_BLE_CHUNK_LEN):
        chunk = record_bytes[offset : offset + MAX_BLE_CHUNK_LEN]
        pkt = build_ble_packet(CMD_WRITE_CONFIG, 0x0000, RECORD_MICRO_SIZE, offset, chunk)
        packets.append((offset, pkt))
    return packets


def build_save_config_packet() -> bytes:
    """Build writeMicroSaveCustomConfig packet (cmd=0x0006, subcmd=0x005B, chunk_len=0, crc=0xFFFF)."""
    return build_ble_packet(CMD_SAVE_CONFIG, CMD_SAVE_SUBCMD, 0, 0, b"")


@dataclass
class MicroHardwareProfile:
    """Represents the 180-byte (0xB4) record_micro structure on the 8BitDo Micro."""

    enabled: bool = True
    turn: int = 0  # 0 (0°), 1 (90°), 2 (180°), 3 (270°)
    slots: dict[str, tuple[int, int, int, int]] = field(
        default_factory=lambda: dict(FACTORY_BUTTON_HID_USAGES)
    )
    header_prefix: bytes = b"\x00\x00\x00"
    tail_bytes: bytes = b"\x00" * 88

    def __post_init__(self) -> None:
        if self.turn in TURN_DEGREES_TO_CODE:
            self.turn = TURN_DEGREES_TO_CODE[self.turn]
        elif self.turn not in TURN_CODE_TO_DEGREES:
            raise ValueError(f"Invalid turn {self.turn!r}; expected 0, 90, 180, 270 or 0..3")

    @property
    def turn_degrees(self) -> int:
        return TURN_CODE_TO_DEGREES.get(self.turn, 0)

    def effective_slots(self) -> dict[str, tuple[int, int, int, int]]:
        """Return the effective button slots (factory defaults when enabled is False)."""
        if not self.enabled:
            return dict(FACTORY_BUTTON_HID_USAGES)
        result = dict(FACTORY_BUTTON_HID_USAGES)
        for slot_name in SLOT_ORDER:
            if slot_name in self.slots:
                result[slot_name] = self.slots[slot_name]
        return result

    def to_bytes(self, base: bytes | None = None) -> bytes:
        """Encode into the 180-byte record_micro binary buffer.

        If `base` (a previously read 180-byte buffer from the controller) is provided,
        bytes +0x00..+0x02 and +0x5C..+0xB3 are preserved from `base` just like libadvance-lib.so.
        """
        buf = bytearray(RECORD_MICRO_SIZE)
        if base is not None:
            if len(base) != RECORD_MICRO_SIZE:
                raise ValueError(f"base buffer must be {RECORD_MICRO_SIZE} bytes, got {len(base)}")
            buf[:] = base
        else:
            buf[0:3] = self.header_prefix[:3].ljust(3, b"\x00")
            buf[0x5C:RECORD_MICRO_SIZE] = self.tail_bytes[:88].ljust(88, b"\x00")

        buf[0x03] = self.turn & 0xFF
        flag_val = FLAG_ENABLE if self.enabled else 0
        struct.pack_into("<II", buf, 0x04, flag_val, flag_val)

        for idx, slot_name in enumerate(SLOT_ORDER):
            offset = 0x0C + idx * 4
            if self.enabled:
                usages = self.slots.get(slot_name, (0, 0, 0, 0))
            else:
                usages = (0, 0, 0, 0)
            padded = tuple(int(x) & 0xFF for x in list(usages)[:4]) + (0, 0, 0, 0)
            buf[offset : offset + 4] = bytes(padded[:4])

        return bytes(buf)

    @classmethod
    def from_bytes(cls, data: bytes) -> MicroHardwareProfile:
        """Decode a 180-byte record_micro buffer read from the controller."""
        if len(data) != RECORD_MICRO_SIZE:
            raise ValueError(f"Expected {RECORD_MICRO_SIZE} bytes for record_micro, got {len(data)}")
        header_prefix = bytes(data[0:3])
        turn = data[0x03] if data[0x03] in TURN_CODE_TO_DEGREES else 0
        flag_outer, flag_inner = struct.unpack_from("<II", data, 0x04)
        enabled = (flag_outer == FLAG_ENABLE) or (flag_inner == FLAG_ENABLE)

        slots: dict[str, tuple[int, int, int, int]] = {}
        for idx, slot_name in enumerate(SLOT_ORDER):
            offset = 0x0C + idx * 4
            b0, b1, b2, b3 = data[offset : offset + 4]
            slots[slot_name] = (b0, b1, b2, b3)

        if not enabled:
            slots = dict(FACTORY_BUTTON_HID_USAGES)

        return cls(
            enabled=enabled,
            turn=turn,
            slots=slots,
            header_prefix=header_prefix,
            tail_bytes=bytes(data[0x5C:RECORD_MICRO_SIZE]),
        )

    @classmethod
    def from_button_Verify_dict(
        cls,
        buttons: dict[str, str | Sequence[str | int]],
        *,
        turn: int = 0,
        enabled: bool = True,
    ) -> MicroHardwareProfile:
        """Build a MicroHardwareProfile from a dictionary of button -> key name(s)."""
        slots: dict[str, tuple[int, int, int, int]] = {name: (0, 0, 0, 0) for name in SLOT_ORDER}
        for raw_btn, raw_keys in buttons.items():
            btn = normalize_button_name(raw_btn)
            if isinstance(raw_keys, str):
                key_list: Iterable[str | int] = [k.strip() for k in raw_keys.split("+") if k.strip()]
            else:
                key_list = raw_keys
            usages = [key_name_to_hid(k) for k in key_list if k != "" and k != "NONE"]
            if len(usages) > 4:
                raise ValueError(f"Button {btn} has {len(usages)} keys; hardware limit is 4 keys per button")
            padded = tuple(usages) + (0,) * (4 - len(usages))
            slots[btn] = (padded[0], padded[1], padded[2], padded[3])
        return cls(enabled=enabled, turn=turn, slots=slots)


def get_builtin_hardware_preset(name: str) -> MicroHardwareProfile:
    """Return one of the official 8BitDo Micro hardware presets ('factory', 'anki', 'supermemo')."""
    key = name.strip().lower()
    if key in ("factory", "default", "reset"):
        return MicroHardwareProfile(
            enabled=False,
            turn=0,
            slots=dict(FACTORY_BUTTON_HID_USAGES),
        )
    if key == "anki":
        # Exact match to com.abitdo.advance.gamepad.MicroUI.getanki()
        return MicroHardwareProfile.from_button_Verify_dict(
            {
                "A": ["2"],
                "B": ["1"],
                "X": ["LEFTCTRL", "LEFTSHIFT", "D"],
                "Y": ["LEFTCTRL", "LEFTALT", "N"],
                "L": ["SPACE"],
                "R": ["4"],
                "L2": ["L"],
                "R2": ["R"],
                "SELECT": ["E"],
                "START": ["KPASTERISK"],
                "STAR": ["T"],
                "HOME": ["S"],
                "UP": ["LEFTCTRL", "DELETE"],
                "DOWN": ["LEFTSHIFT", "2"],
                "LEFT": ["LEFTCTRL", "Z"],
                "RIGHT": ["KPMINUS"],
            },
            turn=0,
            enabled=True,
        )
    if key == "agy":
        # User's custom Antigravity (agy) profile captured directly from live 8BitDo Micro
        return MicroHardwareProfile.from_button_Verify_dict(
            {
                "A": ["BACKSPACE"],
                "B": ["PAGEDOWN"],
                "X": ["PAGEUP"],
                "Y": ["LEFTSHIFT", "ENTER"],
                "L": ["TAB"],
                "R": ["LEFTCTRL", "LEFTSHIFT", "D"],
                "L2": ["LEFTSHIFT"],
                "R2": ["ENTER"],
                "SELECT": ["ESC"],
                "START": ["2", "LEFTSHIFT"],
                "STAR": ["SPACE"],
                "HOME": ["SLASH"],
                "UP": ["UP"],
                "DOWN": ["DOWN"],
                "LEFT": ["LEFT"],
                "RIGHT": ["RIGHT"],
            },
            turn=0,
            enabled=True,
        )
    if key == "supermemo":
        # Exact match to com.abitdo.advance.gamepad.MicroUI.getsuperMemo()
        return MicroHardwareProfile.from_button_Verify_dict(
            {
                "A": ["2"],
                "B": ["1"],
                "X": ["4"],
                "Y": ["LEFTALT", "P"],
                "L": ["ENTER"],
                "R": ["M"],
                "L2": ["L"],
                "R2": ["R"],
                "SELECT": ["N"],
                "START": ["O"],
                "STAR": ["T"],
                "HOME": ["S"],
                "UP": ["DELETE"],
                "DOWN": ["LEFTSHIFT", "LEFTCTRL", "X"],
                "LEFT": ["LEFTALT", "C"],
                "RIGHT": ["LEFTALT", "G"],
            },
            turn=0,
            enabled=True,
        )
    if key == "youtube":
        return MicroHardwareProfile.from_button_Verify_dict(
            {
                "A": ["SPACE"],
                "B": ["M"],
                "X": ["F"],
                "Y": ["C"],
                "L": ["J"],
                "R": ["L"],
                "L2": ["LEFTSHIFT", "COMMA"],
                "R2": ["LEFTSHIFT", "DOT"],
                "SELECT": ["ESC"],
                "START": ["LEFTSHIFT", "N"],
                "STAR": ["T"],
                "HOME": ["I"],
                "UP": ["UP"],
                "DOWN": ["DOWN"],
                "LEFT": ["LEFT"],
                "RIGHT": ["RIGHT"],
            },
            turn=0,
            enabled=True,
        )
    if key in ("sentinel", "host"):
        # Zero-collision Sentinel keymap (F13..F20 + Numpad) for user-space hooks on Windows/macOS
        slots = {}
        for btn in PHYSICAL_BUTTONS:
            sentinel_key = SENTINEL_BUTTON_MAP.get(btn)
            if sentinel_key and sentinel_key in NAME_TO_HID:
                slots[btn] = (NAME_TO_HID[sentinel_key], 0, 0, 0)
            else:
                slots[btn] = (0, 0, 0, 0)
        return MicroHardwareProfile(
            enabled=True,
            turn=0,
            slots=slots,
        )
    raise ValueError(
        f"Unknown built-in preset {name!r} (valid: 'agy', 'youtube', 'factory', 'anki', 'supermemo', 'sentinel')"
    )
