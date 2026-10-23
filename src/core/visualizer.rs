//! Visual SVG diagram and interactive HTML overlay generator for 8BitDo Micro key mappings.

use std::collections::HashMap;

use base64::Engine as _;

use crate::config::{ButtonAction, ProfileConfig};
use crate::protocol::rotate_dpad_button;

pub const CANVAS_WIDTH: u32 = 1200;
pub const CANVAS_HEIGHT: u32 = 650;
pub const CONTROLLER_X: u32 = 334;
pub const CONTROLLER_Y: u32 = 220;
pub const CONTROLLER_WIDTH: u32 = 532;
pub const CONTROLLER_HEIGHT: u32 = 329;

pub const COLOR_DPAD: &str = "#a6e3a1";
pub const COLOR_ACTION: &str = "#f9e2af";
pub const COLOR_BUMPERS: &str = "#fab387";
pub const COLOR_TRIGGERS: &str = "#f38ba8";
pub const COLOR_CENTER: &str = "#89b4fa";
pub const COLOR_SPECIAL: &str = "#cba6f7";

const EMBEDDED_CONTROLLER_PNG: &[u8] = include_bytes!("../../assets/micro-controller.png");

pub fn button_metadata(key: &str) -> (&'static str, &'static str) {
    match key {
        "L" => ("L1", COLOR_BUMPERS),
        "L2" => ("L2", COLOR_TRIGGERS),
        "UP" => ("▲ UP", COLOR_DPAD),
        "LEFT" => ("◀ LEFT", COLOR_DPAD),
        "SELECT" => ("- SELECT", COLOR_CENTER),
        "DOWN" => ("▼ DOWN", COLOR_DPAD),
        "RIGHT" => ("▶ RIGHT", COLOR_DPAD),
        "STAR" => ("★ STAR", COLOR_SPECIAL),
        "R" => ("R1", COLOR_BUMPERS),
        "R2" => ("R2", COLOR_TRIGGERS),
        "START" => ("+ START", COLOR_CENTER),
        "X" => ("X", COLOR_ACTION),
        "Y" => ("Y", COLOR_ACTION),
        "A" => ("A", COLOR_ACTION),
        "B" => ("B", COLOR_ACTION),
        "HOME" => ("♥ HOME", COLOR_SPECIAL),
        _ => ("?", "#cdd6f4"),
    }
}

type CalloutSpec = (&'static str, (i32, i32), (i32, i32), (i32, i32), (i32, i32));

const LEFT_CALLOUTS: &[CalloutSpec] = &[
    ("L", (474, 272), (474, 120), (330, 105), (280, 105)),
    ("L2", (524, 272), (524, 180), (350, 165), (280, 165)),
    ("UP", (489, 346), (430, 265), (340, 225), (280, 225)),
    ("LEFT", (460, 375), (380, 305), (340, 285), (280, 285)),
    ("SELECT", (542, 334), (480, 335), (380, 345), (280, 345)),
    ("DOWN", (489, 404), (430, 405), (360, 405), (280, 405)),
    ("RIGHT", (518, 375), (480, 445), (380, 465), (280, 465)),
    ("STAR", (556, 418), (480, 495), (380, 525), (280, 525)),
];

const RIGHT_CALLOUTS: &[CalloutSpec] = &[
    ("R", (694, 272), (694, 120), (870, 105), (920, 105)),
    ("R2", (644, 272), (644, 180), (850, 165), (920, 165)),
    ("START", (614, 334), (680, 265), (840, 225), (920, 225)),
    ("X", (672, 340), (740, 295), (840, 285), (920, 285)),
    ("Y", (636, 376), (710, 355), (840, 345), (920, 345)),
    ("A", (710, 376), (770, 390), (850, 405), (920, 405)),
    ("B", (672, 414), (740, 445), (840, 465), (920, 465)),
    ("HOME", (600, 418), (670, 495), (820, 525), (920, 525)),
];

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn format_action_text(action: Option<&ButtonAction>) -> String {
    let Some(act) = action else {
        return "None".to_string();
    };
    if let Some(ref ex) = act.exec_cmd {
        return format!("⚡ {}", ex);
    }
    if act.keys.is_empty() {
        return "None".to_string();
    }
    let formatted: Vec<String> = act
        .keys
        .iter()
        .map(|k| {
            let clean = k.strip_prefix("KEY_").unwrap_or(k);
            match clean.to_uppercase().as_str() {
                "LEFTCTRL" | "RIGHTCTRL" | "CTRL" => "Ctrl".to_string(),
                "LEFTSHIFT" | "RIGHTSHIFT" | "SHIFT" => "Shift".to_string(),
                "LEFTALT" | "RIGHTALT" | "ALT" => "Alt".to_string(),
                "LEFTMETA" | "RIGHTMETA" | "SUPER" => "Super".to_string(),
                "KPASTERISK" => "Num *".to_string(),
                "KPMINUS" => "Num -".to_string(),
                other => {
                    let mut chars = other.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                    }
                }
            }
        })
        .collect();
    formatted.join(" + ")
}

