"""Cross-Platform Virtual Keyboard & Input Interception Backends.

Provides a unified, duck-typed VirtualKeyboard interface (100% compatible with evdev.UInput)
and user-space input interception backends for Linux, Windows, and macOS:
- Linux: Kernel evdev grab (EVIOCGRAB) + /dev/uinput
- Windows: Low-Level Keyboard Hook (WH_KEYBOARD_LL) + SendInput with Sentinel keymap
- macOS: CoreGraphics Event Tap (CGEventTap) + CGEventPost with Sentinel keymap
"""

from __future__ import annotations

import asyncio
import logging
import os
from pathlib import Path
import sys
import threading
from typing import Any, Callable, Protocol

from .keycodes import (
    KEY_BY_EVDEV,
    KEY_BY_NAME,
    SENTINEL_BUTTON_MAP,
    SENTINEL_MAC_VK_TO_BUTTON,
    SENTINEL_WIN_SCAN_TO_BUTTON,
)
from .protocol import BUTTON_TO_FACTORY_EVDEV

logger = logging.getLogger("8bitdo-micro")

# 32-bit Magic Tag to identify our own injected keys and prevent self-loop feedback:
# Fits into Windows dwExtraInfo (ULONG_PTR) and macOS kCGEventSourceUserData (int64)
MAGIC_EXTRA_INFO: int = 0x8B17D000


# ---------------------------------------------------------------------------
# VirtualKeyboard Protocol
# ---------------------------------------------------------------------------

class VirtualKeyboard(Protocol):
    """Duck-typed interface matching evdev.UInput for seamless zero-refactor state machine."""

    def write(self, etype: int, evdev_code: int, value: int) -> None:
        """Write an EV_KEY event (value: 1=down, 0=up, 2=repeat)."""
        ...

    def syn(self) -> None:
        """Synchronize/flush pending key events to the OS input subsystem."""
        ...

    def close(self) -> None:
        """Clean up and close the virtual device."""
        ...


# ---------------------------------------------------------------------------
# Linux Backend (evdev.UInput wrapper)
# ---------------------------------------------------------------------------

class LinuxVirtualKeyboard:
    """Virtual keyboard on Linux backed by /dev/uinput."""

    def __init__(self, uinput_dev: Any) -> None:
        self._ui = uinput_dev

    def write(self, etype: int, evdev_code: int, value: int) -> None:
        self._ui.write(etype, evdev_code, value)

    def syn(self) -> None:
        self._ui.syn()

    def close(self) -> None:
        self._ui.close()


# ---------------------------------------------------------------------------
# Windows Backend (SendInput + WH_KEYBOARD_LL Hook)
# ---------------------------------------------------------------------------

# Windows Constants
INPUT_KEYBOARD = 1
KEYEVENTF_EXTENDEDKEY = 0x0001
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_SCANCODE = 0x0008

WH_KEYBOARD_LL = 13
WM_KEYDOWN = 0x0100
WM_KEYUP = 0x0101
WM_SYSKEYDOWN = 0x0104
WM_SYSKEYUP = 0x0105
LLKHF_EXTENDED = 0x01
LLKHF_INJECTED = 0x10


