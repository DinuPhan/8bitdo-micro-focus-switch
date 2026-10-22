"""Command-line interface for 8bitdo-micro-focus-switch."""

from __future__ import annotations

import argparse
import asyncio
import json
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
from .daemon import (
    FocusSwitchDaemon,
    check_uinput_access,
    find_micro_input_devices,
    identify_controller_device,
)
from .hyprland import find_hyprland_socket2, query_active_window
from .protocol import (
    HID_TO_NAME,
    PHYSICAL_BUTTONS,
    get_builtin_hardware_preset,
)
from .recorder import (
    ChordResult,
    capture_keyboard_chord,
    find_physical_keyboards,
    format_keys_catalog,
    set_profile_binding,
)
from .visualizer import generate_html_viewer, generate_svg_diagram

UDEV_RULE_PATH = Path("/etc/udev/rules.d/99-8bitdo-micro-uinput.rules")
UDEV_RULE_CONTENT = (
    'KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", '
    'OPTIONS+="static_node=uinput", TAG+="uaccess"\n'
)
MODULES_LOAD_PATH = Path("/etc/modules-load.d/99-8bitdo-micro-uinput.conf")


def _format_profile_table(config_path: Path) -> str:
    cfg = load_config(config_path)
    lines = [
        f"Config File      : {config_path}",
        f"Default Backend  : {cfg.backend}",
        f"Fallback Profile : {cfg.fallback_profile}",
        "",
        "Available Profiles:",
    ]
    if cfg.global_combos:
        combo_parts = [
            f"{'+'.join(sorted(c))} -> {'+'.join(act.keys) if act.keys else act.exec_cmd}"
            for c, act in cfg.global_combos.items()
        ]
        lines.append("Global Combos (inherited by all profiles by default):")
        lines.append(f"  {', '.join(combo_parts)}")
        lines.append("")

    for name, prof in cfg.profiles.items():
        if name in ("global", "all"):
            lines.append(f"  - {name:<12} (global base template: inherited by all profiles)")
            if prof.combos:
                combo_parts = [
                    f"{'+'.join(sorted(c))} -> {'+'.join(act.keys) if act.keys else act.exec_cmd}"
                    for c, act in prof.combos.items()
                ]
                lines.append(f"      Combos: {', '.join(combo_parts)}")
            continue
        match_parts: list[str] = []
        if prof.match_rules:
            for r in prof.match_rules:
                r_parts = []
                if r.match_class:
                    r_parts.append(f"class~={r.match_class!r}")
                if r.match_title:
                    r_parts.append(f"title~={r.match_title!r}")
                if r_parts:
                    match_parts.append(f"({' && '.join(r_parts)})")
            match_str = " || ".join(match_parts) if match_parts else "(manual / fallback)"
        else:
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

    service_systemd = is_systemd_service_active()
    daemon_pids = [] if service_systemd else find_daemon_pids()
    service_active = service_systemd or bool(daemon_pids)

    devs = find_micro_input_devices()
    devices_info = []
    for d in devs:
        model = identify_controller_device(d) or "8BitDo Controller"
        devices_info.append({
            "path": str(d.path),
            "name": d.name or "",
            "model": model,
            "vendor": f"0x{d.info.vendor:04x}",
            "product": f"0x{d.info.product:04x}",
            "uniq": d.uniq or "",
        })
        d.close()

    uinput_ok, uinput_msg = check_uinput_access()
    if not uinput_ok and shutil.which("wtype"):
        uinput_ok = True
        uinput_msg = "Wayland user-space virtual keyboard (wtype fallback active; no /dev/uinput required)"

    if getattr(args, "json", False):
        def _action_dict(act: object) -> dict[str, object]:
            keys = list(getattr(act, "keys", ()))
            exec_cmd = getattr(act, "exec_cmd", None)
            desc = getattr(act, "description", None)
            label = "+".join(keys) if keys else (f"exec:{exec_cmd}" if exec_cmd else "None")
            return {
                "keys": keys,
                "exec": exec_cmd,
                "label": label,
                "description": desc or "",
            }

        global_combos_list = []
        for c, act in cfg.global_combos.items():
            combo_key = "+".join(sorted(c))
            global_combos_list.append({
                "combo": combo_key,
                "buttons": sorted(c),
                **_action_dict(act),
            })

        profiles_dict = {}
        for pname, prof in cfg.profiles.items():
            btns = {}
            for b in PHYSICAL_BUTTONS:
                b_act = prof.buttons.get(b)
                if b_act is not None:
                    btns[b] = _action_dict(b_act)
                else:
                    btns[b] = {"keys": [], "exec": None, "label": "None", "description": ""}
            p_combos = []
            for c, act in prof.combos.items():
                combo_key = "+".join(sorted(c))
                p_combos.append({
                    "combo": combo_key,
                    "buttons": sorted(c),
                    **_action_dict(act),
                })
            profiles_dict[pname] = {
                "name": prof.name,
                "description": prof.description or "",
                "turn": prof.turn,
                "match_class": prof.match_class or "",
                "match_title": prof.match_title or "",
                "buttons": btns,
                "combos": p_combos,
            }

        payload = {
            "config_path": str(cfg_path),
            "backend": cfg.backend,
            "fallback_profile": cfg.fallback_profile,
            "active_window": {
                "class": win.window_class or "",
                "title": win.window_title or "",
            },
            "active_profile": matched.name,
            "active_turn": matched.turn,
            "service_active": service_active,
            "service_mode": "systemd" if service_systemd else ("standalone" if daemon_pids else "inactive"),
            "uinput_ok": uinput_ok,
            "uinput_message": uinput_msg,
            "connected": len(devices_info) > 0,
            "devices": devices_info,
            "global_combos": global_combos_list,
            "profiles": profiles_dict,
        }
        print(json.dumps(payload, ensure_ascii=False))
        return 0

    print("=== Hyprland Focus Status ===")
    print(f"Socket2 Path     : {sock or 'not found'}")
    print(f"Active Window    : class={win.window_class!r}, title={win.window_title!r}")
    print(f"Resolved Profile : {matched.name} (turn={matched.turn}°)")
    print()

    print("=== Daemon Background Service ===")
    if service_systemd:
        print("Service Status   : Active (running via systemd --user 8bitdo-micro-focus-switch.service)")
    elif daemon_pids:
        print(f"Service Status   : Active (standalone PID(s): {', '.join(map(str, daemon_pids))})")
    else:
        print("Service Status   : Inactive")
    print()

    print("=== Classic HID Input (/dev/input) ===")
    if not devices_info:
        print("8BitDo Micro / Zero 2 Keyboard : not connected")
    else:
        for d_info in devices_info:
            print(
                f"Device           : {d_info['path']} ({d_info['name']!r}, model={d_info['model']}, "
                f"vendor={d_info['vendor']}, product={d_info['product']}, uniq={d_info['uniq']!r})"
            )
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

    num_combos, num_exec = prof.software_only_features
    if num_combos > 0 or num_exec > 0:
        print(
            f"\n⚠️  Notice: Profile {prof.name!r} contains software-only features "
            f"({num_combos} combo(s), {num_exec} shell command(s)).\n"
            "   The 180-byte 8BitDo Micro hardware memory only supports 1-to-1 button mappings.\n"
            "   Combos and shell commands are handled by the daemon (uinput backend) and will not exist in hardware.\n"
        )

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
        config_path=cfg_path,
        backend=args.backend,
        ble_address=args.address,
    )
    await daemon.run()
    return 0


