//! Reverse-engineered 8BitDo Micro ('80EL') binary protocol, key tables, and profile structs.
//!
//! Provenance:
//! - APK: `8BitDo-Ultimate-Software-for-Android.apk`
//! - Native library: `lib/arm64-v8a/libadvance-lib.so`
//!   - `record_micro` @ 0x1694ac (180 bytes / 0xB4)
//!   - `crc16` @ 0x12c678 (CRC-16/ARC, poly=0xA001 reflected, init=0xFFFF)

use std::collections::HashMap;

use crate::keycodes::{
    hid_to_name, key_by_name, EXTRA_MEDIA_EVDEV, SENTINEL_BUTTON_MAP,
};

pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;

// Classic HID Keyboard identifiers (Keyboard mode 'K' / 'R+START')
pub const MICRO_VENDOR_ID: u16 = 0x2DC8;
pub const MICRO_PRODUCT_ID: u16 = 0x9021;
pub const MICRO_INPUT_NAME_SUBSTRING: &str = "8BitDo Micro";

pub const ZERO2_VENDOR_ID: u16 = 0x2DC8;
pub const ZERO2_PRODUCT_ID: u16 = 0x3230;
pub const ZERO2_PRODUCT_ID_ALT: u16 = 0x9018;
pub const ZERO2_PRODUCT_ID_BT: u16 = 0x2830;
pub const ZERO2_INPUT_NAME_SUBSTRING: &str = "8BitDo Zero 2";

pub const SUPPORTED_KEYBOARD_DEVICE_IDS: &[((u16, u16), &str)] = &[
    ((MICRO_VENDOR_ID, MICRO_PRODUCT_ID), "8BitDo Micro"),
    ((ZERO2_VENDOR_ID, ZERO2_PRODUCT_ID), "8BitDo Zero 2"),
    ((ZERO2_VENDOR_ID, ZERO2_PRODUCT_ID_ALT), "8BitDo Zero 2"),
    ((ZERO2_VENDOR_ID, ZERO2_PRODUCT_ID_BT), "8BitDo Zero 2"),
];

pub const SUPPORTED_KEYBOARD_NAME_SUBSTRINGS: &[(&str, &str)] = &[
    ("8bitdo micro", "8BitDo Micro"),
    ("micro gamepad", "8BitDo Micro"),
    ("8bitdo zero 2", "8BitDo Zero 2"),
    ("zero 2 gamepad", "8BitDo Zero 2"),
    ("zero 2", "8BitDo Zero 2"),
];

// BLE GATT identifiers (simultaneous config channel advertised as "80EL")
pub const BLE_DEVICE_NAME: &str = "80EL";
pub const BLE_SERVICE_UUID: &str = "0000ff10-0000-1000-8000-00805f9b34fb";
pub const BLE_CHAR_UUID: &str = "0000ff13-0000-1000-8000-00805f9b34fb";
pub const BLE_CHAR_FALLBACK_UUID: &str = "0000ff11-0000-1000-8000-00805f9b34fb";

// Binary constants from libadvance-lib.so
pub const RECORD_MICRO_SIZE: usize = 0xB4; // 180 bytes
pub const MAX_ATT_MTU: usize = 0x3F; // 63 bytes
pub const MAX_BLE_CHUNK_LEN: usize = MAX_ATT_MTU - 0x12; // 45 (0x2D) bytes per packet
pub const FLAG_ENABLE: u32 = 0x20200911; // BasicAdvanceUI.flag_enable (538970385)
pub const CMD_PACKET_PREFIX: u8 = 0x04;

// BLE Custom Header commands
pub const CMD_WRITE_CONFIG: u16 = 0x0001;
pub const CMD_READ_CONFIG: u16 = 0x0002;
pub const CMD_SAVE_CONFIG: u16 = 0x0006;
pub const CMD_SAVE_SUBCMD: u16 = 0x005B;
pub const CMD_READ_VERSION: u16 = 0x000B;
pub const CMD_GET_PLATFORM: u16 = 0x005A;
pub const VERSION_READ_ADDR: u32 = 0x01017040;

