"""Command-line interface for 8bitdo-micro-focus-switch."""

from __future__ import annotations

import argparse
import asyncio
import logging
from pathlib import Path
import shutil
import sys

from .ble import (
    format_snapshot_report,
    read_hardware_snapshot,
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
    return "\n".join(lines)


async def _cmd_status(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(args.config)
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
    cfg_path = ensure_default_config(args.config)
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
    cfg_path = ensure_default_config(args.config)
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
    ensure_default_config(args.config)
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
ExecStart={exe} daemon --config {args.config}{backend_arg}
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


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="8bitdo-micro-focus-switch",
        description=(
            "Focus-aware input remapper daemon (Hyprland + evdev/uinput) and "
            "direct BLE GATT ('80EL') hardware profile flasher for the 8BitDo Micro."
        ),
    )
    parser.add_argument(
        "-c",
        "--config",
        type=Path,
        default=DEFAULT_CONFIG_PATH,
        help=f"Path to profiles.toml (default: {DEFAULT_CONFIG_PATH})",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="Enable debug logging",
    )

    sub = parser.add_subparsers(dest="command", required=True)

    # status
    p_status = sub.add_parser("status", help="Show Hyprland focus, input device, and profile status")
    p_status.add_argument("--ble", action="store_true", help="Also query live hardware state over BLE GATT")
    p_status.add_argument("--address", help="Optional BLE MAC address override (e.g. E4:17:D8:D8:52:02)")
    p_status.add_argument("--timeout", type=float, default=6.0, help="BLE scan timeout in seconds")

    # daemon
    p_daemon = sub.add_parser("daemon", help="Run the Hyprland focus-aware profile switching daemon")
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

    # init-config
    sub.add_parser("init-config", help="Create default ~/.config/8bitdo-micro/profiles.toml if missing")

    # install-service
    p_svc = sub.add_parser("install-service", help="Generate systemd --user service and show udev setup")
    p_svc.add_argument(
        "--backend",
        choices=("uinput", "ble", "hybrid"),
        default=None,
        help="Backend flag to bake into the systemd service",
    )

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        datefmt="%H:%M:%S",
    )

    try:
        if args.command == "init-config":
            path = ensure_default_config(args.config)
            print(f"Configuration ready at: {path}")
            return 0
        if args.command == "install-service":
            return _cmd_install_service(args)
        if args.command == "status":
            return asyncio.run(_cmd_status(args))
        if args.command == "daemon":
            return asyncio.run(_cmd_daemon(args))
        if args.command == "ble":
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
