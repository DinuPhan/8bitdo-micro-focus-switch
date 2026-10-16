"""Focus-aware input remapper daemon for the 8BitDo Micro on Linux / Hyprland."""

from __future__ import annotations

import asyncio
import logging
import os
from pathlib import Path
import evdev
from evdev import InputDevice, UInput, ecodes

from .ble import MicroBleSession, find_micro_ble_device
from .config import AppConfig, ButtonAction, ProfileConfig
from .hyprland import ActiveWindow, watch_hyprland_focus
from .protocol import (
    FACTORY_EVDEV_TO_BUTTON,
    MICRO_INPUT_NAME_SUBSTRING,
    MICRO_PRODUCT_ID,
    MICRO_VENDOR_ID,
    NAME_TO_EVDEV,
    rotate_dpad_button,
)

VIRTUAL_DEVICE_NAME = "8BitDo Micro Focus-Switch Virtual Keyboard"
logger = logging.getLogger("8bitdo-micro")


def find_micro_input_devices() -> list[InputDevice]:
    """Return all /dev/input/event* devices matching the 8BitDo Micro Keyboard."""
    matched: list[InputDevice] = []
    for path in evdev.list_devices():
        try:
            dev = InputDevice(path)
        except Exception:
            continue
        if dev.name == VIRTUAL_DEVICE_NAME:
            dev.close()
            continue
        info = dev.info
        is_micro_id = info.vendor == MICRO_VENDOR_ID and info.product == MICRO_PRODUCT_ID
        is_micro_name = MICRO_INPUT_NAME_SUBSTRING.lower() in (dev.name or "").lower()
        if is_micro_id or is_micro_name:
            caps = dev.capabilities()
            if ecodes.EV_KEY in caps:
                matched.append(dev)
                continue
        dev.close()
    return matched


def check_uinput_access() -> tuple[bool, str]:
    """Check whether /dev/uinput exists and is writable by the current user."""
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


