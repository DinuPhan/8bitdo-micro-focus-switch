"""Configuration loader and profile matcher for 8bitdo-micro-focus-switch."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
import re
import tomllib
from typing import Any

from .protocol import (
    FACTORY_BUTTON_HID_USAGES,
    HID_TO_NAME,
    MicroHardwareProfile,
    PHYSICAL_BUTTONS,
    TURN_CODE_TO_DEGREES,
    TURN_DEGREES_TO_CODE,
    get_builtin_hardware_preset,
    key_name_to_evdev,
    key_name_to_hid,
    normalize_button_name,
)

DEFAULT_CONFIG_PATH = Path.home() / ".config" / "8bitdo-micro" / "profiles.toml"


@dataclass(slots=True)
class ButtonAction:
    """Action bound to a single 8BitDo Micro physical button in a profile."""

    keys: tuple[str, ...] = ()
    evdev_codes: tuple[int, ...] = ()
    hid_usages: tuple[int, int, int, int] = (0, 0, 0, 0)
    exec_cmd: str | None = None
    description: str | None = None

    @classmethod
    def from_raw(cls, raw: str | list[Any] | dict[str, Any], description: str | None = None) -> ButtonAction:
        exec_cmd: str | None = None
        raw_keys: list[str | int] = []

        if isinstance(raw, str):
            stripped = raw.strip()
            if stripped.lower().startswith("exec:"):
                exec_cmd = stripped[5:].strip()
            elif stripped and stripped.upper() != "NONE":
                raw_keys = [k.strip() for k in stripped.split("+") if k.strip()]
        elif isinstance(raw, (list, tuple)):
            raw_keys = [k for k in raw if str(k).strip().upper() not in ("", "NONE")]
        elif isinstance(raw, dict):
            if "exec" in raw and raw["exec"]:
                exec_cmd = str(raw["exec"]).strip()
            if "description" in raw and raw["description"] and not description:
                description = str(raw["description"]).strip()
            elif "desc" in raw and raw["desc"] and not description:
                description = str(raw["desc"]).strip()
            k_val = raw.get("keys", raw.get("key", []))
            if isinstance(k_val, str):
                raw_keys = [k.strip() for k in k_val.split("+") if k.strip()]
            elif isinstance(k_val, (list, tuple)):
                raw_keys = [k for k in k_val if str(k).strip().upper() not in ("", "NONE")]
        else:
            raise ValueError(f"Unsupported button action specification: {raw!r}")

        key_names: list[str] = []
        evdev_codes: list[int] = []
        hid_list: list[int] = []

        for k in raw_keys:
            k_str = str(k).strip().upper()
            key_names.append(k_str)
            evdev_codes.append(key_name_to_evdev(k))
            try:
                if len(hid_list) < 4:
                    hid_list.append(key_name_to_hid(k))
            except ValueError:
                # Non-HID evdev keys (e.g. XF86 media keys) work in uinput mode
                pass

        padded_hid = tuple(hid_list[:4]) + (0,) * (4 - len(hid_list[:4]))
        return cls(
            keys=tuple(key_names),
            evdev_codes=tuple(evdev_codes),
            hid_usages=(padded_hid[0], padded_hid[1], padded_hid[2], padded_hid[3]),
            exec_cmd=exec_cmd,
            description=description,
        )


def parse_combo_buttons(key: str | list[str] | tuple[str, ...] | frozenset[str]) -> frozenset[str]:
    """Parse a combo specification string like 'L+R' or 'L1 + R1' into canonical button names."""
    if isinstance(key, frozenset):
        return key
    if isinstance(key, str):
        parts = [p.strip() for p in key.split("+") if p.strip()]
    elif isinstance(key, (list, tuple, set)):
        parts = [str(p).strip() for p in key if str(p).strip()]
    else:
        raise ValueError(f"Invalid combo definition: {key!r}")
    if len(parts) < 2:
        raise ValueError(f"Combos require at least 2 simultaneous buttons (got {key!r})")
    normalized = [normalize_button_name(p) for p in parts]
    return frozenset(normalized)


@dataclass
class MatchRule:
    """A single window matching rule checking class and/or title regexes."""

    match_class: str | None = None
    match_title: str | None = None
    _class_re: re.Pattern[str] | None = field(default=None, init=False, repr=False)
    _title_re: re.Pattern[str] | None = field(default=None, init=False, repr=False)

    def __post_init__(self) -> None:
        if self.match_class:
            self._class_re = re.compile(self.match_class, re.IGNORECASE)
        if self.match_title:
            self._title_re = re.compile(self.match_title, re.IGNORECASE)

    def matches(self, window_class: str, window_title: str) -> bool:
        if self._class_re is None and self._title_re is None:
            return False
        if self._class_re is not None and not self._class_re.search(window_class or ""):
            return False
        if self._title_re is not None and not self._title_re.search(window_title or ""):
            return False
        return True


@dataclass
class ProfileConfig:
    """A named mapping profile that can be activated by window focus or flashed via BLE."""

    name: str
    description: str = ""
    match_class: str | None = None
    match_title: str | None = None
    match_rules: list[MatchRule] = field(default_factory=list)
    turn: int = 0  # degrees: 0, 90, 180, 270
    combo_timeout_ms: int = 50  # Window in ms to detect simultaneous combo presses before flushing solo key
    buttons: dict[str, ButtonAction] = field(default_factory=dict)
    combos: dict[frozenset[str], ButtonAction] = field(default_factory=dict)
    _class_re: re.Pattern[str] | None = field(default=None, init=False, repr=False)
    _title_re: re.Pattern[str] | None = field(default=None, init=False, repr=False)

    def __post_init__(self) -> None:
        if self.turn in TURN_CODE_TO_DEGREES and self.turn not in TURN_DEGREES_TO_CODE:
            self.turn = TURN_CODE_TO_DEGREES[self.turn]
        elif self.turn not in TURN_DEGREES_TO_CODE:
            raise ValueError(
                f"Profile {self.name!r}: invalid turn={self.turn!r} (expected 0, 90, 180, or 270)"
            )
        if self.match_class:
            self._class_re = re.compile(self.match_class, re.IGNORECASE)
        if self.match_title:
            self._title_re = re.compile(self.match_title, re.IGNORECASE)

    def matches_window(self, window_class: str, window_title: str) -> bool:
        """Return True if this profile's match rules match the focused window."""
        if self.match_rules:
            for rule in self.match_rules:
                if rule.matches(window_class, window_title):
                    return True
            if self._class_re is None and self._title_re is None:
                return False
        if self._class_re is None and self._title_re is None:
            return False
        if self._class_re is not None and not self._class_re.search(window_class or ""):
            return False
        if self._title_re is not None and not self._title_re.search(window_title or ""):
            return False
        return True

    @property
    def software_only_features(self) -> tuple[int, int]:
        """Return (num_combos, num_exec_cmds) defined in this profile that are software-only."""
        num_combos = len(self.combos)
        num_exec = sum(1 for a in self.buttons.values() if a.exec_cmd) + sum(
            1 for a in self.combos.values() if a.exec_cmd
        )
        return num_combos, num_exec

    def to_hardware_profile(self, *, enabled: bool = True) -> MicroHardwareProfile:
        """Convert this profile into a 180-byte MicroHardwareProfile for BLE GATT flashing.

        Note: The 180-byte hardware record_micro format only stores 1-to-1 button mappings
        (up to 4 HID scancodes per physical button). Combos and shell commands (exec_cmd)
        are software-level features handled by the daemon via uinput and cannot be stored
        in hardware memory.
        """
        slots: dict[str, tuple[int, int, int, int]] = {}
        for btn in PHYSICAL_BUTTONS:
            action = self.buttons.get(btn)
            if action is not None:
                slots[btn] = action.hid_usages
            else:
                slots[btn] = (0, 0, 0, 0)
        return MicroHardwareProfile(
            enabled=enabled,
            turn=TURN_DEGREES_TO_CODE[self.turn],
            slots=slots,
        )

    @classmethod
    def from_hardware_profile(
        cls,
        name: str,
        hw: MicroHardwareProfile,
        *,
        description: str = "",
    ) -> ProfileConfig:
        """Create a ProfileConfig from a decoded MicroHardwareProfile."""
        eff = hw.effective_slots()
        buttons: dict[str, ButtonAction] = {}
        for btn in PHYSICAL_BUTTONS:
            usages = eff.get(btn, (0, 0, 0, 0))
            names = [HID_TO_NAME.get(u, f"0x{u:02X}") for u in usages if u != 0]
            buttons[btn] = ButtonAction.from_raw(names)
        return cls(
            name=name,
            description=description,
            turn=hw.turn_degrees,
            buttons=buttons,
        )


