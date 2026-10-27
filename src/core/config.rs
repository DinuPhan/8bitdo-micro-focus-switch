//! Configuration loader and profile matcher for `8bitdo-micro-focus-switch`.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use regex::{Regex, RegexBuilder};

use crate::keycodes::hid_to_name;
use crate::protocol::{
    get_builtin_hardware_preset, key_name_to_evdev, key_name_to_hid, normalize_button_name,
    turn_code_to_degrees, turn_degrees_to_code, MicroHardwareProfile, PHYSICAL_BUTTONS,
};

pub type ComboSet = BTreeSet<String>;

pub fn get_home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile);
        }
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn default_config_path() -> PathBuf {
    let home = get_home_dir();
    let preferred = home.join(".config").join("8bitdo-micro").join("profiles.toml");
    if preferred.exists() {
        return preferred;
    }
    let fallback = home
        .join(".config")
        .join("8bitdo-micro-focus-switch")
        .join("profiles.toml");
    if fallback.exists() {
        return fallback;
    }
    preferred
}

pub fn resolve_private_runtime_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let trimmed = runtime_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("8bitdo-micro");
        }
    }
    get_home_dir().join(".cache").join("8bitdo-micro")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonAction {
    pub keys: Vec<String>,
    pub evdev_codes: Vec<u16>,
    pub hid_usages: [u8; 4],
    pub exec_cmd: Option<String>,
    pub description: Option<String>,
}

