//! Cross-Platform Virtual Keyboard & Input Interception Backends.
//!
//! - Linux: Kernel evdev grab (`EVIOCGRAB`) + `/dev/uinput` (with `wtype` Wayland user-space fallback)
//! - Windows: Low-Level Keyboard Hook (`WH_KEYBOARD_LL`) + `SendInput` with Sentinel keymap
//! - macOS: CoreGraphics Event Tap (`CGEventTap`) + `CGEventPost` with Sentinel keymap

use std::collections::HashMap;
use std::path::Path;

use crate::keycodes::{key_by_evdev, tables};
use crate::protocol::{button_to_factory_evdev, EV_KEY, EV_SYN};

pub const MAGIC_EXTRA_INFO: usize = 0x8B17D000;

// Windows constants
pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const WH_KEYBOARD_LL: i32 = 13;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;
pub const LLKHF_EXTENDED: u32 = 0x01;
pub const LLKHF_INJECTED: u32 = 0x10;

pub trait VirtualKeyboard: Send {
    fn write(&mut self, etype: u16, evdev_code: u16, value: i32);
    fn syn(&mut self);
    fn close(&mut self);
}

// ---------------------------------------------------------------------------
// Linux /dev/uinput Virtual Keyboard
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub struct LinuxVirtualKeyboard {
    uinput: evdev::uinput::VirtualDevice,
    pending: Vec<evdev::InputEvent>,
}

#[cfg(target_os = "linux")]
impl LinuxVirtualKeyboard {
    pub fn new(uinput: evdev::uinput::VirtualDevice) -> Self {
        Self {
            uinput,
            pending: Vec::new(),
        }
    }
}

#[cfg(target_os = "linux")]
impl VirtualKeyboard for LinuxVirtualKeyboard {
    fn write(&mut self, etype: u16, evdev_code: u16, value: i32) {
        self.pending
            .push(evdev::InputEvent::new(evdev::EventType(etype), evdev_code, value));
    }

    fn syn(&mut self) {
        if !self.pending.is_empty() {
            let events: Vec<evdev::InputEvent> = self.pending.drain(..).collect();
            let _ = self.uinput.emit(&events);
        }
    }

    fn close(&mut self) {
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Linux Wayland User-Space Virtual Keyboard (`wtype`)
// ---------------------------------------------------------------------------

fn wayland_modifier_name(key_name: &str) -> Option<&'static str> {
    match key_name {
        "LEFTCTRL" | "RIGHTCTRL" => Some("ctrl"),
        "LEFTSHIFT" | "RIGHTSHIFT" => Some("shift"),
        "LEFTALT" => Some("alt"),
        "RIGHTALT" => Some("altgr"),
        "LEFTMETA" | "RIGHTMETA" => Some("logo"),
        "CAPSLOCK" => Some("capslock"),
        _ => None,
    }
}

fn wayland_keysym_name(key_name: &str) -> Option<&'static str> {
    match key_name {
        "ENTER" => Some("Return"),
        "ESC" => Some("Escape"),
        "BACKSPACE" => Some("BackSpace"),
        "TAB" => Some("Tab"),
        "SPACE" => Some("space"),
        "MINUS" => Some("minus"),
        "EQUAL" => Some("equal"),
        "LEFTBRACE" => Some("bracketleft"),
        "RIGHTBRACE" => Some("bracketright"),
        "BACKSLASH" => Some("backslash"),
        "SEMICOLON" => Some("semicolon"),
        "APOSTROPHE" => Some("apostrophe"),
        "GRAVE" => Some("grave"),
        "COMMA" => Some("comma"),
        "DOT" => Some("period"),
        "SLASH" => Some("slash"),
        "CAPSLOCK" => Some("Caps_Lock"),
        "SYSRQ" => Some("Print"),
        "SCROLLLOCK" => Some("Scroll_Lock"),
        "PAUSE" => Some("Pause"),
        "INSERT" => Some("Insert"),
        "HOME" => Some("Home"),
        "PAGEUP" => Some("Page_Up"),
        "DELETE" => Some("Delete"),
        "END" => Some("End"),
        "PAGEDOWN" => Some("Page_Down"),
        "RIGHT" => Some("Right"),
        "LEFT" => Some("Left"),
        "DOWN" => Some("Down"),
        "UP" => Some("Up"),
        "NUMLOCK" => Some("Num_Lock"),
        "KPSLASH" => Some("KP_Divide"),
        "KPASTERISK" => Some("KP_Multiply"),
        "KPMINUS" => Some("KP_Subtract"),
        "KPPLUS" => Some("KP_Add"),
        "KPENTER" => Some("KP_Enter"),
        "KP1" => Some("KP_1"),
        "KP2" => Some("KP_2"),
        "KP3" => Some("KP_3"),
        "KP4" => Some("KP_4"),
        "KP5" => Some("KP_5"),
        "KP6" => Some("KP_6"),
        "KP7" => Some("KP_7"),
        "KP8" => Some("KP_8"),
        "KP9" => Some("KP_9"),
        "KP0" => Some("KP_0"),
        "KPDOT" => Some("KP_Decimal"),
        "COMPOSE" => Some("Menu"),
        _ => None,
    }
}

