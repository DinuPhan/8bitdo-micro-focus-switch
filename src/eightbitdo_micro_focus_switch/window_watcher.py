"""Cross-platform active window watcher abstraction for Linux, Windows, and macOS."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
import logging
import sys
from typing import AsyncIterator, Callable

logger = logging.getLogger("8bitdo-micro")


@dataclass(slots=True, frozen=True)
class ActiveWindow:
    """Represents the currently focused window across Linux, Windows, and macOS."""

    window_class: str
    window_title: str
    address: str = ""
    process_name: str = ""


# ---------------------------------------------------------------------------
# Windows Focus Watcher (Win32 via standard library ctypes)
# ---------------------------------------------------------------------------

def query_windows_active_window() -> ActiveWindow:
    """Query the foreground window on Windows using standard ctypes."""
    try:
        import ctypes
        user32 = getattr(ctypes, "windll", None)
        if user32 is None or not hasattr(user32, "user32"):
            return ActiveWindow(window_class="", window_title="")
        u32 = user32.user32
        hwnd = u32.GetForegroundWindow()
        if not hwnd:
            return ActiveWindow(window_class="", window_title="")

        # Title
        length = u32.GetWindowTextLengthW(hwnd)
        title_buf = ctypes.create_unicode_buffer(length + 1)
        u32.GetWindowTextW(hwnd, title_buf, length + 1)
        title = title_buf.value

        # Window Class
        class_buf = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(hwnd, class_buf, 256)
        w_class = class_buf.value

        # Process ID
        proc_name = ""
        pid = ctypes.c_ulong()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value:
            proc_name = f"pid_{pid.value}"

        return ActiveWindow(
            window_class=w_class,
            window_title=title,
            address=hex(hwnd),
            process_name=proc_name,
        )
    except Exception as exc:
        logger.debug("query_windows_active_window failed: %s", exc)
        return ActiveWindow(window_class="", window_title="")


async def watch_windows_focus(
    on_focus_change: Callable[[ActiveWindow], None] | None = None,
    poll_interval: float = 0.15,
) -> AsyncIterator[ActiveWindow]:
    """Poll the active window on Windows with a lightweight sleep loop."""
    last = query_windows_active_window()
    if on_focus_change is not None:
        on_focus_change(last)
    yield last

    while True:
        cur = query_windows_active_window()
        if cur.window_class != last.window_class or cur.window_title != last.window_title:
            last = cur
            if on_focus_change is not None:
                on_focus_change(cur)
            yield cur
        await asyncio.sleep(poll_interval)


# ---------------------------------------------------------------------------
# macOS Focus Watcher (Darwin via NSWorkspace / osascript fallback)
# ---------------------------------------------------------------------------

def query_macos_active_window() -> ActiveWindow:
    """Query the frontmost application and window on macOS.

    Prioritizes in-process AppKit (NSWorkspace) and Quartz (CGWindowListCopyWindowInfo)
    to eliminate subprocess spawning and minimize CPU usage.
    """
    # 1. Try PyObjC AppKit + Quartz (100% in-process, zero subprocess spawning)
    try:
        from AppKit import NSWorkspace  # type: ignore
        app = NSWorkspace.sharedWorkspace().frontmostApplication()
        if app is not None:
            app_name = str(app.localizedName() or "")
            bundle_id = str(app.bundleIdentifier() or "")
            pid = app.processIdentifier()

            window_title = app_name
            # Query Quartz in-process for the window title
            try:
                import Quartz  # type: ignore
                windows = Quartz.CGWindowListCopyWindowInfo(
                    Quartz.kCGWindowListOptionOnScreenOnly | Quartz.kCGWindowListExcludeDesktopElements,
                    Quartz.kCGNullWindowID,
                )
                for w in windows:
                    if w.get(Quartz.kCGWindowOwnerPID) == pid and w.get(Quartz.kCGWindowLayer, 0) == 0:
                        name = w.get(Quartz.kCGWindowName)
                        if name:
                            window_title = str(name)
                            break
            except Exception:
                pass

            return ActiveWindow(
                window_class=bundle_id or app_name,
                window_title=window_title,
                address=str(pid),
                process_name=app_name,
            )
    except Exception:
        pass

    # 2. Try Quartz directly if NSWorkspace wasn't imported
    try:
        import Quartz  # type: ignore
        windows = Quartz.CGWindowListCopyWindowInfo(
            Quartz.kCGWindowListOptionOnScreenOnly | Quartz.kCGWindowListExcludeDesktopElements,
            Quartz.kCGNullWindowID,
        )
        for w in windows:
            if w.get(Quartz.kCGWindowLayer, 0) == 0:
                owner = str(w.get(Quartz.kCGWindowOwnerName, "") or "")
                title = str(w.get(Quartz.kCGWindowName, "") or "")
                pid = w.get(Quartz.kCGWindowOwnerPID, 0)
                if owner or title:
                    return ActiveWindow(
                        window_class=owner.lower(),
                        window_title=title or owner,
                        address=str(pid),
                        process_name=owner,
                    )
    except Exception:
        pass

    # 3. Fallback to osascript only when PyObjC is not installed
    try:
        import subprocess
        script = 'tell application "System Events" to get {name, title of front window} of first application process whose frontmost is true'
        res = subprocess.run(
            ["osascript", "-e", script],
            capture_output=True,
            text=True,
            timeout=1.0,
        )
        if res.returncode == 0 and res.stdout.strip():
            parts = [p.strip() for p in res.stdout.strip().split(",", 1)]
            app_name = parts[0]
            win_title = parts[1] if len(parts) > 1 and parts[1] else app_name
            return ActiveWindow(
                window_class=app_name,
                window_title=win_title,
                process_name=app_name,
            )
    except Exception:
        pass

    return ActiveWindow(window_class="", window_title="")


async def watch_macos_focus(
    on_focus_change: Callable[[ActiveWindow], None] | None = None,
    poll_interval: float = 0.15,
) -> AsyncIterator[ActiveWindow]:
    """Poll the active window on macOS."""
    last = query_macos_active_window()
    if on_focus_change is not None:
        on_focus_change(last)
    yield last

    while True:
        cur = query_macos_active_window()
        if cur.window_class != last.window_class or cur.window_title != last.window_title:
            last = cur
            if on_focus_change is not None:
                on_focus_change(cur)
            yield cur
        await asyncio.sleep(poll_interval)


# ---------------------------------------------------------------------------
# Unified Cross-Platform Dispatcher
# ---------------------------------------------------------------------------

async def watch_platform_focus(
    on_focus_change: Callable[[ActiveWindow], None] | None = None,
) -> AsyncIterator[ActiveWindow]:
    """Dispatch focus watcher depending on the current OS platform."""
    if sys.platform == "win32":
        async for win in watch_windows_focus(on_focus_change):
            yield win
    elif sys.platform == "darwin":
        async for win in watch_macos_focus(on_focus_change):
            yield win
    else:
        # Default Linux / Hyprland
        from .hyprland import watch_hyprland_focus
        async for win in watch_hyprland_focus(on_focus_change):
            yield win
