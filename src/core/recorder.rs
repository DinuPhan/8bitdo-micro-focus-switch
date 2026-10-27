//! Interactive keyboard chord recorder, key catalog, and TOML binding updater.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use regex::RegexBuilder;

use crate::config::{load_config_from_str, parse_combo_buttons};
use crate::keycodes::key_by_evdev;
use crate::protocol::normalize_button_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordResult {
    pub keys: Vec<String>,
    pub cancelled: bool,
    pub raw_codes: Vec<u16>,
}

impl ChordResult {
    pub fn display_str(&self) -> String {
        if self.cancelled {
            "Cancelled (ESC)".to_string()
        } else if self.keys.is_empty() {
            "(none)".to_string()
        } else {
            self.keys.join(" + ")
        }
    }

    pub fn toml_value(&self) -> String {
        if self.keys.len() == 1 {
            serde_json::to_string(&self.keys[0]).unwrap_or_default()
        } else {
            serde_json::to_string(&self.keys).unwrap_or_default()
        }
    }
}

pub fn evdev_to_friendly_name(code: u16) -> Option<String> {
    let fixed = match code {
        29 | 97 => Some("Ctrl"),
        42 | 54 => Some("Shift"),
        56 | 100 => Some("Alt"),
        125 | 126 => Some("Super"),
        57 => Some("Space"),
        28 | 96 => Some("Enter"),
        15 => Some("Tab"),
        14 => Some("Backspace"),
        111 => Some("Delete"),
        110 => Some("Insert"),
        1 => Some("Esc"),
        104 => Some("PageUp"),
        109 => Some("PageDown"),
        102 => Some("Home"),
        107 => Some("End"),
        103 => Some("Up"),
        108 => Some("Down"),
        105 => Some("Left"),
        106 => Some("Right"),
        58 => Some("CapsLock"),
        113 => Some("Mute"),
        115 => Some("VolumeUp"),
        114 => Some("VolumeDown"),
        164 => Some("PlayPause"),
        163 => Some("NextSong"),
        165 => Some("PreviousSong"),
        12 => Some("Minus"),
        13 => Some("Equal"),
        26 => Some("LeftBrace"),
        27 => Some("RightBrace"),
        43 => Some("Backslash"),
        39 => Some("Semicolon"),
        40 => Some("Apostrophe"),
        41 => Some("Grave"),
        51 => Some("Comma"),
        52 => Some("Dot"),
        53 => Some("Slash"),
        _ => None,
    };
    if let Some(s) = fixed {
        return Some(s.to_string());
    }
    key_by_evdev(code).map(|s| s.name.to_string())
}

fn is_modifier_ecode(code: u16) -> bool {
    matches!(code, 29 | 97 | 42 | 54 | 56 | 100 | 125 | 126)
}

fn modifier_order(name: &str) -> u8 {
    match name {
        "Ctrl" => 0,
        "Alt" => 1,
        "Shift" => 2,
        "Super" => 3,
        _ => 99,
    }
}

#[derive(Default)]
pub struct ChordRecorder {
    pub held_modifiers: BTreeSet<u16>,
    pub held_non_modifiers: BTreeSet<u16>,
    pub solo_modifier_candidates: BTreeSet<u16>,
    pub result: Option<ChordResult>,
}

impl ChordRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_event(&mut self, code: u16, value: i32) -> Option<ChordResult> {
        if value == 2 {
            return None;
        }
        // 1. ESC (code 1) cancels immediately
        if code == 1 {
            if value == 1 {
                let res = ChordResult {
                    keys: Vec::new(),
                    cancelled: true,
                    raw_codes: vec![code],
                };
                self.result = Some(res.clone());
                return Some(res);
            }
            return None;
        }

