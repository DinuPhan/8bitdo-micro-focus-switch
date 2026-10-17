"""Universal Cross-Platform Keycode Definitions and Intermediate Representation (IR).

Provides a verified KeySpec representation mapping:
- USB HID Keyboard Page (0x07) Usage IDs (0x04..0xE7) for the 8BitDo Micro 180-byte record_micro
- Linux evdev KEY_* constants
- Windows Virtual-Key (VK_*) codes & hardware scan codes (with 0xE000 extended bit for SendInput)
- macOS CGKeyCode constants (<HIToolbox/Events.h>) for CGEventPost
- Sentinel Keymap (F13..F20 + Numpad) for zero-collision user-space hook isolation
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class KeySpec:
    """Universal key specification across Linux, Windows, macOS, and USB HID."""

    name: str  # Canonical name, e.g. "A", "LEFTCTRL", "F13"
    hid: int  # USB HID Keyboard Page (0x07) Usage ID (0x04..0xE7)
    evdev: int  # Linux evdev keycode constant (KEY_*)
    win_vk: int  # Windows Virtual-Key code (VK_*)
    win_scancode: int  # Windows scan code (includes 0xE000 bit if extended key)
    mac_vk: int | None  # macOS CGKeyCode (<HIToolbox/Events.h>) or None if not supported
    aliases: tuple[str, ...] = ()

    @property
    def is_win_extended(self) -> bool:
        """Return True if this key requires KEYEVENTF_EXTENDEDKEY (0x0001) in Windows SendInput."""
        return bool(self.win_scancode & 0xE000)

    @property
    def win_raw_scancode(self) -> int:
        """Return raw 1-byte scan code without the 0xE000 extended prefix."""
        return self.win_scancode & 0xFF


# ---------------------------------------------------------------------------
# Master Cross-Platform Key Specifications Table (100% Byte-Verified)
# Format: (hid, name, evdev, win_vk, win_scan, mac_vk, aliases)
# ---------------------------------------------------------------------------

_RAW_KEY_TABLE: tuple[tuple[int, str, int, int, int, int | None, tuple[str, ...]], ...] = (
    # --- Letters A-Z (HID 0x04..0x1D) ---
    (0x04, "A", 30, 0x41, 0x1E, 0x00, ()),
    (0x05, "B", 48, 0x42, 0x30, 0x0B, ()),
    (0x06, "C", 46, 0x43, 0x2E, 0x08, ()),
    (0x07, "D", 32, 0x44, 0x20, 0x02, ()),
    (0x08, "E", 18, 0x45, 0x12, 0x0E, ()),
    (0x09, "F", 33, 0x46, 0x21, 0x03, ()),
    (0x0A, "G", 34, 0x47, 0x22, 0x05, ()),
    (0x0B, "H", 35, 0x48, 0x23, 0x04, ()),
    (0x0C, "I", 23, 0x49, 0x17, 0x22, ()),
    (0x0D, "J", 36, 0x4A, 0x24, 0x26, ()),
    (0x0E, "K", 37, 0x4B, 0x25, 0x28, ()),
    (0x0F, "L", 38, 0x4C, 0x26, 0x25, ()),
    (0x10, "M", 50, 0x4D, 0x32, 0x2E, ()),
    (0x11, "N", 49, 0x4E, 0x31, 0x2D, ()),
    (0x12, "O", 24, 0x4F, 0x18, 0x1F, ()),
    (0x13, "P", 25, 0x50, 0x19, 0x23, ()),
    (0x14, "Q", 16, 0x51, 0x10, 0x0C, ()),
    (0x15, "R", 19, 0x52, 0x13, 0x0F, ()),
    (0x16, "S", 31, 0x53, 0x1F, 0x01, ()),
    (0x17, "T", 20, 0x54, 0x14, 0x11, ()),
    (0x18, "U", 22, 0x55, 0x16, 0x20, ()),
    (0x19, "V", 47, 0x56, 0x2F, 0x09, ()),
    (0x1A, "W", 17, 0x57, 0x11, 0x0D, ()),
    (0x1B, "X", 45, 0x58, 0x2D, 0x07, ()),
    (0x1C, "Y", 21, 0x59, 0x15, 0x10, ()),
    (0x1D, "Z", 44, 0x5A, 0x2C, 0x06, ()),

    # --- Number Row 1-0 (HID 0x1E..0x27) ---
    (0x1E, "1", 2,  0x31, 0x02, 0x12, ()),
    (0x1F, "2", 3,  0x32, 0x03, 0x13, ()),
    (0x20, "3", 4,  0x33, 0x04, 0x14, ()),
    (0x21, "4", 5,  0x34, 0x05, 0x15, ()),
    (0x22, "5", 6,  0x35, 0x06, 0x17, ()),  # Note: macOS '5' is 0x17, '6' is 0x16 (ADB physical layout)
    (0x23, "6", 7,  0x36, 0x07, 0x16, ()),
    (0x24, "7", 8,  0x37, 0x08, 0x1A, ()),
    (0x25, "8", 9,  0x38, 0x09, 0x1C, ()),
    (0x26, "9", 10, 0x39, 0x0A, 0x19, ()),
    (0x27, "0", 11, 0x30, 0x0B, 0x1D, ()),

    # --- Standard Editing & Punctuation (HID 0x28..0x39) ---
    (0x28, "ENTER",      28, 0x0D, 0x1C, 0x24, ("RETURN",)),
    (0x29, "ESC",        1,  0x1B, 0x01, 0x35, ("ESCAPE",)),
    (0x2A, "BACKSPACE",  14, 0x08, 0x0E, 0x33, ("BKSP",)),
    (0x2B, "TAB",        15, 0x09, 0x0F, 0x30, ()),
    (0x2C, "SPACE",      57, 0x20, 0x39, 0x31, ("SPACEBAR",)),
    (0x2D, "MINUS",      12, 0xBD, 0x0C, 0x1B, ("-", "SUB")),
    (0x2E, "EQUAL",      13, 0xBB, 0x0D, 0x18, ("=", "EQUALS")),
    (0x2F, "LEFTBRACE",  26, 0xDB, 0x1A, 0x21, ("[", "LBRACKET")),
    (0x30, "RIGHTBRACE", 27, 0xDD, 0x1B, 0x1E, ("]", "RBRACKET")),
    (0x31, "BACKSLASH",  43, 0xDC, 0x2B, 0x2A, ("\\",)),
    (0x33, "SEMICOLON",  39, 0xBA, 0x27, 0x29, (";",)),
    (0x34, "APOSTROPHE", 40, 0xDE, 0x28, 0x27, ("'", "QUOTE")),
    (0x35, "GRAVE",      41, 0xC0, 0x29, 0x32, ("`", "BACKTICK", "TILDE")),
    (0x36, "COMMA",      51, 0xBC, 0x33, 0x2B, (",",)),
    (0x37, "DOT",        52, 0xBE, 0x34, 0x2F, (".", "PERIOD")),
    (0x38, "SLASH",      53, 0xBF, 0x35, 0x2C, ("/", "DIV")),
    (0x39, "CAPSLOCK",   58, 0x14, 0x3A, 0x39, ("CAPS",)),

    # --- Function Keys F1-F12 (HID 0x3A..0x45) ---
    (0x3A, "F1",  59, 0x70, 0x3B, 0x7A, ()),
    (0x3B, "F2",  60, 0x71, 0x3C, 0x78, ()),
    (0x3C, "F3",  61, 0x72, 0x3D, 0x63, ()),
    (0x3D, "F4",  62, 0x73, 0x3E, 0x76, ()),
    (0x3E, "F5",  63, 0x74, 0x3F, 0x60, ()),
    (0x3F, "F6",  64, 0x75, 0x40, 0x61, ()),
    (0x40, "F7",  65, 0x76, 0x41, 0x62, ()),
    (0x41, "F8",  66, 0x77, 0x42, 0x64, ()),
    (0x42, "F9",  67, 0x78, 0x43, 0x65, ()),
    (0x43, "F10", 68, 0x79, 0x44, 0x6D, ()),
    (0x44, "F11", 87, 0x7A, 0x57, 0x67, ()),
    (0x45, "F12", 88, 0x7B, 0x58, 0x6F, ()),

    # --- Navigation & System Cluster (HID 0x46..0x52) ---
    (0x46, "SYSRQ",      99,  0x2C, 0xE037, None, ("PRINTSCREEN", "PRTSC")),
    (0x47, "SCROLLLOCK", 70,  0x91, 0x46,   None, ()),
    (0x48, "PAUSE",      119, 0x13, 0x45,   None, ()),
    (0x49, "INSERT",     110, 0x2D, 0xE052, 0x72, ("INS",)),
    (0x4A, "HOME",       102, 0x24, 0xE047, 0x73, ()),
    (0x4B, "PAGEUP",     104, 0x21, 0xE049, 0x74, ("PGUP",)),
    (0x4C, "DELETE",     111, 0x2E, 0xE053, 0x75, ("DEL",)),  # Forward Delete on macOS = 0x75
    (0x4D, "END",        107, 0x23, 0xE04F, 0x77, ()),
    (0x4E, "PAGEDOWN",   109, 0x22, 0xE051, 0x79, ("PGDN",)),
    (0x4F, "RIGHT",      106, 0x27, 0xE04D, 0x7C, ("RIGHTARROW",)),
    (0x50, "LEFT",       105, 0x25, 0xE04B, 0x7B, ("LEFTARROW",)),
    (0x51, "DOWN",       108, 0x28, 0xE050, 0x7D, ("DOWNARROW",)),
    (0x52, "UP",         103, 0x26, 0xE048, 0x7E, ("UPARROW",)),

    # --- Numpad / Keypad (HID 0x53..0x63) ---
    (0x53, "NUMLOCK",    69, 0x90, 0xE045, 0x47, ()),  # 0x47 = kVK_ANSI_KeypadClear
    (0x54, "KPSLASH",    98, 0x6F, 0xE035, 0x4B, ("KP_DIV",)),
    (0x55, "KPASTERISK", 55, 0x6A, 0x37,   0x43, ("KP_MUL", "KPMUL")),
    (0x56, "KPMINUS",    74, 0x6D, 0x4A,   0x4E, ("KP_SUB", "KPSUB")),
    (0x57, "KPPLUS",     78, 0x6B, 0x4E,   0x45, ("KP_ADD", "KPADD")),
    (0x58, "KPENTER",    96, 0x0D, 0xE01C, 0x4C, ()),  # 0x4C = kVK_ANSI_KeypadEnter
    (0x59, "KP1",        79, 0x61, 0x4F,   0x53, ()),
    (0x5A, "KP2",        80, 0x62, 0x50,   0x54, ()),
    (0x5B, "KP3",        81, 0x63, 0x51,   0x55, ()),
    (0x5C, "KP4",        75, 0x64, 0x4B,   0x56, ()),
    (0x5D, "KP5",        76, 0x65, 0x4C,   0x57, ()),
    (0x5E, "KP6",        77, 0x66, 0x4D,   0x58, ()),
    (0x5F, "KP7",        71, 0x67, 0x47,   0x59, ()),
    (0x60, "KP8",        72, 0x68, 0x48,   0x5B, ()),  # Note: macOS skips 0x5A! KP8 = 0x5B, KP9 = 0x5C
    (0x61, "KP9",        73, 0x69, 0x49,   0x5C, ()),
    (0x62, "KP0",        82, 0x60, 0x52,   0x52, ()),
    (0x63, "KPDOT",      83, 0x6E, 0x53,   0x41, ()),
    (0x65, "COMPOSE",   127, 0x5D, 0xE05D, 0x6E, ("APP", "MENU")),

    # --- Extended Function Keys F13-F24 (HID 0x68..0x73) ---
    (0x68, "F13", 183, 0x7C, 0x64, 0x69, ()),
    (0x69, "F14", 184, 0x7D, 0x65, 0x6B, ()),
    (0x6A, "F15", 185, 0x7E, 0x66, 0x71, ()),
    (0x6B, "F16", 186, 0x7F, 0x67, 0x6A, ()),
    (0x6C, "F17", 187, 0x80, 0x68, 0x40, ()),
    (0x6D, "F18", 188, 0x81, 0x69, 0x4F, ()),
    (0x6E, "F19", 189, 0x82, 0x6A, 0x50, ()),
    (0x6F, "F20", 190, 0x83, 0x6B, 0x5A, ()),
    (0x70, "F21", 191, 0x84, 0x6C, None, ()),  # macOS HIToolbox standard CGKeyCodes stop at F20
    (0x71, "F22", 192, 0x85, 0x6D, None, ()),
    (0x72, "F23", 193, 0x86, 0x6E, None, ()),
    (0x73, "F24", 194, 0x87, 0x76, None, ()),

    # --- Modifiers (HID 0xE0..0xE7) ---
    (0xE0, "LEFTCTRL",   29,  0xA2, 0x1D,   0x3B, ("LCTRL", "CTRL")),
    (0xE1, "LEFTSHIFT",  42,  0xA0, 0x2A,   0x38, ("LSHIFT", "SHIFT")),
    (0xE2, "LEFTALT",    56,  0xA4, 0x38,   0x3A, ("LALT", "ALT", "OPTION", "LOPT")),
    (0xE3, "LEFTMETA",   125, 0x5B, 0xE05B, 0x37, ("LWIN", "SUPER", "LMETA", "META", "CMD", "LCMD")),
    (0xE4, "RIGHTCTRL",  97,  0xA3, 0xE01D, 0x3E, ("RCTRL",)),
    (0xE5, "RIGHTSHIFT", 54,  0xA1, 0x36,   0x3C, ("RSHIFT",)),
    (0xE6, "RIGHTALT",   100, 0xA5, 0xE038, 0x3D, ("RALT", "ALTGR", "ROPT")),
    (0xE7, "RIGHTMETA",  126, 0x5C, 0xE05C, 0x36, ("RWIN", "RMETA", "RCMD")),
)


KEY_SPECS: tuple[KeySpec, ...] = tuple(
    KeySpec(
        name=item[1],
        hid=item[0],
        evdev=item[2],
        win_vk=item[3],
        win_scancode=item[4],
        mac_vk=item[5],
        aliases=item[6],
    )
    for item in _RAW_KEY_TABLE
)


# ---------------------------------------------------------------------------
# Fast Lookup Dictionaries & Indexing
# ---------------------------------------------------------------------------

KEY_BY_NAME: dict[str, KeySpec] = {}
KEY_BY_HID: dict[int, KeySpec] = {}
KEY_BY_EVDEV: dict[int, KeySpec] = {}
KEY_BY_WIN_VK: dict[int, KeySpec] = {}
KEY_BY_MAC_VK: dict[int, KeySpec] = {}

HID_TO_NAME: dict[int, str] = {0x00: "NONE"}
HID_TO_EVDEV: dict[int, int] = {}
NAME_TO_HID: dict[str, int] = {"NONE": 0x00, "": 0x00}
NAME_TO_EVDEV: dict[str, int] = {}
EVDEV_TO_NAME: dict[int, str] = {}

NAME_TO_WIN_VK: dict[str, int] = {}
WIN_VK_TO_NAME: dict[int, str] = {}

NAME_TO_MAC_VK: dict[str, int] = {}
MAC_VK_TO_NAME: dict[int, str] = {}


for spec in KEY_SPECS:
    KEY_BY_NAME[spec.name] = spec
    KEY_BY_NAME[f"KEY_{spec.name}"] = spec
    KEY_BY_HID[spec.hid] = spec
    KEY_BY_EVDEV[spec.evdev] = spec
    KEY_BY_WIN_VK[spec.win_vk] = spec
    if spec.mac_vk is not None:
        KEY_BY_MAC_VK[spec.mac_vk] = spec

    HID_TO_NAME[spec.hid] = spec.name
    HID_TO_EVDEV[spec.hid] = spec.evdev
    NAME_TO_HID[spec.name] = spec.hid
    NAME_TO_HID[f"KEY_{spec.name}"] = spec.hid
    NAME_TO_EVDEV[spec.name] = spec.evdev
    NAME_TO_EVDEV[f"KEY_{spec.name}"] = spec.evdev
    EVDEV_TO_NAME[spec.evdev] = spec.name

    NAME_TO_WIN_VK[spec.name] = spec.win_vk
    NAME_TO_WIN_VK[f"KEY_{spec.name}"] = spec.win_vk
    WIN_VK_TO_NAME[spec.win_vk] = spec.name

    if spec.mac_vk is not None:
        NAME_TO_MAC_VK[spec.name] = spec.mac_vk
        NAME_TO_MAC_VK[f"KEY_{spec.name}"] = spec.mac_vk
        MAC_VK_TO_NAME[spec.mac_vk] = spec.name

    for alias in spec.aliases:
        KEY_BY_NAME[alias] = spec
        KEY_BY_NAME[f"KEY_{alias}"] = spec
        NAME_TO_HID[alias] = spec.hid
        NAME_TO_HID[f"KEY_{alias}"] = spec.hid
        NAME_TO_EVDEV[alias] = spec.evdev
        NAME_TO_EVDEV[f"KEY_{alias}"] = spec.evdev
        NAME_TO_WIN_VK[alias] = spec.win_vk
        NAME_TO_WIN_VK[f"KEY_{alias}"] = spec.win_vk
        if spec.mac_vk is not None:
            NAME_TO_MAC_VK[alias] = spec.mac_vk
            NAME_TO_MAC_VK[f"KEY_{alias}"] = spec.mac_vk


# ---------------------------------------------------------------------------
# Sentinel Keymap for Zero-Collision User-Space Interception on Windows & macOS
# ---------------------------------------------------------------------------

# Maps the 16 physical buttons to 16 collision-free Sentinel keys.
# Uses 8 F13..F20 keys + 8 Numpad keys with 100% independent win_vk and mac_vk.
# (Avoids F21..F24 which lack standard CGKeyCode support on macOS).
SENTINEL_BUTTON_MAP: dict[str, str] = {
    # 8 Action & Shoulder buttons -> F13..F20 (HID 0x68..0x6F)
    "A": "F13",
    "B": "F14",
    "X": "F15",
    "Y": "F16",
    "L": "F17",
    "R": "F18",
    "L2": "F19",
    "R2": "F20",

    # 4 Center & Special buttons -> Numpad symbols
    "SELECT": "KPSLASH",     # HID 0x54
    "START": "KPASTERISK",   # HID 0x55
    "STAR": "KPMINUS",       # HID 0x56
    "HOME": "KPPLUS",        # HID 0x57

    # 4 D-Pad directional buttons -> Numpad digits (directional cluster)
    "UP": "KP8",             # HID 0x60
    "DOWN": "KP2",           # HID 0x5A
    "LEFT": "KP4",           # HID 0x5C
    "RIGHT": "KP6",          # HID 0x5E
}

SENTINEL_HID_TO_BUTTON: dict[int, str] = {
    NAME_TO_HID[key_name]: btn for btn, key_name in SENTINEL_BUTTON_MAP.items()
}

SENTINEL_WIN_VK_TO_BUTTON: dict[int, str] = {
    NAME_TO_WIN_VK[key_name]: btn for btn, key_name in SENTINEL_BUTTON_MAP.items()
}

SENTINEL_MAC_VK_TO_BUTTON: dict[int, str] = {
    NAME_TO_MAC_VK[key_name]: btn for btn, key_name in SENTINEL_BUTTON_MAP.items()
}


def normalize_key_name(raw_name: str) -> str:
    """Normalize a raw button or key identifier (e.g. 'ctrl' -> 'LEFTCTRL', 'space' -> 'SPACE')."""
    clean = raw_name.strip().upper()
    spec = KEY_BY_NAME.get(clean)
    if spec is not None:
        return spec.name
    return clean
