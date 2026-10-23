//! Universal Cross-Platform Keycode Definitions and Intermediate Representation (IR).
//!
//! Maps:
//! - USB HID Keyboard Page (0x07) Usage IDs (0x04..0xE7) for the 8BitDo Micro 180-byte `record_micro`
//! - Linux evdev `KEY_*` constants
//! - Windows Virtual-Key (`VK_*`) codes & hardware scan codes (with `0xE000` extended bit for `SendInput`)
//! - macOS `CGKeyCode` constants (`<HIToolbox/Events.h>`) for `CGEventPost`
//! - Sentinel Keymap (`F13..F20` + Numpad) for zero-collision user-space hook isolation

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub name: &'static str,
    pub hid: u8,
    pub evdev: u16,
    pub win_vk: u16,
    pub win_scancode: u16,
    pub mac_vk: Option<u16>,
    pub aliases: &'static [&'static str],
}

impl KeySpec {
    /// Return true if this key requires `KEYEVENTF_EXTENDEDKEY` (0x0001) in Windows `SendInput`.
    pub const fn is_win_extended(&self) -> bool {
        (self.win_scancode & 0xE000) != 0
    }

    /// Return raw 1-byte scan code without the `0xE000` extended prefix.
    pub const fn win_raw_scancode(&self) -> u16 {
        self.win_scancode & 0x00FF
    }
}

