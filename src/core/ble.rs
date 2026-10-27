//! Direct BLE GATT client for the 8BitDo Micro ('80EL') configuration service using `btleplug`.

use std::time::Duration;

use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use uuid::Uuid;

use crate::protocol::{
    build_get_platform_packet, build_read_config_packets, build_read_version_packet,
    build_save_config_packet, build_write_config_packets, factory_button_hid_usages,
    hid_tuple_to_names, parse_ble_notification, MicroHardwareProfile, BLE_CHAR_FALLBACK_UUID,
    BLE_CHAR_UUID, BLE_DEVICE_NAME, CMD_GET_PLATFORM, CMD_READ_CONFIG, CMD_READ_VERSION,
    CMD_SAVE_CONFIG, CMD_WRITE_CONFIG, PHYSICAL_BUTTONS, RECORD_MICRO_SIZE,
};

#[derive(Debug, Clone)]
pub struct MicroBleSnapshot {
    pub address: String,
    pub name: String,
    pub platform: Option<u8>,
    pub firmware_version: Option<String>,
    pub raw_version: Option<u32>,
    pub raw_record: Vec<u8>,
    pub profile: MicroHardwareProfile,
}

#[derive(Debug, Clone)]
pub struct DiscoveredMicroBle {
    pub name: String,
    pub address: String,
    pub rssi: Option<i16>,
    pub peripheral: Peripheral,
}

async fn get_default_adapter() -> Result<Adapter, String> {
    let manager = Manager::new()
        .await
        .map_err(|e| format!("Failed to initialize BLE manager: {}", e))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| format!("Failed to query BLE adapters: {}", e))?;
    adapters
        .into_iter()
        .next()
        .ok_or_else(|| "No Bluetooth adapter found on this system.".to_string())
}

pub async fn scan_for_micro_ble_devices(
    timeout: Duration,
) -> Result<Vec<DiscoveredMicroBle>, String> {
    let adapter = get_default_adapter().await?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| format!("Failed to start BLE scan: {}", e))?;

    tokio::time::sleep(timeout).await;
    let _ = adapter.stop_scan().await;

    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| format!("Failed to list BLE peripherals: {}", e))?;

    let mut found = Vec::new();
    for p in peripherals {
        if let Ok(Some(props)) = p.properties().await {
            let name = props.local_name.unwrap_or_default();
            if name.trim().eq_ignore_ascii_case(BLE_DEVICE_NAME) {
                found.push(DiscoveredMicroBle {
                    name,
                    address: p.address().to_string(),
                    rssi: props.rssi,
                    peripheral: p,
                });
            }
        }
    }

    found.sort_by(|a, b| b.rssi.unwrap_or(-100).cmp(&a.rssi.unwrap_or(-100)));
    Ok(found)
}

