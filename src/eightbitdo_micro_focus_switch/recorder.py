"""Interactive keyboard chord recorder and key catalog for 8bitdo-micro-focus-switch.

Allows users to press shortcut combinations directly on their physical keyboard
(e.g. Ctrl+Shift+P, Super+K, Space) instead of manually typing keycodes or names.
Pressing ESC discards the input wait immediately.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path
import re
import selectors
import sys
from typing import Sequence

try:
    import termios
    import tty
    HAS_TERMIOS = True
except ImportError:
    HAS_TERMIOS = False

try:
    from evdev import InputDevice, ecodes, list_devices
    HAS_EVDEV = True
except ImportError:
    HAS_EVDEV = False
    InputDevice = object  # type: ignore
    list_devices = lambda: []  # type: ignore
    from .protocol import ecodes

from .config import load_config, parse_combo_buttons
from .protocol import (
    PHYSICAL_BUTTONS,
    normalize_button_name,
)

# Friendly mapping from Linux evdev keycode -> Canonical display name
EVDEV_TO_FRIENDLY_NAME: dict[int, str] = {
    # Modifiers
    ecodes.KEY_LEFTCTRL: "Ctrl",
    ecodes.KEY_RIGHTCTRL: "Ctrl",
    ecodes.KEY_LEFTSHIFT: "Shift",
    ecodes.KEY_RIGHTSHIFT: "Shift",
    ecodes.KEY_LEFTALT: "Alt",
    ecodes.KEY_RIGHTALT: "Alt",
    ecodes.KEY_LEFTMETA: "Super",
    ecodes.KEY_RIGHTMETA: "Super",
    # Common actions / editing
    ecodes.KEY_SPACE: "Space",
    ecodes.KEY_ENTER: "Enter",
    ecodes.KEY_KPENTER: "Enter",
    ecodes.KEY_TAB: "Tab",
    ecodes.KEY_BACKSPACE: "Backspace",
    ecodes.KEY_DELETE: "Delete",
    ecodes.KEY_INSERT: "Insert",
    ecodes.KEY_ESC: "Esc",
    ecodes.KEY_PAGEUP: "PageUp",
    ecodes.KEY_PAGEDOWN: "PageDown",
    ecodes.KEY_HOME: "Home",
    ecodes.KEY_END: "End",
    ecodes.KEY_UP: "Up",
    ecodes.KEY_DOWN: "Down",
    ecodes.KEY_LEFT: "Left",
    ecodes.KEY_RIGHT: "Right",
    ecodes.KEY_CAPSLOCK: "CapsLock",
    # Media / Audio
    ecodes.KEY_MUTE: "Mute",
    ecodes.KEY_VOLUMEUP: "VolumeUp",
    ecodes.KEY_VOLUMEDOWN: "VolumeDown",
    ecodes.KEY_PLAYPAUSE: "PlayPause",
    ecodes.KEY_NEXTSONG: "NextSong",
    ecodes.KEY_PREVIOUSSONG: "PreviousSong",
    # Punctuation / Symbols
    ecodes.KEY_MINUS: "Minus",
    ecodes.KEY_EQUAL: "Equal",
    ecodes.KEY_LEFTBRACE: "LeftBrace",
    ecodes.KEY_RIGHTBRACE: "RightBrace",
    ecodes.KEY_BACKSLASH: "Backslash",
    ecodes.KEY_SEMICOLON: "Semicolon",
    ecodes.KEY_APOSTROPHE: "Apostrophe",
    ecodes.KEY_GRAVE: "Grave",
    ecodes.KEY_COMMA: "Comma",
    ecodes.KEY_DOT: "Dot",
    ecodes.KEY_SLASH: "Slash",
}

# Add standard letters A-Z
for _letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
    _c = getattr(ecodes, f"KEY_{_letter}", None)
    if _c is not None:
        EVDEV_TO_FRIENDLY_NAME[_c] = _letter

# Add digits 0-9
for _digit in "0123456789":
    _c = getattr(ecodes, f"KEY_{_digit}", None)
    if _c is not None:
        EVDEV_TO_FRIENDLY_NAME[_c] = _digit

# Add F1-F24
for _idx in range(1, 25):
    _c = getattr(ecodes, f"KEY_F{_idx}", None)
    if _c is not None:
        EVDEV_TO_FRIENDLY_NAME[_c] = f"F{_idx}"

# Set of modifier codes
MODIFIER_ECODES: set[int] = {
    ecodes.KEY_LEFTCTRL,
    ecodes.KEY_RIGHTCTRL,
    ecodes.KEY_LEFTSHIFT,
    ecodes.KEY_RIGHTSHIFT,
    ecodes.KEY_LEFTALT,
    ecodes.KEY_RIGHTALT,
    ecodes.KEY_LEFTMETA,
    ecodes.KEY_RIGHTMETA,
}

MODIFIER_ORDER: dict[str, int] = {
    "Ctrl": 0,
    "Alt": 1,
    "Shift": 2,
    "Super": 3,
}


@dataclass
class ChordResult:
    """Result of waiting for a keyboard chord."""

    keys: list[str] = field(default_factory=list)
    cancelled: bool = False
    raw_codes: list[int] = field(default_factory=list)

    @property
    def display_str(self) -> str:
        if self.cancelled:
            return "Cancelled (ESC)"
        return " + ".join(self.keys) if self.keys else "(none)"

    @property
    def toml_value(self) -> str:
        if len(self.keys) == 1:
            return json.dumps(self.keys[0])
        return json.dumps(self.keys)


class ChordRecorder:
    """State machine that accumulates modifier states and captures chords."""

    def __init__(self) -> None:
        self.held_modifiers: set[int] = set()
        self.held_non_modifiers: set[int] = set()
        self.solo_modifier_candidates: set[int] = set()
        self.result: ChordResult | None = None

    def process_event(self, code: int, value: int) -> ChordResult | None:
        """Process an EV_KEY event (0=up, 1=down, 2=hold).
        
        Returns ChordResult when a complete chord or cancellation is reached, else None.
        """
        # Ignore repeat events
        if value == 2:
            return None

        # 1. ESC cancels the wait immediately
        if code == ecodes.KEY_ESC:
            if value == 1:
                self.result = ChordResult(cancelled=True, raw_codes=[code])
                return self.result
            return None

        # 2. KEY DOWN (value == 1)
        if value == 1:
            if code in MODIFIER_ECODES:
                self.held_modifiers.add(code)
                self.solo_modifier_candidates.add(code)
                return None
            else:
                # Non-modifier key pressed! This completes the chord!
                self.held_non_modifiers.add(code)
                self.solo_modifier_candidates.clear()

                # Collect held modifiers into friendly sorted list
                mod_names: list[str] = []
                for mod_code in self.held_modifiers:
                    name = EVDEV_TO_FRIENDLY_NAME.get(mod_code)
                    if name and name not in mod_names:
                        mod_names.append(name)
                mod_names.sort(key=lambda m: MODIFIER_ORDER.get(m, 99))

                target_name = EVDEV_TO_FRIENDLY_NAME.get(code)
                if not target_name:
                    # Fallback to ecodes name
                    raw_attr = ecodes.KEY.get(code)
                    if isinstance(raw_attr, list):
                        raw_attr = raw_attr[0]
                    target_name = str(raw_attr).removeprefix("KEY_").title()

                chord_keys = mod_names + [target_name]
                raw_codes = sorted(self.held_modifiers) + [code]
                self.result = ChordResult(keys=chord_keys, cancelled=False, raw_codes=raw_codes)
                return self.result

        # 3. KEY UP (value == 0)
        if value == 0:
            if code in MODIFIER_ECODES:
                # If a solo modifier was pressed and released without any other key, capture it!
                if code in self.solo_modifier_candidates and not self.held_non_modifiers:
                    mod_name = EVDEV_TO_FRIENDLY_NAME.get(code, "Ctrl")
                    self.result = ChordResult(keys=[mod_name], cancelled=False, raw_codes=[code])
                    self.held_modifiers.discard(code)
                    self.solo_modifier_candidates.clear()
                    return self.result
                self.held_modifiers.discard(code)
                self.solo_modifier_candidates.discard(code)
            else:
                self.held_non_modifiers.discard(code)

        return None


def find_physical_keyboards() -> list[InputDevice]:
    """Find all real physical keyboards in /dev/input/event*."""
    if not HAS_EVDEV:
        return []
    keyboards: list[InputDevice] = []
    for p in list_devices():
        try:
            d = InputDevice(p)
        except (PermissionError, OSError):
            continue
        # Skip our own daemon virtual keyboard
        if "Virtual Keyboard" in d.name:
            d.close()
            continue
        caps = d.capabilities()
        if ecodes.EV_KEY in caps:
            keys = caps[ecodes.EV_KEY]
            # Standard keyboard has A, Space, Enter
            if ecodes.KEY_A in keys and ecodes.KEY_SPACE in keys and ecodes.KEY_ENTER in keys:
                keyboards.append(d)
                continue
        d.close()
    return keyboards


def capture_keyboard_chord(
    devices: list[InputDevice] | None = None,
    timeout: float | None = None,
) -> ChordResult:
    """Listen across physical keyboards for the next key chord or ESC cancellation.
    
    Returns ChordResult with .cancelled=True if ESC was pressed.
    """
    close_when_done = False
    if devices is None:
        devices = find_physical_keyboards()
        close_when_done = True

    if not devices:
        if not HAS_EVDEV or sys.platform != "linux":
            raise RuntimeError(
                f"Interactive keyboard chord listening requires Linux evdev.\n"
                f"On {sys.platform}, please specify the key binding directly via argument, "
                f"or edit profiles.toml directly."
            )
        raise RuntimeError(
            "No accessible physical keyboards found in /dev/input/event*.\n"
            "Ensure you have read permissions on /dev/input (e.g. member of group 'input').\n"
            "Run: 8bitdo-micro-focus-switch install-udev"
        )

    recorder = ChordRecorder()
    sel = selectors.DefaultSelector()

    try:
        for dev in devices:
            try:
                sel.register(dev.fileno(), selectors.EVENT_READ, dev)
            except Exception:
                pass

        # Also register terminal stdin in cbreak mode if available
        stdin_fd = sys.stdin.fileno() if sys.stdin.isatty() else None
        old_settings = None
        if stdin_fd is not None:
            try:
                old_settings = termios.tcgetattr(stdin_fd)
                tty.setcbreak(stdin_fd)
                sel.register(stdin_fd, selectors.EVENT_READ, "stdin")
            except Exception:
                pass

        try:
            while True:
                events = sel.select(timeout=timeout)
                if not events:
                    # Timeout reached
                    return ChordResult(cancelled=True)

                for key, _mask in events:
                    if key.data == "stdin":
                        # Stdin keypress
                        ch = sys.stdin.read(1)
                        if ch == "\x1b":  # ESC character from terminal
                            return ChordResult(cancelled=True)
                        continue

                    dev = key.data
                    try:
                        for event in dev.read():
                            if event.type == ecodes.EV_KEY:
                                res = recorder.process_event(event.code, event.value)
                                if res is not None:
                                    return res
                    except OSError:
                        pass
        finally:
            if stdin_fd is not None and old_settings is not None:
                try:
                    termios.tcsetattr(stdin_fd, termios.TCSADRAIN, old_settings)
                except Exception:
                    pass
    finally:
        sel.close()
        if close_when_done:
            for dev in devices:
                try:
                    dev.close()
                except Exception:
                    pass


def get_bindable_keys_catalog() -> dict[str, list[str]]:
    """Return categorized dictionary of all bindable keys."""
    return {
        "Modifiers": ["Ctrl", "Shift", "Alt", "Super"],
        "Navigation": ["Up", "Down", "Left", "Right", "PageUp", "PageDown", "Home", "End"],
        "Actions & Editing": [
            "Space",
            "Enter",
            "Tab",
            "Backspace",
            "Delete",
            "Insert",
            "CapsLock",
        ],
        "Function Keys (F1..F12)": [f"F{i}" for i in range(1, 13)],
        "Extended Function (F13..F24)": [f"F{i}" for i in range(13, 25)],
        "Media & Volume": [
            "PlayPause",
            "Mute",
            "VolumeUp",
            "VolumeDown",
            "NextSong",
            "PreviousSong",
        ],
        "Letters (A..Z)": list("ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
        "Digits (0..9)": list("0123456789"),
        "Punctuation & Symbols": [
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
        "Special Actions": [
            "None (unbound)",
            "exec:<shell-command>  (e.g. exec:playerctl play-pause)",
        ],
    }


def format_keys_catalog(query: str | None = None) -> str:
    """Format the bindable keys catalog as a clean ASCII report, optionally filtered by query."""
    catalog = get_bindable_keys_catalog()
    lines: list[str] = [
        "=== 8BitDo Micro Bindable Keys Catalog ===",
        "Tip: In `bind` or `record` mode, simply press the key/chord on your keyboard directly!",
        "",
    ]
    norm_q = query.strip().lower() if query else ""

    found_any = False
    for category, keys in catalog.items():
        if norm_q:
            matched = [k for k in keys if norm_q in k.lower() or norm_q in category.lower()]
            if matched:
                found_any = True
                lines.append(f"{category}:")
                lines.append("  " + ", ".join(matched))
                lines.append("")
        else:
            found_any = True
            lines.append(f"{category}:")
            # Wrap nicely in columns or comma-separated lines
            if len(keys) > 10:
                chunk_size = 10
                for i in range(0, len(keys), chunk_size):
                    lines.append("  " + ", ".join(keys[i : i + chunk_size]))
            else:
                lines.append("  " + ", ".join(keys))
            lines.append("")

    if norm_q and not found_any:
        lines.append(f"No bindable keys found matching {query!r}.")

    return "\n".join(lines).rstrip()


def update_toml_binding(
    content: str,
    profile: str,
    button_or_combo: str,
    chord_keys: Sequence[str],
) -> str:
    """Update or insert a button or combo binding in profiles.toml content while preserving other sections."""
    is_combo = "+" in button_or_combo
    if is_combo:
        parsed_combo = parse_combo_buttons(button_or_combo)
        key_name = "+".join(sorted(parsed_combo))
        section_sub = "combos"
    else:
        key_name = normalize_button_name(button_or_combo)
        section_sub = "buttons"

    if len(chord_keys) == 1:
        value_repr = json.dumps(chord_keys[0])
    else:
        value_repr = json.dumps(list(chord_keys))

    # 1. Find profile header
    p_header = re.compile(rf"^[ \t]*\[profiles\.{re.escape(profile)}\][ \t]*$", re.MULTILINE | re.IGNORECASE)
    m = p_header.search(content)
    if not m:
        if profile.lower() in ("global", "all"):
            header_insert = f"\n[profiles.{profile}]\ndescription = 'Global base template inherited by all profiles'\n"
            content = content.rstrip() + "\n" + header_insert
            m = p_header.search(content)
        if not m:
            raise ValueError(f"Profile {profile!r} not found in configuration")
    p_start = m.start()

    # 2. Find next profile or general section (end of current profile block)
    p_next = re.compile(r"^[ \t]*\[(?:profiles\.[^.\s\]]+|general)\][ \t]*$", re.MULTILINE | re.IGNORECASE)
    m_next = p_next.search(content, pos=m.end())
    p_end = m_next.start() if m_next else len(content)

    prof_block = content[p_start:p_end]

    # 3. Check for subsection header, e.g. [profiles.agy.buttons] or [profiles.agy.combos]
    sub_header = re.compile(
        rf"^[ \t]*\[profiles\.{re.escape(profile)}\.{re.escape(section_sub)}\][ \t]*$",
        re.MULTILINE | re.IGNORECASE,
    )
    m_sub = sub_header.search(prof_block)

    line_to_put = f"{json.dumps(key_name) if is_combo else key_name} = {value_repr}"

    if m_sub:
        # Find where subsection ends (next '[' at line start or end of prof_block)
        sub_start = m_sub.end()
        sub_next = re.compile(r"^[ \t]*\[", re.MULTILINE).search(prof_block, pos=sub_start)
        sub_end = sub_next.start() if sub_next else len(prof_block)
        sub_text = prof_block[sub_start:sub_end]

        # Look for existing key in subsection
        key_re = re.compile(
            rf"^[ \t]*(?:{re.escape(key_name)}|{re.escape(repr(key_name))}|{re.escape(json.dumps(key_name))})[ \t]*=.*$",
            re.MULTILINE | re.IGNORECASE,
        )
        m_key = key_re.search(sub_text)

        if m_key:
            # Preserve existing inline comment if present
            existing_line = m_key.group(0)
            existing_comment = ""
            if "#" in existing_line:
                in_s, in_d = False, False
                for i_ch, ch in enumerate(existing_line):
                    if ch == '"' and not in_s:
                        in_d = not in_d
                    elif ch == "'" and not in_d:
                        in_s = not in_s
                    elif ch == "#" and not in_s and not in_d:
                        existing_comment = existing_line[i_ch:]
                        break
            if existing_comment:
                line_to_put = f"{line_to_put}  {existing_comment}"
            # Replace existing line
            new_sub_text = sub_text[: m_key.start()] + line_to_put + sub_text[m_key.end() :]
        else:
            # Append right under the subsection header
            new_sub_text = "\n" + line_to_put + sub_text
        new_prof_block = prof_block[:sub_start] + new_sub_text + prof_block[sub_end:]
    else:
        # Subsection header does not exist: create it at the end of profile block
        addition = f"\n[profiles.{profile}.{section_sub}]\n{line_to_put}\n"
        new_prof_block = prof_block.rstrip() + addition + "\n"

    return content[:p_start] + new_prof_block + content[p_end:]


def set_profile_binding(
    config_path: Path,
    profile_name: str,
    button_or_combo: str,
    chord_keys: Sequence[str],
) -> Path:
    """Save binding into config_path and validate TOML syntax."""
    content = config_path.read_text(encoding="utf-8")
    new_content = update_toml_binding(content, profile_name, button_or_combo, chord_keys)
    # Validate before writing
    config_path.write_text(new_content, encoding="utf-8")
    try:
        load_config(config_path)
    except Exception as exc:
        # Revert on failure
        config_path.write_text(content, encoding="utf-8")
        raise ValueError(f"Failed to apply binding (syntax error): {exc}") from exc
    return config_path