def _write_systemd_service_unit(cfg_path: Path, backend: str | None = None) -> Path:
    exe = shutil.which("8bitdo-micro-focus-switch") or str(Path(sys.argv[0]).resolve())
    service_dir = Path.home() / ".config" / "systemd" / "user"
    service_dir.mkdir(parents=True, exist_ok=True)
    service_path = service_dir / "8bitdo-micro-focus-switch.service"

    backend_arg = f" --backend {backend}" if backend else ""
    unit_text = f"""\
[Unit]
Description=8BitDo Micro Focus-Aware Input Remapper Daemon (Hyprland)
After=graphical-session.target
StartLimitIntervalSec=0

[Service]
Type=simple
ExecStart={exe} daemon --config {cfg_path} --ensure-factory-ble{backend_arg}
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
"""
    service_path.write_text(unit_text, encoding="utf-8")
    return service_path


def _enable_and_start_systemd_service() -> tuple[bool, str]:
    if not shutil.which("systemctl"):
        return False, "systemctl not found"
    try:
        subprocess.run(
            ["systemctl", "--user", "daemon-reload"],
            capture_output=True,
            check=False,
        )
        subprocess.run(
            ["systemctl", "--user", "reset-failed", "8bitdo-micro-focus-switch.service"],
            capture_output=True,
            check=False,
        )
        was_active = is_systemd_service_active()
        res_en = subprocess.run(
            ["systemctl", "--user", "enable", "--now", "8bitdo-micro-focus-switch.service"],
            capture_output=True,
            text=True,
            check=False,
        )
        if was_active:
            subprocess.run(
                ["systemctl", "--user", "restart", "8bitdo-micro-focus-switch.service"],
                capture_output=True,
                check=False,
            )
        if res_en.returncode == 0 or is_systemd_service_active():
            return True, "Enabled and started 8bitdo-micro-focus-switch.service"
        err = (res_en.stderr or res_en.stdout or "").strip()
        return False, err or f"systemctl exited with code {res_en.returncode}"
    except Exception as exc:
        return False, str(exc)