@dataclass
class AppConfig:
    """Top-level configuration for 8bitdo-micro-focus-switch."""

    fallback_profile: str = "default"
    backend: str = "uinput"  # "uinput" (instant evdev+uinput) or "ble" (BLE GATT hardware reflash)
    profiles: dict[str, ProfileConfig] = field(default_factory=dict)
    global_combos: dict[frozenset[str], ButtonAction] = field(default_factory=dict)
    global_buttons: dict[str, ButtonAction] = field(default_factory=dict)

    def resolve_profile_for_window(self, window_class: str, window_title: str) -> ProfileConfig:
        """Return the first matching profile for (window_class, window_title), or the fallback profile."""
        for prof in self.profiles.values():
            if prof.name in ("global", "all"):
                continue
            if prof.matches_window(window_class, window_title):
                return prof
        if self.fallback_profile in self.profiles and self.fallback_profile not in ("global", "all"):
            return self.profiles[self.fallback_profile]
        for prof in self.profiles.values():
            if prof.name not in ("global", "all"):
                return prof
        return next(iter(self.profiles.values()))

    def get_profile(self, name: str) -> ProfileConfig:
        """Look up a profile by name from config or built-in presets ('factory', 'anki', 'supermemo')."""
        if name in self.profiles:
            return self.profiles[name]
        for k, v in self.profiles.items():
            if k.lower() == name.strip().lower():
                return v
        hw = get_builtin_hardware_preset(name)
        return ProfileConfig.from_hardware_profile(
            name.strip().lower(),
            hw,
            description=f"Built-in {name} hardware preset",
        )