// 20 button slots in record_micro (+0x0C .. +0x5B, 4 bytes per slot)
pub const SLOT_ORDER: &[&str] = &[
    "A",      // 0 (+0x0C)
    "B",      // 1 (+0x10)
    "X",      // 2 (+0x14)
    "Y",      // 3 (+0x18)
    "L",      // 4 (+0x1C) - KeyMap_L1
    "R",      // 5 (+0x20) - KeyMap_R1
    "L2",     // 6 (+0x24)
    "R2",     // 7 (+0x28)
    "L3",     // 8 (+0x2C) - unused on Micro
    "R3",     // 9 (+0x30) - unused on Micro
    "SELECT", // 10 (+0x34) - KeyMap_Select (-)
    "START",  // 11 (+0x38) - KeyMap_Start (+)
    "STAR",   // 12 (+0x3C) - KeyMap_Share (Star)
    "HOME",   // 13 (+0x40) - KeyMap_Logo (Heart/Home)
    "UP",     // 14 (+0x44)
    "DOWN",   // 15 (+0x48)
    "LEFT",   // 16 (+0x4C)
    "RIGHT",  // 17 (+0x50)
    "P1",     // 18 (+0x54) - unused on Micro
    "P2",     // 19 (+0x58) - unused on Micro
];

pub const PHYSICAL_BUTTONS: &[&str] = &[
    "A", "B", "X", "Y", "L", "R", "L2", "R2", "SELECT", "START", "STAR", "HOME", "UP", "DOWN",
    "LEFT", "RIGHT",
];

pub fn normalize_button_name(name: &str) -> Result<&'static str, String> {
    match name.trim().to_uppercase().as_str() {
        "A" => Ok("A"),
        "B" => Ok("B"),
        "X" => Ok("X"),
        "Y" => Ok("Y"),
        "L" | "L1" => Ok("L"),
        "R" | "R1" => Ok("R"),
        "L2" => Ok("L2"),
        "R2" => Ok("R2"),
        "L3" => Ok("L3"),
        "R3" => Ok("R3"),
        "SELECT" | "MINUS" | "-" => Ok("SELECT"),
        "START" | "PLUS" | "+" => Ok("START"),
        "STAR" | "SHARE" => Ok("STAR"),
        "HOME" | "HEART" | "LOGO" => Ok("HOME"),
        "UP" => Ok("UP"),
        "DOWN" => Ok("DOWN"),
        "LEFT" => Ok("LEFT"),
        "RIGHT" => Ok("RIGHT"),
        "P1" => Ok("P1"),
        "P2" => Ok("P2"),
        _ => Err(format!(
            "Unknown 8BitDo Micro button {:?} (valid: {})",
            name,
            PHYSICAL_BUTTONS.join(", ")
        )),
    }
}

pub fn factory_button_hid_usages() -> HashMap<String, [u8; 4]> {
    let mut m = HashMap::new();
    m.insert("A".to_string(), [0x0A, 0, 0, 0]); // g
    m.insert("B".to_string(), [0x0D, 0, 0, 0]); // j
    m.insert("X".to_string(), [0x0B, 0, 0, 0]); // h
    m.insert("Y".to_string(), [0x0C, 0, 0, 0]); // i
    m.insert("L".to_string(), [0x0E, 0, 0, 0]); // k
    m.insert("R".to_string(), [0x10, 0, 0, 0]); // m
    m.insert("L2".to_string(), [0x0F, 0, 0, 0]); // l
    m.insert("R2".to_string(), [0x15, 0, 0, 0]); // r
    m.insert("L3".to_string(), [0x00, 0, 0, 0]);
    m.insert("R3".to_string(), [0x00, 0, 0, 0]);
    m.insert("SELECT".to_string(), [0x11, 0, 0, 0]); // n
    m.insert("START".to_string(), [0x12, 0, 0, 0]); // o
    m.insert("STAR".to_string(), [0x17, 0, 0, 0]); // t
    m.insert("HOME".to_string(), [0x16, 0, 0, 0]); // s
    m.insert("UP".to_string(), [0x06, 0, 0, 0]); // c
    m.insert("DOWN".to_string(), [0x07, 0, 0, 0]); // d
    m.insert("LEFT".to_string(), [0x08, 0, 0, 0]); // e
    m.insert("RIGHT".to_string(), [0x09, 0, 0, 0]); // f
    m.insert("P1".to_string(), [0x00, 0, 0, 0]);
    m.insert("P2".to_string(), [0x00, 0, 0, 0]);
    m
}