pub async fn find_micro_ble_device(
    address: Option<&str>,
    timeout: Duration,
) -> Result<DiscoveredMicroBle, String> {
    let adapter = get_default_adapter().await?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| format!("Failed to start BLE scan: {}", e))?;

    let deadline = tokio::time::Instant::now() + timeout;
    let target_addr = address.map(|a| a.trim().to_uppercase());

    while tokio::time::Instant::now() < deadline {
        if let Ok(peripherals) = adapter.peripherals().await {
            for p in peripherals {
                let p_addr = p.address().to_string().to_uppercase();
                let props = p.properties().await.ok().flatten();
                let name = props
                    .as_ref()
                    .and_then(|pr| pr.local_name.clone())
                    .unwrap_or_default();
                let rssi = props.as_ref().and_then(|pr| pr.rssi);

                if let Some(ref want_addr) = target_addr {
                    if &p_addr == want_addr {
                        let _ = adapter.stop_scan().await;
                        return Ok(DiscoveredMicroBle {
                            name: if name.is_empty() {
                                BLE_DEVICE_NAME.to_string()
                            } else {
                                name
                            },
                            address: p_addr,
                            rssi,
                            peripheral: p,
                        });
                    }
                } else if name.trim().eq_ignore_ascii_case(BLE_DEVICE_NAME) {
                    let _ = adapter.stop_scan().await;
                    return Ok(DiscoveredMicroBle {
                        name,
                        address: p_addr,
                        rssi,
                        peripheral: p,
                    });
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    let _ = adapter.stop_scan().await;
    if let Some(addr) = address {
        Err(format!(
            "BLE device {} not found within {:.1}s",
            addr,
            timeout.as_secs_f64()
        ))
    } else {
        Err(format!(
            "8BitDo Micro BLE endpoint ({:?}) not found. Ensure the controller is powered on.",
            BLE_DEVICE_NAME
        ))
    }
}

pub struct MicroBleSession {
    peripheral: Peripheral,
    char_handle: Characteristic,
    response_timeout: Duration,
}

impl MicroBleSession {
    pub async fn connect(
        dev: DiscoveredMicroBle,
        response_timeout: Duration,
    ) -> Result<Self, String> {
        let p = dev.peripheral;
        p.connect()
            .await
            .map_err(|e| format!("Failed to connect to BLE device {}: {}", dev.address, e))?;
        p.discover_services()
            .await
            .map_err(|e| format!("Failed to discover GATT services: {}", e))?;

        let primary_uuid = Uuid::parse_str(BLE_CHAR_UUID).unwrap();
        let fallback_uuid = Uuid::parse_str(BLE_CHAR_FALLBACK_UUID).unwrap();

        let chars = p.characteristics();
        let target_char = chars
            .iter()
            .find(|c| c.uuid == primary_uuid)
            .or_else(|| chars.iter().find(|c| c.uuid == fallback_uuid))
            .cloned();

        let Some(char_handle) = target_char else {
            let _ = p.disconnect().await;
            return Err(format!(
                "Neither {} nor {} found on BLE device",
                BLE_CHAR_UUID, BLE_CHAR_FALLBACK_UUID
            ));
        };

        p.subscribe(&char_handle)
            .await
            .map_err(|e| format!("Failed to subscribe to BLE notifications: {}", e))?;

        Ok(Self {
            peripheral: p,
            char_handle,
            response_timeout,
        })
    }

    pub async fn close(self) {
        let _ = self.peripheral.unsubscribe(&self.char_handle).await;
        let _ = self.peripheral.disconnect().await;
    }

    async fn transact(
        &self,
        packet: &[u8],
        expected_subcmd: u16,
    ) -> Result<(u32, Vec<u8>), String> {
        let mut notif_stream = self
            .peripheral
            .notifications()
            .await
            .map_err(|e| format!("Failed to open BLE notification stream: {}", e))?;

        self.peripheral
            .write(&self.char_handle, packet, WriteType::WithoutResponse)
            .await
            .map_err(|e| format!("BLE GATT write failed: {}", e))?;

        let deadline = tokio::time::Instant::now() + self.response_timeout;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err(format!(
                    "Timed out waiting for BLE notification (subcmd=0x{:02x})",
                    expected_subcmd
                ));
            }
            let remaining = deadline - now;
            match tokio::time::timeout(remaining, notif_stream.next()).await {
                Ok(Some(notif)) => {
                    if notif.uuid == self.char_handle.uuid {
                        if let Ok((hdr, payload)) = parse_ble_notification(&notif.value) {
                            if (hdr.subcmd & 0xFF) == (expected_subcmd & 0xFF) {
                                return Ok((hdr.offset, payload));
                            }
                        }
                    }
                }
                Ok(None) => return Err("BLE notification stream ended unexpectedly".to_string()),
                Err(_) => {
                    return Err(format!(
                        "Timed out waiting for BLE notification (subcmd=0x{:02x})",
                        expected_subcmd
                    ))
                }
            }
        }
    }

    pub async fn read_platform(&self) -> Result<u8, String> {
        let pkt = build_get_platform_packet();
        let (_, payload) = self.transact(&pkt, CMD_GET_PLATFORM).await?;
        Ok(payload.first().copied().unwrap_or(0))
    }

    pub async fn read_version(&self) -> Result<(u32, String), String> {
        let pkt = build_read_version_packet();
        let (_, payload) = self.transact(&pkt, CMD_READ_VERSION).await?;
        if payload.len() < 4 {
            return Ok((0, "unknown".to_string()));
        }
        let raw_ver = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        Ok((
            raw_ver,
            format!("v{:.2} (raw={})", raw_ver as f64 / 100.0, raw_ver),
        ))
    }

    pub async fn read_config_bytes(&self) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; RECORD_MICRO_SIZE];
        for (expected_offset, pkt) in build_read_config_packets() {
            let (_, payload) = self.transact(&pkt, CMD_READ_CONFIG).await?;
            let end = std::cmp::min(RECORD_MICRO_SIZE, expected_offset + payload.len());
            let copy_len = end - expected_offset;
            buf[expected_offset..end].copy_from_slice(&payload[..copy_len]);
        }
        Ok(buf)
    }

    pub async fn write_config_bytes(
        &self,
        record_bytes: &[u8],
        save: bool,
    ) -> Result<(), String> {
        for (_offset, pkt) in build_write_config_packets(record_bytes)? {
            self.transact(&pkt, CMD_WRITE_CONFIG).await?;
        }
        if save {
            let save_pkt = build_save_config_packet();
            self.transact(&save_pkt, CMD_SAVE_CONFIG).await?;
        }
        Ok(())
    }
}