def _cmd_install_service(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    service_path = _write_systemd_service_unit(cfg_path, getattr(args, "backend", None))
    print(f"✓ Wrote systemd user unit: {service_path}")
    ok, msg = _enable_and_start_systemd_service()
    if ok:
        print(f"✓ {msg}")
    else:
        print(f"ℹ Could not auto-start systemd service ({msg}). To start manually:")
        print("  systemctl --user daemon-reload && systemctl --user enable --now 8bitdo-micro-focus-switch.service")

    uinput_ok, _ = check_uinput_access()
    if not uinput_ok:
        print("\nNOTE: For multi-button combos or instant <1ms remapping, run:")
        print("  8bitdo-micro-focus-switch install-udev")
    return 0


def _cmd_install_udev(args: argparse.Namespace) -> int:
    uinput_ok, msg = check_uinput_access()
    if uinput_ok and MODULES_LOAD_PATH.exists():
        print("✓ /dev/uinput is already accessible and configured to load on boot!")
        return 0

    print(f"Configuring /dev/uinput udev rule ({UDEV_RULE_PATH}) and boot module ({MODULES_LOAD_PATH}) ...")
    cmd = (
        f"echo '{UDEV_RULE_CONTENT.strip()}' | sudo tee {UDEV_RULE_PATH} > /dev/null "
        f"&& echo 'uinput' | sudo tee {MODULES_LOAD_PATH} > /dev/null "
        "&& sudo modprobe uinput "
        "&& sudo udevadm control --reload-rules "
        "&& sudo udevadm trigger --sysname-match=uinput"
    )
    try:
        res = subprocess.run(["sudo", "sh", "-c", cmd])
        if res.returncode == 0:
            ok, msg = check_uinput_access()
            if ok:
                print("✓ Granted /dev/uinput permissions and enabled uinput module on boot!")
                return 0
            print(f"✓ Rule installed ({msg})")
            return 0
    except Exception as exc:
        print(f"Failed to invoke sudo: {exc}")

    print("\nPlease run the following command in your terminal with sudo:")
    print(f"  echo '{UDEV_RULE_CONTENT.strip()}' | sudo tee {UDEV_RULE_PATH}")
    print(f"  echo 'uinput' | sudo tee {MODULES_LOAD_PATH} && sudo modprobe uinput")
    print("  sudo udevadm control --reload-rules && sudo udevadm trigger --sysname-match=uinput")
    return 1


def _cmd_setup(args: argparse.Namespace) -> int:
    """One-step setup: create default config, configure /dev/uinput udev access, and enable/start systemd service."""
    cfg_path = ensure_default_config(getattr(args, "config", None))
    cfg = load_config(cfg_path)
    print(f"✓ Configuration ready: {cfg_path} ({len(cfg.profiles)} profiles, {len(cfg.global_combos)} global combos)")

    if not getattr(args, "skip_udev", False):
        uinput_ok, _ = check_uinput_access()
        if uinput_ok:
            print("✓ /dev/uinput permissions: OK")
        elif shutil.which("wtype") and not sys.stdin.isatty():
            print("✓ Wayland user-space virtual keyboard (wtype) available (no sudo required)")
        else:
            _cmd_install_udev(args)

    if not getattr(args, "skip_service", False):
        service_path = _write_systemd_service_unit(cfg_path, getattr(args, "backend", None))
        print(f"✓ Installed systemd user service: {service_path}")
        ok, msg = _enable_and_start_systemd_service()
        if ok:
            print(f"✓ {msg}")
        else:
            print(f"ℹ Systemd service written ({msg})")

    return 0


def is_systemd_service_active() -> bool:
    """Return True if 8bitdo-micro-focus-switch.service is active under systemd --user."""
    try:
        proc = subprocess.run(
            ["systemctl", "--user", "is-active", "--quiet", "8bitdo-micro-focus-switch.service"],
            capture_output=True,
            check=False,
        )
        return proc.returncode == 0
    except Exception:
        return False


def find_daemon_pids() -> list[int]:
    """Find process IDs of any running standalone 8bitdo-micro-focus-switch daemon instances."""
    pids: list[int] = []
    my_pid = os.getpid()
    proc_dir = Path("/proc")
    if not proc_dir.exists():
        return pids
    for entry in proc_dir.iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid == my_pid:
            continue
        try:
            cmdline = (entry / "cmdline").read_bytes().split(b"\x00")
            cmd_strs = [c.decode("utf-8", "replace") for c in cmdline if c]
            cmd_full = " ".join(cmd_strs)
            if "8bitdo-micro-focus-switch" in cmd_full and "daemon" in cmd_strs:
                pids.append(pid)
        except (OSError, PermissionError):
            continue
    return pids


def reload_running_daemon() -> tuple[bool, str]:
    """Send SIGHUP to running daemon instances (via systemd or process signals)."""
    import signal

    reloaded_any = False
    details: list[str] = []

    # 1. Systemd user service
    if is_systemd_service_active():
        try:
            res = subprocess.run(
                ["systemctl", "--user", "kill", "-s", "HUP", "8bitdo-micro-focus-switch.service"],
                capture_output=True,
                check=False,
            )
            if res.returncode == 0:
                reloaded_any = True
                details.append("systemd service (8bitdo-micro-focus-switch.service)")
        except Exception:
            pass

    # 2. Standalone daemon processes (if not managed by systemd)
    if not reloaded_any:
        pids = find_daemon_pids()
        for pid in pids:
            try:
                os.kill(pid, signal.SIGHUP)
                reloaded_any = True
                details.append(f"daemon PID {pid}")
            except Exception:
                pass

    if reloaded_any:
        return True, f"Sent reload signal (SIGHUP) to {', '.join(details)}."
    return False, "Daemon is not currently running."


def _cmd_reload(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    # 1. Validate configuration syntax first
    try:
        cfg = load_config(cfg_path)
    except Exception as exc:
        print(f"Error: Invalid configuration in {cfg_path}:\n  {exc}", file=sys.stderr)
        return 1

    print(f"Validated configuration syntax: {cfg_path}")
    active_profs = ", ".join(cfg.profiles.keys())
    print(f"Configured profiles ({len(cfg.profiles)}): {active_profs}")

    success, msg = reload_running_daemon()
    if success:
        print(f"✓ {msg}")
        return 0
    else:
        print(f"ℹ {msg}")
        print("  To start the background service:")
        print("    systemctl --user start 8bitdo-micro-focus-switch.service")
        print("  Or run in foreground:")
        print("    8bitdo-micro-focus-switch daemon")
        return 0


async def _cmd_bind(args: argparse.Namespace) -> int:
    cfg_path = ensure_default_config(getattr(args, "config", None))
    cfg = load_config(cfg_path)

    if getattr(args, "list_keys", False):
        print(format_keys_catalog())
        return 0

    # 1. Determine profile
    target_profile = getattr(args, "profile", None)
    if not target_profile:
        try:
            active_win = await query_active_window()
            if active_win and (active_win.window_class or active_win.window_title):
                matched = cfg.resolve_profile_for_window(active_win.window_class, active_win.window_title)
                target_profile = matched.name
                print(f"Focused Window: {active_win.window_class!r} -> Profile: {target_profile!r}")
        except Exception:
            pass

    if not target_profile:
        target_profile = cfg.fallback_profile or next(iter(cfg.profiles.keys()), "default")

    if target_profile not in cfg.profiles:
        print(f"Error: Profile {target_profile!r} not found in {cfg_path}.", file=sys.stderr)
        print(f"Configured profiles: {', '.join(cfg.profiles.keys())}", file=sys.stderr)
        return 1

    # 2. Determine target button or combo
    target_btn = getattr(args, "button", None)
    if not target_btn:
        if not sys.stdin.isatty():
            print("Error: Button or combo name required in non-interactive session.", file=sys.stderr)
            return 1
        print(f"\nRemapping profile [{target_profile}]:")
        print("  Buttons: A, B, X, Y, L, R, L2, R2, SELECT, START, STAR, HOME, UP, DOWN, LEFT, RIGHT")
        print("  Combos : E.g. 'L+R', 'L2+LEFT', 'Select+Start'")
        print("  Catalog: Type '?' to browse bindable keys")
        try:
            btn_input = input("\nEnter button or combo to bind: ").strip()
            if btn_input in ("?", "help", "list"):
                print()
                print(format_keys_catalog())
                btn_input = input("\nEnter button or combo to bind: ").strip()
            if not btn_input:
                print("Aborted.")
                return 0
            target_btn = btn_input
        except (EOFError, KeyboardInterrupt):
            print("\nAborted.")
            return 130

    # 3. Determine keys to bind
    key_arg = getattr(args, "key", None)
    if key_arg:
        if key_arg.startswith("exec:"):
            keys = [key_arg]
        elif "+" in key_arg and not key_arg.startswith("KEY_"):
            keys = [k.strip() for k in key_arg.split("+") if k.strip()]
        else:
            keys = [key_arg.strip()]
    else:
        print(f"\n>> Listening for keyboard input for [{target_profile}.{target_btn}]...")
        print(">> Press any key or combination on your keyboard (e.g. Space, Ctrl+Shift+P, Super+K).")
        print(">> Press ESC to cancel.")
        try:
            res = capture_keyboard_chord()
        except RuntimeError as err:
            print(f"\n⚠️  {err}\n", file=sys.stderr)
            if sys.stdin.isatty():
                try:
                    manual = input("Enter key or chord manually (e.g. Space, Ctrl+Shift+P): ").strip()
                    if not manual:
                        return 0
                    keys = [manual]
                except (EOFError, KeyboardInterrupt):
                    return 130
            else:
                return 1
        else:
            if res.cancelled:
                print("\n[ESC pressed] Cancelled. No changes made.")
                return 0
            keys = res.keys

    if not keys:
        print("No keys captured. Aborted.")
        return 0

    try:
        desc_arg = getattr(args, "description", None)
        set_profile_binding(cfg_path, target_profile, target_btn, keys, description=desc_arg)
    except Exception as exc:
        print(f"Error saving binding: {exc}", file=sys.stderr)
        return 1

    keys_disp = " + ".join(keys) if len(keys) > 1 else keys[0]
    print(f"\n✓ Bound [{target_profile}] {target_btn} = {keys_disp}")
    print(f"✓ Saved to {cfg_path}")

    success, msg = reload_running_daemon()
    if success:
        print(f"✓ {msg}")

    return 0


def _cmd_record(args: argparse.Namespace) -> int:
    timeout = getattr(args, "timeout", None)
    print("=== 8BitDo Micro Live Keyboard Chord Recorder ===")
    print("Press any key or combination on your keyboard to inspect its chord name and TOML format.")
    print("Press ESC to exit.\n")

    try:
        devices = find_physical_keyboards()
    except Exception as exc:
        print(f"Error accessing keyboards: {exc}", file=sys.stderr)
        return 1

    if not devices:
        print(
            "Error: No accessible physical keyboards found in /dev/input/event*.\n"
            "Ensure you have read permissions on /dev/input (member of group 'input').\n"
            "Run: 8bitdo-micro-focus-switch install-udev",
            file=sys.stderr,
        )
        return 1

    try:
        while True:
            res = capture_keyboard_chord(devices=devices, timeout=timeout)
            if res.cancelled:
                print("\n[ESC pressed] Exited.")
                break
            chord_str = " + ".join(res.keys)
            toml_str = res.toml_value
            print(f"  • Captured: {chord_str:<24} -->  TOML: {toml_str}")
    except KeyboardInterrupt:
        print("\nExited.")
    finally:
        for d in devices:
            try:
                d.close()
            except Exception:
                pass
    return 0


def _cmd_keys(args: argparse.Namespace) -> int:
    query = getattr(args, "query", None)
    print(format_keys_catalog(query))
    return 0


def find_profile_line_number(config_path: Path, profile_name: str) -> int | None:
    """Find the 1-indexed line number where [profiles.<name>] is defined in config."""
    norm = profile_name.strip().lower()
    pattern = re.compile(rf"^\s*\[(?:profiles\.)?{re.escape(norm)}\]\s*$", re.IGNORECASE)
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

                if norm_name in ("global", "all"):
                    template = f"""
# -----------------------------------------------------------------------------
# Global Base Profile: {norm_name.upper()}
# Combos and buttons defined here apply to ALL profiles by default.
# If an individual app profile defines its own combo, that profile takes precedence.
# -----------------------------------------------------------------------------
[profiles.{norm_name}]
description = "Global defaults inherited by all profiles unless overridden"

[profiles.{norm_name}.combos]
"L+R" = ["Super", "K"]               # L1 + R1 -> Super + K (Application launcher)
"Select+Start" = "exec:hyprctl dispatch togglespecialworkspace"
"""
                else:
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
        success, msg = reload_running_daemon()
        if success:
            print(f"✓ {msg}")
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
    print("=== 8BitDo Controller Discovery & Connectivity Check ===\n")

    # 1. Classic Bluetooth / evdev input devices
    print("1. Classic Bluetooth HID Devices (/dev/input/event*):")
    devs = find_micro_input_devices()
    if not devs:
        print("   • No 8BitDo Micro or Zero 2 paired & connected as HID keyboard right now.")
        print("     - 8BitDo Micro : ensure slider is set to 'K'")
        print("     - 8BitDo Zero 2: turn on by holding R + START")
    else:
        for d in devs:
            model = identify_controller_device(d) or "8BitDo Controller"
            print(
                f"   ✓ Found {d.path}: {d.name!r} [{model}] (Vendor=0x{d.info.vendor:04x}, "
                f"Product=0x{d.info.product:04x}, BT_MAC={d.uniq})"
            )
            d.close()
    print()

    # 2. Over-the-air BLE GATT peripherals
    print("2. Over-the-Air BLE GATT Peripherals ('80EL' - 8BitDo Micro only):")
    print(f"   Scanning 2.4GHz BLE advertisements for {timeout:.1f}s ...")
    try:
        ble_devs = await scan_for_micro_ble_devices(timeout=timeout)
        if not ble_devs:
            print("   • No '80EL' BLE peripherals detected nearby.")
            print("     (Note: BLE GATT hardware flashing is specific to 8BitDo Micro)")
        else:
            for b in ble_devs:
                rssi_str = f"RSSI={b.rssi} dBm" if b.rssi is not None else ""
                print(f"   ✓ Found {b.name!r} at BLE_MAC={b.address} ({rssi_str})")
    except Exception as exc:
        print(f"   BLE scan error: {exc}")

    print("\nSummary:")
    print(" • Controller discovery is 100% dynamic: no MAC addresses or event paths are hard-coded.")
    print(" • Any 8BitDo Micro or Zero 2 will be auto-detected upon pairing and powering on.")
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
    # status
    p_status = sub.add_parser(
        "status",
        parents=[common_parser],
        help="[Daily] Show Hyprland focus, connected controller, and profile status",
    )
    p_status.add_argument("--json", action="store_true", help="Output structured JSON status for UI/plugin integration")
    p_status.add_argument("--ble", action="store_true", help="Also query live hardware state over BLE GATT")
    p_status.add_argument("--address", help="Optional BLE MAC address override (e.g. E4:17:D8:D8:52:02)")
    p_status.add_argument("--timeout", type=float, default=6.0, help="BLE scan timeout in seconds")

    # discover / scan
    p_disc = sub.add_parser(
        "discover",
        aliases=["scan"],
        parents=[common_parser],
        help="[Daily] Scan and list all connected and nearby 8BitDo Micro controllers (HID + BLE)",
    )
    p_disc.add_argument("--timeout", type=float, default=4.0, help="BLE scan timeout in seconds")

    # daemon
    p_daemon = sub.add_parser(
        "daemon",
        parents=[common_parser],
        help="[Daemon] Run the Hyprland focus-aware profile switching daemon in foreground",
    )
    p_daemon.add_argument(
        "--backend",
        choices=("uinput", "host", "ble", "hybrid"),
        default=None,
        help="Remapping backend: 'host'/'uinput' (<1ms user-space/evdev grab + virtual keyboard), 'ble' (GATT reflash), or 'hybrid'",
    )
    p_daemon.add_argument(
        "--ensure-factory-ble",
        action="store_true",
        help="On startup, backup hardware config and reset controller to factory keys over BLE if needed",
    )
    p_daemon.add_argument("--address", help="Optional BLE MAC address override")

    # ble
    p_ble = sub.add_parser(
        "ble",
        help="[Hardware] Direct BLE GATT ('80EL') hardware profile reader and flasher (standalone mode)",
    )
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
        help="[Daily] Open profiles.toml in your editor, jump to a profile, or create new profiles",
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

    # bind
    p_bind = sub.add_parser(
        "bind",
        parents=[common_parser],
        help="[Daily] Interactively bind a button or combo by pressing keys on your keyboard (ESC cancels)",
    )
    p_bind.add_argument("profile", nargs="?", help="Profile name (e.g. 'agy', 'youtube'). Defaults to focused window.")
    p_bind.add_argument("button", nargs="?", help="Button (e.g. 'A', 'L1', 'X') or combo (e.g. 'L+R', 'Select+Start').")
    p_bind.add_argument("--key", help="Specify key name or chord to bind directly without waiting for keypress")
    p_bind.add_argument("-d", "--description", help="Optional inline comment/description for the binding")
    p_bind.add_argument("--list-keys", action="store_true", help="Display catalog of all bindable keys and exit")

    # record / listen
    p_rec = sub.add_parser(
        "record",
        aliases=["listen", "capture"],
        parents=[common_parser],
        help="[Daily] Listen and inspect keyboard chords and their TOML representation (ESC exits)",
    )
    p_rec.add_argument("--timeout", type=float, default=None, help="Optional timeout in seconds")

    # keys / catalog
    p_keys = sub.add_parser(
        "keys",
        aliases=["list-keys", "catalog"],
        parents=[common_parser],
        help="[Daily] Display or search catalog of all bindable keys (Letters, Modifiers, Media, etc.)",
    )
    p_keys.add_argument("query", nargs="?", help="Optional keyword filter (e.g. 'volume', 'media', 'nav')")

    # reload
    sub.add_parser(
        "reload",
        parents=[common_parser],
        help="[Daily] Reload profiles.toml into the running daemon (zero downtime)",
    )

    # setup / install / init
    p_setup = sub.add_parser(
        "setup",
        aliases=["install", "init"],
        parents=[common_parser],
        help="[One-Step Setup] Create default profiles.toml, install udev rule, and enable/start systemd service",
    )
    p_setup.add_argument(
        "--backend",
        choices=("uinput", "host", "ble", "hybrid"),
        default=None,
        help="Backend flag to bake into the systemd service",
    )
    p_setup.add_argument(
        "--skip-udev",
        action="store_true",
        help="Skip /dev/uinput udev rule check/installation",
    )
    p_setup.add_argument(
        "--skip-service",
        action="store_true",
        help="Skip systemd --user service installation and startup",
    )

    # install-service
    p_svc = sub.add_parser(
        "install-service",
        parents=[common_parser],
        help="Generate and enable systemd --user service to auto-start daemon on login",
    )
    p_svc.add_argument(
        "--backend",
        choices=("uinput", "host", "ble", "hybrid"),
        default=None,
        help="Backend flag to bake into the systemd service",
    )

    # install-udev
    sub.add_parser(
        "install-udev",
        help="Install /dev/uinput udev rule",
    )

    # show / hud / diagram
    p_show = sub.add_parser(
        "show",
        aliases=["hud", "diagram"],
        parents=[common_parser],
        help="[Daily] Display visual button mapping diagram of focused window or specified profile",
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
        if args.command == "bind":
            return asyncio.run(_cmd_bind(args))
        if args.command in ("record", "listen", "capture"):
            return _cmd_record(args)
        if args.command in ("keys", "list-keys", "catalog"):
            return _cmd_keys(args)
        if args.command == "reload":
            return _cmd_reload(args)
        if args.command in ("setup", "install", "init"):
            return _cmd_setup(args)
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
