"""Unit and golden-packet tests for 8bitdo-micro-focus-switch."""

from __future__ import annotations

from pathlib import Path
from evdev import ecodes
import pytest

from eightbitdo_micro_focus_switch.config import DEFAULT_TOML_CONTENT, load_config
from eightbitdo_micro_focus_switch.daemon import FocusSwitchDaemon
from eightbitdo_micro_focus_switch.hyprland import ActiveWindow, parse_socket2_event_line
from eightbitdo_micro_focus_switch.protocol import (
    CMD_SAVE_CONFIG,
    CMD_SAVE_SUBCMD,
    MicroHardwareProfile,
    RECORD_MICRO_SIZE,
    build_save_config_packet,
    build_write_config_packets,
    crc16_arc,
    get_builtin_hardware_preset,
    parse_ble_notification,
    rotate_dpad_button,
    PHYSICAL_BUTTONS,
)

# Real 180-byte record_micro captured from live 8BitDo Micro ('80EL' / E4:17:D8:D8:52:02)
LIVE_AGY_RECORD_HEX = (
    "d50800001109202011092020"
    "2a0000004e0000004b000000e1280000"
    "2b000000e0e10700e100000028000000"
    "0000000000000000290000001fe10000"
    "2c000000380000005200000051000000"
    "500000004f0000000000000000000000" + ("00" * 88)
)


def test_live_agy_golden_roundtrip() -> None:
    raw = bytes.fromhex(LIVE_AGY_RECORD_HEX)
    assert len(raw) == RECORD_MICRO_SIZE

    decoded = MicroHardwareProfile.from_bytes(raw)
    assert decoded.enabled is True
    assert decoded.turn == 0
    assert decoded.turn_degrees == 0
    assert decoded.header_prefix == bytes.fromhex("d50800")

    agy_preset = get_builtin_hardware_preset("agy")
    assert decoded.slots == agy_preset.slots

    # Re-encoding agy with `base=raw` must reproduce the exact 180 bytes byte-for-byte
    reencoded = agy_preset.to_bytes(base=raw)
    assert reencoded == raw


def test_live_chunk_crc16_matches_controller_notifications() -> None:
    raw = bytes.fromhex(LIVE_AGY_RECORD_HEX)
    packets = build_write_config_packets(raw)
    assert len(packets) == 4

    # Expected CRCs observed directly in BLE GATT notifications from 80EL:
    # chunk 0: 12210 (0x2fb2), chunk 1: 58714 (0xe55a), chunk 2: 53040 (0xcf30), chunk 3: 53040 (0xcf30)
    expected_crcs = [12210, 58714, 53040, 53040]
    for idx, (offset, pkt) in enumerate(packets):
        assert offset == idx * 45
        assert pkt[0] == 0x04
        hdr, payload = parse_ble_notification(pkt[1:])
        assert hdr.chunk_len == 45
        assert hdr.total_len == 180
        assert hdr.offset == idx * 45
        assert hdr.crc16 == expected_crcs[idx]
        assert crc16_arc(payload) == expected_crcs[idx]


def test_save_packet_framing() -> None:
    pkt = build_save_config_packet()
    assert len(pkt) == 17
    assert pkt[0] == 0x04
    hdr, payload = parse_ble_notification(pkt[1:])
    assert hdr.cmd == CMD_SAVE_CONFIG
    assert hdr.subcmd == CMD_SAVE_SUBCMD
    assert hdr.chunk_len == 0
    assert hdr.crc16 == 0xFFFF
    assert hdr.total_len == 0
    assert hdr.offset == 0
    assert payload == b""


def test_dpad_rotation() -> None:
    assert rotate_dpad_button("UP", 0) == "UP"
    assert rotate_dpad_button("UP", 90) == "RIGHT"
    assert rotate_dpad_button("UP", 180) == "DOWN"
    assert rotate_dpad_button("UP", 270) == "LEFT"
    assert rotate_dpad_button("A", 90) == "A"


def test_hyprland_socket2_parser() -> None:
    assert parse_socket2_event_line("workspace>>2\n") is None
    win = parse_socket2_event_line("activewindow>>anki,User 1 - Anki\n")
    assert win == ActiveWindow(window_class="anki", window_title="User 1 - Anki")
    empty = parse_socket2_event_line("activewindow>>,\n")
    assert empty == ActiveWindow(window_class="", window_title="")