pub async fn read_hardware_snapshot(
    address: Option<&str>,
    timeout: Duration,
) -> Result<MicroBleSnapshot, String> {
    let dev = find_micro_ble_device(address, timeout).await?;
    let addr_str = dev.address.clone();
    let name_str = dev.name.clone();
    let session = MicroBleSession::connect(dev, Duration::from_secs(3)).await?;
    let res = async {
        let platform = session.read_platform().await?;
        let (raw_ver, ver_str) = session.read_version().await?;
        let raw_record = session.read_config_bytes().await?;
        let profile = MicroHardwareProfile::from_bytes(&raw_record)?;
        Ok(MicroBleSnapshot {
            address: addr_str,
            name: name_str,
            platform: Some(platform),
            firmware_version: Some(ver_str),
            raw_version: Some(raw_ver),
            raw_record,
            profile,
        })
    }
    .await;
    session.close().await;
    res
}

pub async fn write_hardware_profile(
    profile: &MicroHardwareProfile,
    address: Option<&str>,
    timeout: Duration,
    preserve_device_header: bool,
) -> Result<MicroBleSnapshot, String> {
    let dev = find_micro_ble_device(address, timeout).await?;
    let addr_str = dev.address.clone();
    let name_str = dev.name.clone();
    let session = MicroBleSession::connect(dev, Duration::from_secs(3)).await?;
    let res = async {
        let platform = session.read_platform().await?;
        let (raw_ver, ver_str) = session.read_version().await?;
        let base_bytes = if preserve_device_header {
            Some(session.read_config_bytes().await?)
        } else {
            None
        };
        let payload_bytes = profile.to_bytes(base_bytes.as_deref())?;
        session.write_config_bytes(&payload_bytes, true).await?;
        let verified_bytes = session.read_config_bytes().await?;
        let verified_profile = MicroHardwareProfile::from_bytes(&verified_bytes)?;
        Ok(MicroBleSnapshot {
            address: addr_str,
            name: name_str,
            platform: Some(platform),
            firmware_version: Some(ver_str),
            raw_version: Some(raw_ver),
            raw_record: verified_bytes,
            profile: verified_profile,
        })
    }
    .await;
    session.close().await;
    res
}

pub fn format_snapshot_report(snapshot: &MicroBleSnapshot) -> String {
    let hex_prefix = format!(
        "{:02x}{:02x}{:02x}",
        snapshot.profile.header_prefix[0],
        snapshot.profile.header_prefix[1],
        snapshot.profile.header_prefix[2]
    );
    let mut lines = vec![
        format!(
            "8BitDo Micro BLE GATT Endpoint : {} ({})",
            snapshot.name, snapshot.address
        ),
        format!(
            "Firmware Version               : {} (platform={})",
            snapshot.firmware_version.as_deref().unwrap_or("unknown"),
            snapshot.platform.unwrap_or(0)
        ),
        format!(
            "Custom Mapping Enabled (Flag)  : {} (header_prefix={})",
            snapshot.profile.enabled, hex_prefix
        ),
        format!(
            "D-Pad Orientation (Turn)       : {}° (code={})",
            snapshot.profile.turn_degrees(),
            snapshot.profile.turn
        ),
        String::new(),
        "Button    | Active Hardware Key(s)       | Factory Default".to_string(),
        "----------+------------------------------+----------------".to_string(),
    ];

    let effective = snapshot.profile.effective_slots();
    let factory = factory_button_hid_usages();

    for &btn in PHYSICAL_BUTTONS {
        let active_usages = effective.get(btn).copied().unwrap_or([0, 0, 0, 0]);
        let active_names_vec = hid_tuple_to_names(&active_usages);
        let active_names = if active_names_vec.is_empty() {
            "NONE".to_string()
        } else {
            active_names_vec.join("+")
        };

        let factory_usages = factory.get(btn).copied().unwrap_or([0, 0, 0, 0]);
        let factory_names = hid_tuple_to_names(&factory_usages).join("+");

        lines.push(format!(
            "{:<9} | {:<28} | {}",
            btn, active_names, factory_names
        ));
    }

    lines.join("\n")
}