impl ButtonAction {
    pub fn from_toml_value(
        raw: &toml::Value,
        mut description: Option<String>,
    ) -> Result<Self, String> {
        let mut exec_cmd: Option<String> = None;
        let mut raw_keys: Vec<String> = Vec::new();

        match raw {
            toml::Value::String(s) => {
                let stripped = s.trim();
                if stripped.len() >= 5 && stripped[..5].eq_ignore_ascii_case("exec:") {
                    exec_cmd = Some(stripped[5..].trim().to_string());
                } else if !stripped.is_empty() && !stripped.eq_ignore_ascii_case("NONE") {
                    raw_keys = stripped
                        .split('+')
                        .map(|k| k.trim().to_string())
                        .filter(|k| !k.is_empty())
                        .collect();
                }
            }
            toml::Value::Array(arr) => {
                for item in arr {
                    let s = match item {
                        toml::Value::String(st) => st.trim().to_string(),
                        toml::Value::Integer(i) => i.to_string(),
                        other => other.to_string(),
                    };
                    if !s.is_empty() && !s.eq_ignore_ascii_case("NONE") {
                        raw_keys.push(s);
                    }
                }
            }
            toml::Value::Table(tbl) => {
                if let Some(toml::Value::String(ex)) = tbl.get("exec") {
                    let ex_trim = ex.trim();
                    if !ex_trim.is_empty() {
                        exec_cmd = Some(ex_trim.to_string());
                    }
                }
                if description.is_none() {
                    if let Some(toml::Value::String(d)) =
                        tbl.get("description").or_else(|| tbl.get("desc"))
                    {
                        let d_trim = d.trim();
                        if !d_trim.is_empty() {
                            description = Some(d_trim.to_string());
                        }
                    }
                }
                if let Some(k_val) = tbl.get("keys").or_else(|| tbl.get("key")) {
                    match k_val {
                        toml::Value::String(st) => {
                            raw_keys = st
                                .split('+')
                                .map(|k| k.trim().to_string())
                                .filter(|k| !k.is_empty() && !k.eq_ignore_ascii_case("NONE"))
                                .collect();
                        }
                        toml::Value::Array(arr) => {
                            for item in arr {
                                if let Some(st) = item.as_str() {
                                    let st_trim = st.trim();
                                    if !st_trim.is_empty() && !st_trim.eq_ignore_ascii_case("NONE")
                                    {
                                        raw_keys.push(st_trim.to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            other => {
                return Err(format!("Unsupported button action specification: {:?}", other));
            }
        }

        Self::from_parsed_parts(raw_keys, exec_cmd, description)
    }

    pub fn from_string_slice(
        raw_keys: &[String],
        description: Option<String>,
    ) -> Result<Self, String> {
        let filtered: Vec<String> = raw_keys
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("NONE"))
            .collect();
        Self::from_parsed_parts(filtered, None, description)
    }

    fn from_parsed_parts(
        raw_keys: Vec<String>,
        exec_cmd: Option<String>,
        description: Option<String>,
    ) -> Result<Self, String> {
        let mut key_names = Vec::new();
        let mut evdev_codes = Vec::new();
        let mut hid_list = Vec::new();

        for k in raw_keys {
            let k_upper = k.trim().to_uppercase();
            key_names.push(k_upper.clone());
            evdev_codes.push(key_name_to_evdev(&k_upper)?);
            if hid_list.len() < 4 {
                if let Ok(hid) = key_name_to_hid(&k_upper) {
                    hid_list.push(hid);
                }
            }
        }

        let mut hid_usages = [0u8; 4];
        for (i, &h) in hid_list.iter().take(4).enumerate() {
            hid_usages[i] = h;
        }

        Ok(Self {
            keys: key_names,
            evdev_codes,
            hid_usages,
            exec_cmd,
            description,
        })
    }
}

pub fn parse_combo_buttons(raw: &str) -> Result<ComboSet, String> {
    let parts: Vec<&str> = raw
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err(format!(
            "Combos require at least 2 simultaneous buttons (got {:?})",
            raw
        ));
    }
    let mut set = BTreeSet::new();
    for p in parts {
        set.insert(normalize_button_name(p)?.to_string());
    }
    if set.len() < 2 {
        return Err(format!(
            "Combos require at least 2 distinct buttons (got {:?})",
            raw
        ));
    }
    Ok(set)
}

#[derive(Debug, Clone)]
pub struct MatchRule {
    pub match_class: Option<String>,
    pub match_title: Option<String>,
    class_re: Option<Regex>,
    title_re: Option<Regex>,
}

impl MatchRule {
    pub fn new(match_class: Option<String>, match_title: Option<String>) -> Result<Self, String> {
        let class_re = match &match_class {
            Some(c) if !c.is_empty() => Some(
                RegexBuilder::new(c)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| format!("Invalid match_class regex {:?}: {}", c, e))?,
            ),
            _ => None,
        };
        let title_re = match &match_title {
            Some(t) if !t.is_empty() => Some(
                RegexBuilder::new(t)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| format!("Invalid match_title regex {:?}: {}", t, e))?,
            ),
            _ => None,
        };
        Ok(Self {
            match_class,
            match_title,
            class_re,
            title_re,
        })
    }

    pub fn matches(&self, window_class: &str, window_title: &str) -> bool {
        if self.class_re.is_none() && self.title_re.is_none() {
            return false;
        }
        if let Some(re) = &self.class_re {
            if !re.is_match(window_class) {
                return false;
            }
        }
        if let Some(re) = &self.title_re {
            if !re.is_match(window_title) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct ProfileConfig {
    pub name: String,
    pub description: String,
    pub match_class: Option<String>,
    pub match_title: Option<String>,
    pub match_rules: Vec<MatchRule>,
    pub turn: i32, // degrees: 0, 90, 180, 270
    pub combo_timeout_ms: u64,
    pub buttons: HashMap<String, ButtonAction>,
    pub combos: IndexMap<ComboSet, ButtonAction>,
    class_re: Option<Regex>,
    title_re: Option<Regex>,
}

impl ProfileConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        description: String,
        match_class: Option<String>,
        match_title: Option<String>,
        match_rules: Vec<MatchRule>,
        turn_raw: i32,
        combo_timeout_ms: u64,
        buttons: HashMap<String, ButtonAction>,
        combos: IndexMap<ComboSet, ButtonAction>,
    ) -> Result<Self, String> {
        let code = turn_degrees_to_code(turn_raw)
            .map_err(|_| format!("Profile {:?}: invalid turn={} (expected 0, 90, 180, or 270)", name, turn_raw))?;
        let turn = turn_code_to_degrees(code);

        let class_re = match &match_class {
            Some(c) if !c.is_empty() => Some(
                RegexBuilder::new(c)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| format!("Profile {:?}: invalid match_class regex: {}", name, e))?,
            ),
            _ => None,
        };
        let title_re = match &match_title {
            Some(t) if !t.is_empty() => Some(
                RegexBuilder::new(t)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| format!("Profile {:?}: invalid match_title regex: {}", name, e))?,
            ),
            _ => None,
        };

        Ok(Self {
            name,
            description,
            match_class,
            match_title,
            match_rules,
            turn,
            combo_timeout_ms,
            buttons,
            combos,
            class_re,
            title_re,
        })
    }

    pub fn matches_window(&self, window_class: &str, window_title: &str) -> bool {
        if !self.match_rules.is_empty() {
            for rule in &self.match_rules {
                if rule.matches(window_class, window_title) {
                    return true;
                }
            }
            if self.class_re.is_none() && self.title_re.is_none() {
                return false;
            }
        }
        if self.class_re.is_none() && self.title_re.is_none() {
            return false;
        }
        if let Some(re) = &self.class_re {
            if !re.is_match(window_class) {
                return false;
            }
        }
        if let Some(re) = &self.title_re {
            if !re.is_match(window_title) {
                return false;
            }
        }
        true
    }

    pub fn software_only_features(&self) -> (usize, usize) {
        let num_combos = self.combos.len();
        let num_exec = self
            .buttons
            .values()
            .filter(|a| a.exec_cmd.is_some())
            .count()
            + self
                .combos
                .values()
                .filter(|a| a.exec_cmd.is_some())
                .count();
        (num_combos, num_exec)
    }

    pub fn to_hardware_profile(&self, enabled: bool) -> Result<MicroHardwareProfile, String> {
        let mut slots = HashMap::new();
        for &btn in PHYSICAL_BUTTONS {
            if let Some(action) = self.buttons.get(btn) {
                slots.insert(btn.to_string(), action.hid_usages);
            } else {
                slots.insert(btn.to_string(), [0, 0, 0, 0]);
            }
        }
        MicroHardwareProfile::new(enabled, self.turn, slots)
    }

    pub fn from_hardware_profile(
        name: &str,
        hw: &MicroHardwareProfile,
        description: &str,
    ) -> Result<Self, String> {
        let eff = hw.effective_slots();
        let mut buttons = HashMap::new();
        for &btn in PHYSICAL_BUTTONS {
            let usages = eff.get(btn).copied().unwrap_or([0, 0, 0, 0]);
            let names: Vec<String> = usages
                .iter()
                .copied()
                .filter(|&u| u != 0)
                .map(hid_to_name)
                .collect();
            buttons.insert(btn.to_string(), ButtonAction::from_string_slice(&names, None)?);
        }
        Self::new(
            name.to_string(),
            description.to_string(),
            None,
            None,
            Vec::new(),
            hw.turn_degrees(),
            50,
            buttons,
            IndexMap::new(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub fallback_profile: String,
    pub backend: String,
    pub profile_order: Vec<String>,
    pub profiles: HashMap<String, ProfileConfig>,
    pub global_combos: IndexMap<ComboSet, ButtonAction>,
    pub global_buttons: HashMap<String, ButtonAction>,
    pub capture_factory_keys: bool,
    pub emit_osd_events: bool,
}

impl AppConfig {
    pub fn resolve_profile_for_window(
        &self,
        window_class: &str,
        window_title: &str,
    ) -> ProfileConfig {
        for name in &self.profile_order {
            if name == "global" || name == "all" {
                continue;
            }
            if let Some(prof) = self.profiles.get(name) {
                if prof.matches_window(window_class, window_title) {
                    return prof.clone();
                }
            }
        }
        if self.fallback_profile != "global" && self.fallback_profile != "all" {
            if let Some(prof) = self.profiles.get(&self.fallback_profile) {
                return prof.clone();
            }
        }
        for name in &self.profile_order {
            if name != "global" && name != "all" {
                if let Some(prof) = self.profiles.get(name) {
                    return prof.clone();
                }
            }
        }
        self.profiles.values().next().unwrap().clone()
    }

    pub fn get_profile(&self, name: &str) -> Result<ProfileConfig, String> {
        if let Some(p) = self.profiles.get(name) {
            return Ok(p.clone());
        }
        let lower = name.trim().to_lowercase();
        for (k, v) in &self.profiles {
            if k.to_lowercase() == lower {
                return Ok(v.clone());
            }
        }
        let hw = get_builtin_hardware_preset(&lower)?;
        ProfileConfig::from_hardware_profile(
            &lower,
            &hw,
            &format!("Built-in {} hardware preset", name),
        )
    }
}

pub const DEFAULT_TOML_CONTENT: &str = r#"# 8BitDo Micro Focus-Aware Profile Configuration
# Used by both `8bitdo-micro-focus-switch daemon` (instant Hyprland focus remapper)
# and `8bitdo-micro-focus-switch ble write <profile>` (direct BLE GATT hardware flasher).
#
# Physical buttons:
#   A, B, X, Y, L, R, L2, R2, SELECT, START, STAR, HOME, UP, DOWN, LEFT, RIGHT
# D-pad orientation (`turn`):
#   0 = Normal (horizontal), 90 = Rotated 90° CW (one-handed vertical), 180 = Inverted, 270 = 270° CW

[general]
fallback_profile = "agy"
# backend: "uinput" (instant <1ms host-side remapping; requires controller in factory default `ble reset` mode)
#          "ble"    (flashes hardware profile over BLE GATT '80EL' on focus change)
backend = "uinput"
# combo_timeout_ms: Detection window (in ms) for simultaneous button combinations (default: 50)
combo_timeout_ms = 50
# emit_osd_events: Broadcast keypress events to $XDG_RUNTIME_DIR/8bitdo-micro/last_keypress.json
# for the OmaBoy OSD overlay (default: true). Set to false to disable disk stream entirely.
emit_osd_events = true

# =============================================================================
# Global Base Profile ('global' or 'all')
# Combos and buttons defined here apply to ALL profiles by default.
# If an individual app profile defines its own combo, that profile takes precedence.
# =============================================================================
[profiles.global]
description = "Global shortcuts inherited by all profiles unless overridden"

[profiles.global.combos]
"L+R" = ["Super", "K"]               # Command palette / app launcher
"Select+Start" = "exec:hyprctl dispatch togglespecialworkspace" # Toggle scratchpad
"L2+UP" = ["LEFTMETA", "F"]          # Fullscreen window
"L2+DOWN" = ["LEFTMETA", "T"]        # Toggle floating / tiling
"L2+LEFT" = ["LEFTMETA", "LEFT"]     # Focus window left
"L2+RIGHT" = ["LEFTMETA", "RIGHT"]   # Focus window right
"L2+Y" = "exec:omarchy-launch-or-focus-webapp YouTube https://youtube.com/" # Focus/Open YouTube
"L2+A" = "exec:omarchy-launch-or-focus ghostty omarchy-launch-terminal"     # Focus/Open Terminal
"L2+X" = "exec:omarchy-launch-or-focus-webapp 'Google Photos' https://photos.google.com/" # Focus/Open Google Photos
"L2+B" = ["LEFTMETA", "SPACE"]       # Omarchy Menu / Launcher
"L2+L" = ["LEFTMETA", "TAB"]         # Next workspace
"L2+R" = "exec:omarchy-shell shell toggle dinuphan.omaboy" # Toggle OmaBoy HUD panel
"L2+SELECT" = ["LEFTMETA", "W"]      # Close active window
"L2+START" = "F9"                        # Whisper voice dictation (universal across all profiles)

[profiles.agy]
description = "Antigravity (agy) / Terminal Coding Profile (captured from controller)"
match_class = '^(com\.mitchellh\.ghostty|kitty|Alacritty|foot|org\.wezfurlong\.wezterm)$'
match_title = "agy"
turn = 0

[profiles.agy.combos]
# Simultaneous button chords: pressing both buttons within 40ms emits this action instead of solo keys
"L+R" = ["LEFTMETA", "K"]             # Command palette
"L2+R2" = ["LEFTSHIFT", "TAB"]        # Auto-approve file-edits

[profiles.agy.buttons]
A = "BACKSPACE"                  # Backspace / Delete
B = "PAGEDOWN"                   # Page down
X = "PAGEUP"                     # Page up
Y = ["LEFTSHIFT", "ENTER"]       # Multiline newline
L = "TAB"                        # Tab completion / switch mode
R = ["LEFTCTRL", "LEFTSHIFT", "D"] # Toggle voice / debug mode
L2 = "LEFTSHIFT"                 # Shift / Global combo modifier
R2 = "ENTER"                     # Submit prompt
SELECT = "ESC"                   # Cancel / Escape
START = ["2", "LEFTSHIFT"]       # @ file mention
STAR = "SPACE"                   # Space
HOME = "SLASH"                   # / command
UP = "UP"                        # Up / Previous history
DOWN = "DOWN"                    # Down / Next history
LEFT = "LEFT"                    # Cursor left
RIGHT = "RIGHT"                  # Cursor right

[profiles.anki]
description = "Anki Flashcards (matches official 8BitDo Anki preset)"
match_class = "^(anki|Anki)$"
match_title = "Anki"
turn = 0

[profiles.anki.buttons]
A = "2"                          # Good
B = "1"                          # Again
X = ["LEFTCTRL", "LEFTSHIFT", "D"] # Change deck
Y = ["LEFTCTRL", "LEFTALT", "N"] # Add note
L = "SPACE"                      # Show answer
R = "4"                          # Easy
L2 = "L"                         # Global combo modifier
R2 = "R"                         # Replay audio
SELECT = "E"                     # Edit card
START = "KPASTERISK"             # Mark card
STAR = "T"                       # Stats
HOME = "S"                       # Study deck
UP = ["LEFTCTRL", "DELETE"]      # Delete note
DOWN = ["LEFTSHIFT", "2"]        # Suspend card
LEFT = ["LEFTCTRL", "Z"]         # Undo
RIGHT = "KPMINUS"                # Bury card

[profiles.youtube]
description = "YouTube Video Playback in Browser or Standalone Web App"
match_rules = [
    { class = ".*youtube.*|freetube" },
    { class = "^(firefox|brave-browser|chromium|google-chrome|zen|vivaldi-stable|microsoft-edge)$", title = ".*(YouTube|- YouTube).*" },
]
match_class = '^(firefox|brave-browser|chromium|google-chrome|zen|vivaldi-stable|microsoft-edge)$|.*youtube.*|freetube'
match_title = ".*(YouTube|- YouTube).*"
turn = 0

[profiles.youtube.combos]
"L+R" = ["LEFTSHIFT", "N"]       # Next video
"SELECT+START" = "M"             # Mute / Unmute video
"SELECT+UP" = "PAGEUP"           # Scroll page up while browsing
"SELECT+DOWN" = "PAGEDOWN"       # Scroll page down while browsing

[profiles.youtube.buttons]
A = "SPACE"                      # Play / Pause
B = "M"                          # Mute audio
X = "F"                          # Toggle Fullscreen
Y = "C"                          # Subtitles / Captions
L = "J"                          # Seek backward 10s
R = "L"                          # Seek forward 10s
L2 = ["LEFTSHIFT", "COMMA"]      # Slow down playback
R2 = ["LEFTSHIFT", "DOT"]        # Speed up playback
SELECT = "ESC"                   # Unfocus search / Exit Fullscreen
START = "F9"                     # Whisper voice dictation
STAR = "SLASH"                   # Focus search bar (/)
HOME = ["LEFTALT", "LEFT"]       # Navigate back to results
UP = "UP"                        # Volume up
DOWN = "DOWN"                    # Volume down
LEFT = "LEFT"                    # Seek -5s
RIGHT = "RIGHT"                  # Seek +5s

[profiles.browser]
description = "Web Browsing / Reading (Chrome, Chromium, Firefox, Brave, Zen)"
match_class = "^(firefox|brave-browser|chromium|google-chrome|zen|vivaldi-stable)$"
turn = 0

[profiles.browser.combos]
"R2+Y" = ["LEFTALT", "LEFT"]        # History Back (Y is physically Left)
"R2+A" = ["LEFTALT", "RIGHT"]       # History Forward (A is physically Right)
"R2+X" = ["LEFTCTRL", "R"]          # Reload page
"R2+B" = ["LEFTCTRL", "F"]          # Find in page
"R2+R" = ["LEFTCTRL", "PAGEDOWN"]   # Next tab
"R2+L" = ["LEFTCTRL", "PAGEUP"]     # Previous tab
"R2+UP" = ["LEFTCTRL", "EQUAL"]     # Zoom in
"R2+DOWN" = ["LEFTCTRL", "MINUS"]   # Zoom out
"R2+START" = ["LEFTCTRL", "T"]      # Open new tab

[profiles.browser.buttons]
A = "ENTER"                         # Open focused link / Submit form
B = "PAGEDOWN"                      # Page down
X = "PAGEUP"                        # Page up
Y = ["LEFTCTRL", "L"]               # Focus address bar (search Google)
L = ["LEFTSHIFT", "TAB"]            # Previous hyperlink
R = "TAB"                           # Next hyperlink
L2 = "None"                         # Dedicated Omarchy Fn modifier
R2 = "None"                         # Dedicated Browser modifier
SELECT = ["LEFTCTRL", "W"]          # Close tab
START = ["LEFTCTRL", "T"]           # New tab
STAR = ["LEFTCTRL", "LEFTSHIFT", "T"] # Reopen closed tab (Star button)
HOME = ["LEFTALT", "LEFT"]          # Browser back (single button fallback)
UP = "UP"                           # Scroll up
DOWN = "DOWN"                       # Scroll down
LEFT = "LEFT"                       # Scroll left
RIGHT = "RIGHT"                     # Scroll right

[profiles.media]
description = "Video / Media Player (mpv, VLC)"
match_class = "^(mpv|vlc)$"
turn = 0

[profiles.media.buttons]
A = "SPACE"                      # Play / Pause
B = "F"                          # Toggle Fullscreen
X = "M"                          # Mute audio
Y = "S"                          # Screenshot
L = "LEFTBRACE"                  # Speed down
R = "RIGHTBRACE"                 # Speed up
L2 = "9"                         # Volume down
R2 = "0"                         # Volume up
SELECT = "ESC"                   # Exit Fullscreen
START = "ENTER"                  # Next playlist item
STAR = "J"                       # Cycle subtitles
HOME = "O"                       # Show OSD progress
UP = "UP"                        # Seek +1m
DOWN = "DOWN"                    # Seek -1m
LEFT = "LEFT"                    # Seek -5s
RIGHT = "RIGHT"                  # Seek +5s

[profiles.factory]
description = "Factory K-Mode Default (c..t unique single keys)"
turn = 0

[profiles.factory.buttons]
A = "G"
B = "J"
X = "H"
Y = "I"
L = "K"
R = "M"
L2 = "L"
R2 = "R"
SELECT = "N"
START = "O"
STAR = "T"
HOME = "S"
UP = "C"
DOWN = "D"
LEFT = "E"
RIGHT = "F"
"#;

#[derive(Default)]
pub struct ProfileComments {
    pub buttons: HashMap<String, String>,
    pub combos: HashMap<ComboSet, String>,
}

pub fn extract_toml_inline_comments(text: &str) -> HashMap<String, ProfileComments> {
    let mut result: HashMap<String, ProfileComments> = HashMap::new();
    let mut current_section = String::new();

    for line in text.lines() {
        let line_str = line.trim();
        if line_str.is_empty() {
            continue;
        }
        if line_str.starts_with('[') && line_str.ends_with(']') {
            current_section = line_str[1..line_str.len() - 1].trim().to_string();
            continue;
        }
        if line_str.starts_with('#') {
            continue;
        }

        let mut in_single = false;
        let mut in_double = false;
        let mut escape = false;
        let mut comment_idx = None;

        for (i, ch) in line.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' && in_double {
                escape = true;
                continue;
            }
            if ch == '"' && !in_single {
                in_double = !in_double;
            } else if ch == '\'' && !in_double {
                in_single = !in_single;
            } else if ch == '#' && !in_single && !in_double {
                comment_idx = Some(i);
                break;
            }
        }

        if let Some(c_idx) = comment_idx {
            let code_part = line[..c_idx].trim();
            let comment_text = line[c_idx + 1..].trim();
            if let Some((key_part, _)) = code_part.split_once('=') {
                if !comment_text.is_empty() {
                    let raw_key = key_part.trim().trim_matches(|c| c == '"' || c == '\'');
                    let parts: Vec<String> = current_section
                        .split('.')
                        .map(|p| p.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
                        .collect();

                    let (prof_name, sub) = if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("profiles") {
                        let pn = parts[1].to_lowercase();
                        let sb = if parts.len() > 2 {
                            parts[2].to_lowercase()
                        } else {
                            "auto".to_string()
                        };
                        (pn, sb)
                    } else if !parts.is_empty() {
                        let pn = parts[0].to_lowercase();
                        let sb = if parts.len() > 1 {
                            parts[1].to_lowercase()
                        } else {
                            "auto".to_string()
                        };
                        (pn, sb)
                    } else {
                        continue;
                    };

                    if sub == "combos" || raw_key.contains('+') {
                        if let Ok(combo_key) = parse_combo_buttons(raw_key) {
                            result
                                .entry(prof_name)
                                .or_default()
                                .combos
                                .insert(combo_key, comment_text.to_string());
                        }
                    } else if let Ok(btn_key) = normalize_button_name(raw_key) {
                        result
                            .entry(prof_name)
                            .or_default()
                            .buttons
                            .insert(btn_key.to_string(), comment_text.to_string());
                    }
                }
            }
        }
    }

    result
}

/// Extract ordered profile names as they appear in the TOML text (`[profiles.<name>]`)
fn extract_profile_section_order(text: &str) -> Vec<String> {
    let mut order = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.starts_with("[[") {
            let inner = trimmed[1..trimmed.len() - 1].trim();
            let parts: Vec<&str> = inner
                .split('.')
                .map(|p| p.trim().trim_matches(|c| c == '"' || c == '\''))
                .collect();
            if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("profiles") {
                let pname = parts[1].to_string();
                if !order.contains(&pname) {
                    order.push(pname);
                }
            }
        }
    }
    order
}

pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        use std::os::unix::fs::PermissionsExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("Failed to create directory {}: {}", dir.display(), e))?;
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if dir_name.contains("8bitdo-micro") {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory {}: {}", dir.display(), e))
    }
}

pub fn harden_private_file_permissions(target_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if target_path.exists() {
            let _ = fs::set_permissions(target_path, fs::Permissions::from_mode(0o600));
        }
        if let Some(parent) = target_path.parent() {
            let dir_name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.contains("8bitdo-micro") && parent.exists() {
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
                let backup = parent.join("hardware-backup.bin");
                if backup.exists() {
                    let _ = fs::set_permissions(&backup, fs::Permissions::from_mode(0o600));
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = target_path;
    }
}

pub fn write_private_atomic(target_path: &Path, content: &[u8]) -> Result<(), String> {
    if let Some(parent) = target_path.parent() {
        if !parent.as_os_str().is_empty() {
            ensure_private_dir(parent)?;
        }
    }
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let fname = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let pid = std::process::id();
    let tmp_path = parent.join(format!(".{}.tmp.{}", fname, pid));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;

        let mut opts = fs::OpenOptions::new();
        opts.write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = opts.open(&tmp_path).map_err(|e| {
            format!(
                "Failed to open temporary file {}: {}",
                tmp_path.display(),
                e
            )
        })?;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
        if let Err(e) = file.write_all(content).and_then(|_| file.flush()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("Failed to write {}: {}", target_path.display(), e));
        }
        drop(file);
        if let Err(e) = fs::rename(&tmp_path, target_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("Failed to rename {}: {}", target_path.display(), e));
        }
        harden_private_file_permissions(target_path);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(&tmp_path, content)
            .map_err(|e| format!("Failed to write {}: {}", tmp_path.display(), e))?;
        fs::rename(&tmp_path, target_path)
            .map_err(|e| format!("Failed to rename {}: {}", target_path.display(), e))
    }
}

pub fn ensure_default_config(path: Option<&Path>) -> Result<PathBuf, String> {
    let cfg_path = path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_config_path);
    if !cfg_path.exists() {
        write_private_atomic(&cfg_path, DEFAULT_TOML_CONTENT.as_bytes())?;
    } else {
        harden_private_file_permissions(&cfg_path);
    }
    Ok(cfg_path)
}

