"""Visual diagram generator for 8BitDo Micro key mappings.

Generates standalone vector SVG diagrams and interactive HTML overlays showing
the 8BitDo Micro controller line-art with guide pointers / callouts indicating
each physical button's remapped keys, chords, or commands.
"""

from __future__ import annotations

import base64
import html
from importlib.resources import files
from pathlib import Path

from .config import ButtonAction, ProfileConfig
from .protocol import rotate_dpad_button

# Canvas dimensions
CANVAS_WIDTH = 1200
CANVAS_HEIGHT = 650

# Controller image placement inside canvas
CONTROLLER_X = 334
CONTROLLER_Y = 220
CONTROLLER_WIDTH = 532
CONTROLLER_HEIGHT = 329

# Colors for button categories
COLOR_DPAD = "#a6e3a1"      # Green
COLOR_ACTION = "#f9e2af"    # Peach / Gold
COLOR_BUMPERS = "#fab387"   # Orange
COLOR_TRIGGERS = "#f38ba8"  # Rose
COLOR_CENTER = "#89b4fa"    # Blue
COLOR_SPECIAL = "#cba6f7"   # Mauve / Purple

BUTTON_METADATA: dict[str, tuple[str, str]] = {
    # (Display label, Color)
    "L2": ("L2", COLOR_TRIGGERS),
    "L": ("L1", COLOR_BUMPERS),
    "UP": ("▲ UP", COLOR_DPAD),
    "LEFT": ("◀ LEFT", COLOR_DPAD),
    "SELECT": ("- SELECT", COLOR_CENTER),
    "DOWN": ("▼ DOWN", COLOR_DPAD),
    "RIGHT": ("▶ RIGHT", COLOR_DPAD),
    "STAR": ("★ STAR", COLOR_SPECIAL),

    "R2": ("R2", COLOR_TRIGGERS),
    "R": ("R1", COLOR_BUMPERS),
    "START": ("+ START", COLOR_CENTER),
    "X": ("X", COLOR_ACTION),
    "Y": ("Y", COLOR_ACTION),
    "A": ("A", COLOR_ACTION),
    "B": ("B", COLOR_ACTION),
    "HOME": ("♥ HOME", COLOR_SPECIAL),
}

# Leader line waypoints: (button_key, (origin_x, origin_y), (elbow1_x, elbow1_y), (elbow2_x, elbow2_y), (target_x, target_y))
LEFT_CALLOUTS = (
    ("L2", (474, 272), (474, 120), (330, 105), (280, 105)),
    ("L", (524, 272), (524, 180), (350, 165), (280, 165)),
    ("UP", (489, 346), (430, 265), (340, 225), (280, 225)),
    ("LEFT", (460, 375), (380, 305), (340, 285), (280, 285)),
    ("SELECT", (542, 334), (480, 335), (380, 345), (280, 345)),
    ("DOWN", (489, 404), (430, 405), (360, 405), (280, 405)),
    ("RIGHT", (518, 375), (480, 445), (380, 465), (280, 465)),
    ("STAR", (556, 418), (480, 495), (380, 525), (280, 525)),
)

RIGHT_CALLOUTS = (
    ("R2", (694, 272), (694, 120), (870, 105), (920, 105)),
    ("R", (644, 272), (644, 180), (850, 165), (920, 165)),
    ("START", (614, 334), (680, 265), (840, 225), (920, 225)),
    ("X", (672, 340), (740, 295), (840, 285), (920, 285)),
    ("Y", (636, 376), (710, 355), (840, 345), (920, 345)),
    ("A", (710, 376), (770, 390), (850, 405), (920, 405)),
    ("B", (672, 414), (740, 445), (840, 465), (920, 465)),
    ("HOME", (600, 418), (670, 495), (820, 525), (920, 525)),
)


def _load_embedded_png_base64() -> str:
    """Return the base64-encoded micro-controller.png embedded within the package."""
    try:
        data = files("eightbitdo_micro_focus_switch.assets").joinpath("micro-controller.png").read_bytes()
        return base64.b64encode(data).decode("ascii")
    except Exception:
        fallback = Path("/home/dinu/Projects/micro-controller.png")
        if fallback.exists():
            return base64.b64encode(fallback.read_bytes()).decode("ascii")
        raise FileNotFoundError("micro-controller.png asset not found")


def _format_action_text(action: ButtonAction | None) -> str:
    """Format a ButtonAction into a readable keyboard chord string."""
    if action is None:
        return "None"
    if action.exec_cmd:
        return f"⚡ {action.exec_cmd}"
    if not action.keys:
        return "None"
    # Pretty keycap names
    formatted: list[str] = []
    for k in action.keys:
        clean = k.replace("KEY_", "").title()
        if clean == "Leftctrl":
            clean = "Ctrl"
        elif clean == "Leftshift":
            clean = "Shift"
        elif clean == "Leftalt":
            clean = "Alt"
        elif clean == "Leftmeta":
            clean = "Super"
        elif clean == "Kpasterisk":
            clean = "Num *"
        elif clean == "Kpminus":
            clean = "Num -"
        formatted.append(clean)
    return " + ".join(formatted)