        // 2. KEY DOWN (value == 1)
        if value == 1 {
            if is_modifier_ecode(code) {
                self.held_modifiers.insert(code);
                self.solo_modifier_candidates.insert(code);
                return None;
            } else {
                self.held_non_modifiers.insert(code);
                self.solo_modifier_candidates.clear();

                let mut mod_names: Vec<String> = Vec::new();
                for &mod_code in &self.held_modifiers {
                    if let Some(name) = evdev_to_friendly_name(mod_code) {
                        if !mod_names.contains(&name) {
                            mod_names.push(name);
                        }
                    }
                }
                mod_names.sort_by_key(|m| modifier_order(m));

                let target_name =
                    evdev_to_friendly_name(code).unwrap_or_else(|| format!("Key{}", code));
                let mut chord_keys = mod_names;
                chord_keys.push(target_name);

                let mut raw_codes: Vec<u16> = self.held_modifiers.iter().copied().collect();
                raw_codes.push(code);

                let res = ChordResult {
                    keys: chord_keys,
                    cancelled: false,
                    raw_codes,
                };
                self.result = Some(res.clone());
                return Some(res);
            }
        }

        // 3. KEY UP (value == 0)
        if value == 0 {
            if is_modifier_ecode(code) {
                if self.solo_modifier_candidates.contains(&code)
                    && self.held_non_modifiers.is_empty()
                {
                    let mod_name =
                        evdev_to_friendly_name(code).unwrap_or_else(|| "Ctrl".to_string());
                    let res = ChordResult {
                        keys: vec![mod_name],
                        cancelled: false,
                        raw_codes: vec![code],
                    };
                    self.result = Some(res.clone());
                    self.held_modifiers.remove(&code);
                    self.solo_modifier_candidates.clear();
                    return Some(res);
                }
                self.held_modifiers.remove(&code);
                self.solo_modifier_candidates.remove(&code);
            } else {
                self.held_non_modifiers.remove(&code);
            }
        }

        None
    }
}

pub async fn capture_keyboard_chord(timeout: Option<Duration>) -> Result<ChordResult, String> {
    #[cfg(target_os = "linux")]
    {
        let mut streams = Vec::new();
        for (path, dev) in evdev::enumerate() {
            if !crate::daemon::is_physical_evdev_device(&path, &dev) {
                continue;
            }
            if let Some(keys) = dev.supported_keys() {
                if keys.contains(evdev::Key::KEY_A)
                    && keys.contains(evdev::Key::KEY_SPACE)
                    && keys.contains(evdev::Key::KEY_ENTER)
                {
                    if let Ok(stream) = dev.into_event_stream() {
                        streams.push(stream);
                    }
                }
            }
        }

        if streams.is_empty() {
            return Err(
                "No accessible physical keyboards found in /dev/input/event*.\n\
                 Ensure you have read permissions on /dev/input (member of group 'input').\n\
                 Run: 8bitdo-micro-focus-switch install-udev"
                    .to_string(),
            );
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for mut s in streams {
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                while let Ok(ev) = s.next_event().await {
                    if ev.event_type() == evdev::EventType::KEY {
                        if tx_clone.send((ev.code(), ev.value())).is_err() {
                            break;
                        }
                    }
                }
            });
        }

        let mut recorder = ChordRecorder::new();
        let deadline = timeout.map(|t| tokio::time::Instant::now() + t);

        loop {
            let next_ev = if let Some(dl) = deadline {
                let now = tokio::time::Instant::now();
                if now >= dl {
                    return Ok(ChordResult {
                        keys: Vec::new(),
                        cancelled: true,
                        raw_codes: Vec::new(),
                    });
                }
                tokio::time::timeout(dl - now, rx.recv()).await.ok().flatten()
            } else {
                rx.recv().await
            };

            let Some((code, val)) = next_ev else {
                return Ok(ChordResult {
                    keys: Vec::new(),
                    cancelled: true,
                    raw_codes: Vec::new(),
                });
            };
            if let Some(res) = recorder.process_event(code, val) {
                return Ok(res);
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = timeout;
        Err(
            "Interactive keyboard chord listening requires Linux evdev.\n\
             Please specify the key binding directly via `--key` or edit profiles.toml."
                .to_string(),
        )
    }
}

pub fn get_bindable_keys_catalog() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("Modifiers", vec!["Ctrl", "Shift", "Alt", "Super"]),
        (
            "Navigation",
            vec!["Up", "Down", "Left", "Right", "PageUp", "PageDown", "Home", "End"],
        ),
        (
            "Actions & Editing",
            vec!["Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "CapsLock"],
        ),
        (
            "Function Keys (F1..F12)",
            vec!["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"],
        ),
        (
            "Extended Function (F13..F24)",
            vec![
                "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23",
                "F24",
            ],
        ),
        (
            "Media & Volume",
            vec![
                "PlayPause",
                "Mute",
                "VolumeUp",
                "VolumeDown",
                "NextSong",
                "PreviousSong",
            ],
        ),
        (
            "Letters (A..Z)",
            vec![
                "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
                "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
            ],
        ),
        (
            "Digits (0..9)",
            vec!["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"],
        ),
        (
            "Punctuation & Symbols",
            vec![
                "Minus",
                "Equal",
                "LeftBrace",
                "RightBrace",
                "Backslash",
                "Semicolon",
                "Apostrophe",
                "Grave",
                "Comma",
                "Dot",
                "Slash",
            ],
        ),
        (
            "Special Actions",
            vec![
                "None (unbound)",
                "exec:<shell-command>  (e.g. exec:playerctl play-pause)",
            ],
        ),
    ]
}