class _FakeUInput:
    def __init__(self) -> None:
        self.events: list[tuple[int, int, int]] = []

    def write(self, etype: int, code: int, value: int) -> None:
        self.events.append((etype, code, value))

    def syn(self) -> None:
        self.events.append((ecodes.EV_SYN, 0, 0))


def test_daemon_focus_switch_and_chord_release_safety(tmp_path: Path) -> None:
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    cfg = load_config(cfg_file)

    daemon = FocusSwitchDaemon(cfg)
    ui = _FakeUInput()

    # 1. Focus Anki window -> resolves to 'anki' profile (where X = LEFTCTRL+LEFTSHIFT+D)
    daemon._on_focus_change(ActiveWindow(window_class="anki", window_title="Anki"))
    assert daemon.active_profile.name == "anki"

    # Press physical X button (factory key KEY_H)
    daemon.handle_key_event(ui, ecodes.KEY_H, 1)
    assert ui.events == [
        (ecodes.EV_KEY, ecodes.KEY_LEFTCTRL, 1),
        (ecodes.EV_KEY, ecodes.KEY_LEFTSHIFT, 1),
        (ecodes.EV_KEY, ecodes.KEY_D, 1),
        (ecodes.EV_SYN, 0, 0),
    ]
    ui.events.clear()

    # 2. Switch focus to Firefox WHILE physical X button is still held down!
    daemon._on_focus_change(ActiveWindow(window_class="firefox", window_title="Mozilla Firefox"))
    assert daemon.active_profile.name == "browser"

    # Release physical X button -> must release LEFTCTRL+LEFTSHIFT+D in reverse order (not browser's PAGEUP)
    daemon.handle_key_event(ui, ecodes.KEY_H, 0)
    assert ui.events == [
        (ecodes.EV_KEY, ecodes.KEY_D, 0),
        (ecodes.EV_KEY, ecodes.KEY_LEFTSHIFT, 0),
        (ecodes.EV_KEY, ecodes.KEY_LEFTCTRL, 0),
        (ecodes.EV_SYN, 0, 0),
    ]
    ui.events.clear()

    # 3. Now press physical X button in Firefox -> emits browser's PAGEUP
    daemon.handle_key_event(ui, ecodes.KEY_H, 1)
    daemon.handle_key_event(ui, ecodes.KEY_H, 0)
    assert ui.events == [
        (ecodes.EV_KEY, ecodes.KEY_PAGEUP, 1),
        (ecodes.EV_SYN, 0, 0),
        (ecodes.EV_KEY, ecodes.KEY_PAGEUP, 0),
        (ecodes.EV_SYN, 0, 0),
    ]


def test_youtube_profile_matching_and_actions(tmp_path: Path) -> None:
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    cfg = load_config(cfg_file)

    # Browser tab with YouTube in title -> resolves to 'youtube'
    prof_yt = cfg.resolve_profile_for_window("firefox", "Lofi Hip Hop - YouTube")
    assert prof_yt.name == "youtube"

    # Browser tab without YouTube -> resolves to 'browser'
    prof_gh = cfg.resolve_profile_for_window("firefox", "GitHub - Issues")
    assert prof_gh.name == "browser"

    daemon = FocusSwitchDaemon(cfg)
    ui = _FakeUInput()
    daemon._on_focus_change(ActiveWindow(window_class="firefox", window_title="Some Video - YouTube"))
    assert daemon.active_profile.name == "youtube"

    # In YouTube profile:
    # A (factory KEY_G) -> SPACE (play/pause)
    daemon.handle_key_event(ui, ecodes.KEY_G, 1)
    daemon.handle_key_event(ui, ecodes.KEY_G, 0)
    assert (ecodes.EV_KEY, ecodes.KEY_SPACE, 1) in ui.events
    ui.events.clear()

    # L (factory KEY_K) -> J (seek -10s)
    daemon.handle_key_event(ui, ecodes.KEY_K, 1)
    daemon.handle_key_event(ui, ecodes.KEY_K, 0)
    assert (ecodes.EV_KEY, ecodes.KEY_J, 1) in ui.events
    ui.events.clear()

    # R2 (factory KEY_R) -> LEFTSHIFT + DOT (speed up)
    daemon.handle_key_event(ui, ecodes.KEY_R, 1)
    assert ui.events == [
        (ecodes.EV_KEY, ecodes.KEY_LEFTSHIFT, 1),
        (ecodes.EV_KEY, ecodes.KEY_DOT, 1),
        (ecodes.EV_SYN, 0, 0),
    ]
    ui.events.clear()
    daemon.handle_key_event(ui, ecodes.KEY_R, 0)
    assert ui.events == [
        (ecodes.EV_KEY, ecodes.KEY_DOT, 0),
        (ecodes.EV_KEY, ecodes.KEY_LEFTSHIFT, 0),
        (ecodes.EV_SYN, 0, 0),
    ]

    # Hardware preset test
    yt_hw = get_builtin_hardware_preset("youtube")
    raw = yt_hw.to_bytes()
    assert len(raw) == RECORD_MICRO_SIZE
    decoded = MicroHardwareProfile.from_bytes(raw)
    assert decoded.enabled is True