def generate_svg_diagram(profile: ProfileConfig) -> str:
    """Generate a self-contained vector SVG diagram illustrating the profile's button mappings."""
    b64_png = _load_embedded_png_base64()
    turn_str = f"{profile.turn}°" if profile.turn != 0 else "Normal (0°)"
    desc_str = html.escape(profile.description or f"Profile: {profile.name}")
    title_str = html.escape(f"8BitDo Micro Keymap — {profile.name.upper()}")

    lines: list[str] = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="650" viewBox="0 0 1200 650" '
        'font-family="DejaVu Sans, Noto Sans, sans-serif">',
        '  <rect width="1200" height="650" fill="#11111b" rx="16" />',
        '  <defs>',
        '    <filter id="glow" x="-40%" y="-40%" width="180%" height="180%">',
        '      <feGaussianBlur stdDeviation="3.5" result="blur" />',
        '      <feComposite in="SourceGraphic" in2="blur" operator="over" />',
        '    </filter>',
        '    <linearGradient id="card-grad" x1="0%" y1="0%" x2="100%" y2="100%">',
        '      <stop offset="0%" stop-color="#1e1e2e" />',
        '      <stop offset="100%" stop-color="#181825" />',
        '    </linearGradient>',
        '  </defs>',
        # Header
        f'  <text x="600" y="38" text-anchor="middle" fill="#cdd6f4" font-size="22" font-weight="bold" letter-spacing="1">{title_str}</text>',
        f'  <text x="600" y="60" text-anchor="middle" fill="#89b4fa" font-size="12" font-weight="600">Mode: Keyboard (K)  •  D-Pad Turn: {turn_str}</text>',
        f'  <text x="600" y="80" text-anchor="middle" fill="#a6adc8" font-size="12">{desc_str}</text>',
        # Controller Image
        f'  <image href="data:image/png;base64,{b64_png}" x="{CONTROLLER_X}" y="{CONTROLLER_Y}" width="{CONTROLLER_WIDTH}" height="{CONTROLLER_HEIGHT}" />',
    ]

    # Render Left Callouts
    for key, p0, p1, p2, p3 in LEFT_CALLOUTS:
        label, col = BUTTON_METADATA.get(key, (key, "#cdd6f4"))
        logical_key = rotate_dpad_button(key, profile.turn)
        action = profile.buttons.get(logical_key)
        action_text = html.escape(_format_action_text(action))
        safe_label = html.escape(label)

        pts = f"{p0[0]},{p0[1]} {p1[0]},{p1[1]} {p2[0]},{p2[1]} {p3[0]},{p3[1]}"
        lines.append(f'  <polyline points="{pts}" fill="none" stroke="{col}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" opacity="0.85" />')
        lines.append(f'  <circle cx="{p0[0]}" cy="{p0[1]}" r="4.5" fill="{col}" filter="url(#glow)" />')

        bx, by = p3[0] - 250, p3[1] - 18
        lines.append(f'  <g transform="translate({bx},{by})">')
        lines.append('    <rect width="250" height="36" rx="8" fill="url(#card-grad)" stroke="#313244" stroke-width="1.2" />')
        lines.append(f'    <rect x="6" y="6" width="68" height="24" rx="5" fill="{col}" fill-opacity="0.18" stroke="{col}" stroke-width="1" />')
        lines.append(f'    <text x="40" y="22" text-anchor="middle" fill="{col}" font-size="11" font-weight="bold">{safe_label}</text>')
        lines.append(f'    <text x="84" y="23" text-anchor="start" fill="#cdd6f4" font-size="12" font-weight="bold">{action_text}</text>')
        lines.append('  </g>')

    # Render Right Callouts
    for key, p0, p1, p2, p3 in RIGHT_CALLOUTS:
        label, col = BUTTON_METADATA.get(key, (key, "#cdd6f4"))
        logical_key = rotate_dpad_button(key, profile.turn)
        action = profile.buttons.get(logical_key)
        action_text = html.escape(_format_action_text(action))
        safe_label = html.escape(label)

        pts = f"{p0[0]},{p0[1]} {p1[0]},{p1[1]} {p2[0]},{p2[1]} {p3[0]},{p3[1]}"
        lines.append(f'  <polyline points="{pts}" fill="none" stroke="{col}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" opacity="0.85" />')
        lines.append(f'  <circle cx="{p0[0]}" cy="{p0[1]}" r="4.5" fill="{col}" filter="url(#glow)" />')

        bx, by = p3[0], p3[1] - 18
        lines.append(f'  <g transform="translate({bx},{by})">')
        lines.append('    <rect width="250" height="36" rx="8" fill="url(#card-grad)" stroke="#313244" stroke-width="1.2" />')
        lines.append(f'    <rect x="6" y="6" width="68" height="24" rx="5" fill="{col}" fill-opacity="0.18" stroke="{col}" stroke-width="1" />')
        lines.append(f'    <text x="40" y="22" text-anchor="middle" fill="{col}" font-size="11" font-weight="bold">{safe_label}</text>')
        lines.append(f'    <text x="84" y="23" text-anchor="start" fill="#cdd6f4" font-size="12" font-weight="bold">{action_text}</text>')
        lines.append('  </g>')

    # Combos & Chords Footer
    if profile.combos:
        lines.append('  <!-- Combos & Simultaneous Chords Footer -->')
        lines.append('  <g transform="translate(140, 568)">')
        lines.append('    <rect width="920" height="56" rx="10" fill="#181825" stroke="#45475a" stroke-width="1.2" />')
        lines.append('    <rect x="12" y="14" width="90" height="28" rx="6" fill="#fab387" fill-opacity="0.18" stroke="#fab387" stroke-width="1" />')
        lines.append('    <text x="57" y="32" text-anchor="middle" fill="#fab387" font-size="11" font-weight="bold">⚡ COMBOS</text>')
        chip_x = 114
        for combo_btns, act in profile.combos.items():
            btn_display = " + ".join(BUTTON_METADATA.get(b, (b, ""))[0] for b in sorted(combo_btns))
            act_display = _format_action_text(act)
            chip_text = f"{btn_display}  ➔  {act_display}"
            chip_w = max(170, len(chip_text) * 8 + 24)
            if chip_x + chip_w > 908:
                break
            arrow_x = 12 + len(btn_display) * 7 + 4
            act_x = arrow_x + 18
            lines.append(f'    <g transform="translate({chip_x}, 12)">')
            lines.append(f'      <rect width="{chip_w}" height="32" rx="6" fill="#1e1e2e" stroke="#313244" stroke-width="1" />')
            lines.append(f'      <text x="12" y="21" fill="#f9e2af" font-size="11" font-weight="bold">{html.escape(btn_display)}</text>')
            lines.append(f'      <text x="{arrow_x}" y="21" fill="#89b4fa" font-size="11">➔</text>')
            lines.append(f'      <text x="{act_x}" y="21" fill="#cdd6f4" font-size="11" font-weight="bold">{html.escape(act_display)}</text>')
            lines.append('    </g>')
            chip_x += chip_w + 10
        lines.append('  </g>')
    else:
        lines.append('  <!-- Combos Hint Footer -->')
        lines.append('  <g transform="translate(240, 574)">')
        lines.append('    <rect width="720" height="42" rx="8" fill="#181825" stroke="#313244" stroke-width="1" />')
        lines.append(f'    <text x="360" y="26" text-anchor="middle" fill="#6c7086" font-size="11">💡 Tip: Define multi-button chords like "L+R" under [profiles.{profile.name}.combos]</text>')
        lines.append('  </g>')

    lines.append('</svg>')
    return '\n'.join(lines)