pub fn format_keys_catalog(query: Option<&str>) -> String {
    let catalog = get_bindable_keys_catalog();
    let mut lines = vec![
        "=== 8BitDo Micro Bindable Keys Catalog ===".to_string(),
        "Tip: In `bind` or `record` mode, simply press the key/chord on your keyboard directly!"
            .to_string(),
        String::new(),
    ];
    let norm_q = query.map(|q| q.trim().to_lowercase()).unwrap_or_default();
    let mut found_any = false;

    for (category, keys) in catalog {
        if !norm_q.is_empty() {
            let matched: Vec<&str> = keys
                .into_iter()
                .filter(|k| {
                    k.to_lowercase().contains(&norm_q)
                        || category.to_lowercase().contains(&norm_q)
                })
                .collect();
            if !matched.is_empty() {
                found_any = true;
                lines.push(format!("{}:", category));
                lines.push(format!("  {}", matched.join(", ")));
                lines.push(String::new());
            }
        } else {
            found_any = true;
            lines.push(format!("{}:", category));
            if keys.len() > 10 {
                for chunk in keys.chunks(10) {
                    lines.push(format!("  {}", chunk.join(", ")));
                }
            } else {
                lines.push(format!("  {}", keys.join(", ")));
            }
            lines.push(String::new());
        }
    }

    if !norm_q.is_empty() && !found_any {
        lines.push(format!("No bindable keys found matching {:?}.", query.unwrap_or("")));
    }

    lines.join("\n").trim_end().to_string()
}

