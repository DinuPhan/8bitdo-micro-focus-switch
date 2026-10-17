"""Zero-polling Hyprland active window watcher via .socket2.sock and hyprctl."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
import json
import os
from pathlib import Path
from typing import AsyncIterator, Callable


from .window_watcher import ActiveWindow


def find_hyprland_socket2() -> Path | None:
    """Locate Hyprland's event socket (.socket2.sock), even from systemd --user."""
    sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    xdg_runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"

    candidates: list[Path] = []
    if sig:
        candidates.append(Path(xdg_runtime) / "hypr" / sig / ".socket2.sock")
        candidates.append(Path("/tmp/hypr") / sig / ".socket2.sock")

    for base in (Path(xdg_runtime) / "hypr", Path("/tmp/hypr")):
        if base.is_dir():
            sock_files = sorted(
                base.glob("*/.socket2.sock"),
                key=lambda p: p.stat().st_mtime,
                reverse=True,
            )
            candidates.extend(sock_files)

    for cand in candidates:
        if cand.is_socket():
            return cand
    return None


async def query_active_window() -> ActiveWindow:
    """Query `hyprctl activewindow -j` for the initial or current focused window."""
    sock = find_hyprland_socket2()
    env = dict(os.environ)
    if sock is not None and "HYPRLAND_INSTANCE_SIGNATURE" not in env:
        env["HYPRLAND_INSTANCE_SIGNATURE"] = sock.parent.name

    try:
        proc = await asyncio.create_subprocess_exec(
            "hyprctl",
            "activewindow",
            "-j",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL,
            env=env,
        )
        stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=2.0)
        if proc.returncode == 0 and stdout:
            data = json.loads(stdout.decode("utf-8", errors="replace"))
            if isinstance(data, dict):
                w_class = str(data.get("class") or data.get("initialClass") or "")
                w_title = str(data.get("title") or data.get("initialTitle") or "")
                w_addr = str(data.get("address") or "")
                return ActiveWindow(window_class=w_class, window_title=w_title, address=w_addr)
    except Exception:
        pass
    return ActiveWindow(window_class="", window_title="")


def parse_socket2_event_line(line: str) -> ActiveWindow | None:
    """Parse a single line from Hyprland's .socket2.sock if it is an `activewindow>>` event.

    Hyprland format: `activewindow>>CLASS,TITLE`
    """
    stripped = line.rstrip("\r\n")
    if not stripped.startswith("activewindow>>"):
        return None
    payload = stripped[len("activewindow>>") :]
    if "," not in payload:
        return ActiveWindow(window_class=payload, window_title="")
    w_class, w_title = payload.split(",", 1)
    return ActiveWindow(window_class=w_class, window_title=w_title)


async def watch_hyprland_focus(
    on_focus_change: Callable[[ActiveWindow], None] | None = None,
) -> AsyncIterator[ActiveWindow]:
    """Yield ActiveWindow events immediately on startup and whenever Hyprland focus changes."""
    initial = await query_active_window()
    if on_focus_change is not None:
        on_focus_change(initial)
    yield initial
    last = initial

    while True:
        sock_path = find_hyprland_socket2()
        if sock_path is None:
            await asyncio.sleep(2.0)
            continue

        try:
            reader, writer = await asyncio.open_unix_connection(str(sock_path))
            try:
                while not reader.at_eof():
                    raw_line = await reader.readline()
                    if not raw_line:
                        break
                    line = raw_line.decode("utf-8", errors="replace")
                    parsed = parse_socket2_event_line(line)
                    if parsed is not None and (
                        parsed.window_class != last.window_class
                        or parsed.window_title != last.window_title
                    ):
                        last = parsed
                        if on_focus_change is not None:
                            on_focus_change(parsed)
                        yield parsed
            finally:
                writer.close()
                try:
                    await writer.wait_closed()
                except Exception:
                    pass
        except asyncio.CancelledError:
            raise
        except Exception:
            await asyncio.sleep(1.0)
