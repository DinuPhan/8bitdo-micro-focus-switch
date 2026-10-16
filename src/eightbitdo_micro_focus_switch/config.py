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

    @classmethod
    def from_raw(cls, raw: str | list[Any] | dict[str, Any]) -> ButtonAction:
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
        )


@dataclass
class ProfileConfig:
    """A named mapping profile that can be activated by window focus or flashed via BLE."""

    name: str
    description: str = ""
    match_class: str | None = None
    match_title: str | None = None
    turn: int = 0  # degrees: 0, 90, 180, 270
    buttons: dict[str, ButtonAction] = field(default_factory=dict)
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
        if self._class_re is None and self._title_re is None:
            return False
        if self._class_re is not None and not self._class_re.search(window_class or ""):
            return False
        if self._title_re is not None and not self._title_re.search(window_title or ""):
            return False
        return True

    def to_hardware_profile(self, *, enabled: bool = True) -> MicroHardwareProfile:
        """Convert this profile into a 180-byte MicroHardwareProfile for BLE GATT flashing."""
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

    def resolve_profile_for_window(self, window_class: str, window_title: str) -> ProfileConfig:
        """Return the first matching profile for (window_class, window_title), or the fallback profile."""
        for prof in self.profiles.values():
            if prof.matches_window(window_class, window_title):
                return prof
        if self.fallback_profile in self.profiles:
            return self.profiles[self.fallback_profile]
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

[profiles.agy]
description = "Antigravity (agy) / Terminal Coding Profile (captured from controller)"
match_class = '^(com\.mitchellh\.ghostty|kitty|Alacritty|foot|org\.wezfurlong\.wezterm)$'
match_title = "agy"
turn = 0

[profiles.agy.buttons]
A = "BACKSPACE"
B = "PAGEDOWN"
X = "PAGEUP"
Y = ["LEFTSHIFT", "ENTER"]
L = "TAB"
R = ["LEFTCTRL", "LEFTSHIFT", "D"]
L2 = "LEFTSHIFT"
R2 = "ENTER"
SELECT = "ESC"
START = ["2", "LEFTSHIFT"]
STAR = "SPACE"
HOME = "SLASH"
UP = "UP"
DOWN = "DOWN"
LEFT = "LEFT"
RIGHT = "RIGHT"

[profiles.anki]
description = "Anki Flashcards (matches official 8BitDo Anki preset)"
match_class = "^(anki|Anki)$"
match_title = "Anki"
turn = 0

[profiles.anki.buttons]
A = "2"
B = "1"
X = ["LEFTCTRL", "LEFTSHIFT", "D"]
Y = ["LEFTCTRL", "LEFTALT", "N"]
L = "SPACE"
R = "4"
L2 = "L"
R2 = "R"
SELECT = "E"
START = "KPASTERISK"
STAR = "T"
HOME = "S"
UP = ["LEFTCTRL", "DELETE"]
DOWN = ["LEFTSHIFT", "2"]
LEFT = ["LEFTCTRL", "Z"]
RIGHT = "KPMINUS"

[profiles.youtube]
description = "YouTube Video Playback in Browser (native player shortcuts)"
match_class = '^(firefox|brave-browser|chromium|google-chrome|zen|vivaldi-stable)$'
match_title = ".*(YouTube|- YouTube).*"
turn = 0

[profiles.youtube.buttons]
A = "SPACE"
B = "M"
X = "F"
Y = "C"
L = "J"
R = "L"
L2 = ["LEFTSHIFT", "COMMA"]
R2 = ["LEFTSHIFT", "DOT"]
SELECT = "ESC"
START = ["LEFTSHIFT", "N"]
STAR = "T"
HOME = "I"
UP = "UP"
DOWN = "DOWN"
LEFT = "LEFT"
RIGHT = "RIGHT"

[profiles.browser]
description = "Web Browsing / Reading (Firefox, Chromium, Brave, Zen)"
match_class = "^(firefox|brave-browser|chromium|google-chrome|zen|vivaldi-stable)$"
turn = 0

[profiles.browser.buttons]
A = "SPACE"
B = "PAGEDOWN"
X = "PAGEUP"
Y = ["LEFTCTRL", "T"]
L = ["LEFTCTRL", "PGUP"]
R = ["LEFTCTRL", "PGDN"]
L2 = ["LEFTALT", "LEFT"]
R2 = ["LEFTALT", "RIGHT"]
SELECT = ["LEFTCTRL", "W"]
START = ["LEFTCTRL", "SHIFT", "T"]
STAR = "F5"
HOME = "ESC"
UP = "UP"
DOWN = "DOWN"
LEFT = "LEFT"
RIGHT = "RIGHT"

[profiles.media]
description = "Video / Media Player (mpv, VLC)"
match_class = "^(mpv|vlc)$"
turn = 0

[profiles.media.buttons]
A = "SPACE"
B = "F"
X = "M"
Y = "S"
L = "LEFTBRACE"
R = "RIGHTBRACE"
L2 = "9"
R2 = "0"
SELECT = "ESC"
START = "ENTER"
STAR = "J"
HOME = "O"
UP = "UP"
DOWN = "DOWN"
LEFT = "LEFT"
RIGHT = "RIGHT"

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


def ensure_default_config(path: Path = DEFAULT_CONFIG_PATH) -> Path:
    """Create the default profiles.toml if it does not already exist."""
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    return path


def load_config(path: Path | None = None) -> AppConfig:
    """Load AppConfig from a TOML file (creating the default config if using the default path)."""
    cfg_path = path or DEFAULT_CONFIG_PATH
    if not cfg_path.exists():
        if path is None:
            ensure_default_config(cfg_path)
        else:
            raise FileNotFoundError(f"Configuration file not found: {cfg_path}")

    raw = tomllib.loads(cfg_path.read_text(encoding="utf-8"))
    general = raw.get("general", {})
    fallback_profile = str(general.get("fallback_profile", "agy"))
    backend = str(general.get("backend", "uinput")).lower()

    profiles: dict[str, ProfileConfig] = {}
    for prof_name, prof_data in raw.get("profiles", {}).items():
        if not isinstance(prof_data, dict):
            continue
        raw_buttons = prof_data.get("buttons", {})
        buttons: dict[str, ButtonAction] = {}
        for btn_key, btn_spec in raw_buttons.items():
            norm_btn = normalize_button_name(btn_key)
            buttons[norm_btn] = ButtonAction.from_raw(btn_spec)
        profiles[prof_name] = ProfileConfig(
            name=prof_name,
            description=str(prof_data.get("description", "")),
            match_class=prof_data.get("match_class"),
            match_title=prof_data.get("match_title"),
            turn=int(prof_data.get("turn", 0)),
            buttons=buttons,
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

    return AppConfig(
        fallback_profile=fallback_profile,
        backend=backend,
        profiles=profiles,
    )
