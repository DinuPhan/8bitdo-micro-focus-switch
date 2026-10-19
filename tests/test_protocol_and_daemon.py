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

    # Standalone Chrome / Chromium Web App / PWA (with or without YouTube in title)
    prof_pwa = cfg.resolve_profile_for_window("chrome-youtube.com__-Profile_3", "(729) Matt Pocock - YouTube")
    assert prof_pwa.name == "youtube"

    prof_pwa_notitle = cfg.resolve_profile_for_window("chrome-youtube.com__-Profile_3", "Arbitrary Video Title")
    assert prof_pwa_notitle.name == "youtube"

    # Standalone FreeTube desktop app
    prof_freetube = cfg.resolve_profile_for_window("freetube", "Playing video")
    assert prof_freetube.name == "youtube"

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


def test_cli_discover_command(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from eightbitdo_micro_focus_switch import cli
    from eightbitdo_micro_focus_switch.ble import DiscoveredMicroBle

    async def _mock_scan(timeout: float = 4.0) -> list[DiscoveredMicroBle]:
        return [DiscoveredMicroBle(name="80EL", address="AA:BB:CC:DD:EE:FF", rssi=-42, device=None)]  # type: ignore

    monkeypatch.setattr(cli, "scan_for_micro_ble_devices", _mock_scan)
    ret = cli.main(["discover", "--timeout", "1.0"])
    assert ret == 0
    out = capsys.readouterr().out
    assert "8BitDo Micro Discovery & Connectivity Check" in out
    assert "AA:BB:CC:DD:EE:FF" in out
    assert "RSSI=-42 dBm" in out


def test_combo_parsing_and_normalization() -> None:
    from eightbitdo_micro_focus_switch.config import parse_combo_buttons

    assert parse_combo_buttons("L+R") == frozenset({"L", "R"})
    assert parse_combo_buttons("L1 + R1") == frozenset({"L", "R"})
    assert parse_combo_buttons("R1+L1") == frozenset({"L", "R"})
    assert parse_combo_buttons("SELECT + START") == frozenset({"SELECT", "START"})
    assert parse_combo_buttons("MINUS + PLUS") == frozenset({"SELECT", "START"})
    assert parse_combo_buttons(["L2", "R2"]) == frozenset({"L2", "R2"})

    with pytest.raises(ValueError, match="at least 2 simultaneous buttons"):
        parse_combo_buttons("L")


def test_daemon_combos_and_chording_mechanics(tmp_path: Path) -> None:
    import asyncio
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    cfg = load_config(cfg_file)

    prof_agy = cfg.get_profile("agy")
    assert frozenset({"L", "R"}) in prof_agy.combos
    combo_act = prof_agy.combos[frozenset({"L", "R"})]
    assert combo_act.keys == ("LEFTMETA", "K")

    async def _run_async_test() -> None:
        daemon = FocusSwitchDaemon(cfg)
        ui = _FakeUInput()
        daemon._on_focus_change(ActiveWindow(window_class="ghostty", window_title="agy terminal"))
        assert daemon.active_profile.name == "agy"

        # 1. Non-combo button (A -> BACKSPACE) has ZERO delay (emits immediately)
        daemon.handle_key_event(ui, ecodes.KEY_G, 1)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_BACKSPACE, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()
        daemon.handle_key_event(ui, ecodes.KEY_G, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_BACKSPACE, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 2. Simultaneous Combo (L + R pressed within 20ms < 40ms)
        daemon.handle_key_event(ui, ecodes.KEY_K, 1)
        assert len(ui.events) == 0  # Buffered! Solo TAB is NOT emitted

        await asyncio.sleep(0.015)  # 15ms later, press R (factory KEY_M)
        daemon.handle_key_event(ui, ecodes.KEY_M, 1)

        # Matched combo L+R -> Emits LEFTMETA + K down! Solo TAB was NEVER emitted!
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_LEFTMETA, 1),
            (ecodes.EV_KEY, ecodes.KEY_K, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # Repeat on combo
        daemon.handle_key_event(ui, ecodes.KEY_M, 2)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_K, 2),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # Release L -> Releases combo keys (K up, LEFTMETA up)
        daemon.handle_key_event(ui, ecodes.KEY_K, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_K, 0),
            (ecodes.EV_KEY, ecodes.KEY_LEFTMETA, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # Release R -> Already released, clean no-op
        daemon.handle_key_event(ui, ecodes.KEY_M, 0)
        assert len(ui.events) == 0

        # 3. Solo key timeout fallback: User presses L ALONE and holds it
        daemon.handle_key_event(ui, ecodes.KEY_K, 1)
        assert len(ui.events) == 0  # Buffered
        await asyncio.sleep(0.065)  # Wait 65ms (> 50ms combo_timeout_ms)

        # Timer expired -> Solo TAB is flushed down!
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_TAB, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # User releases L -> TAB up
        daemon.handle_key_event(ui, ecodes.KEY_K, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_TAB, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 4. Fast tap: User taps L quickly (< 40ms, e.g. 10ms tap duration)
        daemon.handle_key_event(ui, ecodes.KEY_K, 1)
        assert len(ui.events) == 0
        await asyncio.sleep(0.010)
        daemon.handle_key_event(ui, ecodes.KEY_K, 0)

        # Fast tap emits TAB down and TAB up immediately!
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_TAB, 1),
            (ecodes.EV_SYN, 0, 0),
            (ecodes.EV_KEY, ecodes.KEY_TAB, 0),
            (ecodes.EV_SYN, 0, 0),
        ]

    asyncio.run(_run_async_test())


def test_daemon_hold_and_press_combos(tmp_path: Path) -> None:
    import asyncio

    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(
        """\
[general]
backend = "uinput"
fallback_profile = "work"
combo_timeout_ms = 40

[profiles.global.combos]
"L2+LEFT" = ["LEFTMETA", "LEFT"]
"L2+RIGHT" = ["LEFTMETA", "RIGHT"]

[profiles.work]
description = "Work Profile"
turn = 0
[profiles.work.buttons]
L2 = "L"
LEFT = "Left"
RIGHT = "Right"
""",
        encoding="utf-8",
    )
    cfg = load_config(cfg_file)
    prof = cfg.get_profile("work")
    assert frozenset({"L2", "LEFT"}) in prof.combos
    assert frozenset({"L2", "RIGHT"}) in prof.combos

    async def _run() -> None:
        daemon = FocusSwitchDaemon(cfg)
        ui = _FakeUInput()
        daemon._on_focus_change(ActiveWindow(window_class="work", window_title="work"))

        # 1. User holds L2 down (factory KEY_L)
        daemon.handle_key_event(ui, ecodes.KEY_L, 1)
        assert len(ui.events) == 0  # Buffered in pending combos
        await asyncio.sleep(0.055)  # Wait 55ms (> 40ms combo_timeout_ms)

        # Solo L key is emitted down!
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_L, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 2. While L2 is STILL held, user taps LEFT (factory KEY_E)
        daemon.handle_key_event(ui, ecodes.KEY_E, 1)

        # Solo L was retracted, and combo (LEFTMETA + LEFT) is emitted down!
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_L, 0),
            (ecodes.EV_SYN, 0, 0),
            (ecodes.EV_KEY, ecodes.KEY_LEFTMETA, 1),
            (ecodes.EV_KEY, ecodes.KEY_LEFT, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 3. User releases LEFT (factory KEY_E, value 0) while still holding L2
        daemon.handle_key_event(ui, ecodes.KEY_E, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_LEFT, 0),
            (ecodes.EV_KEY, ecodes.KEY_LEFTMETA, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 4. User taps RIGHT (factory KEY_F) while still holding L2
        daemon.handle_key_event(ui, ecodes.KEY_F, 1)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_LEFTMETA, 1),
            (ecodes.EV_KEY, ecodes.KEY_RIGHT, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 5. User releases RIGHT (factory KEY_F, value 0)
        daemon.handle_key_event(ui, ecodes.KEY_F, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_RIGHT, 0),
            (ecodes.EV_KEY, ecodes.KEY_LEFTMETA, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 6. User finally releases L2 (factory KEY_L, value 0)
        daemon.handle_key_event(ui, ecodes.KEY_L, 0)
        # L2 solo was already retracted and no combo active on L2 release -> clean no-op
        assert len(ui.events) == 0

    asyncio.run(_run())


def test_visualizer_combos_rendering(tmp_path: Path) -> None:
    from eightbitdo_micro_focus_switch.visualizer import generate_svg_diagram

    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")
    cfg = load_config(cfg_file)

    prof_agy = cfg.get_profile("agy")
    svg = generate_svg_diagram(prof_agy)
    assert "COMBOS" in svg
    assert "L1 + R1" in svg
    assert "Super + K" in svg


def test_daemon_in_process_reload(tmp_path: Path) -> None:
    cfg_file = tmp_path / "profiles.toml"
    content_v1 = """\
[general]
backend = "uinput"
fallback_profile = "test"

[profiles.test]
description = "Test v1"
turn = 0
[profiles.test.buttons]
A = "Space"
"""
    cfg_file.write_text(content_v1, encoding="utf-8")
    cfg1 = load_config(cfg_file)
    daemon = FocusSwitchDaemon(cfg1, config_path=cfg_file)
    assert daemon.active_profile.turn == 0

    content_v2 = """\
[general]
backend = "uinput"
fallback_profile = "test"

[profiles.test]
description = "Test v2"
turn = 90
[profiles.test.buttons]
A = "Enter"
"""
    cfg_file.write_text(content_v2, encoding="utf-8")
    daemon.reload_config()
    assert daemon.active_profile.turn == 90
    assert daemon.config.profiles["test"].buttons["A"].keys == ("ENTER",)


def test_cli_reload_commands(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from eightbitdo_micro_focus_switch.cli import main

    # 1. Invalid syntax refusal
    broken = tmp_path / "broken.toml"
    broken.write_text("this is invalid toml ::: [[[", encoding="utf-8")
    rc = main(["--config", str(broken), "reload"])
    assert rc == 1
    err = capsys.readouterr().err
    assert "Invalid configuration" in err

    # 2. Valid config, running daemon
    valid = tmp_path / "valid.toml"
    valid.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")

    monkeypatch.setattr(
        "eightbitdo_micro_focus_switch.cli.reload_running_daemon",
        lambda: (True, "Sent reload signal (SIGHUP) to systemd service"),
    )
    rc = main(["--config", str(valid), "reload"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Validated configuration syntax" in out
    assert "Sent reload signal (SIGHUP)" in out

    # 3. Valid config, daemon not running
    monkeypatch.setattr(
        "eightbitdo_micro_focus_switch.cli.reload_running_daemon",
        lambda: (False, "Daemon is not currently running."),
    )
    rc = main(["--config", str(valid), "reload"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Daemon is not currently running." in out


def test_chord_recorder_state_machine() -> None:
    from eightbitdo_micro_focus_switch.recorder import ChordRecorder

    # 1. Non-modifier single press
    rec1 = ChordRecorder()
    assert rec1.process_event(ecodes.KEY_A, 2) is None  # repeat ignored
    res1 = rec1.process_event(ecodes.KEY_SPACE, 1)
    assert res1 is not None
    assert res1.keys == ["Space"]
    assert res1.cancelled is False

    # 2. Modifiers + key chord (Ctrl + Shift + P)
    rec2 = ChordRecorder()
    assert rec2.process_event(ecodes.KEY_LEFTCTRL, 1) is None
    assert rec2.process_event(ecodes.KEY_LEFTSHIFT, 1) is None
    res2 = rec2.process_event(ecodes.KEY_P, 1)
    assert res2 is not None
    assert res2.keys == ["Ctrl", "Shift", "P"]
    assert res2.cancelled is False

    # 3. Solo modifier tap & release (Super key)
    rec3 = ChordRecorder()
    assert rec3.process_event(ecodes.KEY_LEFTMETA, 1) is None
    res3 = rec3.process_event(ecodes.KEY_LEFTMETA, 0)
    assert res3 is not None
    assert res3.keys == ["Super"]
    assert res3.cancelled is False

    # 4. ESC key cancels immediately
    rec4 = ChordRecorder()
    assert rec4.process_event(ecodes.KEY_LEFTALT, 1) is None
    res4 = rec4.process_event(ecodes.KEY_ESC, 1)
    assert res4 is not None
    assert res4.cancelled is True


def test_keys_catalog_formatting() -> None:
    from eightbitdo_micro_focus_switch.recorder import format_keys_catalog, get_bindable_keys_catalog

    cat = get_bindable_keys_catalog()
    assert "Modifiers" in cat
    assert "Media & Volume" in cat
    assert "VolumeUp" in cat["Media & Volume"]

    text_all = format_keys_catalog()
    assert "Bindable Keys Catalog" in text_all
    assert "PlayPause" in text_all

    text_filtered = format_keys_catalog("volume")
    assert "VolumeUp" in text_filtered
    assert "PlayPause" in text_filtered  # in Media & Volume
    assert "Digits" not in text_filtered


def test_cli_bind_and_keys_commands(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from eightbitdo_micro_focus_switch.cli import main
    from eightbitdo_micro_focus_switch.recorder import ChordResult

    test_cfg = tmp_path / "profiles.toml"
    test_cfg.write_text(DEFAULT_TOML_CONTENT, encoding="utf-8")

    monkeypatch.setattr(
        "eightbitdo_micro_focus_switch.cli.reload_running_daemon",
        lambda: (True, "Sent reload signal (SIGHUP)"),
    )

    # 1. `keys` command
    rc = main(["keys", "media"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Media & Volume" in out

    # 2. `bind --list-keys`
    rc = main(["--config", str(test_cfg), "bind", "--list-keys"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Bindable Keys Catalog" in out

    # 3. Direct bind with --key
    rc = main(["--config", str(test_cfg), "bind", "agy", "A", "--key", "Space"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Bound [agy] A = Space" in out
    loaded = load_config(test_cfg)
    assert loaded.profiles["agy"].buttons["A"].keys == ("SPACE",)

    # 4. Direct bind combo with --key
    rc = main(["--config", str(test_cfg), "bind", "agy", "L+R", "--key", "Super+K"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Bound [agy] L+R = Super + K" in out
    loaded = load_config(test_cfg)
    combo = loaded.profiles["agy"].combos[frozenset({"L", "R"})]
    assert combo.keys == ("SUPER", "K")

    # 5. Interactive bind mocked with captured chord
    monkeypatch.setattr(
        "eightbitdo_micro_focus_switch.cli.capture_keyboard_chord",
        lambda: ChordResult(keys=["Ctrl", "Shift", "P"], cancelled=False),
    )
    rc = main(["--config", str(test_cfg), "bind", "agy", "Y"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Bound [agy] Y = Ctrl + Shift + P" in out
    loaded = load_config(test_cfg)
    assert loaded.profiles["agy"].buttons["Y"].keys == ("CTRL", "SHIFT", "P")

    # 6. Interactive bind cancelled with ESC
    monkeypatch.setattr(
        "eightbitdo_micro_focus_switch.cli.capture_keyboard_chord",
        lambda: ChordResult(cancelled=True),
    )
    rc = main(["--config", str(test_cfg), "bind", "agy", "B"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "Cancelled. No changes made." in out


def test_global_profile_combo_and_button_inheritance(tmp_path: Path) -> None:
    test_cfg = tmp_path / "profiles.toml"
    content = """\
[general]
fallback_profile = "agy"

[profiles.global]
description = "Global defaults"

[profiles.global.combos]
"L+R" = ["Super", "K"]
"Select+Start" = "exec:notify-send Hello"

[profiles.global.buttons]
STAR = "Mute"

[profiles.agy]
description = "AGY"
match_class = "ghostty"

[profiles.agy.combos]
"L2+Left" = ["LEFTMETA", "LEFT"]

[profiles.youtube]
description = "YouTube"
match_class = "firefox"

[profiles.youtube.combos]
"L+R" = ["F"]  # Profile-specific override!
"""
    test_cfg.write_text(content, encoding="utf-8")
    cfg = load_config(test_cfg)

    # 1. Global combos recorded in AppConfig
    assert frozenset({"L", "R"}) in cfg.global_combos
    assert frozenset({"SELECT", "START"}) in cfg.global_combos

    # 2. agy inherits L+R and Select+Start, and keeps its own L2+Left
    agy = cfg.profiles["agy"]
    assert frozenset({"L", "R"}) in agy.combos
    assert agy.combos[frozenset({"L", "R"})].keys == ("SUPER", "K")
    assert frozenset({"SELECT", "START"}) in agy.combos
    assert agy.combos[frozenset({"SELECT", "START"})].exec_cmd == "notify-send Hello"
    assert frozenset({"L2", "LEFT"}) in agy.combos

    # 3. agy inherits STAR button
    assert agy.buttons["STAR"].keys == ("MUTE",)

    # 4. youtube overrides L+R with "F", while still inheriting Select+Start
    yt = cfg.profiles["youtube"]
    assert yt.combos[frozenset({"L", "R"})].keys == ("F",)
    assert frozenset({"SELECT", "START"}) in yt.combos

    # 5. Window resolution never matches 'global'
    matched = cfg.resolve_profile_for_window("ghostty", "")
    assert matched.name == "agy"
    unmatched = cfg.resolve_profile_for_window("random_app", "")
    assert unmatched.name != "global"


def test_inline_comments_and_visualizer_tooltips(tmp_path: Path) -> None:
    from eightbitdo_micro_focus_switch.config import extract_toml_inline_comments
    from eightbitdo_micro_focus_switch.visualizer import generate_svg_diagram, generate_html_viewer
    from eightbitdo_micro_focus_switch.recorder import update_toml_binding

    toml_doc = """\
[general]
fallback_profile = "media"

[profiles.global.combos]
"L+R" = ["Super", "K"]               # Global app launcher
"Select+Start" = "exec:echo hi"      # Global scratchpad

[profiles.media]
description = "Media player"
turn = 0

[profiles.media.buttons]
A = "Space"                          # Play / Pause toggle
B = "M"                              # Mute audio

[profiles.media.combos]
"L2+Left" = ["LEFTALT", "LEFT"]      # Previous chapter
"""
    # 1. Test extraction directly
    comments = extract_toml_inline_comments(toml_doc)
    assert comments["media"]["buttons"]["A"] == "Play / Pause toggle"
    assert comments["media"]["buttons"]["B"] == "Mute audio"
    assert comments["media"]["combos"][frozenset({"L2", "LEFT"})] == "Previous chapter"
    assert comments["global"]["combos"][frozenset({"L", "R"})] == "Global app launcher"

    # 2. Test config loading and inheritance of descriptions
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)
    media = cfg.get_profile("media")

    assert media.buttons["A"].description == "Play / Pause toggle"
    assert media.buttons["B"].description == "Mute audio"
    assert media.combos[frozenset({"L2", "LEFT"})].description == "Previous chapter"
    # Inherited global combo preserves its description
    assert media.combos[frozenset({"L", "R"})].description == "Global app launcher"

    # 3. Test SVG diagram generation includes tooltips (<title>) and subtitles
    svg = generate_svg_diagram(media)
    assert "<title>A: Space — Play / Pause toggle</title>" in svg
    assert "<title>B: M — Mute audio</title>" in svg
    assert "Play / Pause toggle" in svg
    assert "Mute audio" in svg
    assert "Global app launcher" in svg
    assert "Previous chapter" in svg

    # 4. Test HTML viewer contains both SVG and tooltips
    html_out = generate_html_viewer(cfg.profiles, "media")
    assert "<title>A: Space — Play / Pause toggle</title>" in html_out
    assert ".callout-card" in html_out
    assert ".combo-chip" in html_out

    # 5. Test update_toml_binding preserves existing inline comments
    updated = update_toml_binding(toml_doc, "media", "A", ["Enter"])
    assert "A = \"Enter\"  # Play / Pause toggle" in updated


def test_daemon_bg_tasks_lifecycle(tmp_path: Path) -> None:
    import asyncio

    toml_doc = """
[profiles.test]
turn = 0
match_class = "test"
A = { exec = "sleep 0.05" }
"""
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)

    async def _run() -> None:
        daemon = FocusSwitchDaemon(cfg)
        ui = _FakeUInput()
        daemon._on_focus_change(ActiveWindow(window_class="test", window_title="Test"))

        assert len(daemon._bg_tasks) == 0
        daemon.handle_key_event(ui, ecodes.KEY_G, 1)
        daemon.handle_key_event(ui, ecodes.KEY_G, 0)
        assert len(daemon._bg_tasks) == 1
        task = next(iter(daemon._bg_tasks))
        assert not task.done()

        await task
        assert len(daemon._bg_tasks) == 0

    asyncio.run(_run())


def test_daemon_disconnect_cleanup_syn_invariant(tmp_path: Path) -> None:
    toml_doc = """
[profiles.test]
turn = 0
match_class = "test"

[profiles.test.combos]
"A+B" = ["F11"]
"""
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)

    daemon = FocusSwitchDaemon(cfg)
    ui = _FakeUInput()
    daemon._on_focus_change(ActiveWindow(window_class="test", window_title="Test"))

    # Press A (KEY_G) and B (KEY_J) to trigger combo A+B -> F11
    daemon.handle_key_event(ui, ecodes.KEY_G, 1)
    daemon.handle_key_event(ui, ecodes.KEY_J, 1)

    assert (ecodes.EV_KEY, ecodes.KEY_F11, 1) in ui.events
    assert frozenset({"A", "B"}) in daemon._active_combos
    assert len(daemon._held_chords) == 0  # Only combo is active!
    ui.events.clear()

    # Disconnect cleanup logic simulation
    needs_syn = False
    for p in daemon._pending_buttons.values():
        if p.timer_handle is not None:
            p.timer_handle.cancel()
    daemon._pending_buttons.clear()

    for combo_act in daemon._active_combos.values():
        if combo_act.evdev_codes:
            for code in reversed(combo_act.evdev_codes):
                ui.write(ecodes.EV_KEY, code, 0)
            needs_syn = True
    daemon._active_combos.clear()
    daemon._held_combos.clear()
    daemon._physical_held_buttons.clear()

    for held in daemon._held_chords.values():
        for code in reversed(held):
            ui.write(ecodes.EV_KEY, code, 0)
        needs_syn = True
    daemon._held_chords.clear()

    if needs_syn:
        ui.syn()

    assert ui.events == [
        (ecodes.EV_KEY, ecodes.KEY_F11, 0),
        (ecodes.EV_SYN, 0, 0),
    ]


def test_software_only_features_property_and_ble_warning(tmp_path: Path) -> None:
    toml_doc = """
[profiles.test]
turn = 0
match_class = "test"
A = { exec = "echo hello" }
B = "Space"

[profiles.test.combos]
"L+R" = "F11"
"""
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)
    prof = cfg.get_profile("test")

    combos_cnt, exec_cnt = prof.software_only_features
    assert combos_cnt == 1
    assert exec_cnt == 1

    hw = prof.to_hardware_profile()
    # 0x2C is HID usage for Keyboard Spacebar
    assert hw.slots["B"] == (0x2C, 0, 0, 0)
    assert "L+R" not in hw.slots


def test_daemon_combo_key_repeat_while_pending(tmp_path: Path) -> None:
    import asyncio

    toml_doc = """
[general]
combo_timeout_ms = 40

[profiles.test]
turn = 0
match_class = "test"
L2 = "L"

[profiles.test.combos]
"L2+LEFT" = ["LEFTMETA", "LEFT"]
"""
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)

    async def _run() -> None:
        daemon = FocusSwitchDaemon(cfg)
        ui = _FakeUInput()
        daemon._on_focus_change(ActiveWindow(window_class="test", window_title="Test"))

        # 1. Press L2 (KEY_L) -> enters combo pending buffer
        daemon.handle_key_event(ui, ecodes.KEY_L, 1)
        assert len(ui.events) == 0

        # 2. Kernel/controller sends repeat (value=2) before 40ms timeout pops
        daemon.handle_key_event(ui, ecodes.KEY_L, 2)
        # Should immediately flush solo L keydown and emit repeat!
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_L, 1),
            (ecodes.EV_SYN, 0, 0),
            (ecodes.EV_KEY, ecodes.KEY_L, 2),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 3. Release L2 (value=0) -> should cleanly emit keyup
        daemon.handle_key_event(ui, ecodes.KEY_L, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_L, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        assert len(daemon._held_chords) == 0
        assert len(daemon._pending_buttons) == 0

    asyncio.run(_run())


def test_daemon_duplicate_press_timer_leak_prevention(tmp_path: Path) -> None:
    import asyncio

    toml_doc = """
[general]
combo_timeout_ms = 40

[profiles.test]
turn = 0
match_class = "test"
L2 = "L"

[profiles.test.combos]
"L2+LEFT" = ["LEFTMETA", "LEFT"]
"""
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)

    async def _run() -> None:
        daemon = FocusSwitchDaemon(cfg)
        ui = _FakeUInput()
        daemon._on_focus_change(ActiveWindow(window_class="test", window_title="Test"))

        # 1. First press of KEY_L
        daemon.handle_key_event(ui, ecodes.KEY_L, 1)
        assert len(ui.events) == 0
        assert ecodes.KEY_L in daemon._pending_buttons
        first_handle = daemon._pending_buttons[ecodes.KEY_L].timer_handle

        # 2. Second press of KEY_L before timeout (e.g. bounce or repeat as value 1)
        await asyncio.sleep(0.015)
        daemon.handle_key_event(ui, ecodes.KEY_L, 1)
        # First timer must be cancelled, new handle created
        assert first_handle.cancelled()

        # 3. Wait for second timer to pop (>40ms)
        await asyncio.sleep(0.05)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_L, 1),
            (ecodes.EV_SYN, 0, 0),
        ]
        ui.events.clear()

        # 4. Third press while already held -> ignored
        daemon.handle_key_event(ui, ecodes.KEY_L, 1)
        assert len(ui.events) == 0

        # 5. Clean release
        daemon.handle_key_event(ui, ecodes.KEY_L, 0)
        assert ui.events == [
            (ecodes.EV_KEY, ecodes.KEY_L, 0),
            (ecodes.EV_SYN, 0, 0),
        ]
        assert len(daemon._held_chords) == 0

    asyncio.run(_run())


def test_daemon_flush_after_early_release_safety(tmp_path: Path) -> None:
    toml_doc = """
[general]
combo_timeout_ms = 40

[profiles.test]
turn = 0
match_class = "test"
LEFT = "E"

[profiles.test.combos]
"L2+LEFT" = ["LEFTMETA", "LEFT"]
"""
    cfg_file = tmp_path / "profiles.toml"
    cfg_file.write_text(toml_doc, encoding="utf-8")
    cfg = load_config(cfg_file)

    daemon = FocusSwitchDaemon(cfg)
    ui = _FakeUInput()
    daemon._on_focus_change(ActiveWindow(window_class="test", window_title="Test"))

    # Put LEFT (KEY_E) into pending
    from eightbitdo_micro_focus_switch.daemon import _PendingButton
    action = cfg.get_profile("test").buttons["LEFT"]
    daemon._pending_buttons[ecodes.KEY_E] = _PendingButton(
        keycode=ecodes.KEY_E,
        button_name="LEFT",
        action=action,
        timer_handle=None,
    )
    # Simulate that physical button is NO LONGER held down
    daemon._physical_held_buttons.pop(ecodes.KEY_E, None)

    # Now flush callback executes
    daemon._flush_pending_button(ui, ecodes.KEY_E)

    # Must NOT emit solo down because physical button is not held!
    assert len(ui.events) == 0
    assert len(daemon._held_chords) == 0