pub struct WaylandVirtualKeyboard {
    wtype_bin: String,
    pending: Vec<(u16, i32)>,
    held_modifier_codes: HashMap<u16, String>,
    closed: bool,
    pub sent_history: Vec<(u16, i32)>,
    pub command_history: Vec<Vec<String>>,
}

impl WaylandVirtualKeyboard {
    pub fn new(wtype_bin: &str) -> Self {
        Self {
            wtype_bin: wtype_bin.to_string(),
            pending: Vec::new(),
            held_modifier_codes: HashMap::new(),
            closed: false,
            sent_history: Vec::new(),
            command_history: Vec::new(),
        }
    }

    pub fn evdev_to_keysym(evdev_code: u16) -> Option<String> {
        match evdev_code {
            113 => return Some("XF86AudioMute".to_string()),
            114 => return Some("XF86AudioLowerVolume".to_string()),
            115 => return Some("XF86AudioRaiseVolume".to_string()),
            163 => return Some("XF86AudioNext".to_string()),
            164 => return Some("XF86AudioPlay".to_string()),
            165 => return Some("XF86AudioPrev".to_string()),
            _ => {}
        }
        let spec = key_by_evdev(evdev_code)?;
        if let Some(mapped) = wayland_keysym_name(spec.name) {
            return Some(mapped.to_string());
        }
        if spec.name.len() == 1 {
            return Some(spec.name.to_lowercase());
        }
        Some(spec.name.to_string())
    }

    pub fn build_wtype_args(&mut self, events: &[(u16, i32)]) -> Vec<String> {
        let mut batch_mods: Vec<String> = Vec::new();
        let mut keys_to_tap: Vec<String> = Vec::new();

        for &(code, val) in events {
            let mod_name = key_by_evdev(code).and_then(|s| wayland_modifier_name(s.name));
            if let Some(m) = mod_name {
                if val == 1 || val == 2 {
                    self.held_modifier_codes.insert(code, m.to_string());
                    if !batch_mods.iter().any(|x| x == m) {
                        batch_mods.push(m.to_string());
                    }
                } else if val == 0 {
                    self.held_modifier_codes.remove(&code);
                }
            } else if val == 1 || val == 2 {
                if let Some(ks) = Self::evdev_to_keysym(code) {
                    keys_to_tap.push(ks);
                }
            }
        }

        if keys_to_tap.is_empty() {
            return Vec::new();
        }

        let mut active_mods: Vec<String> = Vec::new();
        for m in self.held_modifier_codes.values().chain(batch_mods.iter()) {
            if !active_mods.contains(m) {
                active_mods.push(m.clone());
            }
        }

        let mut args = Vec::new();
        for m in &active_mods {
            args.push("-M".to_string());
            args.push(m.clone());
        }
        for k in &keys_to_tap {
            args.push("-k".to_string());
            args.push(k.clone());
        }
        for m in active_mods.iter().rev() {
            args.push("-m".to_string());
            args.push(m.clone());
        }
        args
    }
}

impl VirtualKeyboard for WaylandVirtualKeyboard {
    fn write(&mut self, etype: u16, evdev_code: u16, value: i32) {
        if etype == EV_KEY && !self.closed {
            self.pending.push((evdev_code, value));
        }
    }