pub fn factory_evdev_to_button(code: u16) -> Option<&'static str> {
    match code {
        34 => Some("A"),      // KEY_G
        36 => Some("B"),      // KEY_J
        35 => Some("X"),      // KEY_H
        23 => Some("Y"),      // KEY_I
        37 => Some("L"),      // KEY_K
        50 => Some("R"),      // KEY_M
        38 => Some("L2"),     // KEY_L
        19 => Some("R2"),     // KEY_R
        49 => Some("SELECT"), // KEY_N
        24 => Some("START"),  // KEY_O
        20 => Some("STAR"),   // KEY_T
        31 => Some("HOME"),   // KEY_S
        46 => Some("UP"),     // KEY_C
        32 => Some("DOWN"),   // KEY_D
        18 => Some("LEFT"),   // KEY_E
        33 => Some("RIGHT"),  // KEY_F
        _ => None,
    }
}

pub fn sentinel_evdev_to_button(code: u16) -> Option<&'static str> {
    match code {
        183 => Some("A"),      // KEY_F13
        184 => Some("B"),      // KEY_F14
        185 => Some("X"),      // KEY_F15
        186 => Some("Y"),      // KEY_F16
        187 => Some("L"),      // KEY_F17
        188 => Some("R"),      // KEY_F18
        189 => Some("L2"),     // KEY_F19
        190 => Some("R2"),     // KEY_F20
        98  => Some("SELECT"), // KEY_KPSLASH
        55  => Some("START"),  // KEY_KPASTERISK
        74  => Some("STAR"),   // KEY_KPMINUS
        78  => Some("HOME"),   // KEY_KPPLUS
        72  => Some("UP"),     // KEY_KP8
        80  => Some("DOWN"),   // KEY_KP2
        75  => Some("LEFT"),   // KEY_KP4
        77  => Some("RIGHT"),  // KEY_KP6
        _ => None,
    }
}

pub fn agy_evdev_to_button(code: u16) -> Option<&'static str> {
    match code {
        14 => Some("A"),      // KEY_BACKSPACE
        109 => Some("B"),     // KEY_PAGEDOWN
        104 => Some("X"),     // KEY_PAGEUP
        15 => Some("L"),      // KEY_TAB
        42 => Some("L2"),     // KEY_LEFTSHIFT
        28 => Some("R2"),     // KEY_ENTER
        1  => Some("SELECT"), // KEY_ESC
        57 => Some("STAR"),   // KEY_SPACE
        53 => Some("HOME"),   // KEY_SLASH
        103 => Some("UP"),    // KEY_UP
        108 => Some("DOWN"),  // KEY_DOWN
        105 => Some("LEFT"),  // KEY_LEFT
        106 => Some("RIGHT"), // KEY_RIGHT
        _ => None,
    }
}

/// Resolve physical button name from evdev keycode, supporting Factory, Sentinel, and AGY hardware presets.
pub fn evdev_to_button(code: u16) -> Option<&'static str> {
    factory_evdev_to_button(code)
        .or_else(|| sentinel_evdev_to_button(code))
        .or_else(|| agy_evdev_to_button(code))
}

pub fn identify_hardware_profile_name(hw: &MicroHardwareProfile) -> &'static str {
    for name in &["sentinel", "factory", "agy", "youtube", "anki", "supermemo"] {
        if let Ok(preset) = get_builtin_hardware_preset(name) {
            let mut all_match = true;
            for &btn in PHYSICAL_BUTTONS {
                let s1 = hw.slots.get(btn).copied().unwrap_or([0, 0, 0, 0]);
                let s2 = preset.slots.get(btn).copied().unwrap_or([0, 0, 0, 0]);
                if s1 != s2 {
                    all_match = false;
                    break;
                }
            }
            if all_match {
                return name;
            }
        }
    }
    "custom"
}