class WindowsVirtualKeyboard:
    """Virtual keyboard on Windows using Win32 SendInput with MAGIC_EXTRA_INFO tagging."""

    def __init__(self) -> None:
        self._pending: list[tuple[int, int]] = []  # (evdev_code, value)
        self._closed = False
        self.sent_history: list[tuple[int, int]] = []  # For inspection/testing

    def write(self, etype: int, evdev_code: int, value: int) -> None:
        # etype 1 is EV_KEY
        if etype == 1 and not self._closed:
            self._pending.append((evdev_code, value))

    def build_win32_inputs(self, events: list[tuple[int, int]]) -> list[dict[str, Any]]:
        """Convert evdev events to Win32 INPUT parameter dicts (for testing and execution)."""
        res: list[dict[str, Any]] = []
        for code, val in events:
            spec = KEY_BY_EVDEV.get(code)
            vk = spec.win_vk if spec else 0
            scan = spec.win_raw_scancode if spec else 0
            flags = 0

            if spec and spec.is_win_extended:
                flags |= KEYEVENTF_EXTENDEDKEY
            if val == 0:
                flags |= KEYEVENTF_KEYUP

            res.append({
                "type": INPUT_KEYBOARD,
                "wVk": vk,
                "wScan": scan,
                "dwFlags": flags,
                "time": 0,
                "dwExtraInfo": MAGIC_EXTRA_INFO,
            })
        return res

    def syn(self) -> None:
        if not self._pending or self._closed:
            return

        events_to_send = list(self._pending)
        self._pending.clear()
        self.sent_history.extend(events_to_send)

        if sys.platform != "win32":
            return

        import ctypes
        from ctypes import wintypes

        class KEYBDINPUT(ctypes.Structure):
            _fields_ = [
                ("wVk", wintypes.WORD),
                ("wScan", wintypes.WORD),
                ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_size_t),
            ]

        class _INPUT_UNION(ctypes.Union):
            _fields_ = [("ki", KEYBDINPUT)]

        class INPUT(ctypes.Structure):
            _anonymous_ = ("u",)
            _fields_ = [("type", wintypes.DWORD), ("u", _INPUT_UNION)]

        input_dicts = self.build_win32_inputs(events_to_send)
        inputs = (INPUT * len(input_dicts))()
        for idx, item in enumerate(input_dicts):
            inputs[idx].type = item["type"]
            inputs[idx].ki.wVk = item["wVk"]
            inputs[idx].ki.wScan = item["wScan"]
            inputs[idx].ki.dwFlags = item["dwFlags"]
            inputs[idx].ki.time = item["time"]
            inputs[idx].ki.dwExtraInfo = item["dwExtraInfo"]

        user32 = ctypes.windll.user32
        user32.SendInput(len(input_dicts), inputs, ctypes.sizeof(INPUT))

    def close(self) -> None:
        self._closed = True


class WindowsInputBackend:
    """Interceps 8BitDo Micro Sentinel keys in user-space via WH_KEYBOARD_LL."""

    def __init__(
        self,
        on_key_event: Callable[[int, int], None],
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        self.on_key_event = on_key_event
        self.loop = loop
        self._thread: threading.Thread | None = None
        self._thread_id: int = 0
        self._hook = None
        self._proc = None
        self._stop_event = threading.Event()
        self._held_buttons: set[str] = set()
        self.stopped = False

    def start(self) -> None:
        self._stop_event.clear()
        self.stopped = False
        self._thread = threading.Thread(target=self._run_hook_thread, daemon=True, name="Win32HookThread")
        self._thread.start()

    def process_hook_event(
        self,
        vk_code: int,
        scan_code: int,
        flags: int,
        dw_extra_info: int,
        w_param: int,
    ) -> bool:
        """Process a low-level keyboard hook event. Returns True if swallowed, False if passed through."""
        # 1. Skip self-injected keys to prevent infinite feedback loops
        if dw_extra_info == MAGIC_EXTRA_INFO or bool(flags & LLKHF_INJECTED):
            return False

        # 2. Check Sentinel key match by (raw_scancode, is_extended)
        is_extended = bool(flags & LLKHF_EXTENDED)
        raw_scancode = scan_code & 0xFF
        btn = SENTINEL_WIN_SCAN_TO_BUTTON.get((raw_scancode, is_extended))
        if btn is not None:
            factory_evdev = BUTTON_TO_FACTORY_EVDEV.get(btn)
            if factory_evdev is not None:
                is_up = w_param in (WM_KEYUP, WM_SYSKEYUP)
                if is_up:
                    self._held_buttons.discard(btn)
                    val = 0
                else:
                    if btn in self._held_buttons:
                        val = 2  # Repeat
                    else:
                        self._held_buttons.add(btn)
                        val = 1  # Press
                if self.loop is not None and self.loop.is_running():
                    self.loop.call_soon_threadsafe(self.on_key_event, factory_evdev, val)
                else:
                    self.on_key_event(factory_evdev, val)
                return True  # Swallow the Sentinel key

        return False

    def _run_hook_thread(self) -> None:
        if sys.platform != "win32":
            return

        import ctypes
        from ctypes import wintypes

        user32 = ctypes.windll.user32
        kernel32 = ctypes.windll.kernel32
        self._thread_id = kernel32.GetCurrentThreadId()

        class KBDLLHOOKSTRUCT(ctypes.Structure):
            _fields_ = [
                ("vkCode", wintypes.DWORD),
                ("scanCode", wintypes.DWORD),
                ("flags", wintypes.DWORD),
                ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_size_t),
            ]

        HOOKPROC = ctypes.WINFUNCTYPE(ctypes.c_longlong, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM)

        def _hook_callback(nCode: int, wParam: int, lParam: int) -> int:
            if nCode >= 0:
                kb = ctypes.cast(lParam, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
                if self.process_hook_event(kb.vkCode, kb.scanCode, kb.flags, kb.dwExtraInfo, wParam):
                    return 1  # Swallow the Sentinel key
            return int(user32.CallNextHookEx(self._hook, nCode, wParam, lParam))

        self._proc = HOOKPROC(_hook_callback)
        self._hook = user32.SetWindowsHookExW(WH_KEYBOARD_LL, self._proc, None, 0)
        if not self._hook:
            logger.error("Failed to install WH_KEYBOARD_LL hook on Windows.")
            self.stopped = True
            return

        msg = wintypes.MSG()
        while not self._stop_event.is_set():
            ret = user32.GetMessageW(ctypes.byref(msg), 0, 0, 0)
            if ret <= 0:
                break
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))

        if self._hook:
            user32.UnhookWindowsHookEx(self._hook)
            self._hook = None
        self.stopped = True

    def stop(self) -> None:
        self._stop_event.set()
        if sys.platform == "win32" and self._thread_id:
            try:
                import ctypes
                ctypes.windll.user32.PostThreadMessageW(self._thread_id, 0x0012, 0, 0)  # WM_QUIT
            except Exception:
                pass
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=1.5)
        self.stopped = True