pub const KEY_SPECS: &[KeySpec] = &[
    // --- Letters A-Z (HID 0x04..0x1D) ---
    KeySpec { hid: 0x04, name: "A", evdev: 30, win_vk: 0x41, win_scancode: 0x1E, mac_vk: Some(0x00), aliases: &[] },
    KeySpec { hid: 0x05, name: "B", evdev: 48, win_vk: 0x42, win_scancode: 0x30, mac_vk: Some(0x0B), aliases: &[] },
    KeySpec { hid: 0x06, name: "C", evdev: 46, win_vk: 0x43, win_scancode: 0x2E, mac_vk: Some(0x08), aliases: &[] },
    KeySpec { hid: 0x07, name: "D", evdev: 32, win_vk: 0x44, win_scancode: 0x20, mac_vk: Some(0x02), aliases: &[] },
    KeySpec { hid: 0x08, name: "E", evdev: 18, win_vk: 0x45, win_scancode: 0x12, mac_vk: Some(0x0E), aliases: &[] },
    KeySpec { hid: 0x09, name: "F", evdev: 33, win_vk: 0x46, win_scancode: 0x21, mac_vk: Some(0x03), aliases: &[] },
    KeySpec { hid: 0x0A, name: "G", evdev: 34, win_vk: 0x47, win_scancode: 0x22, mac_vk: Some(0x05), aliases: &[] },
    KeySpec { hid: 0x0B, name: "H", evdev: 35, win_vk: 0x48, win_scancode: 0x23, mac_vk: Some(0x04), aliases: &[] },
    KeySpec { hid: 0x0C, name: "I", evdev: 23, win_vk: 0x49, win_scancode: 0x17, mac_vk: Some(0x22), aliases: &[] },
    KeySpec { hid: 0x0D, name: "J", evdev: 36, win_vk: 0x4A, win_scancode: 0x24, mac_vk: Some(0x26), aliases: &[] },
    KeySpec { hid: 0x0E, name: "K", evdev: 37, win_vk: 0x4B, win_scancode: 0x25, mac_vk: Some(0x28), aliases: &[] },
    KeySpec { hid: 0x0F, name: "L", evdev: 38, win_vk: 0x4C, win_scancode: 0x26, mac_vk: Some(0x25), aliases: &[] },
    KeySpec { hid: 0x10, name: "M", evdev: 50, win_vk: 0x4D, win_scancode: 0x32, mac_vk: Some(0x2E), aliases: &[] },
    KeySpec { hid: 0x11, name: "N", evdev: 49, win_vk: 0x4E, win_scancode: 0x31, mac_vk: Some(0x2D), aliases: &[] },
    KeySpec { hid: 0x12, name: "O", evdev: 24, win_vk: 0x4F, win_scancode: 0x18, mac_vk: Some(0x1F), aliases: &[] },
    KeySpec { hid: 0x13, name: "P", evdev: 25, win_vk: 0x50, win_scancode: 0x19, mac_vk: Some(0x23), aliases: &[] },
    KeySpec { hid: 0x14, name: "Q", evdev: 16, win_vk: 0x51, win_scancode: 0x10, mac_vk: Some(0x0C), aliases: &[] },
    KeySpec { hid: 0x15, name: "R", evdev: 19, win_vk: 0x52, win_scancode: 0x13, mac_vk: Some(0x0F), aliases: &[] },
    KeySpec { hid: 0x16, name: "S", evdev: 31, win_vk: 0x53, win_scancode: 0x1F, mac_vk: Some(0x01), aliases: &[] },
    KeySpec { hid: 0x17, name: "T", evdev: 20, win_vk: 0x54, win_scancode: 0x14, mac_vk: Some(0x11), aliases: &[] },
    KeySpec { hid: 0x18, name: "U", evdev: 22, win_vk: 0x55, win_scancode: 0x16, mac_vk: Some(0x20), aliases: &[] },
    KeySpec { hid: 0x19, name: "V", evdev: 47, win_vk: 0x56, win_scancode: 0x2F, mac_vk: Some(0x09), aliases: &[] },
    KeySpec { hid: 0x1A, name: "W", evdev: 17, win_vk: 0x57, win_scancode: 0x11, mac_vk: Some(0x0D), aliases: &[] },
    KeySpec { hid: 0x1B, name: "X", evdev: 45, win_vk: 0x58, win_scancode: 0x2D, mac_vk: Some(0x07), aliases: &[] },
    KeySpec { hid: 0x1C, name: "Y", evdev: 21, win_vk: 0x59, win_scancode: 0x15, mac_vk: Some(0x10), aliases: &[] },
    KeySpec { hid: 0x1D, name: "Z", evdev: 44, win_vk: 0x5A, win_scancode: 0x2C, mac_vk: Some(0x06), aliases: &[] },

    // --- Number Row 1-0 (HID 0x1E..0x27) ---
    KeySpec { hid: 0x1E, name: "1", evdev: 2,  win_vk: 0x31, win_scancode: 0x02, mac_vk: Some(0x12), aliases: &[] },
    KeySpec { hid: 0x1F, name: "2", evdev: 3,  win_vk: 0x32, win_scancode: 0x03, mac_vk: Some(0x13), aliases: &[] },
    KeySpec { hid: 0x20, name: "3", evdev: 4,  win_vk: 0x33, win_scancode: 0x04, mac_vk: Some(0x14), aliases: &[] },
    KeySpec { hid: 0x21, name: "4", evdev: 5,  win_vk: 0x34, win_scancode: 0x05, mac_vk: Some(0x15), aliases: &[] },
    KeySpec { hid: 0x22, name: "5", evdev: 6,  win_vk: 0x35, win_scancode: 0x06, mac_vk: Some(0x17), aliases: &[] },
    KeySpec { hid: 0x23, name: "6", evdev: 7,  win_vk: 0x36, win_scancode: 0x07, mac_vk: Some(0x16), aliases: &[] },
    KeySpec { hid: 0x24, name: "7", evdev: 8,  win_vk: 0x37, win_scancode: 0x08, mac_vk: Some(0x1A), aliases: &[] },
    KeySpec { hid: 0x25, name: "8", evdev: 9,  win_vk: 0x38, win_scancode: 0x09, mac_vk: Some(0x1C), aliases: &[] },
    KeySpec { hid: 0x26, name: "9", evdev: 10, win_vk: 0x39, win_scancode: 0x0A, mac_vk: Some(0x19), aliases: &[] },
    KeySpec { hid: 0x27, name: "0", evdev: 11, win_vk: 0x30, win_scancode: 0x0B, mac_vk: Some(0x1D), aliases: &[] },

    // --- Standard Editing & Punctuation (HID 0x28..0x39) ---
    KeySpec { hid: 0x28, name: "ENTER",      evdev: 28, win_vk: 0x0D, win_scancode: 0x1C, mac_vk: Some(0x24), aliases: &["RETURN"] },
    KeySpec { hid: 0x29, name: "ESC",        evdev: 1,  win_vk: 0x1B, win_scancode: 0x01, mac_vk: Some(0x35), aliases: &["ESCAPE"] },
    KeySpec { hid: 0x2A, name: "BACKSPACE",  evdev: 14, win_vk: 0x08, win_scancode: 0x0E, mac_vk: Some(0x33), aliases: &["BKSP"] },
    KeySpec { hid: 0x2B, name: "TAB",        evdev: 15, win_vk: 0x09, win_scancode: 0x0F, mac_vk: Some(0x30), aliases: &[] },
    KeySpec { hid: 0x2C, name: "SPACE",      evdev: 57, win_vk: 0x20, win_scancode: 0x39, mac_vk: Some(0x31), aliases: &["SPACEBAR"] },
    KeySpec { hid: 0x2D, name: "MINUS",      evdev: 12, win_vk: 0xBD, win_scancode: 0x0C, mac_vk: Some(0x1B), aliases: &["-", "SUB"] },
    KeySpec { hid: 0x2E, name: "EQUAL",      evdev: 13, win_vk: 0xBB, win_scancode: 0x0D, mac_vk: Some(0x18), aliases: &["=", "EQUALS"] },
    KeySpec { hid: 0x2F, name: "LEFTBRACE",  evdev: 26, win_vk: 0xDB, win_scancode: 0x1A, mac_vk: Some(0x21), aliases: &["[", "LBRACKET"] },
    KeySpec { hid: 0x30, name: "RIGHTBRACE", evdev: 27, win_vk: 0xDD, win_scancode: 0x1B, mac_vk: Some(0x1E), aliases: &["]", "RBRACKET"] },
    KeySpec { hid: 0x31, name: "BACKSLASH",  evdev: 43, win_vk: 0xDC, win_scancode: 0x2B, mac_vk: Some(0x2A), aliases: &["\\"] },
    KeySpec { hid: 0x33, name: "SEMICOLON",  evdev: 39, win_vk: 0xBA, win_scancode: 0x27, mac_vk: Some(0x29), aliases: &[";"] },
    KeySpec { hid: 0x34, name: "APOSTROPHE", evdev: 40, win_vk: 0xDE, win_scancode: 0x28, mac_vk: Some(0x27), aliases: &["'", "QUOTE"] },
    KeySpec { hid: 0x35, name: "GRAVE",      evdev: 41, win_vk: 0xC0, win_scancode: 0x29, mac_vk: Some(0x32), aliases: &["`", "BACKTICK", "TILDE"] },
    KeySpec { hid: 0x36, name: "COMMA",      evdev: 51, win_vk: 0xBC, win_scancode: 0x33, mac_vk: Some(0x2B), aliases: &[","] },
    KeySpec { hid: 0x37, name: "DOT",        evdev: 52, win_vk: 0xBE, win_scancode: 0x34, mac_vk: Some(0x2F), aliases: &[".", "PERIOD"] },
    KeySpec { hid: 0x38, name: "SLASH",      evdev: 53, win_vk: 0xBF, win_scancode: 0x35, mac_vk: Some(0x2C), aliases: &["/", "DIV"] },
    KeySpec { hid: 0x39, name: "CAPSLOCK",   evdev: 58, win_vk: 0x14, win_scancode: 0x3A, mac_vk: Some(0x39), aliases: &["CAPS"] },

    // --- Function Keys F1-F12 (HID 0x3A..0x45) ---
    KeySpec { hid: 0x3A, name: "F1",  evdev: 59, win_vk: 0x70, win_scancode: 0x3B, mac_vk: Some(0x7A), aliases: &[] },
    KeySpec { hid: 0x3B, name: "F2",  evdev: 60, win_vk: 0x71, win_scancode: 0x3C, mac_vk: Some(0x78), aliases: &[] },
    KeySpec { hid: 0x3C, name: "F3",  evdev: 61, win_vk: 0x72, win_scancode: 0x3D, mac_vk: Some(0x63), aliases: &[] },
    KeySpec { hid: 0x3D, name: "F4",  evdev: 62, win_vk: 0x73, win_scancode: 0x3E, mac_vk: Some(0x76), aliases: &[] },
    KeySpec { hid: 0x3E, name: "F5",  evdev: 63, win_vk: 0x74, win_scancode: 0x3F, mac_vk: Some(0x60), aliases: &[] },
    KeySpec { hid: 0x3F, name: "F6",  evdev: 64, win_vk: 0x75, win_scancode: 0x40, mac_vk: Some(0x61), aliases: &[] },
    KeySpec { hid: 0x40, name: "F7",  evdev: 65, win_vk: 0x76, win_scancode: 0x41, mac_vk: Some(0x62), aliases: &[] },
    KeySpec { hid: 0x41, name: "F8",  evdev: 66, win_vk: 0x77, win_scancode: 0x42, mac_vk: Some(0x64), aliases: &[] },
    KeySpec { hid: 0x42, name: "F9",  evdev: 67, win_vk: 0x78, win_scancode: 0x43, mac_vk: Some(0x65), aliases: &[] },
    KeySpec { hid: 0x43, name: "F10", evdev: 68, win_vk: 0x79, win_scancode: 0x44, mac_vk: Some(0x6D), aliases: &[] },
    KeySpec { hid: 0x44, name: "F11", evdev: 87, win_vk: 0x7A, win_scancode: 0x57, mac_vk: Some(0x67), aliases: &[] },
    KeySpec { hid: 0x45, name: "F12", evdev: 88, win_vk: 0x7B, win_scancode: 0x58, mac_vk: Some(0x6F), aliases: &[] },

    // --- Navigation & System Cluster (HID 0x46..0x52) ---
    KeySpec { hid: 0x46, name: "SYSRQ",      evdev: 99,  win_vk: 0x2C, win_scancode: 0xE037, mac_vk: None,       aliases: &["PRINTSCREEN", "PRTSC"] },
    KeySpec { hid: 0x47, name: "SCROLLLOCK", evdev: 70,  win_vk: 0x91, win_scancode: 0x46,   mac_vk: None,       aliases: &[] },
    KeySpec { hid: 0x48, name: "PAUSE",      evdev: 119, win_vk: 0x13, win_scancode: 0x45,   mac_vk: None,       aliases: &[] },
    KeySpec { hid: 0x49, name: "INSERT",     evdev: 110, win_vk: 0x2D, win_scancode: 0xE052, mac_vk: Some(0x72), aliases: &["INS"] },
    KeySpec { hid: 0x4A, name: "HOME",       evdev: 102, win_vk: 0x24, win_scancode: 0xE047, mac_vk: Some(0x73), aliases: &[] },
    KeySpec { hid: 0x4B, name: "PAGEUP",     evdev: 104, win_vk: 0x21, win_scancode: 0xE049, mac_vk: Some(0x74), aliases: &["PGUP"] },
    KeySpec { hid: 0x4C, name: "DELETE",     evdev: 111, win_vk: 0x2E, win_scancode: 0xE053, mac_vk: Some(0x75), aliases: &["DEL"] },
    KeySpec { hid: 0x4D, name: "END",        evdev: 107, win_vk: 0x23, win_scancode: 0xE04F, mac_vk: Some(0x77), aliases: &[] },
    KeySpec { hid: 0x4E, name: "PAGEDOWN",   evdev: 109, win_vk: 0x22, win_scancode: 0xE051, mac_vk: Some(0x79), aliases: &["PGDN"] },
    KeySpec { hid: 0x4F, name: "RIGHT",      evdev: 106, win_vk: 0x27, win_scancode: 0xE04D, mac_vk: Some(0x7C), aliases: &["RIGHTARROW"] },
    KeySpec { hid: 0x50, name: "LEFT",       evdev: 105, win_vk: 0x25, win_scancode: 0xE04B, mac_vk: Some(0x7B), aliases: &["LEFTARROW"] },
    KeySpec { hid: 0x51, name: "DOWN",       evdev: 108, win_vk: 0x28, win_scancode: 0xE050, mac_vk: Some(0x7D), aliases: &["DOWNARROW"] },
    KeySpec { hid: 0x52, name: "UP",         evdev: 103, win_vk: 0x26, win_scancode: 0xE048, mac_vk: Some(0x7E), aliases: &["UPARROW"] },

    // --- Numpad / Keypad (HID 0x53..0x63) ---
    KeySpec { hid: 0x53, name: "NUMLOCK",    evdev: 69,  win_vk: 0x90, win_scancode: 0xE045, mac_vk: Some(0x47), aliases: &[] },
    KeySpec { hid: 0x54, name: "KPSLASH",    evdev: 98,  win_vk: 0x6F, win_scancode: 0xE035, mac_vk: Some(0x4B), aliases: &["KP_DIV"] },
    KeySpec { hid: 0x55, name: "KPASTERISK", evdev: 55,  win_vk: 0x6A, win_scancode: 0x37,   mac_vk: Some(0x43), aliases: &["KP_MUL", "KPMUL"] },
    KeySpec { hid: 0x56, name: "KPMINUS",    evdev: 74,  win_vk: 0x6D, win_scancode: 0x4A,   mac_vk: Some(0x4E), aliases: &["KP_SUB", "KPSUB"] },
    KeySpec { hid: 0x57, name: "KPPLUS",     evdev: 78,  win_vk: 0x6B, win_scancode: 0x4E,   mac_vk: Some(0x45), aliases: &["KP_ADD", "KPADD"] },
    KeySpec { hid: 0x58, name: "KPENTER",    evdev: 96,  win_vk: 0x0D, win_scancode: 0xE01C, mac_vk: Some(0x4C), aliases: &[] },
    KeySpec { hid: 0x59, name: "KP1",        evdev: 79,  win_vk: 0x61, win_scancode: 0x4F,   mac_vk: Some(0x53), aliases: &[] },
    KeySpec { hid: 0x5A, name: "KP2",        evdev: 80,  win_vk: 0x62, win_scancode: 0x50,   mac_vk: Some(0x54), aliases: &[] },
    KeySpec { hid: 0x5B, name: "KP3",        evdev: 81,  win_vk: 0x63, win_scancode: 0x51,   mac_vk: Some(0x55), aliases: &[] },
    KeySpec { hid: 0x5C, name: "KP4",        evdev: 75,  win_vk: 0x64, win_scancode: 0x4B,   mac_vk: Some(0x56), aliases: &[] },
    KeySpec { hid: 0x5D, name: "KP5",        evdev: 76,  win_vk: 0x65, win_scancode: 0x4C,   mac_vk: Some(0x57), aliases: &[] },
    KeySpec { hid: 0x5E, name: "KP6",        evdev: 77,  win_vk: 0x66, win_scancode: 0x4D,   mac_vk: Some(0x58), aliases: &[] },
    KeySpec { hid: 0x5F, name: "KP7",        evdev: 71,  win_vk: 0x67, win_scancode: 0x47,   mac_vk: Some(0x59), aliases: &[] },
    KeySpec { hid: 0x60, name: "KP8",        evdev: 72,  win_vk: 0x68, win_scancode: 0x48,   mac_vk: Some(0x5B), aliases: &[] },
    KeySpec { hid: 0x61, name: "KP9",        evdev: 73,  win_vk: 0x69, win_scancode: 0x49,   mac_vk: Some(0x5C), aliases: &[] },
    KeySpec { hid: 0x62, name: "KP0",        evdev: 82,  win_vk: 0x60, win_scancode: 0x52,   mac_vk: Some(0x52), aliases: &[] },
    KeySpec { hid: 0x63, name: "KPDOT",      evdev: 83,  win_vk: 0x6E, win_scancode: 0x53,   mac_vk: Some(0x41), aliases: &[] },
    KeySpec { hid: 0x65, name: "COMPOSE",    evdev: 127, win_vk: 0x5D, win_scancode: 0xE05D, mac_vk: Some(0x6E), aliases: &["APP", "MENU"] },

    // --- Extended Function Keys F13-F24 (HID 0x68..0x73) ---
    KeySpec { hid: 0x68, name: "F13", evdev: 183, win_vk: 0x7C, win_scancode: 0x64, mac_vk: Some(0x69), aliases: &[] },
    KeySpec { hid: 0x69, name: "F14", evdev: 184, win_vk: 0x7D, win_scancode: 0x65, mac_vk: Some(0x6B), aliases: &[] },
    KeySpec { hid: 0x6A, name: "F15", evdev: 185, win_vk: 0x7E, win_scancode: 0x66, mac_vk: Some(0x71), aliases: &[] },
    KeySpec { hid: 0x6B, name: "F16", evdev: 186, win_vk: 0x7F, win_scancode: 0x67, mac_vk: Some(0x6A), aliases: &[] },
    KeySpec { hid: 0x6C, name: "F17", evdev: 187, win_vk: 0x80, win_scancode: 0x68, mac_vk: Some(0x40), aliases: &[] },
    KeySpec { hid: 0x6D, name: "F18", evdev: 188, win_vk: 0x81, win_scancode: 0x69, mac_vk: Some(0x4F), aliases: &[] },
    KeySpec { hid: 0x6E, name: "F19", evdev: 189, win_vk: 0x82, win_scancode: 0x6A, mac_vk: Some(0x50), aliases: &[] },
    KeySpec { hid: 0x6F, name: "F20", evdev: 190, win_vk: 0x83, win_scancode: 0x6B, mac_vk: Some(0x5A), aliases: &[] },
    KeySpec { hid: 0x70, name: "F21", evdev: 191, win_vk: 0x84, win_scancode: 0x6C, mac_vk: None,       aliases: &[] },
    KeySpec { hid: 0x71, name: "F22", evdev: 192, win_vk: 0x85, win_scancode: 0x6D, mac_vk: None,       aliases: &[] },
    KeySpec { hid: 0x72, name: "F23", evdev: 193, win_vk: 0x86, win_scancode: 0x6E, mac_vk: None,       aliases: &[] },
    KeySpec { hid: 0x73, name: "F24", evdev: 194, win_vk: 0x87, win_scancode: 0x76, mac_vk: None,       aliases: &[] },

    // --- Modifiers (HID 0xE0..0xE7) ---
    KeySpec { hid: 0xE0, name: "LEFTCTRL",   evdev: 29,  win_vk: 0xA2, win_scancode: 0x1D,   mac_vk: Some(0x3B), aliases: &["LCTRL", "CTRL"] },
    KeySpec { hid: 0xE1, name: "LEFTSHIFT",  evdev: 42,  win_vk: 0xA0, win_scancode: 0x2A,   mac_vk: Some(0x38), aliases: &["LSHIFT", "SHIFT"] },
    KeySpec { hid: 0xE2, name: "LEFTALT",    evdev: 56,  win_vk: 0xA4, win_scancode: 0x38,   mac_vk: Some(0x3A), aliases: &["LALT", "ALT", "OPTION", "LOPT"] },
    KeySpec { hid: 0xE3, name: "LEFTMETA",   evdev: 125, win_vk: 0x5B, win_scancode: 0xE05B, mac_vk: Some(0x37), aliases: &["LWIN", "SUPER", "LMETA", "META", "CMD", "LCMD"] },
    KeySpec { hid: 0xE4, name: "RIGHTCTRL",  evdev: 97,  win_vk: 0xA3, win_scancode: 0xE01D, mac_vk: Some(0x3E), aliases: &["RCTRL"] },
    KeySpec { hid: 0xE5, name: "RIGHTSHIFT", evdev: 54,  win_vk: 0xA1, win_scancode: 0x36,   mac_vk: Some(0x3C), aliases: &["RSHIFT"] },
    KeySpec { hid: 0xE6, name: "RIGHTALT",   evdev: 100, win_vk: 0xA5, win_scancode: 0xE038, mac_vk: Some(0x3D), aliases: &["RALT", "ALTGR", "ROPT"] },
    KeySpec { hid: 0xE7, name: "RIGHTMETA",  evdev: 126, win_vk: 0x5C, win_scancode: 0xE05C, mac_vk: Some(0x36), aliases: &["RWIN", "RMETA", "RCMD"] },
];