pub fn button_to_factory_evdev(btn: &str) -> Option<u16> {
    match btn {
        "A" => Some(34),
        "B" => Some(36),
        "X" => Some(35),
        "Y" => Some(23),
        "L" => Some(37),
        "R" => Some(50),
        "L2" => Some(38),
        "R2" => Some(19),
        "SELECT" => Some(49),
        "START" => Some(24),
        "STAR" => Some(20),
        "HOME" => Some(31),
        "UP" => Some(46),
        "DOWN" => Some(32),
        "LEFT" => Some(18),
        "RIGHT" => Some(33),
        _ => None,
    }
}

pub fn key_name_to_hid(name: &str) -> Result<u8, String> {
    let cleaned = name.trim().to_uppercase();
    if cleaned.is_empty() || cleaned == "NONE" {
        return Ok(0);
    }
    if let Some(hex_str) = cleaned.strip_prefix("0X") {
        if let Ok(val) = u8::from_str_radix(hex_str, 16) {
            return Ok(val);
        }
    }
    if let Some(spec) = key_by_name(&cleaned) {
        return Ok(spec.hid);
    }
    Err(format!("Unknown key name for HID mapping: {:?}", name))
}

pub fn key_name_to_evdev(name: &str) -> Result<u16, String> {
    let cleaned = name.trim().to_uppercase();
    if let Some(spec) = key_by_name(&cleaned) {
        return Ok(spec.evdev);
    }
    for &(media_name, code) in EXTRA_MEDIA_EVDEV {
        if cleaned == media_name {
            return Ok(code);
        }
    }
    if let Ok(num) = cleaned.parse::<u16>() {
        if num > 10 {
            return Ok(num);
        }
    }
    Err(format!("Unknown evdev key name: {:?}", name))
}

pub fn hid_tuple_to_names(usages: &[u8; 4]) -> Vec<String> {
    usages
        .iter()
        .copied()
        .filter(|&u| u != 0)
        .map(hid_to_name)
        .collect()
}

pub fn turn_degrees_to_code(deg_or_code: i32) -> Result<u8, String> {
    match deg_or_code {
        0 => Ok(0),
        90 | 1 => Ok(1),
        180 | 2 => Ok(2),
        270 | 3 => Ok(3),
        _ => Err(format!(
            "Invalid D-pad turn value {} (expected 0, 90, 180, 270 or 0..3)",
            deg_or_code
        )),
    }
}

pub fn turn_code_to_degrees(code: u8) -> i32 {
    match code {
        1 => 90,
        2 => 180,
        3 => 270,
        _ => 0,
    }
}

const DPAD_RING: [&str; 4] = ["UP", "RIGHT", "DOWN", "LEFT"];

pub fn rotate_dpad_button(button: &str, turn_degrees_or_code: i32) -> Result<String, String> {
    let btn = normalize_button_name(button)?;
    let Some(idx) = DPAD_RING.iter().position(|&b| b == btn) else {
        return Ok(btn.to_string());
    };
    let steps = turn_degrees_to_code(turn_degrees_or_code)? as usize;
    Ok(DPAD_RING[(idx + steps) % 4].to_string())
}

/// Compute CRC-16/ARC (reflected polynomial 0xA001, init 0xFFFF) matching `libadvance-lib.so @ 0x12c678`.
pub fn crc16_arc(data: &[u8]) -> u16 {
    if data.is_empty() {
        return 0xFFFF;
    }
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = ((crc >> 1) & 0x7FFF) ^ 0xA001;
            } else {
                crc = (crc >> 1) & 0x7FFF;
            }
        }
    }
    crc
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlePacketHeader {
    pub cmd: u16,
    pub subcmd: u16,
    pub chunk_len: u16,
    pub crc16: u16,
    pub total_len: u32,
    pub offset: u32,
}