def test_visualizer_svg_and_html_diagrams(tmp_path: Path) -> None:
    import xml.etree.ElementTree as ET
    from eightbitdo_micro_focus_switch.visualizer import generate_html_viewer, generate_svg_diagram

    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    cfg = load_config(cfg_file)

    # 1. Test AGY SVG diagram
    prof_agy = cfg.get_profile("agy")
    svg_agy = generate_svg_diagram(prof_agy)
    root_agy = ET.fromstring(svg_agy)
    assert root_agy.tag.endswith("svg")
    assert "8BitDo Micro Keymap — AGY" in svg_agy
    assert "Backspace" in svg_agy
    assert "Ctrl + Shift + D" in svg_agy
    assert "Pageup" in svg_agy

    # 2. Test YouTube SVG diagram
    prof_yt = cfg.get_profile("youtube")
    svg_yt = generate_svg_diagram(prof_yt)
    root_yt = ET.fromstring(svg_yt)
    assert root_yt.tag.endswith("svg")
    assert "8BitDo Micro Keymap — YOUTUBE" in svg_yt
    assert "Shift + Dot" in svg_yt
    assert "Shift + Comma" in svg_yt

    # Verify all 16 buttons are represented
    for btn in PHYSICAL_BUTTONS:
        assert f">{btn}<" in svg_agy or f" {btn}<" in svg_agy or f">{btn} " in svg_agy or btn in svg_agy

    # 3. Test multi-profile interactive HTML viewer
    html = generate_html_viewer(cfg.profiles, "agy")
    assert "<!DOCTYPE html>" in html
    assert "selectProfile" in html
    assert 'class="tab-btn active"' in html
    for name in cfg.profiles:
        assert f"selectProfile('{name}')" in html
        assert f'id="profile-{name}"' in html


def test_cli_show_command(tmp_path: Path) -> None:
    from eightbitdo_micro_focus_switch.cli import main

    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")

    out_svg = tmp_path / "output.svg"
    ret = main(["--config", str(cfg_file), "show", "agy", "-o", str(out_svg)])
    assert ret == 0
    assert out_svg.exists()
    assert "<svg" in out_svg.read_text(encoding="utf-8")

    out_html = tmp_path / "viewer.html"
    ret_html = main(["--config", str(cfg_file), "show", "--interactive", "-o", str(out_html)])
    assert ret_html == 0
    assert out_html.exists()
    assert "<!DOCTYPE html>" in out_html.read_text(encoding="utf-8")


def test_cli_config_command(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    from eightbitdo_micro_focus_switch.cli import main

    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")

    # 1. Test --path
    ret = main(["--config", str(cfg_file), "config", "--path"])
    assert ret == 0
    assert str(cfg_file) in capsys.readouterr().out

    # 2. Test --list
    ret = main(["--config", str(cfg_file), "config", "--list"])
    assert ret == 0
    out = capsys.readouterr().out
    assert "youtube" in out
    assert "line" in out

    # 3. Test jumping to existing profile with --no-edit
    ret = main(["--config", str(cfg_file), "config", "youtube", "--no-edit"])
    assert ret == 0
    out = capsys.readouterr().out
    assert "Found existing profile [youtube]" in out

    # 4. Test creating new profile with --yes --no-edit
    ret = main(["--config", str(cfg_file), "config", "discord", "--yes", "--no-edit"])
    assert ret == 0
    out = capsys.readouterr().out
    assert "Created new profile [discord]" in out

    # Verify 'discord' is now in config and valid
    reloaded = load_config(cfg_file)
    assert "discord" in reloaded.profiles
    assert reloaded.profiles["discord"].match_class == "discord"
    assert reloaded.profiles["discord"].buttons["A"].keys == ("SPACE",)

    # 5. Test init-config backward compatibility
    custom_cfg = tmp_path / "subdir" / "new_profiles.toml"
    ret_init = main(["--config", str(custom_cfg), "init-config"])
    assert ret_init == 0
    assert custom_cfg.exists()