# ---------------------------------------------------------------------------
# macOS Backend (CGEventPost + CGEventTap)
# ---------------------------------------------------------------------------

# CoreGraphics Event Constants
kCGHIDEventTap = 0
kCGHeadInsertEventTap = 0
kCGEventTapOptionDefault = 0

kCGEventKeyDown = 10
kCGEventKeyUp = 11
kCGEventFlagsChanged = 12
kCGEventTapDisabledByTimeout = 0xFFFFFFFE
kCGEventTapDisabledByUserInput = 0xFFFFFFFF

kCGKeyboardEventKeycode = 9
kCGKeyboardEventAutorepeat = 10
kCGEventSourceUserData = 42

kCGEventFlagMaskShift = 0x00020000
kCGEventFlagMaskControl = 0x00040000
kCGEventFlagMaskAlternate = 0x00080000
kCGEventFlagMaskCommand = 0x00100000

# Modifier key code to flag mask
MAC_MODIFIER_FLAGS: dict[str, int] = {
    "LEFTCTRL": kCGEventFlagMaskControl,
    "RIGHTCTRL": kCGEventFlagMaskControl,
    "LEFTSHIFT": kCGEventFlagMaskShift,
    "RIGHTSHIFT": kCGEventFlagMaskShift,
    "LEFTALT": kCGEventFlagMaskAlternate,
    "RIGHTALT": kCGEventFlagMaskAlternate,
    "LEFTMETA": kCGEventFlagMaskCommand,
    "RIGHTMETA": kCGEventFlagMaskCommand,
}


class MacOSVirtualKeyboard:
    """Virtual keyboard on macOS using CGEventPost with modifier flag tracking & magic tag."""

    def __init__(self) -> None:
        self._pending: list[tuple[int, int]] = []
        self._active_modifiers: int = 0
        self._closed = False
        self.sent_history: list[tuple[int, int]] = []

    def write(self, etype: int, evdev_code: int, value: int) -> None:
        if etype == 1 and not self._closed:
            self._pending.append((evdev_code, value))
            spec = KEY_BY_EVDEV.get(evdev_code)
            if spec and spec.name in MAC_MODIFIER_FLAGS:
                flag = MAC_MODIFIER_FLAGS[spec.name]
                if value in (1, 2):
                    self._active_modifiers |= flag
                elif value == 0:
                    self._active_modifiers &= ~flag

    def syn(self) -> None:
        if not self._pending or self._closed:
            return

        events_to_send = list(self._pending)
        self._pending.clear()
        self.sent_history.extend(events_to_send)

        if sys.platform != "darwin":
            return

        try:
            import ctypes
            cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
            cf = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")

            for code, val in events_to_send:
                spec = KEY_BY_EVDEV.get(code)
                if spec is None or spec.mac_vk is None:
                    continue

                is_down = val in (1, 2)
                event = cg.CGEventCreateKeyboardEvent(None, spec.mac_vk, is_down)
                if not event:
                    continue

                cg.CGEventSetFlags(event, self._active_modifiers)
                cg.CGEventSetIntegerValueField(event, kCGEventSourceUserData, ctypes.c_int64(MAGIC_EXTRA_INFO))
                cg.CGEventPost(kCGHIDEventTap, event)
                cf.CFRelease(event)
        except Exception as exc:
            logger.debug("MacOSVirtualKeyboard SendEvent error: %s", exc)

    def close(self) -> None:
        self._closed = True