def generate_html_viewer(profiles: dict[str, ProfileConfig], active_profile_name: str) -> str:
    """Generate an interactive HTML overlay viewer with profile tabs."""
    tabs_html: list[str] = []
    svgs_html: list[str] = []

    for name, prof in profiles.items():
        is_active = (name == active_profile_name)
        active_class = "active" if is_active else ""
        tabs_html.append(
            f'<button class="tab-btn {active_class}" onclick="selectProfile(\'{name}\')">{name.upper()}</button>'
        )
        svg_content = generate_svg_diagram(prof)
        display_style = "block" if is_active else "none"
        svgs_html.append(
            f'<div id="profile-{name}" class="profile-view" style="display:{display_style};">{svg_content}</div>'
        )

    return f"""<!DOCTYPE html>
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
    .tab-btn:hover {{
      background: #313244;
      color: #cdd6f4;
    }}
    .tab-btn.active {{
      background: #89b4fa;
      color: #11111b;
      border-color: #89b4fa;
    }}
    .viewer-container {{
      max-width: 1200px;
      width: 100%;
      border-radius: 12px;
      overflow: hidden;
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    }}
    svg {{
      width: 100%;
      height: auto;
      display: block;
    }}
  </style>
</head>
<body>
  <div class="tabs-bar">
    {''.join(tabs_html)}
  </div>
  <div class="viewer-container">
    {''.join(svgs_html)}
  </div>
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
</html>
"""