pub fn generate_svg_diagram(profile: &ProfileConfig) -> String {
    let b64_png = base64::engine::general_purpose::STANDARD.encode(EMBEDDED_CONTROLLER_PNG);
    let turn_str = if profile.turn != 0 {
        format!("{}°", profile.turn)
    } else {
        "Normal (0°)".to_string()
    };
    let raw_desc = if profile.description.is_empty() {
        format!("Profile: {}", profile.name)
    } else {
        profile.description.clone()
    };
    let desc_str = escape_html(&raw_desc);
    let title_str = escape_html(&format!("8BitDo Micro Keymap — {}", profile.name.to_uppercase()));

    let mut lines = vec![
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="650" viewBox="0 0 1200 650" font-family="DejaVu Sans, Noto Sans, sans-serif">"##.to_string(),
        r##"  <rect width="1200" height="650" fill="#11111b" rx="16" />"##.to_string(),
        r##"  <defs>"##.to_string(),
        r##"    <filter id="glow" x="-40%" y="-40%" width="180%" height="180%">"##.to_string(),
        r##"      <feGaussianBlur stdDeviation="3.5" result="blur" />"##.to_string(),
        r##"      <feComposite in="SourceGraphic" in2="blur" operator="over" />"##.to_string(),
        r##"    </filter>"##.to_string(),
        r##"    <linearGradient id="card-grad" x1="0%" y1="0%" x2="100%" y2="100%">"##.to_string(),
        r##"      <stop offset="0%" stop-color="#1e1e2e" />"##.to_string(),
        r##"      <stop offset="100%" stop-color="#181825" />"##.to_string(),
        r##"    </linearGradient>"##.to_string(),
        r##"  </defs>"##.to_string(),
        format!(
            r##"  <text x="600" y="38" text-anchor="middle" fill="#cdd6f4" font-size="22" font-weight="bold" letter-spacing="1">{}</text>"##,
            title_str
        ),
        format!(
            r##"  <text x="600" y="60" text-anchor="middle" fill="#89b4fa" font-size="12" font-weight="600">Mode: Keyboard (K)  •  D-Pad Turn: {}</text>"##,
            turn_str
        ),
        format!(
            r##"  <text x="600" y="80" text-anchor="middle" fill="#a6adc8" font-size="12">{}</text>"##,
            desc_str
        ),
        format!(
            r##"  <image href="data:image/png;base64,{}" x="{}" y="{}" width="{}" height="{}" />"##,
            b64_png, CONTROLLER_X, CONTROLLER_Y, CONTROLLER_WIDTH, CONTROLLER_HEIGHT
        ),
    ];

    for (is_left, callouts) in [(true, LEFT_CALLOUTS), (false, RIGHT_CALLOUTS)] {
        for &(key, p0, p1, p2, p3) in callouts {
            let (label, col) = button_metadata(key);
            let logical_key =
                rotate_dpad_button(key, profile.turn).unwrap_or_else(|_| key.to_string());
            let action = profile.buttons.get(&logical_key);
            let action_text = escape_html(&format_action_text(action));
            let safe_label = escape_html(label);
            let desc_opt = action.and_then(|a| a.description.as_deref()).filter(|d| !d.is_empty());
            let tooltip_str = if let Some(d) = desc_opt {
                format!("{}: {} — {}", label, action_text, d)
            } else {
                format!("{}: {}", label, action_text)
            };
            let safe_tooltip = escape_html(&tooltip_str);

            let pts = format!(
                "{},{} {},{} {},{} {},{}",
                p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1
            );
            lines.push(format!(
                r##"  <polyline points="{}" fill="none" stroke="{}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" opacity="0.85" />"##,
                pts, col
            ));
            lines.push(format!(
                r##"  <circle cx="{}" cy="{}" r="4.5" fill="{}" filter="url(#glow)" />"##,
                p0.0, p0.1, col
            ));

            let bx = if is_left { p3.0 - 250 } else { p3.0 };
            let by = p3.1 - 18;
            lines.push(format!(
                r##"  <g class="callout-card" transform="translate({},{})">"##,
                bx, by
            ));
            lines.push(format!(r##"    <title>{}</title>"##, safe_tooltip));
            lines.push(
                r##"    <rect width="250" height="36" rx="8" fill="url(#card-grad)" stroke="#313244" stroke-width="1.2" />"##
                    .to_string(),
            );
            lines.push(format!(
                r##"    <rect x="6" y="6" width="68" height="24" rx="5" fill="{}" fill-opacity="0.18" stroke="{}" stroke-width="1" />"##,
                col, col
            ));
            lines.push(format!(
                r##"    <text x="40" y="22" text-anchor="middle" fill="{}" font-size="11" font-weight="bold">{}</text>"##,
                col, safe_label
            ));
            if let Some(d) = desc_opt {
                let desc_short = if d.chars().count() <= 22 {
                    d.to_string()
                } else {
                    format!("{}…", d.chars().take(21).collect::<String>())
                };
                lines.push(format!(
                    r##"    <text x="84" y="17" text-anchor="start" fill="#cdd6f4" font-size="11" font-weight="bold">{}</text>"##,
                    action_text
                ));
                lines.push(format!(
                    r##"    <text x="84" y="29" text-anchor="start" fill="#a6adc8" font-size="9.5">{}</text>"##,
                    escape_html(&desc_short)
                ));
            } else {
                lines.push(format!(
                    r##"    <text x="84" y="23" text-anchor="start" fill="#cdd6f4" font-size="12" font-weight="bold">{}</text>"##,
                    action_text
                ));
            }
            lines.push("  </g>".to_string());
        }
    }

    if !profile.combos.is_empty() {
        lines.push("  <!-- Combos & Simultaneous Chords Footer -->".to_string());
        lines.push(r##"  <g transform="translate(140, 568)">"##.to_string());
        lines.push(
            r##"    <rect width="920" height="56" rx="10" fill="#181825" stroke="#45475a" stroke-width="1.2" />"##
                .to_string(),
        );
        lines.push(
            r##"    <rect x="12" y="14" width="90" height="28" rx="6" fill="#fab387" fill-opacity="0.18" stroke="#fab387" stroke-width="1" />"##
                .to_string(),
        );
        lines.push(
            r##"    <text x="57" y="32" text-anchor="middle" fill="#fab387" font-size="11" font-weight="bold">⚡ COMBOS</text>"##
                .to_string(),
        );
        let mut chip_x = 114;
        for (combo_btns, act) in &profile.combos {
            let btn_display: Vec<&str> = combo_btns
                .iter()
                .map(|b| button_metadata(b).0)
                .collect();
            let btn_display_str = btn_display.join(" + ");
            let act_display = format_action_text(Some(act));
            let chip_w = std::cmp::max(
                170,
                ((btn_display_str.len() + act_display.len() + 6) as i32) * 8 + 24,
            );
            if chip_x + chip_w > 908 {
                break;
            }
            let arrow_x = 12 + (btn_display_str.len() as i32) * 7 + 4;
            let act_x = arrow_x + 18;
            lines.push(format!(
                r##"    <g class="combo-chip" transform="translate({}, 12)">"##,
                chip_x
            ));
            lines.push(format!(
                r##"      <rect width="{}" height="32" rx="6" fill="#1e1e2e" stroke="#313244" stroke-width="1" />"##,
                chip_w
            ));
            lines.push(format!(
                r##"      <text x="12" y="21" fill="#f9e2af" font-size="11" font-weight="bold">{}</text>"##,
                escape_html(&btn_display_str)
            ));
            lines.push(format!(
                r##"      <text x="{}" y="21" fill="#89b4fa" font-size="11">➔</text>"##,
                arrow_x
            ));
            lines.push(format!(
                r##"      <text x="{}" y="21" fill="#cdd6f4" font-size="11" font-weight="bold">{}</text>"##,
                act_x,
                escape_html(&act_display)
            ));
            lines.push("    </g>".to_string());
            chip_x += chip_w + 10;
        }
        lines.push("  </g>".to_string());
    } else {
        lines.push("  <!-- Combos Hint Footer -->".to_string());
        lines.push(r##"  <g transform="translate(240, 574)">"##.to_string());
        lines.push(
            r##"    <rect width="720" height="42" rx="8" fill="#181825" stroke="#313244" stroke-width="1" />"##
                .to_string(),
        );
        lines.push(format!(
            r##"    <text x="360" y="26" text-anchor="middle" fill="#6c7086" font-size="11">💡 Tip: Define multi-button chords like "L+R" under [profiles.{}.combos]</text>"##,
            escape_html(&profile.name)
        ));
        lines.push("  </g>".to_string());
    }

    lines.push("</svg>".to_string());
    lines.join("\n")
}

pub fn generate_html_viewer(
    profiles: &HashMap<String, ProfileConfig>,
    profile_order: &[String],
    active_profile_name: &str,
) -> String {
    let mut tabs_html = Vec::new();
    let mut svgs_html = Vec::new();

    for name in profile_order {
        let Some(prof) = profiles.get(name) else {
            continue;
        };
        let is_active = name == active_profile_name;
        let active_class = if is_active { "active" } else { "" };
        tabs_html.push(format!(
            r##"<button class="tab-btn {}" onclick="selectProfile('{}')">{}</button>"##,
            active_class,
            name,
            name.to_uppercase()
        ));
        let svg_content = generate_svg_diagram(prof);
        let display_style = if is_active { "block" } else { "none" };
        svgs_html.push(format!(
            r##"<div id="profile-{}" class="profile-view" style="display:{};">{}</div>"##,
            name, display_style, svg_content
        ));
    }

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>8BitDo Micro Keymap Guide</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      background: #11111b;
      color: #cdd6f4;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
      display: flex;
      flex-direction: column;
      align-items: center;
      min-height: 100vh;
      padding: 16px;
    }}
    .tabs-bar {{
      display: flex;
      gap: 8px;
      margin-bottom: 16px;
      flex-wrap: wrap;
      justify-content: center;
    }}
    .tab-btn {{
      background: #1e1e2e;
      color: #a6adc8;
      border: 1px solid #313244;
      padding: 8px 18px;
      border-radius: 8px;
      font-weight: 600;
      font-size: 13px;
      cursor: pointer;
      transition: all 0.15s ease;
    }}
    .tab-btn:hover {{ background: #313244; color: #cdd6f4; }}
    .tab-btn.active {{ background: #89b4fa; color: #11111b; border-color: #89b4fa; }}
    .viewer-container {{
      max-width: 1200px;
      width: 100%;
      border-radius: 12px;
      overflow: hidden;
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    }}
    svg {{ width: 100%; height: auto; display: block; }}
  </style>
</head>
<body>
  <div class="tabs-bar">{}</div>
  <div class="viewer-container">{}</div>
  <script>
    function selectProfile(name) {{
      document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
      document.querySelectorAll('.profile-view').forEach(v => v.style.display = 'none');
      const btn = Array.from(document.querySelectorAll('.tab-btn')).find(b => b.innerText.toLowerCase() === name.toLowerCase());
      if (btn) btn.classList.add('active');
      const target = document.getElementById('profile-' + name);
      if (target) target.style.display = 'block';
    }}
  </script>
</body>
</html>"##,
        tabs_html.join(""),
        svgs_html.join("")
    )
}