class MacOSInputBackend:
    """Intercepts 8BitDo Micro Sentinel keys on macOS via CGEventTap."""

    def __init__(
        self,
        on_key_event: Callable[[int, int], None],
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        self.on_key_event = on_key_event
        self.loop = loop
        self._thread: threading.Thread | None = None
        self._held_buttons: set[str] = set()
        self._run_loop = None
        self._tap = None
        self._stop_event = threading.Event()
        self.stopped = False

    def start(self) -> None:
        self._stop_event.clear()
        self.stopped = False
        self._thread = threading.Thread(target=self._run_tap_thread, daemon=True, name="MacEventTapThread")
        self._thread.start()

    def process_tap_event(
        self,
        event_type: int,
        mac_vk: int,
        user_data: int,
        is_repeat: bool = False,
    ) -> bool:
        """Process a CoreGraphics tap event. Returns True if swallowed, False if passed through."""
        # Auto-recovery if event tap is disabled by macOS due to timeout or user input
        if event_type in (kCGEventTapDisabledByTimeout, kCGEventTapDisabledByUserInput):
            if self._tap and sys.platform == "darwin":
                try:
                    import ctypes
                    cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
                    cg.CGEventTapEnable(self._tap, True)
                except Exception:
                    pass
            return False

        # Ignore keys injected by ourselves
        if user_data == MAGIC_EXTRA_INFO:
            return False

        btn = SENTINEL_MAC_VK_TO_BUTTON.get(mac_vk)
        if btn is not None:
            factory_evdev = BUTTON_TO_FACTORY_EVDEV.get(btn)
            if factory_evdev is not None:
                if event_type == kCGEventKeyUp:
                    self._held_buttons.discard(btn)
                    val = 0
                elif event_type == kCGEventKeyDown:
                    if is_repeat or btn in self._held_buttons:
                        val = 2
                    else:
                        self._held_buttons.add(btn)
                        val = 1
                else:
                    return False

                if self.loop is not None and self.loop.is_running():
                    self.loop.call_soon_threadsafe(self.on_key_event, factory_evdev, val)
                else:
                    self.on_key_event(factory_evdev, val)
                return True  # Swallow Sentinel key!

        return False

    def _run_tap_thread(self) -> None:
        if sys.platform != "darwin":
            return

        try:
            import ctypes
            cg = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
            cf = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")

            EVENT_MASK = (1 << kCGEventKeyDown) | (1 << kCGEventKeyUp) | (1 << kCGEventFlagsChanged)

            def _tap_callback(proxy: Any, event_type: int, event: Any, refcon: Any) -> Any:
                if event_type in (kCGEventTapDisabledByTimeout, kCGEventTapDisabledByUserInput):
                    self.process_tap_event(event_type, 0, 0)
                    return event

                user_data = cg.CGEventGetIntegerValueField(event, kCGEventSourceUserData)
                mac_vk = cg.CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode)
                is_repeat = bool(cg.CGEventGetIntegerValueField(event, kCGKeyboardEventAutorepeat))

                if self.process_tap_event(event_type, mac_vk, user_data, is_repeat):
                    return None  # Swallow Sentinel key!
                return event

            TAP_FUNC = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
            self._proc = TAP_FUNC(_tap_callback)

            self._tap = cg.CGEventTapCreate(
                kCGHIDEventTap,
                kCGHeadInsertEventTap,
                kCGEventTapOptionDefault,
                ctypes.c_uint64(EVENT_MASK),
                self._proc,
                None,
            )
            if not self._tap:
                logger.error("Failed to create CGEventTap. Ensure Accessibility permissions are granted.")
                self.stopped = True
                return

            run_loop_source = cf.CFMachPortCreateRunLoopSource(None, self._tap, 0)
            self._run_loop = cf.CFRunLoopGetCurrent()
            kCFRunLoopCommonModes = ctypes.c_void_p.in_dll(cf, "kCFRunLoopCommonModes")
            cf.CFRunLoopAddSource(self._run_loop, run_loop_source, kCFRunLoopCommonModes)
            cg.CGEventTapEnable(self._tap, True)

            # Run CFRunLoop until stopped
            while not self._stop_event.is_set():
                cf.CFRunLoopRunInMode(kCFRunLoopCommonModes, ctypes.c_double(0.2), True)

            cf.CFRelease(run_loop_source)
            cf.CFRelease(self._tap)
            self._tap = None
        except Exception as exc:
            logger.error("MacOSInputBackend run loop failed: %s", exc)
        finally:
            self.stopped = True

    def stop(self) -> None:
        self._stop_event.set()
        if sys.platform == "darwin" and self._run_loop:
            try:
                import ctypes
                cf = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
                cf.CFRunLoopStop(self._run_loop)
            except Exception:
                pass
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=1.5)
        self.stopped = True