/// Non-HID extra Linux evdev media keycodes supported in uinput/host mode
pub const EXTRA_MEDIA_EVDEV: &[(&str, u16)] = &[
    ("MUTE", 113),
    ("XF86AUDIOMUTE", 113),
    ("KEY_MUTE", 113),
    ("VOLUMEDOWN", 114),
    ("XF86AUDIOLOWERVOLUME", 114),
    ("KEY_VOLUMEDOWN", 114),
    ("VOLUMEUP", 115),
    ("XF86AUDIORAISEVOLUME", 115),
    ("KEY_VOLUMEUP", 115),
    ("NEXTSONG", 163),
    ("XF86AUDIONEXT", 163),
    ("KEY_NEXTSONG", 163),
    ("PLAYPAUSE", 164),
    ("XF86AUDIOPLAY", 164),
    ("KEY_PLAYPAUSE", 164),
    ("PREVIOUSSONG", 165),
    ("XF86AUDIOPREV", 165),
    ("KEY_PREVIOUSSONG", 165),
];

/// Sentinel Keymap for Zero-Collision User-Space Interception on Windows & macOS
pub const SENTINEL_BUTTON_MAP: &[(&str, &str)] = &[
    // 8 Action & Shoulder buttons -> F13..F20 (HID 0x68..0x6F)
    ("A", "F13"),
    ("B", "F14"),
    ("X", "F15"),
    ("Y", "F16"),
    ("L", "F17"),
    ("R", "F18"),
    ("L2", "F19"),
    ("R2", "F20"),
    // 4 Center & Special buttons -> Numpad symbols
    ("SELECT", "KPSLASH"),
    ("START", "KPASTERISK"),
    ("STAR", "KPMINUS"),
    ("HOME", "KPPLUS"),
    // 4 D-Pad directional buttons -> Numpad digits
    ("UP", "KP8"),
    ("DOWN", "KP2"),
    ("LEFT", "KP4"),
    ("RIGHT", "KP6"),
];