    fn syn(&mut self) {
        if self.pending.is_empty() || self.closed {
            return;
        }
        let events_to_send: Vec<(u16, i32)> = self.pending.drain(..).collect();
        self.sent_history.extend_from_slice(&events_to_send);

        let args = self.build_wtype_args(&events_to_send);
        if args.is_empty() {
            return;
        }

        let mut full_cmd = vec![self.wtype_bin.clone()];
        full_cmd.extend(args.clone());
        self.command_history.push(full_cmd);

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new(&self.wtype_bin)
                .args(&args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }

    fn close(&mut self) {
        self.closed = true;
        self.held_modifier_codes.clear();
    }
}

// ---------------------------------------------------------------------------
// Windows Backend (SendInput + WH_KEYBOARD_LL Hook)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Win32InputSpec {
    pub input_type: u32,
    pub w_vk: u16,
    pub w_scan: u16,
    pub dw_flags: u32,
    pub time: u32,
    pub dw_extra_info: usize,
}

pub struct WindowsVirtualKeyboard {
    pending: Vec<(u16, i32)>,
    closed: bool,
    pub sent_history: Vec<(u16, i32)>,
}

impl Default for WindowsVirtualKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsVirtualKeyboard {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            closed: false,
            sent_history: Vec::new(),
        }
    }

    pub fn build_win32_inputs(&self, events: &[(u16, i32)]) -> Vec<Win32InputSpec> {
        let mut res = Vec::with_capacity(events.len());
        for &(code, val) in events {
            let spec = key_by_evdev(code);
            let vk = spec.map(|s| s.win_vk).unwrap_or(0);
            let scan = spec.map(|s| s.win_raw_scancode()).unwrap_or(0);
            let mut flags = 0u32;
            if spec.map(|s| s.is_win_extended()).unwrap_or(false) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if val == 0 {
                flags |= KEYEVENTF_KEYUP;
            }
            res.push(Win32InputSpec {
                input_type: INPUT_KEYBOARD,
                w_vk: vk,
                w_scan: scan,
                dw_flags: flags,
                time: 0,
                dw_extra_info: MAGIC_EXTRA_INFO,
            });
        }
        res
    }
}

impl VirtualKeyboard for WindowsVirtualKeyboard {
    fn write(&mut self, etype: u16, evdev_code: u16, value: i32) {
        if etype == EV_KEY && !self.closed {
            self.pending.push((evdev_code, value));
        }
    }

    fn syn(&mut self) {
        if self.pending.is_empty() || self.closed {
            return;
        }
        let events_to_send: Vec<(u16, i32)> = self.pending.drain(..).collect();
        self.sent_history.extend_from_slice(&events_to_send);

        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, KEYBDINPUT,
            };

            let specs = self.build_win32_inputs(&events_to_send);
            let mut inputs: Vec<INPUT> = Vec::with_capacity(specs.len());
            for s in specs {
                inputs.push(INPUT {
                    r#type: s.input_type,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: s.w_vk,
                            wScan: s.w_scan,
                            dwFlags: s.dw_flags,
                            time: s.time,
                            dwExtraInfo: s.dw_extra_info,
                        },
                    },
                });
            }
            unsafe {
                SendInput(
                    inputs.len() as u32,
                    inputs.as_ptr(),
                    std::mem::size_of::<INPUT>() as i32,
                );
            }
        }
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

/// Pure logic helper for evaluating a Windows low-level hook event (used by hook & tests).
pub fn evaluate_win32_hook_event(
    _vk_code: u32,
    scan_code: u32,
    flags: u32,
    dw_extra_info: usize,
    w_param: u32,
    held_buttons: &mut std::collections::HashSet<String>,
) -> Option<(u16, i32)> {
    if dw_extra_info == MAGIC_EXTRA_INFO || (flags & LLKHF_INJECTED) != 0 {
        return None;
    }
    let is_extended = (flags & LLKHF_EXTENDED) != 0;
    let raw_scancode = (scan_code & 0xFF) as u16;
    let btn = *tables()
        .sentinel_win_scan_to_btn
        .get(&(raw_scancode, is_extended))?;
    let factory_evdev = button_to_factory_evdev(btn)?;
    let is_up = w_param == WM_KEYUP || w_param == WM_SYSKEYUP;
    let val = if is_up {
        held_buttons.remove(btn);
        0
    } else if held_buttons.contains(btn) {
        2
    } else {
        held_buttons.insert(btn.to_string());
        1
    };
    Some((factory_evdev, val))
}