DEFAULT_TOML_CONTENT = r"""# 8BitDo Micro Focus-Aware Profile Configuration
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
"""


def extract_toml_inline_comments(text: str) -> dict[str, dict[str, dict[Any, str]]]:
    """Parse single-line comments placed inline with button and combo mappings in a TOML document.

    Returns:
        dict mapping lowercase profile name -> {"buttons": {button_name: comment}, "combos": {frozenset: comment}}
    """
    result: dict[str, dict[str, dict[Any, str]]] = {}
    current_section = ""
    for line in text.splitlines():
        line_str = line.strip()
        if not line_str:
            continue
        if line_str.startswith("[") and line_str.endswith("]"):
            current_section = line_str[1:-1].strip()
            continue
        if line_str.startswith("#"):
            continue

        in_single = False
        in_double = False
        escape = False
        comment_idx = -1
        for i, ch in enumerate(line):
            if escape:
                escape = False
                continue
            if ch == "\\" and in_double:
                escape = True
                continue
            if ch == '"' and not in_single:
                in_double = not in_double
            elif ch == "'" and not in_double:
                in_single = not in_single
            elif ch == "#" and not in_single and not in_double:
                comment_idx = i
                break

        if comment_idx != -1:
            code_part = line[:comment_idx].strip()
            comment_text = line[comment_idx + 1:].strip()
            if "=" in code_part and comment_text:
                key_part, _ = code_part.split("=", 1)
                raw_key = key_part.strip().strip("\"'")
                parts = [p.strip().strip("\"'") for p in current_section.split(".")]
                if len(parts) >= 2 and parts[0].lower() == "profiles":
                    prof_name = parts[1].lower()
                    sub = parts[2].lower() if len(parts) > 2 else "auto"
                elif len(parts) >= 1 and parts[0].lower() in ("global", "all"):
                    prof_name = parts[0].lower()
                    sub = parts[1].lower() if len(parts) > 1 else "auto"
                else:
                    prof_name = parts[0].lower()
                    sub = parts[1].lower() if len(parts) > 1 else "auto"

                if sub == "combos" or "+" in raw_key:
                    try:
                        combo_key = parse_combo_buttons(raw_key)
                        result.setdefault(prof_name, {}).setdefault("combos", {})[combo_key] = comment_text
                    except ValueError:
                        pass
                else:
                    try:
                        btn_key = normalize_button_name(raw_key)
                        result.setdefault(prof_name, {}).setdefault("buttons", {})[btn_key] = comment_text
                    except ValueError:
                        pass
    return result


def _lookup_comments(comments_dict: dict[str, Any], name: str) -> dict[str, dict[Any, str]]:
    """Case-insensitive comment dictionary lookup for a profile name."""
    name_lower = name.strip().lower()
    return comments_dict.get(name_lower, {})


def ensure_default_config(path: Path | None = None) -> Path:
    """Create the default profiles.toml if it does not already exist."""
    cfg_path = path if path is not None else DEFAULT_CONFIG_PATH
    if not cfg_path.exists():
        cfg_path.parent.mkdir(parents=True, exist_ok=True)
        cfg_path.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    return cfg_path