pub fn sanitize_toml_inline_comment(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn update_toml_binding(
    content: &str,
    profile: &str,
    button_or_combo: &str,
    chord_keys: &[String],
    description: Option<&str>,
) -> Result<String, String> {
    let profile = profile.trim();
    if profile.is_empty()
        || !profile
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "Invalid profile name {:?}: must contain only ASCII letters, digits, '_', or '-'",
            profile
        ));
    }

    let clean_description = description
        .map(sanitize_toml_inline_comment)
        .filter(|s| !s.is_empty());

    let is_combo = button_or_combo.contains('+');
    let (key_name, section_sub) = if is_combo {
        let parsed = parse_combo_buttons(button_or_combo)?;
        let joined: Vec<String> = parsed.into_iter().collect();
        (joined.join("+"), "combos")
    } else {
        (normalize_button_name(button_or_combo)?.to_string(), "buttons")
    };

    let value_repr = if chord_keys.len() == 1 {
        serde_json::to_string(&chord_keys[0]).unwrap_or_default()
    } else {
        serde_json::to_string(chord_keys).unwrap_or_default()
    };

    let mut working_content = content.to_string();
    let p_header_pat = format!(r"(?m)^[ \t]*\[profiles\.{}\][ \t]*$", regex::escape(profile));
    let p_header_re = RegexBuilder::new(&p_header_pat)
        .case_insensitive(true)
        .build()
        .unwrap();

    if p_header_re.find(&working_content).is_none() {
        if profile.eq_ignore_ascii_case("global") || profile.eq_ignore_ascii_case("all") {
            let insert = format!(
                "\n[profiles.{}]\ndescription = 'Global base template inherited by all profiles'\n",
                profile
            );
            working_content = format!("{}\n{}", working_content.trim_end(), insert);
        } else {
            return Err(format!("Profile {:?} not found in configuration", profile));
        }
    }

    let m = p_header_re
        .find(&working_content)
        .ok_or_else(|| format!("Profile {:?} not found in configuration", profile))?;
    let p_start = m.start();

    let p_next_re = RegexBuilder::new(r"(?m)^[ \t]*\[(?:profiles\.[^.\s\]]+|general)\][ \t]*$")
        .case_insensitive(true)
        .build()
        .unwrap();
    let p_end = p_next_re
        .find_at(&working_content, m.end())
        .map(| next_m | next_m.start())
        .unwrap_or(working_content.len());

    let prof_block = &working_content[p_start..p_end];

    let sub_header_pat = format!(
        r"(?m)^[ \t]*\[profiles\.{}\.{}\][ \t]*$",
        regex::escape(profile),
        regex::escape(section_sub)
    );
    let sub_header_re = RegexBuilder::new(&sub_header_pat)
        .case_insensitive(true)
        .build()
        .unwrap();

    let key_lhs = if is_combo {
        serde_json::to_string(&key_name).unwrap_or_default()
    } else {
        key_name.clone()
    };
    let mut line_to_put = format!("{} = {}", key_lhs, value_repr);

    let new_prof_block = if let Some(m_sub) = sub_header_re.find(prof_block) {
        let sub_start = m_sub.end();
        let next_bracket_re = RegexBuilder::new(r"(?m)^[ \t]*\[").build().unwrap();
        let sub_end = next_bracket_re
            .find_at(prof_block, sub_start)
            .map(|nm| nm.start())
            .unwrap_or(prof_block.len());
        let sub_text = &prof_block[sub_start..sub_end];

        let key_pat = format!(
            r#"(?m)^[ \t]*(?:{}|'{}'|"{}")[ \t]*=.*$"#,
            regex::escape(&key_name),
            regex::escape(&key_name),
            regex::escape(&key_name)
        );
        let key_re = RegexBuilder::new(&key_pat)
            .case_insensitive(true)
            .build()
            .unwrap();

        let new_sub_text = if let Some(m_key) = key_re.find(sub_text) {
            let existing_line = m_key.as_str();
            let mut existing_comment = String::new();
            if let Some(ref clean) = clean_description {
                existing_comment = format!("# {}", clean);
            } else if existing_line.contains('#') {
                let mut in_s = false;
                let mut in_d = false;
                for (i, ch) in existing_line.char_indices() {
                    if ch == '"' && !in_s {
                        in_d = !in_d;
                    } else if ch == '\'' && !in_d {
                        in_s = !in_s;
                    } else if ch == '#' && !in_s && !in_d {
                        existing_comment = existing_line[i..].to_string();
                        break;
                    }
                }
            }
            if !existing_comment.is_empty() {
                line_to_put = format!("{}  {}", line_to_put, existing_comment);
            }
            format!(
                "{}{}{}",
                &sub_text[..m_key.start()],
                line_to_put,
                &sub_text[m_key.end()..]
            )
        } else {
            if let Some(ref clean) = clean_description {
                line_to_put = format!("{}  # {}", line_to_put, clean);
            }
            format!("\n{}{}", line_to_put, sub_text)
        };
        format!(
            "{}{}{}",
            &prof_block[..sub_start],
            new_sub_text,
            &prof_block[sub_end..]
        )
    } else {
        if let Some(ref clean) = clean_description {
            line_to_put = format!("{}  # {}", line_to_put, clean);
        }
        format!(
            "{}\n[profiles.{}.{}]\n{}\n\n",
            prof_block.trim_end(),
            profile,
            section_sub,
            line_to_put
        )
    };

    Ok(format!(
        "{}{}{}",
        &working_content[..p_start],
        new_prof_block,
        &working_content[p_end..]
    ))
}

pub fn set_profile_binding(
    config_path: &Path,
    profile_name: &str,
    button_or_combo: &str,
    chord_keys: &[String],
    description: Option<&str>,
) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path.display(), e))?;
    let new_content = update_toml_binding(
        &content,
        profile_name,
        button_or_combo,
        chord_keys,
        description,
    )?;
    load_config_from_str(&new_content)
        .map_err(|e| format!("Failed to apply binding (syntax error): {}", e))?;
    crate::config::write_private_atomic(config_path, new_content.as_bytes())
}