pub fn build_ble_packet(
    cmd: u16,
    subcmd: u16,
    total_len: u32,
    offset: u32,
    payload: &[u8],
    header_chunk_len: Option<u16>,
) -> Vec<u8> {
    let c_len = header_chunk_len.unwrap_or(payload.len() as u16);
    let crc = crc16_arc(payload);
    let mut out = Vec::with_capacity(1 + 16 + payload.len());
    out.push(CMD_PACKET_PREFIX);
    out.extend_from_slice(&cmd.to_le_bytes());
    out.extend_from_slice(&subcmd.to_le_bytes());
    out.extend_from_slice(&c_len.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&total_len.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn parse_ble_notification(data: &[u8]) -> Result<(BlePacketHeader, Vec<u8>), String> {
    if data.len() < 16 {
        return Err(format!(
            "BLE notification too short ({} bytes < 16)",
            data.len()
        ));
    }
    let cmd = u16::from_le_bytes([data[0], data[1]]);
    let subcmd = u16::from_le_bytes([data[2], data[3]]);
    let chunk_len = u16::from_le_bytes([data[4], data[5]]);
    let crc16 = u16::from_le_bytes([data[6], data[7]]);
    let total_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let offset = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let end = std::cmp::min(data.len(), 16 + chunk_len as usize);
    let payload = data[16..end].to_vec();
    Ok((
        BlePacketHeader {
            cmd,
            subcmd,
            chunk_len,
            crc16,
            total_len,
            offset,
        },
        payload,
    ))
}

pub fn build_get_platform_packet() -> Vec<u8> {
    build_ble_packet(CMD_GET_PLATFORM, 0x0000, 1, 0, &[0x00], None)
}

pub fn build_read_version_packet() -> Vec<u8> {
    build_ble_packet(CMD_READ_VERSION, 0x0000, 4, VERSION_READ_ADDR, &[0u8; 4], None)
}

pub fn build_read_config_packets() -> Vec<(usize, Vec<u8>)> {
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < RECORD_MICRO_SIZE {
        let chunk_len = std::cmp::min(MAX_BLE_CHUNK_LEN, RECORD_MICRO_SIZE - offset);
        let zeros = vec![0u8; chunk_len];
        let pkt = build_ble_packet(
            CMD_READ_CONFIG,
            0x0000,
            RECORD_MICRO_SIZE as u32,
            offset as u32,
            &zeros,
            None,
        );
        packets.push((offset, pkt));
        offset += MAX_BLE_CHUNK_LEN;
    }
    packets
}

pub fn build_write_config_packets(record_bytes: &[u8]) -> Result<Vec<(usize, Vec<u8>)>, String> {
    if record_bytes.len() != RECORD_MICRO_SIZE {
        return Err(format!(
            "record_micro must be {} bytes, got {}",
            RECORD_MICRO_SIZE,
            record_bytes.len()
        ));
    }
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < RECORD_MICRO_SIZE {
        let end = std::cmp::min(RECORD_MICRO_SIZE, offset + MAX_BLE_CHUNK_LEN);
        let chunk = &record_bytes[offset..end];
        let pkt = build_ble_packet(
            CMD_WRITE_CONFIG,
            0x0000,
            RECORD_MICRO_SIZE as u32,
            offset as u32,
            chunk,
            None,
        );
        packets.push((offset, pkt));
        offset += MAX_BLE_CHUNK_LEN;
    }
    Ok(packets)
}

pub fn build_save_config_packet() -> Vec<u8> {
    build_ble_packet(CMD_SAVE_CONFIG, CMD_SAVE_SUBCMD, 0, 0, &[], None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicroHardwareProfile {
    pub enabled: bool,
    pub turn: u8, // 0 (0°), 1 (90°), 2 (180°), 3 (270°)
    pub slots: HashMap<String, [u8; 4]>,
    pub header_prefix: [u8; 3],
    pub tail_bytes: Vec<u8>,
}

impl MicroHardwareProfile {
    pub fn new(
        enabled: bool,
        turn_deg_or_code: i32,
        slots: HashMap<String, [u8; 4]>,
    ) -> Result<Self, String> {
        let turn = turn_degrees_to_code(turn_deg_or_code)?;
        Ok(Self {
            enabled,
            turn,
            slots,
            header_prefix: [0, 0, 0],
            tail_bytes: vec![0u8; 88],
        })
    }

    pub fn turn_degrees(&self) -> i32 {
        turn_code_to_degrees(self.turn)
    }

    pub fn effective_slots(&self) -> HashMap<String, [u8; 4]> {
        let mut result = factory_button_hid_usages();
        if !self.enabled {
            return result;
        }
        for &slot_name in SLOT_ORDER {
            if let Some(&s) = self.slots.get(slot_name) {
                result.insert(slot_name.to_string(), s);
            }
        }
        result
    }

    pub fn to_bytes(&self, base: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; RECORD_MICRO_SIZE];
        if let Some(b) = base {
            if b.len() != RECORD_MICRO_SIZE {
                return Err(format!(
                    "base buffer must be {} bytes, got {}",
                    RECORD_MICRO_SIZE,
                    b.len()
                ));
            }
            buf.copy_from_slice(b);
        } else {
            buf[0..3].copy_from_slice(&self.header_prefix);
            let tail_len = std::cmp::min(88, self.tail_bytes.len());
            buf[0x5C..0x5C + tail_len].copy_from_slice(&self.tail_bytes[..tail_len]);
        }

        buf[0x03] = self.turn;
        let flag_val: u32 = if self.enabled { FLAG_ENABLE } else { 0 };
        buf[0x04..0x08].copy_from_slice(&flag_val.to_le_bytes());
        buf[0x08..0x0C].copy_from_slice(&flag_val.to_le_bytes());

        for (idx, &slot_name) in SLOT_ORDER.iter().enumerate() {
            let offset = 0x0C + idx * 4;
            let usages = if self.enabled {
                self.slots.get(slot_name).copied().unwrap_or([0, 0, 0, 0])
            } else {
                [0, 0, 0, 0]
            };
            buf[offset..offset + 4].copy_from_slice(&usages);
        }

        Ok(buf)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() != RECORD_MICRO_SIZE {
            return Err(format!(
                "Expected {} bytes for record_micro, got {}",
                RECORD_MICRO_SIZE,
                data.len()
            ));
        }
        let header_prefix = [data[0], data[1], data[2]];
        let turn = if data[0x03] <= 3 { data[0x03] } else { 0 };
        let flag_outer = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let flag_inner = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let enabled = (flag_outer == FLAG_ENABLE) || (flag_inner == FLAG_ENABLE);

        let mut slots = HashMap::new();
        for (idx, &slot_name) in SLOT_ORDER.iter().enumerate() {
            let offset = 0x0C + idx * 4;
            slots.insert(
                slot_name.to_string(),
                [
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ],
            );
        }

        if !enabled {
            slots = factory_button_hid_usages();
        }

        Ok(Self {
            enabled,
            turn,
            slots,
            header_prefix,
            tail_bytes: data[0x5C..RECORD_MICRO_SIZE].to_vec(),
        })
    }

    pub fn from_button_slice(
        buttons: &[(&str, &[&str])],
        turn: i32,
        enabled: bool,
    ) -> Result<Self, String> {
        let mut slots: HashMap<String, [u8; 4]> = SLOT_ORDER
            .iter()
            .map(|&s| (s.to_string(), [0, 0, 0, 0]))
            .collect();

        for &(raw_btn, raw_keys) in buttons {
            let btn = normalize_button_name(raw_btn)?;
            let mut usages = Vec::new();
            for &k in raw_keys {
                let k_trim = k.trim();
                if !k_trim.is_empty() && !k_trim.eq_ignore_ascii_case("NONE") {
                    usages.push(key_name_to_hid(k_trim)?);
                }
            }
            if usages.len() > 4 {
                return Err(format!(
                    "Button {} has {} keys; hardware limit is 4 keys per button",
                    btn,
                    usages.len()
                ));
            }
            let mut padded = [0u8; 4];
            for (i, &u) in usages.iter().enumerate() {
                padded[i] = u;
            }
            slots.insert(btn.to_string(), padded);
        }

        Self::new(enabled, turn, slots)
    }
}

pub fn get_builtin_hardware_preset(name: &str) -> Result<MicroHardwareProfile, String> {
    let key = name.trim().to_lowercase();
    match key.as_str() {
        "factory" | "default" | "reset" => {
            MicroHardwareProfile::new(true, 0, factory_button_hid_usages())
        }
        "anki" => MicroHardwareProfile::from_button_slice(
            &[
                ("A", &["2"]),
                ("B", &["1"]),
                ("X", &["LEFTCTRL", "LEFTSHIFT", "D"]),
                ("Y", &["LEFTCTRL", "LEFTALT", "N"]),
                ("L", &["SPACE"]),
                ("R", &["4"]),
                ("L2", &["L"]),
                ("R2", &["R"]),
                ("SELECT", &["E"]),
                ("START", &["KPASTERISK"]),
                ("STAR", &["T"]),
                ("HOME", &["S"]),
                ("UP", &["LEFTCTRL", "DELETE"]),
                ("DOWN", &["LEFTSHIFT", "2"]),
                ("LEFT", &["LEFTCTRL", "Z"]),
                ("RIGHT", &["KPMINUS"]),
            ],
            0,
            true,
        ),
        "agy" => MicroHardwareProfile::from_button_slice(
            &[
                ("A", &["BACKSPACE"]),
                ("B", &["PAGEDOWN"]),
                ("X", &["PAGEUP"]),
                ("Y", &["LEFTSHIFT", "ENTER"]),
                ("L", &["TAB"]),
                ("R", &["LEFTCTRL", "LEFTSHIFT", "D"]),
                ("L2", &["LEFTSHIFT"]),
                ("R2", &["ENTER"]),
                ("SELECT", &["ESC"]),
                ("START", &["2", "LEFTSHIFT"]),
                ("STAR", &["SPACE"]),
                ("HOME", &["SLASH"]),
                ("UP", &["UP"]),
                ("DOWN", &["DOWN"]),
                ("LEFT", &["LEFT"]),
                ("RIGHT", &["RIGHT"]),
            ],
            0,
            true,
        ),
        "supermemo" => MicroHardwareProfile::from_button_slice(
            &[
                ("A", &["2"]),
                ("B", &["1"]),
                ("X", &["4"]),
                ("Y", &["LEFTALT", "P"]),
                ("L", &["ENTER"]),
                ("R", &["M"]),
                ("L2", &["L"]),
                ("R2", &["R"]),
                ("SELECT", &["N"]),
                ("START", &["O"]),
                ("STAR", &["T"]),
                ("HOME", &["S"]),
                ("UP", &["DELETE"]),
                ("DOWN", &["LEFTSHIFT", "LEFTCTRL", "X"]),
                ("LEFT", &["LEFTALT", "C"]),
                ("RIGHT", &["LEFTALT", "G"]),
            ],
            0,
            true,
        ),
        "youtube" => MicroHardwareProfile::from_button_slice(
            &[
                ("A", &["SPACE"]),
                ("B", &["M"]),
                ("X", &["F"]),
                ("Y", &["C"]),
                ("L", &["J"]),
                ("R", &["L"]),
                ("L2", &["LEFTSHIFT", "COMMA"]),
                ("R2", &["LEFTSHIFT", "DOT"]),
                ("SELECT", &["ESC"]),
                ("START", &["LEFTSHIFT", "N"]),
                ("STAR", &["T"]),
                ("HOME", &["I"]),
                ("UP", &["UP"]),
                ("DOWN", &["DOWN"]),
                ("LEFT", &["LEFT"]),
                ("RIGHT", &["RIGHT"]),
            ],
            0,
            true,
        ),
        "sentinel" | "host" => {
            let mut slots: HashMap<String, [u8; 4]> = SLOT_ORDER
                .iter()
                .map(|&s| (s.to_string(), [0, 0, 0, 0]))
                .collect();
            for &(btn, sentinel_key) in SENTINEL_BUTTON_MAP {
                if let Ok(hid) = key_name_to_hid(sentinel_key) {
                    slots.insert(btn.to_string(), [hid, 0, 0, 0]);
                }
            }
            MicroHardwareProfile::new(true, 0, slots)
        }
        _ => Err(format!(
            "Unknown built-in preset {:?} (valid: 'agy', 'youtube', 'factory', 'anki', 'supermemo', 'sentinel')",
            name
        )),
    }
}