# ---------------------------------------------------------------------------
# Mock Virtual Keyboard (for unit testing & headless simulation)
# ---------------------------------------------------------------------------

class MockVirtualKeyboard:
    """Mock virtual keyboard for testing event generation & syn count."""

    def __init__(self) -> None:
        self.events: list[tuple[int, int, int]] = []  # (etype, evdev_code, value)
        self.syn_count: int = 0
        self.closed: bool = False

    def write(self, etype: int, evdev_code: int, value: int) -> None:
        self.events.append((etype, evdev_code, value))

    def syn(self) -> None:
        self.syn_count += 1

    def close(self) -> None:
        self.closed = True


# ---------------------------------------------------------------------------
# Factory & Permission Checks
# ---------------------------------------------------------------------------

def create_virtual_keyboard(uinput_dev: Any = None) -> VirtualKeyboard:
    """Create the appropriate virtual keyboard for the current platform."""
    if sys.platform == "win32":
        return WindowsVirtualKeyboard()
    elif sys.platform == "darwin":
        return MacOSVirtualKeyboard()
    else:
        # Default Linux
        if uinput_dev is not None:
            return LinuxVirtualKeyboard(uinput_dev)
        # Will be created by caller or daemon._create_uinput()
        raise ValueError("On Linux, pass the initialized UInput device to create_virtual_keyboard")


def check_platform_input_access() -> tuple[bool, str]:
    """Check whether the current system grants required virtual input permissions."""
    if sys.platform == "linux":
        uinput_path = Path("/dev/uinput")
        if not uinput_path.exists():
            return False, "/dev/uinput does not exist (run `sudo modprobe uinput`)"
        if not os.access(uinput_path, os.W_OK):
            return (
                False,
                "/dev/uinput is not writable by current user "
                "(run `8bitdo-micro-focus-switch install-udev` to grant input group access)",
            )
        return True, "/dev/uinput is writable"

    if sys.platform == "darwin":
        # Check macOS Accessibility permission (AXIsProcessTrusted)
        try:
            import ctypes
            lib = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices")
            if hasattr(lib, "AXIsProcessTrusted"):
                lib.AXIsProcessTrusted.restype = ctypes.c_bool
                trusted = lib.AXIsProcessTrusted()
                if not trusted:
                    return (
                        False,
                        "Accessibility permissions not granted.\n"
                        "Please enable in System Settings -> Privacy & Security -> Accessibility\n"
                        "for your Terminal or Python process.",
                    )
                return True, "macOS Accessibility permissions granted"
        except Exception:
            pass
        return True, "macOS Accessibility check completed"

    if sys.platform == "win32":
        return True, "Windows user-space hook available"

    return True, f"Platform {sys.platform} supported"
