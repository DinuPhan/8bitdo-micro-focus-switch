"""Direct BLE GATT client for the 8BitDo Micro ('80EL') configuration service."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
import struct
import warnings

from bleak import BleakClient, BleakScanner
from bleak.backends.device import BLEDevice

from .protocol import (
    BLE_CHAR_FALLBACK_UUID,
    BLE_CHAR_UUID,
    BLE_DEVICE_NAME,
    CMD_GET_PLATFORM,
    CMD_READ_CONFIG,
    CMD_READ_VERSION,
    CMD_SAVE_CONFIG,
    CMD_WRITE_CONFIG,
     FACTORY_BUTTON_HID_USAGES,
    MicroHardwareProfile,
    PHYSICAL_BUTTONS,
    RECORD_MICRO_SIZE,
    build_get_platform_packet,
    build_read_config_packets,
    build_read_version_packet,
    build_save_config_packet,
    build_write_config_packets,
    hid_tuple_to_names,
    parse_ble_notification,
)


@dataclass(slots=True)
class MicroBleSnapshot:
    """Complete snapshot read from the 8BitDo Micro over BLE GATT."""

    address: str
    name: str
    platform: int | None
    firmware_version: str | None
    raw_version: int | None
    raw_record: bytes
    profile: MicroHardwareProfile


async def find_micro_ble_device(
    address: str | None = None,
    timeout: float = 6.0,
) -> BLEDevice:
    """Locate the 8BitDo Micro BLE GATT endpoint ('80EL')."""
    if address:
        dev = await BleakScanner.find_device_by_address(address, timeout=timeout)
        if dev is not None:
            return dev
        raise TimeoutError(f"BLE device {address} not found within {timeout:.1f}s")

    dev = await BleakScanner.find_device_by_name(BLE_DEVICE_NAME, timeout=timeout)
    if dev is not None:
        return dev

    # Fallback: scan all devices and match name substring
    devices = await BleakScanner.discover(timeout=min(timeout, 3.0))
    for d in devices:
        if d.name and BLE_DEVICE_NAME.lower() in d.name.lower():
            return d

    raise TimeoutError(
        f"8BitDo Micro BLE endpoint ({BLE_DEVICE_NAME!r}) not found. "
        "Ensure the controller is powered on."
    )


class MicroBleSession:
    """Managed BLE GATT session to the 8BitDo Micro ('80EL') over 0000FF10 / 0000FF13."""

    def __init__(self, device: BLEDevice | str, response_timeout: float = 3.0) -> None:
        self._target = device
        self._timeout = response_timeout
        self._client: BleakClient | None = None
        self._char_uuid: str = BLE_CHAR_UUID
        self._rx_queue: asyncio.Queue[bytes] = asyncio.Queue()

    async def __aenter__(self) -> MicroBleSession:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            self._client = BleakClient(self._target)
            await self._client.connect()

            # Acquire BlueZ MTU if available to silence bleak warning
            acquire_mtu = getattr(self._client._backend, "_acquire_mtu", None)
            if callable(acquire_mtu):
                try:
                    await acquire_mtu()
                except Exception:
                    pass

            # Determine whether 0000FF13 (gamepad_type=1) or 0000FF11 is present
            char = self._client.services.get_characteristic(BLE_CHAR_UUID)
            if char is not None:
                self._char_uuid = BLE_CHAR_UUID
            else:
                fallback = self._client.services.get_characteristic(BLE_CHAR_FALLBACK_UUID)
                if fallback is None:
                    await self._client.disconnect()
                    raise RuntimeError(
                        f"Neither {BLE_CHAR_UUID} nor {BLE_CHAR_FALLBACK_UUID} found on device"
                    )
                self._char_uuid = BLE_CHAR_FALLBACK_UUID

            await self._client.start_notify(self._char_uuid, self._on_notify)
        return self

    async def __aexit__(self, exc_type, exc, tb) -> None:
        if self._client is not None and self._client.is_connected:
            try:
                await self._client.stop_notify(self._char_uuid)
            except Exception:
                pass
            await self._client.disconnect()

    def _on_notify(self, _sender: object, data: bytearray) -> None:
        self._rx_queue.put_nowait(bytes(data))

    async def _transact(self, packet: bytes, expected_subcmd: int) -> tuple[int, bytes]:
        """Write a custom command packet and wait for the matching notification."""
        if self._client is None:
            raise RuntimeError("BLE session is not connected")

        # Drain any stale notifications
        while not self._rx_queue.empty():
            self._rx_queue.get_nowait()

        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            await self._client.write_gatt_char(self._char_uuid, packet, response=False)

        deadline = asyncio.get_running_loop().time() + self._timeout
        while True:
            remaining = deadline - asyncio.get_running_loop().time()
            if remaining <= 0:
                raise TimeoutError(
                    f"Timed out waiting for BLE notification (subcmd=0x{expected_subcmd:02x})"
                )
            raw = await asyncio.wait_for(self._rx_queue.get(), timeout=remaining)
            hdr, payload = parse_ble_notification(raw)
            if (hdr.subcmd & 0xFF) == (expected_subcmd & 0xFF):
                return hdr.offset, payload

    async def read_platform(self) -> int:
        """Send getPlatForm_Micro (0x005A) and return the 1-byte platform ID."""
        _, payload = await self._transact(build_get_platform_packet(), CMD_GET_PLATFORM)
        return payload[0] if payload else 0

    async def read_version(self) -> tuple[int, str]:
        """Send readVersion_Micro (0x000B) and return (raw_u32, formatted_version)."""
        _, payload = await self._transact(build_read_version_packet(), CMD_READ_VERSION)
        if len(payload) < 4:
            return 0, "unknown"
        raw_ver = struct.unpack("<I", payload[:4])[0]
        return raw_ver, f"v{raw_ver / 100.0:.2f} (raw={raw_ver})"

    async def read_config_bytes(self) -> bytes:
        """Read all 180 bytes of record_micro across 4 chunks of 45 bytes."""
        buf = bytearray(RECORD_MICRO_SIZE)
        for expected_offset, pkt in build_read_config_packets():
            _, payload = await self._transact(pkt, CMD_READ_CONFIG)
            end = min(RECORD_MICRO_SIZE, expected_offset + len(payload))
            buf[expected_offset:end] = payload[: end - expected_offset]
        return bytes(buf)

    async def write_config_bytes(self, record_bytes: bytes, *, save: bool = True) -> None:
        """Write all 180 bytes of record_micro across 4 chunks and commit with Save (0x0006/0x005B)."""
        for _offset, pkt in build_write_config_packets(record_bytes):
            await self._transact(pkt, CMD_WRITE_CONFIG)
        if save:
            await self._transact(build_save_config_packet(), CMD_SAVE_CONFIG)


async def read_hardware_snapshot(
    address: str | None = None,
    timeout: float = 6.0,
) -> MicroBleSnapshot:
    """Connect to the 8BitDo Micro over BLE GATT and read platform, firmware version, and profile."""
    dev = await find_micro_ble_device(address=address, timeout=timeout)
    async with MicroBleSession(dev) as session:
        platform = await session.read_platform()
        raw_ver, ver_str = await session.read_version()
        raw_record = await session.read_config_bytes()
        profile = MicroHardwareProfile.from_bytes(raw_record)
        return MicroBleSnapshot(
            address=dev.address,
            name=dev.name or BLE_DEVICE_NAME,
            platform=platform,
            firmware_version=ver_str,
            raw_version=raw_ver,
            raw_record=raw_record,
            profile=profile,
        )


async def write_hardware_profile(
    profile: MicroHardwareProfile,
    *,
    address: str | None = None,
    timeout: float = 6.0,
    preserve_device_header: bool = True,
) -> MicroBleSnapshot:
    """Write and save a MicroHardwareProfile to the controller over BLE GATT and verify it."""
    dev = await find_micro_ble_device(address=address, timeout=timeout)
    async with MicroBleSession(dev) as session:
        platform = await session.read_platform()
        raw_ver, ver_str = await session.read_version()
        base_bytes = await session.read_config_bytes() if preserve_device_header else None
        payload_bytes = profile.to_bytes(base=base_bytes)
        await session.write_config_bytes(payload_bytes, save=True)
        verified_bytes = await session.read_config_bytes()
        verified_profile = MicroHardwareProfile.from_bytes(verified_bytes)
        return MicroBleSnapshot(
            address=dev.address,
            name=dev.name or BLE_DEVICE_NAME,
            platform=platform,
            firmware_version=ver_str,
            raw_version=raw_ver,
            raw_record=verified_bytes,
            profile=verified_profile,
        )


def format_snapshot_report(snapshot: MicroBleSnapshot) -> str:
    """Format a human-readable summary of a MicroBleSnapshot."""
    lines = [
        f"8BitDo Micro BLE GATT Endpoint : {snapshot.name} ({snapshot.address})",
        f"Firmware Version               : {snapshot.firmware_version} (platform={snapshot.platform})",
        f"Custom Mapping Enabled (Flag)  : {snapshot.profile.enabled} "
        f"(header_prefix={snapshot.profile.header_prefix.hex()})",
        f"D-Pad Orientation (Turn)       : {snapshot.profile.turn_degrees}° (code={snapshot.profile.turn})",
        "",
        "Button    | Active Hardware Key(s)       | Factory Default",
        "----------+------------------------------+----------------",
    ]
    effective = snapshot.profile.effective_slots()
    for btn in PHYSICAL_BUTTONS:
        active_names = "+".join(hid_tuple_to_names(effective.get(btn, (0, 0, 0, 0)))) or "NONE"
        factory_names = "+".join(hid_tuple_to_names(FACTORY_BUTTON_HID_USAGES[btn]))
        lines.append(f"{btn:<9} | {active_names:<28} | {factory_names}")
    return "\n".join(lines)