pub fn load_config(path: Option<&Path>) -> Result<AppConfig, String> {
    let cfg_path = match path {
        Some(p) => {
            if !p.exists() {
                return Err(format!("Configuration file not found: {}", p.display()));
            }
            harden_private_file_permissions(p);
            p.to_path_buf()
        }
        None => ensure_default_config(None)?,
    };

    let raw_text = fs::read_to_string(&cfg_path)
        .map_err(|e| format!("Failed to read {}: {}", cfg_path.display(), e))?;
    load_config_from_str(&raw_text)
}

pub fn load_config_from_str(raw_text: &str) -> Result<AppConfig, String> {
    let comments = extract_toml_inline_comments(raw_text);
    let mut profile_order = extract_profile_section_order(raw_text);
    let raw: toml::Table = toml::from_str(raw_text)
        .map_err(|e| format!("TOML syntax error: {}", e))?;

    let general = raw.get("general").and_then(|v| v.as_table());
    let fallback_profile = general
        .and_then(|g| g.get("fallback_profile"))
        .and_then(|v| v.as_str())
        .unwrap_or("agy")
        .to_string();
    let backend = general
        .and_then(|g| g.get("backend"))
        .and_then(|v| v.as_str())
        .unwrap_or("uinput")
        .to_lowercase();
    let default_combo_timeout_ms = general
        .and_then(|g| g.get("combo_timeout_ms"))
        .and_then(|v| v.as_integer())
        .unwrap_or(50) as u64;
    let capture_factory_keys = general
        .and_then(|g| g.get("capture_factory_keys"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let emit_osd_events = general
        .and_then(|g| g.get("emit_osd_events"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut profiles: HashMap<String, ProfileConfig> = HashMap::new();

    if let Some(toml::Value::Table(profs_table)) = raw.get("profiles") {
        for prof_name in profs_table.keys() {
            if !profile_order.contains(prof_name) {
                profile_order.push(prof_name.clone());
            }
        }

        for (prof_name, prof_val) in profs_table {
            let Some(prof_data) = prof_val.as_table() else {
                continue;
            };
            let prof_comments = comments.get(&prof_name.to_lowercase());

            let mut raw_buttons: HashMap<String, toml::Value> = HashMap::new();
            let mut raw_combos: IndexMap<ComboSet, toml::Value> = IndexMap::new();

            if let Some(toml::Value::Table(btns)) = prof_data.get("buttons") {
                for (k, v) in btns {
                    let norm_btn = normalize_button_name(k)?;
                    raw_buttons.insert(norm_btn.to_string(), v.clone());
                }
            }
            if let Some(toml::Value::Table(cmbs)) = prof_data.get("combos") {
                for (k, v) in cmbs {
                    let norm_combo = parse_combo_buttons(k)?;
                    raw_combos.insert(norm_combo, v.clone());
                }
            }

            // Also check inline button/combo keys directly under [profiles.<name>]
            for (k, v) in prof_data {
                if matches!(
                    k.as_str(),
                    "buttons"
                        | "combos"
                        | "description"
                        | "match_class"
                        | "match_title"
                        | "match_rules"
                        | "turn"
                        | "combo_timeout_ms"
                ) {
                    continue;
                }
                if k.contains('+') {
                    if let Ok(norm_c) = parse_combo_buttons(k) {
                        raw_combos.entry(norm_c).or_insert_with(|| v.clone());
                    }
                } else if let Ok(norm_k) = normalize_button_name(k) {
                    raw_buttons
                        .entry(norm_k.to_string())
                        .or_insert_with(|| v.clone());
                }
            }

            let mut buttons = HashMap::new();
            for (norm_btn, btn_spec) in raw_buttons {
                let btn_desc = prof_comments
                    .and_then(|c| c.buttons.get(&norm_btn))
                    .cloned();
                buttons.insert(
                    norm_btn,
                    ButtonAction::from_toml_value(&btn_spec, btn_desc)?,
                );
            }

            let mut combos = IndexMap::new();
            for (norm_combo, combo_spec) in raw_combos {
                let combo_desc = prof_comments
                    .and_then(|c| c.combos.get(&norm_combo))
                    .cloned();
                combos.insert(
                    norm_combo,
                    ButtonAction::from_toml_value(&combo_spec, combo_desc)?,
                );
            }

            let combo_timeout_ms = prof_data
                .get("combo_timeout_ms")
                .and_then(|v| v.as_integer())
                .map(|v| v as u64)
                .unwrap_or(default_combo_timeout_ms);

            let mut match_rules = Vec::new();
            if let Some(toml::Value::Array(arr)) = prof_data.get("match_rules") {
                for r in arr {
                    if let Some(r_tbl) = r.as_table() {
                        let c_pat = r_tbl
                            .get("class")
                            .or_else(|| r_tbl.get("match_class"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let t_pat = r_tbl
                            .get("title")
                            .or_else(|| r_tbl.get("match_title"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        match_rules.push(MatchRule::new(c_pat, t_pat)?);
                    }
                }
            }

            let description = prof_data
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let match_class = prof_data
                .get("match_class")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let match_title = prof_data
                .get("match_title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let turn = prof_data
                .get("turn")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as i32;

            profiles.insert(
                prof_name.clone(),
                ProfileConfig::new(
                    prof_name.clone(),
                    description,
                    match_class,
                    match_title,
                    match_rules,
                    turn,
                    combo_timeout_ms,
                    buttons,
                    combos,
                )?,
            );
        }
    }

    // Ensure built-in presets are reachable if not overridden
    for builtin_name in ["agy", "youtube", "factory", "anki", "supermemo"] {
        if !profiles.contains_key(builtin_name) {
            let hw = get_builtin_hardware_preset(builtin_name)?;
            let prof = ProfileConfig::from_hardware_profile(
                builtin_name,
                &hw,
                &format!("Built-in {} preset", builtin_name),
            )?;
            profiles.insert(builtin_name.to_string(), prof);
            profile_order.push(builtin_name.to_string());
        }
    }

    let mut global_combos: IndexMap<ComboSet, ButtonAction> = IndexMap::new();
    let mut global_buttons: HashMap<String, ButtonAction> = HashMap::new();

    // 1. From [profiles.global] or [profiles.all]
    for g_name in ["global", "all"] {
        if let Some(g_prof) = profiles.get(g_name) {
            for (c, act) in &g_prof.combos {
                global_combos.entry(c.clone()).or_insert_with(|| act.clone());
            }
            for (b, act) in &g_prof.buttons {
                global_buttons.entry(b.clone()).or_insert_with(|| act.clone());
            }
        }
    }

    // 2. Also support top-level [global] or [all] tables
    for g_key in ["global", "all"] {
        if let Some(toml::Value::Table(g_data)) = raw.get(g_key) {
            let g_comments = comments.get(g_key);
            if let Some(toml::Value::Table(cmbs)) = g_data.get("combos") {
                for (c_k, c_v) in cmbs {
                    if let Ok(c_norm) = parse_combo_buttons(c_k) {
                        let c_desc = g_comments.and_then(|c| c.combos.get(&c_norm)).cloned();
                        if let Ok(act) = ButtonAction::from_toml_value(c_v, c_desc) {
                            global_combos.entry(c_norm).or_insert(act);
                        }
                    }
                }
            }
            if let Some(toml::Value::Table(btns)) = g_data.get("buttons") {
                for (b_k, b_v) in btns {
                    if let Ok(b_norm) = normalize_button_name(b_k) {
                        let b_desc = g_comments.and_then(|c| c.buttons.get(b_norm)).cloned();
                        if let Ok(act) = ButtonAction::from_toml_value(b_v, b_desc) {
                            global_buttons.entry(b_norm.to_string()).or_insert(act);
                        }
                    }
                }
            }
        }
    }

    // 3. Inherit global defaults into all profiles (profile-specific bindings take precedence)
    for (p_name, p_prof) in profiles.iter_mut() {
        if p_name == "global" || p_name == "all" {
            continue;
        }
        for (c, act) in &global_combos {
            p_prof
                .combos
                .entry(c.clone())
                .or_insert_with(|| act.clone());
        }
        for (b, act) in &global_buttons {
            p_prof
                .buttons
                .entry(b.clone())
                .or_insert_with(|| act.clone());
        }
    }

    Ok(AppConfig {
        fallback_profile,
        backend,
        profile_order,
        profiles,
        global_combos,
        global_buttons,
        capture_factory_keys,
        emit_osd_events,
    })
}