pub struct KeyTables {
    pub by_name: HashMap<String, &'static KeySpec>,
    pub by_hid: HashMap<u8, &'static KeySpec>,
    pub by_evdev: HashMap<u16, &'static KeySpec>,
    pub by_win_vk: HashMap<u16, &'static KeySpec>,
    pub by_mac_vk: HashMap<u16, &'static KeySpec>,
    pub sentinel_win_scan_to_btn: HashMap<(u16, bool), &'static str>,
    pub sentinel_mac_vk_to_btn: HashMap<u16, &'static str>,
}

pub fn tables() -> &'static KeyTables {
    static TABLES: OnceLock<KeyTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut by_name = HashMap::new();
        let mut by_hid = HashMap::new();
        let mut by_evdev = HashMap::new();
        let mut by_win_vk = HashMap::new();
        let mut by_mac_vk = HashMap::new();

        for spec in KEY_SPECS {
            by_name.insert(spec.name.to_string(), spec);
            by_name.insert(format!("KEY_{}", spec.name), spec);
            by_hid.insert(spec.hid, spec);
            by_evdev.insert(spec.evdev, spec);
            by_win_vk.insert(spec.win_vk, spec);
            if let Some(mvk) = spec.mac_vk {
                by_mac_vk.insert(mvk, spec);
            }
            for alias in spec.aliases {
                by_name.insert((*alias).to_string(), spec);
                by_name.insert(format!("KEY_{}", alias), spec);
            }
        }

        let mut sentinel_win_scan_to_btn = HashMap::new();
        let mut sentinel_mac_vk_to_btn = HashMap::new();

        for &(btn, key_name) in SENTINEL_BUTTON_MAP {
            if let Some(spec) = by_name.get(key_name) {
                sentinel_win_scan_to_btn
                    .insert((spec.win_raw_scancode(), spec.is_win_extended()), btn);
                if let Some(mvk) = spec.mac_vk {
                    sentinel_mac_vk_to_btn.insert(mvk, btn);
                }
            }
        }

        KeyTables {
            by_name,
            by_hid,
            by_evdev,
            by_win_vk,
            by_mac_vk,
            sentinel_win_scan_to_btn,
            sentinel_mac_vk_to_btn,
        }
    })
}

pub fn key_by_name(name: &str) -> Option<&'static KeySpec> {
    tables().by_name.get(&name.trim().to_uppercase()).copied()
}

pub fn key_by_hid(hid: u8) -> Option<&'static KeySpec> {
    tables().by_hid.get(&hid).copied()
}

pub fn key_by_evdev(evdev: u16) -> Option<&'static KeySpec> {
    tables().by_evdev.get(&evdev).copied()
}

pub fn hid_to_name(hid: u8) -> String {
    if hid == 0 {
        return "NONE".to_string();
    }
    if let Some(spec) = key_by_hid(hid) {
        spec.name.to_string()
    } else {
        format!("0x{:02X}", hid)
    }
}

pub fn normalize_key_name(raw_name: &str) -> String {
    let clean = raw_name.trim().to_uppercase();
    if let Some(spec) = key_by_name(&clean) {
        spec.name.to_string()
    } else {
        clean
    }
}