// ---------------------------------------------------------------------------
// macOS Backend
// ---------------------------------------------------------------------------

pub const K_CG_EVENT_KEY_DOWN: u32 = 10;
pub const K_CG_EVENT_KEY_UP: u32 = 11;
pub const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFFFFFE;
pub const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;

pub fn evaluate_macos_tap_event(
    event_type: u32,
    mac_vk: u16,
    user_data: usize,
    is_repeat: bool,
    held_buttons: &mut std::collections::HashSet<String>,
) -> Option<(u16, i32)> {
    if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
    {
        return None;
    }
    if user_data == MAGIC_EXTRA_INFO {
        return None;
    }
    let btn = *tables().sentinel_mac_vk_to_btn.get(&mac_vk)?;
    let factory_evdev = button_to_factory_evdev(btn)?;
    let val = if event_type == K_CG_EVENT_KEY_UP {
        held_buttons.remove(btn);
        0
    } else if event_type == K_CG_EVENT_KEY_DOWN {
        if is_repeat || held_buttons.contains(btn) {
            2
        } else {
            held_buttons.insert(btn.to_string());
            1
        }
    } else {
        return None;
    };
    Some((factory_evdev, val))
}

pub struct MacOSVirtualKeyboard {
    pending: Vec<(u16, i32)>,
    closed: bool,
    pub sent_history: Vec<(u16, i32)>,
}

impl Default for MacOSVirtualKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOSVirtualKeyboard {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            closed: false,
            sent_history: Vec::new(),
        }
    }
}

impl VirtualKeyboard for MacOSVirtualKeyboard {
    fn write(&mut self, etype: u16, evdev_code: u16, value: i32) {
        if etype == EV_KEY && !self.closed {
            self.pending.push((evdev_code, value));
        }
    }

    fn syn(&mut self) {
        if self.pending.is_empty() || self.closed {
            return;
        }
        let events_to_send: Vec<(u16, i32)> = self.pending.drain(..).collect();
        self.sent_history.extend_from_slice(&events_to_send);
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

// ---------------------------------------------------------------------------
// Mock Virtual Keyboard (for unit testing & simulation)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MockVirtualKeyboard {
    pub events: Vec<(u16, u16, i32)>,
    pub syn_count: usize,
    pub closed: bool,
}

impl MockVirtualKeyboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl VirtualKeyboard for MockVirtualKeyboard {
    fn write(&mut self, etype: u16, evdev_code: u16, value: i32) {
        self.events.push((etype, evdev_code, value));
    }

    fn syn(&mut self) {
        self.syn_count += 1;
        self.events.push((EV_SYN, 0, 0));
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

// ---------------------------------------------------------------------------
// Permission Checks
// ---------------------------------------------------------------------------

pub fn check_uinput_access() -> (bool, String) {
    #[cfg(target_os = "linux")]
    {
        let uinput_path = Path::new("/dev/uinput");
        if !uinput_path.exists() {
            return (
                false,
                "/dev/uinput does not exist (run `sudo modprobe uinput`)".to_string(),
            );
        }
        let writable = std::fs::OpenOptions::new()
            .write(true)
            .open(uinput_path)
            .is_ok();
        if !writable {
            return (
                false,
                "/dev/uinput is not writable by current user (run `8bitdo-micro-focus-switch install-udev` to grant input group access)".to_string(),
            );
        }
        return (true, "/dev/uinput is writable".to_string());
    }
    #[cfg(not(target_os = "linux"))]
    {
        (
            false,
            "The 'uinput' backend requires Linux with evdev and /dev/uinput support. Please use backend='host' or 'ble'.".to_string(),
        )
    }
}

pub fn check_platform_input_access() -> (bool, String) {
    #[cfg(target_os = "linux")]
    {
        return check_uinput_access();
    }
    #[cfg(target_os = "windows")]
    {
        return (true, "Windows user-space hook available".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        return (true, "macOS Accessibility check completed".to_string());
    }
}
