"""Command-line interface for 8bitdo-micro-focus-switch."""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys

from .ble import (
    format_snapshot_report,
    read_hardware_snapshot,
    scan_for_micro_ble_devices,
    write_hardware_profile,
)
from .config import DEFAULT_CONFIG_PATH, ensure_default_config, load_config
from .daemon import FocusSwitchDaemon, check_uinput_access, find_micro_input_devices
from .hyprland import find_hyprland_socket2, query_active_window
from .protocol import (
    HID_TO_NAME,
    PHYSICAL_BUTTONS,
    get_builtin_hardware_preset,
)
from .visualizer import generate_html_viewer, generate_svg_diagram

UDEV_RULE_PATH = Path("/etc/udev/rules.d/99-8bitdo-micro-uinput.rules")
UDEV_RULE_CONTENT = (
    'KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", '
    'OPTIONS+="static_node=uinput", TAG+="uaccess"\n'
)


def _format_profile_table(config_path: Path) -> str:
    cfg = load_config(config_path)
    lines = [
        f"Config File      : {config_path}",
        f"Default Backend  : {cfg.backend}",
        f"Fallback Profile : {cfg.fallback_profile}",
        "",
        "Available Profiles:",
    ]
    for name, prof in cfg.profiles.items():
        match_parts: list[str] = []
        if prof.match_class:
            match_parts.append(f"class~={prof.match_class!r}")
        if prof.match_title:
            match_parts.append(f"title~={prof.match_title!r}")
        match_str = ", ".join(match_parts) if match_parts else "(manual / fallback)"
        lines.append(f"  - {name:<12} [turn={prof.turn:>3}°] {match_str}")
        if prof.description:
            lines.append(f"      {prof.description}")
        if prof.combos:
            combo_parts = [
                f"{'+'.join(sorted(c))} -> {'+'.join(act.keys) if act.keys else act.exec_cmd}"
                for c, act in prof.combos.items()
            ]
            lines.append(f"      Combos: {', '.join(combo_parts)}")
    return "\n".join(lines)