def load_config(path: Path | None = None) -> AppConfig:
    """Load AppConfig from a TOML file (creating the default config if using the default path)."""
    cfg_path = path or DEFAULT_CONFIG_PATH
    if not cfg_path.exists():
        if path is None:
            ensure_default_config(cfg_path)
        else:
            raise FileNotFoundError(f"Configuration file not found: {cfg_path}")

    raw_text = cfg_path.read_text(encoding="utf-8")
    comments = extract_toml_inline_comments(raw_text)
    raw = tomllib.loads(raw_text)
    general = raw.get("general", {})
    fallback_profile = str(general.get("fallback_profile", "agy"))
    backend = str(general.get("backend", "uinput")).lower()

    profiles: dict[str, ProfileConfig] = {}
    for prof_name, prof_data in raw.get("profiles", {}).items():
        if not isinstance(prof_data, dict):
            continue
        prof_comments = _lookup_comments(comments, prof_name)
        raw_buttons = dict(prof_data.get("buttons", {}))
        raw_combos = dict(prof_data.get("combos", {}))
        for k, v in prof_data.items():
            if k in ("buttons", "combos", "description", "match_class", "match_title", "match_rules", "turn", "combo_timeout_ms"):
                continue
            if "+" in k:
                try:
                    norm_c = parse_combo_buttons(k)
                    raw_combos.setdefault(norm_c, v)
                except ValueError:
                    pass
            else:
                try:
                    norm_k = normalize_button_name(k)
                    raw_buttons.setdefault(norm_k, v)
                except ValueError:
                    pass
        buttons: dict[str, ButtonAction] = {}
        for btn_key, btn_spec in raw_buttons.items():
            norm_btn = normalize_button_name(btn_key)
            btn_desc = prof_comments.get("buttons", {}).get(norm_btn)
            buttons[norm_btn] = ButtonAction.from_raw(btn_spec, description=btn_desc)

        combos: dict[frozenset[str], ButtonAction] = {}
        for combo_key, combo_spec in raw_combos.items():
            norm_combo = parse_combo_buttons(combo_key) if not isinstance(combo_key, frozenset) else combo_key
            combo_desc = prof_comments.get("combos", {}).get(norm_combo)
            combos[norm_combo] = ButtonAction.from_raw(combo_spec, description=combo_desc)

        combo_timeout_ms = int(prof_data.get("combo_timeout_ms", general.get("combo_timeout_ms", 50)))

        raw_rules = prof_data.get("match_rules", [])
        match_rules: list[MatchRule] = []
        if isinstance(raw_rules, list):
            for r in raw_rules:
                if isinstance(r, dict):
                    c_pat = r.get("class", r.get("match_class"))
                    t_pat = r.get("title", r.get("match_title"))
                    match_rules.append(MatchRule(match_class=c_pat, match_title=t_pat))

        profiles[prof_name] = ProfileConfig(
            name=prof_name,
            description=str(prof_data.get("description", "")),
            match_class=prof_data.get("match_class"),
            match_title=prof_data.get("match_title"),
            match_rules=match_rules,
            turn=int(prof_data.get("turn", 0)),
            combo_timeout_ms=combo_timeout_ms,
            buttons=buttons,
            combos=combos,
        )

    # Always ensure built-in presets are reachable if not overridden
    for builtin_name in ("agy", "youtube", "factory", "anki", "supermemo"):
        if builtin_name not in profiles:
            hw = get_builtin_hardware_preset(builtin_name)
            profiles[builtin_name] = ProfileConfig.from_hardware_profile(
                builtin_name,
                hw,
                description=f"Built-in {builtin_name} preset",
            )

    # Check for global base template profile ('global' or 'all') or top-level [global]
    global_combos: dict[frozenset[str], ButtonAction] = {}
    global_buttons: dict[str, ButtonAction] = {}

    # 1. From [profiles.global] or [profiles.all]
    for g_name in ("global", "all"):
        if g_name in profiles:
            g_prof = profiles[g_name]
            for c, act in g_prof.combos.items():
                global_combos.setdefault(c, act)
            for b, act in g_prof.buttons.items():
                global_buttons.setdefault(b, act)

    # 2. Also support top-level [global] or [all] tables
    for g_key in ("global", "all"):
        g_data = raw.get(g_key, {})
        g_comments = _lookup_comments(comments, g_key)
        if isinstance(g_data, dict):
            for c_k, c_v in g_data.get("combos", {}).items():
                try:
                    c_norm = parse_combo_buttons(c_k)
                    c_desc = g_comments.get("combos", {}).get(c_norm)
                    global_combos.setdefault(c_norm, ButtonAction.from_raw(c_v, description=c_desc))
                except ValueError:
                    pass
            for b_k, b_v in g_data.get("buttons", {}).items():
                try:
                    b_norm = normalize_button_name(b_k)
                    b_desc = g_comments.get("buttons", {}).get(b_norm)
                    global_buttons.setdefault(b_norm, ButtonAction.from_raw(b_v, description=b_desc))
                except ValueError:
                    pass

    # 3. Inherit global defaults into all profiles (profile-specific bindings take precedence)
    for p_name, p_prof in profiles.items():
        if p_name in ("global", "all"):
            continue
        # Inherit combos
        for c, act in global_combos.items():
            if c not in p_prof.combos:
                p_prof.combos[c] = act
        # Inherit buttons
        for b, act in global_buttons.items():
            if b not in p_prof.buttons:
                p_prof.buttons[b] = act

    return AppConfig(
        fallback_profile=fallback_profile,
        backend=backend,
        profiles=profiles,
        global_combos=global_combos,
        global_buttons=global_buttons,
    )