class FocusSwitchDaemon:
    """Daemon that watches Hyprland active window focus and remaps 8BitDo Micro inputs."""

    def __init__(
        self,
        config: AppConfig,
        *,
        backend: str | None = None,
        ble_address: str | None = None,
    ) -> None:
        self.config = config
        self.backend = (backend or config.backend or "uinput").lower()
        self.ble_address = ble_address
        self.active_window = ActiveWindow(window_class="", window_title="")
        self.active_profile: ProfileConfig = config.resolve_profile_for_window("", "")
        # Tracks physical evdev keycode -> tuple of virtual evdev keycodes pressed on key-down
        # so releasing a button after a focus switch always releases the exact keys pressed.
        self._held_chords: dict[int, tuple[int, ...]] = {}
        self._ble_trigger = asyncio.Event()

    def _on_focus_change(self, window: ActiveWindow) -> None:
        self.active_window = window
        new_profile = self.config.resolve_profile_for_window(
            window.window_class,
            window.window_title,
        )
        if new_profile.name != self.active_profile.name:
            logger.info(
                "Focus -> class=%r title=%r | Switching profile: %s -> %s (turn=%d°)",
                window.window_class,
                window.window_title,
                self.active_profile.name,
                new_profile.name,
                new_profile.turn,
            )
            self.active_profile = new_profile
            self._ble_trigger.set()
        else:
            logger.debug(
                "Focus -> class=%r title=%r | Keeping profile: %s",
                window.window_class,
                window.window_title,
                self.active_profile.name,
            )

    def translate_button_action(self, physical_button: str) -> ButtonAction | None:
        """Resolve a physical button (with D-pad rotation applied) in the active profile."""
        logical_btn = rotate_dpad_button(physical_button, self.active_profile.turn)
        return self.active_profile.buttons.get(logical_btn)

    def _create_uinput(self) -> UInput:
        all_keys = set(NAME_TO_EVDEV.values())
        for prof in self.config.profiles.values():
            for act in prof.buttons.values():
                all_keys.update(act.evdev_codes)
        # Include standard keyboard range so pass-through of non-factory keys also works
        for code in range(1, 249):
            all_keys.add(code)
        return UInput(
            {ecodes.EV_KEY: sorted(all_keys)},
            name=VIRTUAL_DEVICE_NAME,
            vendor=MICRO_VENDOR_ID,
            product=MICRO_PRODUCT_ID,
            version=0x0101,
        )

    async def _spawn_exec(self, cmd: str) -> None:
        try:
            proc = await asyncio.create_subprocess_shell(
                cmd,
                stdout=asyncio.subprocess.DEVNULL,
                stderr=asyncio.subprocess.DEVNULL,
            )
            await proc.wait()
        except Exception as exc:
            logger.warning("exec command %r failed: %s", cmd, exc)

    def handle_key_event(self, ui: UInput, keycode: int, value: int) -> None:
        """Process a single EV_KEY event (value: 0=up, 1=down, 2=hold) and emit to UInput."""
        # Release: always release whatever chord was recorded when this physical key was pressed
        if value == 0:
            held = self._held_chords.pop(keycode, None)
            if held is not None:
                for code in reversed(held):
                    ui.write(ecodes.EV_KEY, code, 0)
                ui.syn()
            return

        button_name = FACTORY_EVDEV_TO_BUTTON.get(keycode)
        if button_name is None:
            # Non-factory key (e.g., if controller was not in factory preset): pass through
            if value == 1:
                self._held_chords[keycode] = (keycode,)
                ui.write(ecodes.EV_KEY, keycode, 1)
                ui.syn()
            elif value == 2 and keycode in self._held_chords:
                ui.write(ecodes.EV_KEY, keycode, 2)
                ui.syn()
            return

        action = self.translate_button_action(button_name)
        if action is None:
            return

        if value == 1:
            if action.exec_cmd:
                asyncio.create_task(self._spawn_exec(action.exec_cmd))
            if action.evdev_codes:
                self._held_chords[keycode] = action.evdev_codes
                for code in action.evdev_codes:
                    ui.write(ecodes.EV_KEY, code, 1)
                ui.syn()
        elif value == 2:
            held = self._held_chords.get(keycode)
            if held:
                # Emit repeat on the last key of the chord (primary key)
                ui.write(ecodes.EV_KEY, held[-1], 2)
                ui.syn()

    async def _evdev_loop(self) -> None:
        """Continuously attach to the 8BitDo Micro input device, grab it, and remap events."""
        ui = self._create_uinput()
        logger.info("Created virtual keyboard: %s", VIRTUAL_DEVICE_NAME)
        try:
            while True:
                devices = find_micro_input_devices()
                if not devices:
                    logger.info("Waiting for 8BitDo Micro Keyboard in /dev/input/event* ...")
                    await asyncio.sleep(1.5)
                    continue

                dev = devices[0]
                for extra in devices[1:]:
                    extra.close()

                logger.info(
                    "Attached to %s (%s, uniq=%s) | Active profile: %s",
                    dev.path,
                    dev.name,
                    dev.uniq,
                    self.active_profile.name,
                )
                try:
                    dev.grab()
                    async for event in dev.async_read_loop():
                        if event.type == ecodes.EV_KEY:
                            self.handle_key_event(ui, event.code, event.value)
                except OSError as exc:
                    logger.info("8BitDo Micro disconnected (%s); releasing held keys.", exc)
                finally:
                    # Release any held keys on disconnect so nothing sticks
                    for held in self._held_chords.values():
                        for code in reversed(held):
                            ui.write(ecodes.EV_KEY, code, 0)
                    if self._held_chords:
                        ui.syn()
                    self._held_chords.clear()
                    try:
                        dev.ungrab()
                    except Exception:
                        pass
                    dev.close()
                await asyncio.sleep(1.0)
        finally:
            ui.close()

    async def _ble_loop(self) -> None:
        """Keep BLE GATT synced to the active profile whenever Hyprland focus changes."""
        last_flashed: str | None = None
        base_record: bytes | None = None

        while True:
            await self._ble_trigger.wait()
            self._ble_trigger.clear()
            target_profile = self.active_profile
            if target_profile.name == last_flashed:
                continue

            try:
                dev = await find_micro_ble_device(address=self.ble_address, timeout=4.0)
                async with MicroBleSession(dev) as session:
                    if base_record is None:
                        base_record = await session.read_config_bytes()
                    hw_prof = target_profile.to_hardware_profile(
                        enabled=(target_profile.name != "factory")
                    )
                    payload = hw_prof.to_bytes(base=base_record)
                    await session.write_config_bytes(payload, save=True)
                    last_flashed = target_profile.name
                    logger.info(
                        "Flashed profile %r to 8BitDo Micro (%s) over BLE GATT",
                        target_profile.name,
                        dev.address,
                    )
            except Exception as exc:
                logger.warning("BLE GATT auto-flash for profile %r failed: %s", target_profile.name, exc)

    async def run(self) -> None:
        """Run the Hyprland focus watcher and the selected remapping backend concurrently."""
        tasks: list[asyncio.Task[None]] = []

        async def _focus_task() -> None:
            async for _ in watch_hyprland_focus(self._on_focus_change):
                pass

        tasks.append(asyncio.create_task(_focus_task()))

        if self.backend in ("uinput", "hybrid"):
            ok, msg = check_uinput_access()
            if not ok:
                raise PermissionError(msg)
            tasks.append(asyncio.create_task(self._evdev_loop()))

        if self.backend in ("ble", "hybrid"):
            self._ble_trigger.set()
            tasks.append(asyncio.create_task(self._ble_loop()))

        try:
            await asyncio.gather(*tasks)
        finally:
            for t in tasks:
                t.cancel()