async def _cmd_status(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    cfg = load_config(cfg_path)

    sock = find_hyprland_socket2()
    win = await query_active_window()
    matched = cfg.resolve_profile_for_window(win.window_class, win.window_title)

    print("=== Hyprland Focus Status ===")
    print(f"Socket2 Path     : {sock or 'not found'}")
    print(f"Active Window    : class={win.window_class!r}, title={win.window_title!r}")
    print(f"Resolved Profile : {matched.name} (turn={matched.turn}°)")
    print()

    print("=== Classic HID Input (/dev/input) ===")
    devs = find_micro_input_devices()
    if not devs:
        print("8BitDo Micro Keyboard : not connected")
    else:
        for d in devs:
            print(
                f"Device           : {d.path} ({d.name!r}, "
                f"vendor=0x{d.info.vendor:04x}, product=0x{d.info.product:04x}, uniq={d.uniq!r})"
            )
            d.close()
    uinput_ok, uinput_msg = check_uinput_access()
    print(f"uinput Status    : {'OK' if uinput_ok else 'NEEDS PERMISSION'} ({uinput_msg})")
    print()

    print("=== Configured Profiles ===")
    print(_format_profile_table(cfg_path))

    if args.ble:
        print("\n=== Live BLE GATT Hardware Snapshot ('80EL') ===")
        snapshot = await read_hardware_snapshot(address=args.address, timeout=args.timeout)
        print(format_snapshot_report(snapshot))

    return 0


async def _cmd_ble_read(args: argparse.Namespace) -> int:
    snapshot = await read_hardware_snapshot(address=args.address, timeout=args.timeout)
    print(format_snapshot_report(snapshot))
    if args.save_bin:
        out_path = Path(args.save_bin).expanduser()
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(snapshot.raw_record)
        print(f"\nSaved raw 180-byte record_micro to {out_path}")
    return 0


async def _cmd_ble_write(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    cfg = load_config(cfg_path)
    profile_name = args.profile

    prof = cfg.get_profile(profile_name)
    is_factory = profile_name.strip().lower() in ("factory", "default", "reset")
    hw_prof = prof.to_hardware_profile(enabled=not is_factory)
    if args.turn is not None:
        hw_prof = prof.to_hardware_profile(enabled=not is_factory)
        hw_prof.turn = args.turn if args.turn in (0, 1, 2, 3) else {0: 0, 90: 1, 180: 2, 270: 3}[args.turn]

    print(f"Flashing profile {prof.name!r} (enabled={hw_prof.enabled}, turn={hw_prof.turn_degrees}°) to '80EL' ...")
    snapshot = await write_hardware_profile(
        hw_prof,
        address=args.address,
        timeout=args.timeout,
    )
    print("Flash & Save complete! Verified hardware state:\n")
    print(format_snapshot_report(snapshot))
    return 0


async def _cmd_ble_reset(args: argparse.Namespace) -> int:
    backup_path = DEFAULT_CONFIG_PATH.parent / "hardware-backup.bin"
    print("Reading current hardware profile before reset ...")
    before = await read_hardware_snapshot(address=args.address, timeout=args.timeout)
    backup_path.parent.mkdir(parents=True, exist_ok=True)
    backup_path.write_bytes(before.raw_record)
    print(f"Backed up previous 180-byte hardware config to {backup_path}")

    factory_hw = get_builtin_hardware_preset("factory")
    print("Resetting 8BitDo Micro hardware profile to Factory Default (unique c..t keys) ...")
    after = await write_hardware_profile(
        factory_hw,
        address=args.address,
        timeout=args.timeout,
    )
    print("Reset complete! Verified hardware state:\n")
    print(format_snapshot_report(after))
    return 0


async def _cmd_daemon(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    cfg = load_config(cfg_path)

    if args.ensure_factory_ble:
        try:
            snap = await read_hardware_snapshot(address=args.address, timeout=4.0)
            if snap.profile.enabled:
                backup_path = cfg_path.parent / "hardware-backup.bin"
                backup_path.write_bytes(snap.raw_record)
                logging.getLogger("8bitdo-micro").info(
                    "Backed up custom hardware profile to %s and resetting controller to factory keys for uinput mode...",
                    backup_path,
                )
                await write_hardware_profile(
                    get_builtin_hardware_preset("factory"),
                    address=args.address,
                    timeout=4.0,
                )
        except Exception as exc:
            logging.getLogger("8bitdo-micro").warning(
                "Could not verify/reset BLE hardware state (%s); continuing.",
                exc,
            )

    daemon = FocusSwitchDaemon(
        cfg,
        backend=args.backend,
        ble_address=args.address,
    )
    await daemon.run()
    return 0


def _cmd_install_service(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    exe = shutil.which("8bitdo-micro-focus-switch") or str(Path(sys.argv[0]).resolve())
    service_dir = Path.home() / ".config" / "systemd" / "user"
    service_dir.mkdir(parents=True, exist_ok=True)
    service_path = service_dir / "8bitdo-micro-focus-switch.service"

    backend_arg = f" --backend {args.backend}" if args.backend else ""
    unit_text = f"""\
[Unit]
Description=8BitDo Micro Focus-Aware Input Remapper Daemon (Hyprland)
After=graphical-session.target

[Service]
Type=simple
ExecStart={exe} daemon --config {cfg_path}{backend_arg}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
"""
    service_path.write_text(unit_text, encoding="utf-8")
    print(f"Wrote systemd user unit: {service_path}")
    print("\nTo enable and start the service:")
    print("  systemctl --user daemon-reload")
    print("  systemctl --user enable --now 8bitdo-micro-focus-switch.service")

    uinput_ok, _ = check_uinput_access()
    if not uinput_ok:
        print("\nNOTE: For `--backend uinput` (<1ms virtual keyboard mode), grant /dev/uinput access once:")
        print(f"  echo '{UDEV_RULE_CONTENT.strip()}' | sudo tee {UDEV_RULE_PATH}")
        print("  sudo udevadm control --reload-rules && sudo udevadm trigger /dev/uinput")
    return 0


def _cmd_install_udev(args: argparse.Namespace) -> int:
    uinput_ok, msg = check_uinput_access()
    if uinput_ok:
        print("✓ /dev/uinput is already accessible and writable by your user!")
        return 0

    print("=== 8BitDo Micro udev Setup ===")
    print(f"Target rule path : {UDEV_RULE_PATH}")
    print(f"Rule content     : {UDEV_RULE_CONTENT.strip()}\n")
    print("Attempting to write udev rule with sudo...")

    cmd = (
        f"echo '{UDEV_RULE_CONTENT.strip()}' | sudo tee {UDEV_RULE_PATH} > /dev/null "
        "&& sudo udevadm control --reload-rules "
        "&& sudo udevadm trigger /dev/uinput"
    )
    try:
        res = subprocess.run(["sudo", "sh", "-c", cmd])
        if res.returncode == 0:
            ok, msg = check_uinput_access()
            if ok:
                print("\n✓ Successfully granted /dev/uinput permissions to group 'input'!")
                return 0
            else:
                print(f"\nRule installed, but: {msg}")
                return 0
    except Exception as exc:
        print(f"Failed to invoke sudo: {exc}")

    print("\nPlease run the following command in your terminal with sudo:")
    print(f"  echo '{UDEV_RULE_CONTENT.strip()}' | sudo tee {UDEV_RULE_PATH}")
    print("  sudo udevadm control --reload-rules && sudo udevadm trigger /dev/uinput")
    return 1


def find_profile_line_number(config_path: Path, profile_name: str) -> int | None:
    """Find the 1-indexed line number where [profiles.<name>] is defined in config."""
    norm = profile_name.strip().lower()
    pattern = re.compile(rf"^\s*\[profiles\.{re.escape(norm)}\]\s*$", re.IGNORECASE)
    try:
        content = config_path.read_text(encoding="utf-8")
        for idx, line in enumerate(content.splitlines(), start=1):
            if pattern.match(line.strip()):
                return idx
    except Exception:
        pass
    return None


def build_editor_command(editor_cmd: str, file_path: Path, line_number: int | None = None) -> list[str]:
    """Build command arguments to launch the editor at file_path (and optional line_number)."""
    parts = shlex.split(editor_cmd)
    if not parts:
        parts = ["nvim"] if shutil.which("nvim") else ["nano"]

    if line_number is None or line_number <= 0:
        return parts + [str(file_path)]

    base = Path(parts[0]).name.lower()
    if base in ("code", "codium", "cursor"):
        return parts + ["-g", f"{file_path}:{line_number}"]
    elif base in ("hx", "helix"):
        return parts + [f"{file_path}:{line_number}"]
    else:
        # Standard POSIX / vi line option (+<line>) supported by nvim, vim, nano, micro, omarchy-launch-editor
        return parts + [f"+{line_number}", str(file_path)]


def _cmd_config(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))

    # Backward compatibility: 'init-config' without arguments just creates/ensures config and prints path
    if getattr(args, "command", "") == "init-config" and not getattr(args, "profile", None):
        print(f"Configuration ready at: {cfg_path}")
        return 0

    if getattr(args, "path", False):
        print(cfg_path)
        return 0

    if getattr(args, "list", False):
        cfg = load_config(cfg_path)
        print(f"Config File: {cfg_path}")
        print("Configured Profiles:")
        for name in cfg.profiles:
            lineno = find_profile_line_number(cfg_path, name)
            loc = f" (line {lineno})" if lineno else ""
            print(f"  - {name:<14}{loc}")
        return 0

    target_profile = getattr(args, "profile", None)
    target_line: int | None = None

    if target_profile:
        norm_name = target_profile.strip().lower()
        existing_line = find_profile_line_number(cfg_path, norm_name)
        if existing_line is not None:
            target_line = existing_line
            print(f"Found existing profile [{norm_name}] in {cfg_path} (line {target_line}).")
        else:
            print(f"Profile {norm_name!r} not found in {cfg_path}.")
            create_new = getattr(args, "yes", False)
            if not create_new:
                if not sys.stdin.isatty():
                    print("Non-interactive session: use --yes to create a new profile.", file=sys.stderr)
                    return 1
                try:
                    ans = input(f"Would you like to create a new profile for {norm_name!r}? [Y/n]: ").strip().lower()
                    if ans not in ("", "y", "yes"):
                        print("Aborted.")
                        return 0
                    create_new = True
                except (EOFError, KeyboardInterrupt):
                    print("\nAborted.")
                    return 130

            if create_new:
                desc = f"{norm_name.title()} Shortcuts"
                match_cls = norm_name
                turn = 0
                if sys.stdin.isatty() and not getattr(args, "yes", False):
                    try:
                        d_input = input(f"Profile Description [{desc}]: ").strip()
                        if d_input:
                            desc = d_input
                        c_input = input(f"Window Class Regex (match_class) [{match_cls}]: ").strip()
                        if c_input:
                            match_cls = c_input
                        t_input = input("D-pad Turn Angle (0 or 90) [0]: ").strip()
                        if t_input in ("0", "90", "180", "270"):
                            turn = int(t_input)
                    except (EOFError, KeyboardInterrupt):
                        print("\nAborted.")
                        return 130

                template = f"""
# -----------------------------------------------------------------------------
# Profile: {norm_name.upper()}
# -----------------------------------------------------------------------------
[profiles.{norm_name}]
description = {desc!r}
match_class = {match_cls!r}
turn = {turn}

[profiles.{norm_name}.buttons]
# Face buttons
A = "Space"
B = "Enter"
X = "PageUp"
Y = "PageDown"

# Shoulders
L = "Tab"
R = "Escape"
L2 = "LeftShift"
R2 = "LeftCtrl"

# Center & Bottom
SELECT = "Esc"
START = "Enter"
STAR = "None"
HOME = "None"

# D-Pad
UP = "Up"
DOWN = "Down"
LEFT = "Left"
RIGHT = "Right"

# Optional simultaneous button combinations (e.g. L + R pressed within 40ms)
# [profiles.{norm_name}.combos]
# "L+R" = ["Super", "K"]
# "Select+Start" = "exec:notify-send 'Profile' 'Switched'"
"""
                existing_text = cfg_path.read_text(encoding="utf-8")
                if not existing_text.endswith("\n"):
                    existing_text += "\n"
                cfg_path.write_text(existing_text + template.lstrip(), encoding="utf-8")
                print(f"Created new profile [{norm_name}] in {cfg_path}.")
                target_line = find_profile_line_number(cfg_path, norm_name)
    else:
        target_line = 1

    if getattr(args, "no_edit", False):
        return 0

    editor = getattr(args, "editor", None) or os.environ.get("VISUAL") or os.environ.get("EDITOR") or "nvim"
    cmd = build_editor_command(editor, cfg_path, target_line)
    line_info = f" at line {target_line}" if target_line else ""
    print(f"Opening {cfg_path.name}{line_info} with {cmd[0]}...")

    try:
        subprocess.run(cmd)
    except FileNotFoundError:
        fallback = "nano" if shutil.which("nano") else "vi"
        fallback_cmd = build_editor_command(fallback, cfg_path, target_line)
        subprocess.run(fallback_cmd)

    try:
        load_config(cfg_path)
        print(f"✓ Configuration syntax is valid ({cfg_path}).")
    except Exception as exc:
        print(f"⚠️ Warning: Configuration syntax error in {cfg_path}: {exc}", file=sys.stderr)

    return 0


async def _cmd_show(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    cfg = load_config(cfg_path)

    active_win = None
    target_profile_name = args.profile
    if not target_profile_name:
        try:
            active_win = await query_active_window()
            if active_win and (active_win.window_class or active_win.window_title):
                matched = cfg.resolve_profile_for_window(active_win.window_class, active_win.window_title)
                target_profile_name = matched.name
                print(
                    f"Focused Window: class={active_win.window_class!r}, title={active_win.window_title!r} "
                    f"-> Profile: {target_profile_name}",
                    file=sys.stderr,
                )
        except Exception:
            pass

    if not target_profile_name:
        target_profile_name = cfg.fallback_profile or next(iter(cfg.profiles.keys()), "default")

    prof = cfg.get_profile(target_profile_name)

    is_interactive = getattr(args, "interactive", False) or args.format == "html"
    if is_interactive:
        content = generate_html_viewer(cfg.profiles, prof.name)
        ext = ".html"
    elif args.format == "png":
        svg_content = generate_svg_diagram(prof)
        ext = ".png"
    else:
        content = generate_svg_diagram(prof)
        ext = ".svg"

    if getattr(args, "stdout", False):
        if args.format == "png":
            if shutil.which("resvg"):
                import subprocess
                import tempfile
                with tempfile.NamedTemporaryFile("w", suffix=".svg") as s_f:
                    s_f.write(svg_content)
                    s_f.flush()
                    proc = subprocess.run(["resvg", s_f.name, "-"], capture_output=True, check=True)
                    sys.stdout.buffer.write(proc.stdout)
                return 0
            print("Error: 'resvg' is required for PNG stdout export.", file=sys.stderr)
            return 1
        sys.stdout.write(content)
        return 0

    if args.save:
        out_path = Path(args.save).expanduser()
        out_path.parent.mkdir(parents=True, exist_ok=True)
        if args.format == "png":
            if shutil.which("resvg"):
                import subprocess
                import tempfile
                with tempfile.NamedTemporaryFile("w", suffix=".svg") as s_f:
                    s_f.write(svg_content)
                    s_f.flush()
                    subprocess.run(["resvg", s_f.name, str(out_path)], check=True)
                print(f"Saved PNG diagram to: {out_path}")
                return 0
            print("Error: 'resvg' is required for PNG output.", file=sys.stderr)
            return 1
        out_path.write_text(content, encoding="utf-8")
        print(f"Saved diagram to: {out_path}")
        return 0

    import tempfile
    tmp_path = Path(tempfile.gettempdir()) / f"8bitdo-micro-{prof.name}{ext}"
    if args.format == "png":
        if shutil.which("resvg"):
            import subprocess
            tmp_svg = tmp_path.with_suffix(".svg")
            tmp_svg.write_text(svg_content, encoding="utf-8")
            subprocess.run(["resvg", str(tmp_svg), str(tmp_path)], check=True)
        else:
            print("Error: 'resvg' is required for PNG preview.", file=sys.stderr)
            return 1
    else:
        tmp_path.write_text(content, encoding="utf-8")

    viewer = args.viewer
    if not viewer:
        if ext == ".html":
            viewer = "chromium" if shutil.which("chromium") else "xdg-open"
        else:
            viewer = "imv" if shutil.which("imv") else "xdg-open"

    print(f"Displaying {prof.name} ({ext[1:].upper()}) via {viewer} ...", file=sys.stderr)
    proc = await asyncio.create_subprocess_exec(
        viewer,
        str(tmp_path),
        stdout=asyncio.subprocess.DEVNULL,
        stderr=asyncio.subprocess.DEVNULL,
    )
    if not getattr(args, "no_wait", False):
        await proc.wait()
    return 0


async def _cmd_discover(args: argparse.Namespace) -> int:
    timeout = getattr(args, "timeout", 4.0)
    print("=== 8BitDo Micro Discovery & Connectivity Check ===\n")

    # 1. Classic Bluetooth / evdev input devices
    print("1. Classic Bluetooth HID Devices (/dev/input/event*):")
    devs = find_micro_input_devices()
    if not devs:
        print("   • No 8BitDo Micro paired & connected as HID keyboard right now.")
        print("     (Make sure slider is at 'K' and controller is paired in Bluetooth settings)")
    else:
        for d in devs:
            print(
                f"   ✓ Found {d.path}: {d.name!r} (Vendor=0x{d.info.vendor:04x}, "
                f"Product=0x{d.info.product:04x}, BT_MAC={d.uniq})"
            )
            d.close()
    print()

    # 2. Over-the-air BLE GATT peripherals
    print("2. Over-the-Air BLE GATT Peripherals ('80EL'):")
    print(f"   Scanning 2.4GHz BLE advertisements for {timeout:.1f}s ...")
    try:
        ble_devs = await scan_for_micro_ble_devices(timeout=timeout)
        if not ble_devs:
            print("   • No '80EL' BLE peripherals detected nearby.")
            print("     (Ensure controller is powered on and within range)")
        else:
            for b in ble_devs:
                rssi_str = f"RSSI={b.rssi} dBm" if b.rssi is not None else ""
                print(f"   ✓ Found {b.name!r} at BLE_MAC={b.address} ({rssi_str})")
    except Exception as exc:
        print(f"   BLE scan error: {exc}")

    print("\nSummary:")
    print(" • Controller discovery is 100% dynamic: no MAC addresses or event paths are hard-coded.")
    print(" • Any 8BitDo Micro will be auto-detected upon pairing and powering on.")
    return 0


def build_parser() -> argparse.ArgumentParser:
    common_parser = argparse.ArgumentParser(add_help=False)
    common_parser.add_argument(
        "-c",
        "--config",
        type=Path,
        default=argparse.SUPPRESS,
        help=f"Path to profiles.toml (default: {DEFAULT_CONFIG_PATH})",
    )
    common_parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        default=argparse.SUPPRESS,
        help="Enable debug logging",
    )

    parser = argparse.ArgumentParser(
        prog="8bitdo-micro-focus-switch",
        parents=[common_parser],
        description=(
            "Focus-aware input remapper daemon (Hyprland + evdev/uinput) and "
            "direct BLE GATT ('80EL') hardware profile flasher for the 8BitDo Micro."
        ),
    )

    sub = parser.add_subparsers(dest="command", required=True)

    # status
    p_status = sub.add_parser(
        "status",
        parents=[common_parser],
        help="Show Hyprland focus, input device, and profile status",
    )
    p_status.add_argument("--ble", action="store_true", help="Also query live hardware state over BLE GATT")
    p_status.add_argument("--address", help="Optional BLE MAC address override (e.g. E4:17:D8:D8:52:02)")
    p_status.add_argument("--timeout", type=float, default=6.0, help="BLE scan timeout in seconds")

    # discover / scan
    p_disc = sub.add_parser(
        "discover",
        aliases=["scan"],
        parents=[common_parser],
        help="Scan and list all connected and nearby 8BitDo Micro controllers (HID + BLE)",
    )
    p_disc.add_argument("--timeout", type=float, default=4.0, help="BLE scan timeout in seconds")

    # daemon
    p_daemon = sub.add_parser(
        "daemon",
        parents=[common_parser],
        help="Run the Hyprland focus-aware profile switching daemon",
    )
    p_daemon.add_argument(
        "--backend",
        choices=("uinput", "ble", "hybrid"),
        default=None,
        help="Remapping backend: 'uinput' (<1ms evdev grab + virtual keyboard) or 'ble' (GATT reflash)",
    )
    p_daemon.add_argument(
        "--ensure-factory-ble",
        action="store_true",
        help="On startup, backup hardware config and reset controller to factory keys over BLE if needed",
    )
    p_daemon.add_argument("--address", help="Optional BLE MAC address override")

    # ble
    p_ble = sub.add_parser("ble", help="Direct BLE GATT ('80EL') hardware profile reader and flasher")
    ble_sub = p_ble.add_subparsers(dest="ble_command", required=True)

    p_ble_read = ble_sub.add_parser("read", help="Read current firmware version and mapping from controller")
    p_ble_read.add_argument("--address", help="Optional BLE MAC address override")
    p_ble_read.add_argument("--timeout", type=float, default=6.0, help="BLE scan timeout in seconds")
    p_ble_read.add_argument("--save-bin", help="Save the raw 180-byte record_micro buffer to a file")

    p_ble_write = ble_sub.add_parser(
        "write",
        help="Flash a profile from profiles.toml (or preset: 'supermemo', 'anki', 'factory') to controller",
    )
    p_ble_write.add_argument("profile", help="Profile name in profiles.toml or 'supermemo'/'anki'/'factory'")
    p_ble_write.add_argument(
        "--turn",
        type=int,
        choices=(0, 90, 180, 270),
        default=None,
        help="Override D-pad rotation angle in degrees (0, 90, 180, 270)",
    )
    p_ble_write.add_argument("--address", help="Optional BLE MAC address override")
    p_ble_write.add_argument("--timeout", type=float, default=6.0, help="BLE scan timeout in seconds")

    p_ble_reset = ble_sub.add_parser(
        "reset",
        help="Backup current hardware profile and reset controller to factory K-mode default (c..t keys)",
    )
    p_ble_reset.add_argument("--address", help="Optional BLE MAC address override")
    p_ble_reset.add_argument("--timeout", type=float, default=6.0, help="BLE scan timeout in seconds")

    p_ble_scan = ble_sub.add_parser("scan", help="Scan for nearby 8BitDo Micro BLE GATT endpoints")
    p_ble_scan.add_argument("--timeout", type=float, default=4.0, help="Scan timeout in seconds")

    # config / edit
    p_config = sub.add_parser(
        "config",
        aliases=["edit", "init-config"],
        parents=[common_parser],
        help="Open profiles.toml in your editor, jump to a profile, or create a new profile template",
    )
    p_config.add_argument(
        "profile",
        nargs="?",
        help="Profile name to jump to or create (e.g. 'youtube', 'spotify'). If omitted, opens config file at top.",
    )
    p_config.add_argument(
        "--path",
        action="store_true",
        help="Print path to profiles.toml and exit",
    )
    p_config.add_argument(
        "--list",
        action="store_true",
        help="List all configured profiles with line numbers and exit",
    )
    p_config.add_argument(
        "-y",
        "--yes",
        action="store_true",
        help="Automatically confirm new profile creation without prompting",
    )
    p_config.add_argument(
        "--editor",
        help="Override editor command (default: $VISUAL, $EDITOR, or nvim)",
    )
    p_config.add_argument(
        "--no-edit",
        action="store_true",
        help="Do not launch editor after creating/validating profile (useful for scripts)",
    )

    # install-service
    p_svc = sub.add_parser("install-service", help="Generate systemd --user service and show udev setup")
    p_svc.add_argument(
        "--backend",
        choices=("uinput", "ble", "hybrid"),
        default=None,
        help="Backend flag to bake into the systemd service",
    )

    # install-udev
    sub.add_parser("install-udev", help="Grant input group write access to /dev/uinput via udev rule")

    # show / hud / diagram
    p_show = sub.add_parser(
        "show",
        aliases=["hud", "diagram"],
        parents=[common_parser],
        help="Display visual button mapping diagram of focused window or specified profile",
    )
    p_show.add_argument(
        "profile",
        nargs="?",
        help="Profile name (e.g. 'agy', 'youtube'). If omitted, auto-detects currently focused Hyprland window.",
    )
    p_show.add_argument(
        "--format",
        choices=("svg", "html", "png"),
        default="svg",
        help="Diagram format (default: 'svg')",
    )
    p_show.add_argument(
        "-i",
        "--interactive",
        "--all",
        action="store_true",
        help="Open interactive multi-profile HTML viewer with tabs",
    )
    p_show.add_argument(
        "-o",
        "--save",
        type=Path,
        help="Save diagram to specified file path instead of opening viewer",
    )
    p_show.add_argument(
        "--stdout",
        action="store_true",
        help="Output raw diagram content directly to stdout",
    )
    p_show.add_argument(
        "--viewer",
        help="Custom image/browser viewer command (e.g. 'imv', 'chromium', 'xdg-open')",
    )
    p_show.add_argument(
        "--no-wait",
        action="store_true",
        help="Do not block waiting for the viewer process to close",
    )

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.DEBUG if getattr(args, "verbose", False) else logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        datefmt="%H:%M:%S",
    )

    try:
        if args.command in ("discover", "scan"):
            return asyncio.run(_cmd_discover(args))
        if args.command in ("show", "hud", "diagram"):
            return asyncio.run(_cmd_show(args))
        if args.command in ("config", "edit", "init-config"):
            return _cmd_config(args)
        if args.command == "install-service":
            return _cmd_install_service(args)
        if args.command == "install-udev":
            return _cmd_install_udev(args)
        if args.command == "status":
            return asyncio.run(_cmd_status(args))
        if args.command == "daemon":
            return asyncio.run(_cmd_daemon(args))
        if args.command == "ble":
            if args.ble_command in ("scan", "discover"):
                return asyncio.run(_cmd_discover(args))
            if args.ble_command == "read":
                return asyncio.run(_cmd_ble_read(args))
            if args.ble_command == "write":
                return asyncio.run(_cmd_ble_write(args))
            if args.ble_command == "reset":
                return asyncio.run(_cmd_ble_reset(args))
    except KeyboardInterrupt:
        return 130
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
