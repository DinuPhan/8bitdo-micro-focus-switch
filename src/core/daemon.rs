//! Focus-aware input remapper daemon for the 8BitDo Micro across Linux (Hyprland + evdev/uinput), Windows, and macOS.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::config::{get_home_dir, load_config, AppConfig, ButtonAction, ComboSet, ProfileConfig};
use crate::input_backend::{check_uinput_access, VirtualKeyboard, WaylandVirtualKeyboard};
use crate::protocol::{
    evdev_to_button, rotate_dpad_button, EV_KEY, SUPPORTED_KEYBOARD_DEVICE_IDS,
    SUPPORTED_KEYBOARD_NAME_SUBSTRINGS,
};
use crate::window_watcher::{spawn_focus_watcher, ActiveWindow};

pub const VIRTUAL_DEVICE_NAME: &str = "8BitDo Micro Focus-Switch Virtual Keyboard";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerDeviceInfo {
    pub path: String,
    pub name: String,
    pub model: String,
    pub vendor: String,
    pub product: String,
    pub uniq: String,
}

pub fn identify_controller_name_and_ids(vendor: u16, product: u16, name: &str) -> Option<String> {
    if name == VIRTUAL_DEVICE_NAME {
        return None;
    }
    for &((vid, pid), model) in SUPPORTED_KEYBOARD_DEVICE_IDS {
        if vendor == vid && product == pid {
            return Some(model.to_string());
        }
    }
    let lower = name.to_lowercase();
    for &(substr, model_name) in SUPPORTED_KEYBOARD_NAME_SUBSTRINGS {
        if lower.contains(substr) {
            return Some(model_name.to_string());
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub fn is_physical_evdev_device(path: &Path, dev: &evdev::Device) -> bool {
    let dev_name = dev.name().unwrap_or("");
    if dev_name == VIRTUAL_DEVICE_NAME || dev_name.contains("Virtual Keyboard") {
        return false;
    }
    let has_phys = dev
        .physical_path()
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false);
    if !has_phys {
        return false;
    }
    if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
        let sysfs_device_link = PathBuf::from(format!("/sys/class/input/{}/device", fname));
        match fs::canonicalize(&sysfs_device_link) {
            Ok(canon) => {
                if canon.to_string_lossy().starts_with("/sys/devices/virtual/input/") {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

pub fn find_micro_input_devices_info() -> Vec<ControllerDeviceInfo> {
    #[cfg(target_os = "linux")]
    {
        let mut matched = Vec::new();
        for (path, dev) in evdev::enumerate() {
            if !is_physical_evdev_device(&path, &dev) {
                continue;
            }
            let dev_name = dev.name().unwrap_or("");
            let id = dev.input_id();
            if let Some(model) = identify_controller_name_and_ids(id.vendor(), id.product(), dev_name)
            {
                if dev.supported_keys().is_some() {
                    matched.push(ControllerDeviceInfo {
                        path: path.to_string_lossy().to_string(),
                        name: dev_name.to_string(),
                        model,
                        vendor: format!("0x{:04x}", id.vendor()),
                        product: format!("0x{:04x}", id.product()),
                        uniq: dev.unique_name().unwrap_or("").to_string(),
                    });
                }
            }
        }
        matched.sort_by(|a, b| a.path.cmp(&b.path));
        return matched;
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::{
            GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICENAME,
            RIM_TYPEHID, RIM_TYPEKEYBOARD,
        };

        let mut count = 0u32;
        let list_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        let ret = unsafe { GetRawInputDeviceList(std::ptr::null_mut(), &mut count, list_size) };
        if ret == u32::MAX || count == 0 {
            return Vec::new();
        }

        let mut devices = vec![
            RAWINPUTDEVICELIST {
                hDevice: std::ptr::null_mut(),
                dwType: 0,
            };
            count as usize
        ];

        let actual = unsafe { GetRawInputDeviceList(devices.as_mut_ptr(), &mut count, list_size) };
        if actual == u32::MAX {
            return Vec::new();
        }

        let mut matched = Vec::new();
        for dev in devices.iter().take(actual as usize) {
            if dev.dwType != RIM_TYPEKEYBOARD && dev.dwType != RIM_TYPEHID {
                continue;
            }

            let mut name_len = 0u32;
            unsafe {
                GetRawInputDeviceInfoW(
                    dev.hDevice,
                    RIDI_DEVICENAME,
                    std::ptr::null_mut(),
                    &mut name_len,
                );
            }
            if name_len == 0 {
                continue;
            }

            let mut name_buf = vec![0u16; name_len as usize];
            let res = unsafe {
                GetRawInputDeviceInfoW(
                    dev.hDevice,
                    RIDI_DEVICENAME,
                    name_buf.as_mut_ptr() as *mut _,
                    &mut name_len,
                )
            };
            if res == u32::MAX || res == 0 {
                continue;
            }

            let device_path = String::from_utf16_lossy(&name_buf)
                .trim_matches('\0')
                .to_string();

            let path_upper = device_path.to_uppercase();
            // Check 8BitDo Vendor ID: 0x2DC8 (or known controller models)
            if path_upper.contains("VID_2DC8")
                || path_upper.contains("2DC8:9021")
                || path_upper.contains("8BITDO")
            {
                let model = if path_upper.contains("PID_9021") || path_upper.contains("9021") {
                    "8BitDo Micro (Keyboard mode)".to_string()
                } else if path_upper.contains("PID_9020") || path_upper.contains("9020") {
                    "8BitDo Zero 2 (Keyboard mode)".to_string()
                } else {
                    "8BitDo Controller".to_string()
                };

                let prod = if path_upper.contains("PID_9021") {
                    "0x9021".to_string()
                } else if path_upper.contains("PID_9020") {
                    "0x9020".to_string()
                } else {
                    "Unknown".to_string()
                };

                matched.push(ControllerDeviceInfo {
                    path: device_path,
                    name: "8BitDo Micro".to_string(),
                    model,
                    vendor: "0x2dc8".to_string(),
                    product: prod,
                    uniq: String::new(),
                });
            }
        }
        return matched;
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

pub fn check_linux_input_permissions_diagnostic() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/bus/input/devices") {
            for block in content.split("\n\n") {
                let lower = block.to_lowercase();
                if lower.contains("8bitdo") || lower.contains("2dc8") {
                    for line in block.lines() {
                        if line.starts_with("H: Handlers=") {
                            for token in line.split_whitespace() {
                                if token.starts_with("event") {
                                    let dev_path = format!("/dev/input/{}", token);
                                    if let Err(e) = std::fs::File::open(&dev_path) {
                                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                                            return Some(format!(
                                                "Controller is connected at {} but cannot be opened (Permission denied).\nTo fix, grant permissions via:\n  sudo usermod -aG input $USER\n  or run: 8bitdo-micro-focus-switch setup",
                                                dev_path
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

async fn normalize_controller_to_sentinel_if_needed(
    ble_address: Option<&str>,
    backup_dir: Option<&Path>,
) {
    match crate::ble::ensure_sentinel_baseline(ble_address, Duration::from_secs(4), backup_dir)
        .await
    {
        Ok(report) => {
            if let Some(ref bak) = report.backed_up_to {
                println!(
                    "[8bitdo-micro] Backed up previous hardware profile ('{}', turn={}°) to {}",
                    report.previous_profile,
                    report.previous_turn_degrees,
                    bak.display()
                );
            }
            if report.flashed_sentinel {
                println!(
                    "[8bitdo-micro] Normalized controller ({}) hardware profile from '{}' (turn={}°) -> 'sentinel' (F13..F20 + Numpad baseline)",
                    report.address, report.previous_profile, report.previous_turn_degrees
                );
            } else {
                println!(
                    "[8bitdo-micro] BLE verified controller ({}) hardware profile is already 'sentinel' (turn=0°)",
                    report.address
                );
            }
        }
        Err(_) => {
            // Controller may be 8BitDo Zero 2 (no BLE GATT) or BLE advertisement temporarily asleep.
        }
    }
}

fn emit_chord_down(ui: &mut dyn VirtualKeyboard, codes: &[u16]) {
    for &c in codes {
        ui.write(EV_KEY, c, 1);
    }
    ui.syn();
}

fn emit_chord_up(ui: &mut dyn VirtualKeyboard, codes: &[u16]) {
    for &c in codes.iter().rev() {
        ui.write(EV_KEY, c, 0);
    }
    ui.syn();
}

#[derive(Debug, Clone)]
pub struct PendingButton {
    pub keycode: u16,
    pub button_name: String,
    pub action: Option<ButtonAction>,
    pub pressed_at: std::time::Instant,
}

pub struct FocusSwitchDaemon {
    pub config: AppConfig,
    pub config_path: Option<PathBuf>,
    pub backend: String,
    pub ble_address: Option<String>,
    pub active_window: ActiveWindow,
    pub active_profile: ProfileConfig,
    pub held_chords: HashMap<u16, Vec<u16>>,
    pub pending_buttons: HashMap<u16, PendingButton>,
    pub active_combos: HashMap<ComboSet, ButtonAction>,
    pub held_combos: HashMap<u16, ComboSet>,
    pub physical_held_buttons: HashMap<u16, String>,
    pub event_seq: u64,
    pub keypress_event_path: Option<PathBuf>,
    pub ble_dirty: bool,
    pub capture_factory_keys: bool,
}

pub fn resolve_keypress_event_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let trimmed = runtime_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed)
                .join("8bitdo-micro")
                .join("last_keypress.json");
        }
    }
    get_home_dir()
        .join(".cache")
        .join("8bitdo-micro")
        .join("last_keypress.json")
}

impl FocusSwitchDaemon {
    pub fn new(
        config: AppConfig,
        config_path: Option<PathBuf>,
        backend: Option<String>,
        ble_address: Option<String>,
    ) -> Self {
        let default_backend = if cfg!(target_os = "linux") {
            "uinput"
        } else {
            "host"
        };
        let mut resolved_backend = backend
            .unwrap_or_else(|| {
                if config.backend.is_empty() {
                    default_backend.to_string()
                } else {
                    config.backend.clone()
                }
            })
            .to_lowercase();
        if resolved_backend == "uinput" && !cfg!(target_os = "linux") {
            resolved_backend = "host".to_string();
        }

        let capture_factory_keys = config.capture_factory_keys;
        let active_profile = config.resolve_profile_for_window("", "");
        let keypress_event_path = if config_path.is_some() {
            Some(resolve_keypress_event_path())
        } else {
            None
        };

        let daemon = Self {
            config,
            config_path,
            backend: resolved_backend,
            ble_address,
            active_window: ActiveWindow::default(),
            active_profile,
            held_chords: HashMap::new(),
            pending_buttons: HashMap::new(),
            active_combos: HashMap::new(),
            held_combos: HashMap::new(),
            physical_held_buttons: HashMap::new(),
            event_seq: 0,
            keypress_event_path,
            ble_dirty: true,
            capture_factory_keys,
        };
        daemon.ensure_keypress_event_file();
        daemon
    }

    pub fn resolve_button_name(&self, keycode: u16) -> Option<&'static str> {
        evdev_to_button(keycode)
    }

    pub fn ensure_keypress_event_file(&self) {
        let Some(ref p) = self.keypress_event_path else {
            return;
        };
        if let Some(parent) = p.parent() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::DirBuilder::new().recursive(true).mode(0o700).create(parent);
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            }
            #[cfg(not(unix))]
            {
                let _ = fs::create_dir_all(parent);
            }
        }
        if !p.exists() {
            #[cfg(unix)]
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                use std::os::unix::fs::PermissionsExt;
                if let Ok(mut f) = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(p)
                {
                    let _ = f.write_all(b"{\"seq\": 0}\n");
                    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
                }
            }
            #[cfg(not(unix))]
            {
                let _ = fs::write(p, "{\"seq\": 0}\n");
            }
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
            }
        }
    }

    fn write_event_file_atomic(&self, target_path: &Path, content: &[u8]) {
        let parent = match target_path.parent() {
            Some(p) => p,
            None => return,
        };
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::DirBuilderExt;
            use std::os::unix::fs::OpenOptionsExt;
            use std::os::unix::fs::PermissionsExt;

            let _ = fs::DirBuilder::new().recursive(true).mode(0o700).create(parent);
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));

            let pid = std::process::id();
            let tmp_path = parent.join(format!(".last_keypress.json.tmp.{}", pid));
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true).mode(0o600);
            if let Ok(mut file) = opts.open(&tmp_path) {
                let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
                if file.write_all(content).is_ok() && file.flush().is_ok() {
                    drop(file);
                    let _ = fs::rename(&tmp_path, target_path);
                    return;
                }
            }
            let _ = fs::remove_file(&tmp_path);
        }
        #[cfg(not(unix))]
        {
            let _ = fs::create_dir_all(parent);
            let pid = std::process::id();
            let tmp_path = parent.join(format!(".last_keypress.json.tmp.{}", pid));
            if fs::write(&tmp_path, content).is_ok() {
                let _ = fs::rename(&tmp_path, target_path);
            }
        }
    }

    pub fn order_combo_buttons(combo_set: &ComboSet) -> Vec<String> {
        let priority = |b: &str| -> u8 {
            match b {
                "L2" => 0,
                "R2" => 1,
                "L" => 2,
                "R" => 3,
                "SELECT" => 4,
                "START" => 5,
                "STAR" => 6,
                "HOME" => 7,
                "UP" => 8,
                "DOWN" => 9,
                "LEFT" => 10,
                "RIGHT" => 11,
                "X" => 12,
                "Y" => 13,
                "A" => 14,
                "B" => 15,
                _ => 99,
            }
        };
        let mut list: Vec<String> = combo_set.iter().cloned().collect();
        list.sort_by(|a, b| (priority(a), a).cmp(&(priority(b), b)));
        list
    }

    fn record_key_event(&mut self, buttons: &[String], action: &ButtonAction, is_combo: bool) {
        if !self.config.emit_osd_events {
            return;
        }
        let Some(ref path) = self.keypress_event_path else {
            return;
        };
        if action.exec_cmd.is_none() && action.evdev_codes.is_empty() {
            return;
        }
        self.event_seq += 1;
        let clean_buttons: Vec<String> = buttons
            .iter()
            .filter(|b| !b.is_empty())
            .cloned()
            .collect();
        let raw_key = if let Some(ref ex) = action.exec_cmd {
            format!("exec:{}", ex)
        } else {
            action.keys.join("+")
        };
        let is_global_layer = is_combo && clean_buttons.iter().any(|b| b == "L2");

        let payload = serde_json::json!({
            "seq": self.event_seq,
            "button": clean_buttons.join("+"),
            "buttons": clean_buttons,
            "key": raw_key,
            "description": action.description.as_deref().unwrap_or(""),
            "profile": self.active_profile.name,
            "turn": self.active_profile.turn,
            "is_combo": is_combo,
            "is_global_layer": is_global_layer,
        });
        if let Ok(mut s) = serde_json::to_string(&payload) {
            s.push('\n');
            self.write_event_file_atomic(path, s.as_bytes());
        }
    }

    pub fn reload_config(&mut self) {
        let Some(ref path) = self.config_path else {
            return;
        };
        if let Ok(new_cfg) = load_config(Some(path)) {
            self.capture_factory_keys = new_cfg.capture_factory_keys;
            self.config = new_cfg;
            let new_prof = self.config.resolve_profile_for_window(
                &self.active_window.window_class,
                &self.active_window.window_title,
            );
            self.active_profile = new_prof;
            self.ble_dirty = true;
        }
    }

    pub fn on_focus_change(&mut self, window: ActiveWindow) {
        let prev_profile = self.active_profile.name.clone();
        self.active_window = window.clone();
        let new_profile = self
            .config
            .resolve_profile_for_window(&window.window_class, &window.window_title);
        if new_profile.name != prev_profile {
            self.pending_buttons.clear();
            self.active_profile = new_profile;
            self.ble_dirty = true;
            println!(
                "[8bitdo-micro] Focus -> class='{}' title='{}' | Switching profile: {} -> {} (turn={}°)",
                window.window_class,
                window.window_title,
                prev_profile,
                self.active_profile.name,
                self.active_profile.turn
            );
        }
    }

    pub fn translate_button_action(&self, physical_button: &str) -> Option<ButtonAction> {
        let logical_btn = rotate_dpad_button(physical_button, self.active_profile.turn).ok()?;
        self.active_profile.buttons.get(&logical_btn).cloned()
    }

    fn spawn_exec(&self, cmd: &str) {
        let cmd_str = cmd.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut env_path = std::env::var("PATH").unwrap_or_default();
                let home_local = get_home_dir().join(".local").join("bin");
                for extra in ["/usr/share/omarchy/bin", home_local.to_str().unwrap_or("")] {
                    if !extra.is_empty() && Path::new(extra).is_dir() && !env_path.contains(extra) {
                        env_path = format!("{}:{}", extra, env_path);
                    }
                }

                #[cfg(target_os = "windows")]
                let mut child = tokio::process::Command::new("cmd");
                #[cfg(target_os = "windows")]
                child.args(["/C", &cmd_str]);

                #[cfg(not(target_os = "windows"))]
                let mut child = tokio::process::Command::new("sh");
                #[cfg(not(target_os = "windows"))]
                child.args(["-c", &cmd_str]);

                child.env("PATH", env_path);
                if std::env::var_os("OMARCHY_PATH").is_none()
                    && Path::new("/usr/share/omarchy").is_dir()
                {
                    child.env("OMARCHY_PATH", "/usr/share/omarchy");
                }
                let _ = child.output().await;
            });
        }
    }

    fn emit_solo_down(
        &mut self,
        ui: &mut dyn VirtualKeyboard,
        keycode: u16,
        action: &ButtonAction,
        button_name: &str,
    ) {
        let resolved_name = if button_name.is_empty() {
            self.resolve_button_name(keycode)
                .and_then(|raw| rotate_dpad_button(raw, self.active_profile.turn).ok())
                .unwrap_or_default()
        } else {
            button_name.to_string()
        };
        if !resolved_name.is_empty() {
            self.record_key_event(&[resolved_name], action, false);
        }
        if let Some(ref ex) = action.exec_cmd {
            self.spawn_exec(ex);
        }
        if !action.evdev_codes.is_empty() {
            self.held_chords.insert(keycode, action.evdev_codes.clone());
            emit_chord_down(ui, &action.evdev_codes);
        }
    }

    pub fn flush_pending_button(&mut self, ui: &mut dyn VirtualKeyboard, keycode: u16) {
        let Some(pending) = self.pending_buttons.remove(&keycode) else {
            return;
        };
        if !self.physical_held_buttons.contains_key(&keycode) {
            return;
        }
        if let Some(ref action) = pending.action {
            self.emit_solo_down(ui, pending.keycode, action, &pending.button_name);
        }
    }

    pub fn flush_expired_pending_buttons(&mut self, ui: &mut dyn VirtualKeyboard) {
        let timeout = Duration::from_millis(self.active_profile.combo_timeout_ms.max(5));
        let now = std::time::Instant::now();
        let expired: Vec<u16> = self
            .pending_buttons
            .iter()
            .filter(|(_, p)| now.duration_since(p.pressed_at) >= timeout)
            .map(|(&k, _)| k)
            .collect();
        for code in expired {
            self.flush_pending_button(ui, code);
        }
    }

    pub fn handle_key_event(
        &mut self,
        ui: &mut dyn VirtualKeyboard,
        keycode: u16,
        value: i32,
    ) {
        self.handle_key_event_with_mode(ui, keycode, value, false);
    }

    /// Process an EV_KEY event. When `async_combo_buffer` is true (live async daemon loop),
    /// combo-participating buttons are buffered for `combo_timeout_ms`. When false (synchronous
    /// unit test calls without an active timer loop), solo presses emit immediately unless a
    /// companion button is already held/pending.
    pub fn handle_key_event_with_mode(
        &mut self,
        ui: &mut dyn VirtualKeyboard,
        keycode: u16,
        value: i32,
        async_combo_buffer: bool,
    ) {
        // 1. RELEASE (value == 0)
        if value == 0 {
            self.physical_held_buttons.remove(&keycode);

            // Case A: Button was pending in combo buffer -> Fast tap!
            if let Some(pending) = self.pending_buttons.remove(&keycode) {
                if let Some(ref action) = pending.action {
                    self.record_key_event(&[pending.button_name], action, false);
                    if let Some(ref ex) = action.exec_cmd {
                        self.spawn_exec(ex);
                    }
                    if !action.evdev_codes.is_empty() {
                        emit_chord_down(ui, &action.evdev_codes);
                        emit_chord_up(ui, &action.evdev_codes);
                    }
                }
                return;
            }

            // Case B: Button was part of an active combo
            if let Some(combo_set) = self.held_combos.remove(&keycode) {
                if let Some(combo_act) = self.active_combos.remove(&combo_set) {
                    if !combo_act.evdev_codes.is_empty() {
                        emit_chord_up(ui, &combo_act.evdev_codes);
                    }
                }
                return;
            }

            // Case C: Solo held chord release
            if let Some(held) = self.held_chords.remove(&keycode) {
                emit_chord_up(ui, &held);
            }
            return;
        }

        // 2. REPEAT (value == 2)
        if value == 2 {
            if let Some(combo_set) = self.held_combos.get(&keycode) {
                if let Some(combo_act) = self.active_combos.get(combo_set) {
                    if let Some(&last_code) = combo_act.evdev_codes.last() {
                        ui.write(EV_KEY, last_code, 2);
                        ui.syn();
                    }
                }
                return;
            }
            if let Some(held) = self.held_chords.get(&keycode) {
                if let Some(&last_code) = held.last() {
                    ui.write(EV_KEY, last_code, 2);
                    ui.syn();
                }
                return;
            }
            if let Some(pending) = self.pending_buttons.remove(&keycode) {
                if let Some(ref action) = pending.action {
                    self.emit_solo_down(ui, pending.keycode, action, &pending.button_name);
                    if let Some(&last_code) = action.evdev_codes.last() {
                        ui.write(EV_KEY, last_code, 2);
                        ui.syn();
                    }
                }
            }
            return;
        }

        // 3. PRESS (value == 1)
        self.pending_buttons.remove(&keycode);

        if self.held_chords.contains_key(&keycode) || self.held_combos.contains_key(&keycode) {
            return;
        }

        let Some(button_name) = self.resolve_button_name(keycode) else {
            self.held_chords.insert(keycode, vec![keycode]);
            ui.write(EV_KEY, keycode, 1);
            ui.syn();
            return;
        };

        let logical_btn =
            rotate_dpad_button(button_name, self.active_profile.turn).unwrap_or_else(|_| button_name.to_string());
        self.physical_held_buttons
            .insert(keycode, logical_btn.clone());

        if self.active_profile.combos.is_empty() {
            if let Some(action) = self.active_profile.buttons.get(&logical_btn).cloned() {
                self.emit_solo_down(ui, keycode, &action, &logical_btn);
            }
            return;
        }

        let participates_in_combo = self
            .active_profile
            .combos
            .keys()
            .any(|c| c.contains(&logical_btn));
        if !participates_in_combo {
            if let Some(action) = self.active_profile.buttons.get(&logical_btn).cloned() {
                self.emit_solo_down(ui, keycode, &action, &logical_btn);
            }
            return;
        }

        // Check if pressing this button completes a combo with:
        // A) a pending companion button
        // B) an already physically held button (hold-and-press leader style)
        let mut matched_combo: Option<ComboSet> = None;
        let mut matched_pending: Vec<PendingButton> = Vec::new();
        let mut matched_held_code: Option<u16> = None;

        for (&p_code, p) in &self.pending_buttons {
            if p_code != keycode {
                let mut cand = BTreeSet::new();
                cand.insert(logical_btn.clone());
                cand.insert(p.button_name.clone());
                if self.active_profile.combos.contains_key(&cand) {
                    matched_combo = Some(cand);
                    matched_pending.push(p.clone());
                    break;
                }
            }
        }

        if matched_combo.is_none() {
            for (&h_code, h_btn) in &self.physical_held_buttons {
                if h_code != keycode {
                    let mut cand = BTreeSet::new();
                    cand.insert(logical_btn.clone());
                    cand.insert(h_btn.clone());
                    if self.active_profile.combos.contains_key(&cand) {
                        matched_combo = Some(cand);
                        matched_held_code = Some(h_code);
                        break;
                    }
                }
            }
        }

        if let Some(combo_set) = matched_combo {
            for p in &matched_pending {
                self.pending_buttons.remove(&p.keycode);
            }
            if let Some(h_code) = matched_held_code {
                if let Some(held_solo) = self.held_chords.remove(&h_code) {
                    emit_chord_up(ui, &held_solo);
                }
            }
            if let Some(combo_act) = self.active_profile.combos.get(&combo_set).cloned() {
                let ordered = Self::order_combo_buttons(&combo_set);
                self.record_key_event(&ordered, &combo_act, true);
                if let Some(ref ex) = combo_act.exec_cmd {
                    self.spawn_exec(ex);
                }
                if !combo_act.evdev_codes.is_empty() {
                    self.active_combos
                        .insert(combo_set.clone(), combo_act.clone());
                    self.held_combos.insert(keycode, combo_set.clone());
                    for p in &matched_pending {
                        self.held_combos.insert(p.keycode, combo_set.clone());
                    }
                    if let Some(h_code) = matched_held_code {
                        self.held_combos.insert(h_code, combo_set);
                    }
                    emit_chord_down(ui, &combo_act.evdev_codes);
                }
            }
            return;
        }

        let single_act = self.active_profile.buttons.get(&logical_btn).cloned();
        if async_combo_buffer {
            self.pending_buttons.insert(
                keycode,
                PendingButton {
                    keycode,
                    button_name: logical_btn,
                    action: single_act,
                    pressed_at: std::time::Instant::now(),
                },
            );
        } else if let Some(ref act) = single_act {
            self.emit_solo_down(ui, keycode, act, &logical_btn);
        }
    }

    pub fn cleanup_held_keys_on_disconnect(&mut self, ui: &mut dyn VirtualKeyboard) {
        let mut needs_syn = false;
        self.pending_buttons.clear();

        for combo_act in self.active_combos.values() {
            for &code in combo_act.evdev_codes.iter().rev() {
                ui.write(EV_KEY, code, 0);
                needs_syn = true;
            }
        }
        self.active_combos.clear();
        self.held_combos.clear();
        self.physical_held_buttons.clear();

        for held in self.held_chords.values() {
            for &code in held.iter().rev() {
                ui.write(EV_KEY, code, 0);
                needs_syn = true;
            }
        }
        self.held_chords.clear();

        if needs_syn {
            ui.syn();
        }
    }

    pub async fn run(
        mut self,
        mut stop_rx: watch::Receiver<bool>,
        mut reload_rx: mpsc::UnboundedReceiver<()>,
    ) -> Result<(), String> {
        let mut focus_rx = spawn_focus_watcher();
        let (key_tx, mut key_rx) = mpsc::unbounded_channel::<(u16, i32)>();
        let (ble_probe_tx, mut ble_probe_rx) = mpsc::channel::<()>(1);

        if self.backend != "ble" {
            let ble_addr = self.ble_address.clone();
            let backup_dir = self
                .config_path
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            tokio::spawn(async move {
                // Initial normalization check on daemon startup
                normalize_controller_to_sentinel_if_needed(
                    ble_addr.as_deref(),
                    backup_dir.as_deref(),
                )
                .await;
                while ble_probe_rx.recv().await.is_some() {
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    normalize_controller_to_sentinel_if_needed(
                        ble_addr.as_deref(),
                        backup_dir.as_deref(),
                    )
                    .await;
                }
            });
        }

        // Spawn OS input listener
        #[cfg(target_os = "linux")]
        if self.backend == "uinput" || self.backend == "host" || self.backend == "hybrid" {
            let key_tx_linux = key_tx.clone();
            let ble_probe_tx_linux = ble_probe_tx.clone();
            let mut stop_linux = stop_rx.clone();
            tokio::spawn(async move {
                loop {
                    if *stop_linux.borrow() {
                        break;
                    }
                    let mut target_dev: Option<(PathBuf, evdev::Device)> = None;
                    for (path, dev) in evdev::enumerate() {
                        if !is_physical_evdev_device(&path, &dev) {
                            continue;
                        }
                        let name = dev.name().unwrap_or("");
                        let id = dev.input_id();
                        if identify_controller_name_and_ids(id.vendor(), id.product(), name)
                            .is_some()
                            && dev.supported_keys().is_some()
                        {
                            target_dev = Some((path, dev));
                            break;
                        }
                    }

                    let Some((path, mut dev)) = target_dev else {
                        static WARNED_PERM: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !WARNED_PERM.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            if let Some(msg) = check_linux_input_permissions_diagnostic() {
                                eprintln!("[8bitdo-micro] {}", msg);
                            }
                        }
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(1500)) => continue,
                            _ = stop_linux.changed() => break,
                        }
                    };

                    let dev_name = dev.name().unwrap_or("").to_string();
                    println!("[8bitdo-micro] Attaching to {:?} ({})", path, dev_name);
                    if let Err(e) = dev.grab() {
                        eprintln!(
                            "[8bitdo-micro] Warning: Failed to grab exclusive access to {:?}: {}",
                            path, e
                        );
                    } else {
                        println!(
                            "[8bitdo-micro] Successfully grabbed exclusive access to {:?}",
                            path
                        );
                        let _ = ble_probe_tx_linux.try_send(());
                    }

                    match dev.into_event_stream() {
                        Ok(mut events) => {
                            loop {
                                tokio::select! {
                                    ev = events.next_event() => {
                                        match ev {
                                            Ok(event) => {
                                                if event.event_type() == evdev::EventType::KEY {
                                                    let _ = key_tx_linux.send((event.code(), event.value()));
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("[8bitdo-micro] {:?} event stream error: {}", path, e);
                                                break;
                                            }
                                        }
                                    }
                                    _ = stop_linux.changed() => break,
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[8bitdo-micro] Failed to create event stream for {:?}: {}", path, e);
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }

        #[cfg(target_os = "windows")]
        let _win_hook_guard = if self.backend == "uinput" || self.backend == "host" || self.backend == "hybrid" {
            let allow_fac = self.capture_factory_keys;
            match crate::input_backend::windows_hook::start_windows_hook(key_tx.clone(), allow_fac) {
                Ok(guard) => {
                    println!(
                        "[8bitdo-micro] Windows low-level keyboard hook active (capture_factory_keys={})",
                        allow_fac
                    );
                    Some(guard)
                }
                Err(e) => {
                    eprintln!(
                        "[8bitdo-micro] Failed to activate Windows low-level input hook: {}",
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        // Create virtual keyboard output
        let mut ui: Box<dyn VirtualKeyboard> = {
            #[cfg(target_os = "linux")]
            {
                let (ok, msg) = check_uinput_access();
                let mut created: Option<Box<dyn VirtualKeyboard>> = None;
                if ok {
                    let mut keys = evdev::AttributeSet::<evdev::Key>::new();
                    for code in 1..249u16 {
                        keys.insert(evdev::Key::new(code));
                    }
                    if let Ok(builder) = evdev::uinput::VirtualDeviceBuilder::new() {
                        if let Ok(b2) = builder
                            .name(VIRTUAL_DEVICE_NAME)
                            .input_id(evdev::InputId::new(
                                evdev::BusType::BUS_USB,
                                0x1209,
                                0x0001,
                                0x0101,
                            ))
                            .with_keys(&keys)
                        {
                            if let Ok(vdev) = b2.build() {
                                println!("[8bitdo-micro] Created Linux /dev/uinput virtual keyboard");
                                created =
                                    Some(Box::new(crate::input_backend::LinuxVirtualKeyboard::new(vdev)));
                            }
                        }
                    }
                }
                created.unwrap_or_else(|| {
                    println!("[8bitdo-micro] Virtual keyboard fallback: wtype ({})", msg);
                    Box::new(WaylandVirtualKeyboard::new("wtype"))
                })
            }
            #[cfg(target_os = "windows")]
            {
                Box::new(crate::input_backend::WindowsVirtualKeyboard::new())
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                Box::new(crate::input_backend::MacOSVirtualKeyboard::new())
            }
        };

        let mut combo_tick = tokio::time::interval(Duration::from_millis(10));

        #[cfg(unix)]
        let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()).ok();

        loop {
            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        break;
                    }
                }
                Some(_) = reload_rx.recv() => {
                    self.reload_config();
                }
                _ = async {
                    #[cfg(unix)]
                    if let Some(ref mut sig) = sighup {
                        let _ = sig.recv().await;
                    } else {
                        futures::future::pending::<()>().await;
                    }
                    #[cfg(not(unix))]
                    futures::future::pending::<()>().await;
                } => {
                    self.reload_config();
                }
                Some(win) = focus_rx.recv() => {
                    self.on_focus_change(win);
                }
                Some((code, val)) = key_rx.recv() => {
                    self.handle_key_event_with_mode(ui.as_mut(), code, val, true);
                }
                _ = combo_tick.tick() => {
                    if !self.pending_buttons.is_empty() {
                        self.flush_expired_pending_buttons(ui.as_mut());
                    }
                }
            }
        }

        self.cleanup_held_keys_on_disconnect(ui.as_mut());
        ui.close();
        Ok(())
    }
}
