//! Native Windows Runtime (WinRT) BLE GATT transport for ZMK Studio.
//!
//! Unlike the btleplug-based [`super::ble::BleTransport`], this transport uses
//! WinRT APIs directly via the `windows` crate.  The key advantage is that
//! [`windows::Devices::Bluetooth::BluetoothLEDevice::FromBluetoothAddressAsync`]
//! works for **already-paired and currently-connected** peripherals, which
//! don't re-advertise and therefore cannot be found by a BLE scan.  This is
//! the common Windows situation when a ZMK keyboard is connected via Bluetooth
//! to the host (it shows up as a HID device and stops advertising).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristicProperties, GattClientCharacteristicConfigurationDescriptorValue,
    GattCommunicationStatus, GattDeviceService, GattValueChangedEventArgs, GattWriteOption,
};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter};
use windows::core::{GUID, Ref};

use super::BleDeviceInfo;

// ── UUID constants ────────────────────────────────────────────────────────────

/// ZMK Studio BLE service: `00000000-0196-6107-c967-c5cfb1c2482a`
const ZMK_SERVICE_GUID: GUID = GUID::from_values(
    0x0000_0000,
    0x0196,
    0x6107,
    [0xc9, 0x67, 0xc5, 0xcf, 0xb1, 0xc2, 0x48, 0x2a],
);

/// ZMK Studio RPC characteristic: `00000001-0196-6107-c967-c5cfb1c2482a`
const ZMK_RPC_CHAR_GUID: GUID = GUID::from_values(
    0x0000_0001,
    0x0196,
    0x6107,
    [0xc9, 0x67, 0xc5, 0xcf, 0xb1, 0xc2, 0x48, 0x2a],
);


const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

// ── Address helpers ───────────────────────────────────────────────────────────

/// Convert a 6-byte BT address to the 64-bit integer WinRT expects.
/// The address bytes are in big-endian order (most-significant byte first).
pub fn bt_addr_bytes_to_u64(addr: [u8; 6]) -> u64 {
    addr.iter().fold(0u64, |acc, &b| (acc << 8) | (b as u64))
}

/// Format a 64-bit BT address as `"AA:BB:CC:DD:EE:FF"`.
/// This matches the string format btleplug uses for `PeripheralId::to_string()`
/// on Windows, so the same device_id can be used with both transports.
pub fn bt_addr_u64_to_string(addr: u64) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        (addr >> 40) & 0xFF,
        (addr >> 32) & 0xFF,
        (addr >> 24) & 0xFF,
        (addr >> 16) & 0xFF,
        (addr >> 8) & 0xFF,
        addr & 0xFF,
    )
}

/// Parse a device-id string produced by [`bt_addr_u64_to_string`] back into a
/// `u64`.  Accepts any string that contains exactly 12 hex digits (colons and
/// other separators are stripped).
pub fn parse_device_id_to_u64(device_id: &str) -> Option<u64> {
    let hex_only: String = device_id
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    if hex_only.len() == 12 {
        u64::from_str_radix(&hex_only, 16).ok()
    } else {
        None
    }
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum WinRtTransportError {
    InvalidDeviceId(String),
    DeviceNotFound,
    ServiceNotFound,
    CharacteristicNotFound,
    NotificationNotSupported,
    WriteNotSupported,
    CccdWriteFailed {
        status: GattCommunicationStatus,
        protocol_error: Option<u8>,
    },
    WriteFailed {
        status: GattCommunicationStatus,
        protocol_error: Option<u8>,
    },
    SubscribeFailed(windows::core::Error),
    WinRt(windows::core::Error),
}

impl std::fmt::Display for WinRtTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDeviceId(s) => write!(f, "Invalid BT device id: {s}"),
            Self::DeviceNotFound => write!(f, "BLE device not found by WinRT"),
            Self::ServiceNotFound => write!(f, "ZMK Studio GATT service not found on device"),
            Self::CharacteristicNotFound => {
                write!(f, "ZMK Studio RPC characteristic not found")
            }
            Self::NotificationNotSupported => write!(
                f,
                "RPC characteristic does not support notifications or indications"
            ),
            Self::WriteNotSupported => {
                write!(f, "RPC characteristic does not support writes")
            }
            Self::CccdWriteFailed {
                status,
                protocol_error,
            } => {
                if let Some(protocol_error) = protocol_error {
                    write!(
                        f,
                        "Failed to enable GATT notifications: {status:?} (ATT error 0x{protocol_error:02X})"
                    )
                } else {
                    write!(f, "Failed to enable GATT notifications: {status:?}")
                }
            }
            Self::WriteFailed {
                status,
                protocol_error,
            } => {
                if let Some(protocol_error) = protocol_error {
                    write!(
                        f,
                        "GATT write failed: {status:?} (ATT error 0x{protocol_error:02X})"
                    )
                } else {
                    write!(f, "GATT write failed: {status:?}")
                }
            }
            Self::SubscribeFailed(e) => write!(f, "Failed to subscribe to GATT notifications: {e}"),
            Self::WinRt(e) => write!(f, "WinRT error: {e}"),
        }
    }
}

impl std::error::Error for WinRtTransportError {}

impl From<windows::core::Error> for WinRtTransportError {
    fn from(e: windows::core::Error) -> Self {
        Self::WinRt(e)
    }
}

// ── Device probing (discovery) ────────────────────────────────────────────────

/// Probe a single Bluetooth device by address using native WinRT APIs.
///
/// Returns `Some(BleDeviceInfo)` if the device can be reached **and** it
/// exposes the ZMK Studio GATT service.  Returns `None` otherwise.
///
/// This works even when the peripheral is already paired and connected as a
/// HID device (and therefore not advertising), which is the usual Windows
/// situation.
pub fn probe_ble_device(bt_addr: [u8; 6]) -> Option<BleDeviceInfo> {
    let addr_u64 = bt_addr_bytes_to_u64(bt_addr);

    let device = BluetoothLEDevice::FromBluetoothAddressAsync(addr_u64)
        .ok()?
        .get()
        .ok()?;

    // Check whether the ZMK Studio service is present.
    let services_result = device
        .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .ok()?
        .get()
        .ok()?;

    if services_result.Status().ok()? != GattCommunicationStatus::Success {
        return None;
    }

    let services = services_result.Services().ok()?;
    let has_zmk = (0..services.Size().ok()?)
        .filter_map(|i| services.GetAt(i).ok())
        .any(
            |s: windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService| {
                s.Uuid().ok().as_ref() == Some(&ZMK_SERVICE_GUID)
            },
        );

    if !has_zmk {
        return None;
    }

    // Use the Windows-cached display name — works for paired devices without
    // an extra GATT round-trip.
    let name = device.Name().ok().map(|h| h.to_string()).filter(|s| !s.is_empty());
    let device_id = bt_addr_u64_to_string(addr_u64);

    Some(BleDeviceInfo {
        device_id,
        local_name: name,
    })
}

// ── WinRtGattTransport ────────────────────────────────────────────────────────

/// Blocking Read+Write transport backed by native WinRT GATT APIs.
///
/// Unlike [`super::ble::BleTransport`] (which uses btleplug and requires the
/// device to be visible in a BLE scan), this transport connects directly via
/// `BluetoothLEDevice::FromBluetoothAddressAsync` and works for devices that
/// are already paired and connected as HID peripherals.
pub struct WinRtGattTransport {
    /// Keep the underlying device alive for the transport lifetime.
    _device: BluetoothLEDevice,
    /// Keep the service alive for the transport lifetime.
    _service: GattDeviceService,
    /// The WinRT GATT characteristic used for both read notifications and writes.
    characteristic: windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    /// Token returned when registering the ValueChanged handler; used for cleanup.
    _notify_token: i64,
    /// Incoming notification packets received from the WinRT thread-pool callback.
    read_rx: Receiver<Vec<u8>>,
    /// Byte-level queue built from incoming packets to satisfy `Read`.
    read_queue: VecDeque<u8>,
    /// Write procedure selected from characteristic properties.
    write_option: GattWriteOption,
    read_timeout: Duration,
}

impl WinRtGattTransport {
    /// Connect to a ZMK Studio peripheral identified by `device_id`.
    ///
    /// `device_id` must be a Bluetooth address in the format produced by
    /// [`bt_addr_u64_to_string`] (`"AA:BB:CC:DD:EE:FF"`).
    pub fn connect_device(device_id: &str) -> Result<Self, WinRtTransportError> {
        let addr_u64 = parse_device_id_to_u64(device_id)
            .ok_or_else(|| WinRtTransportError::InvalidDeviceId(device_id.to_string()))?;

        let (device, service, characteristic) = get_rpc_objects(addr_u64)?;

        let (read_tx, read_rx) = mpsc::sync_channel::<Vec<u8>>(64);
        let notify_token = register_notification_handler(&characteristic, read_tx)?;

        let props = characteristic.CharacteristicProperties()?;
        let cccd_mode = if props.contains(GattCharacteristicProperties::Notify) {
            GattClientCharacteristicConfigurationDescriptorValue::Notify
        } else if props.contains(GattCharacteristicProperties::Indicate) {
            GattClientCharacteristicConfigurationDescriptorValue::Indicate
        } else {
            return Err(WinRtTransportError::NotificationNotSupported);
        };
        let write_option = if props.contains(GattCharacteristicProperties::WriteWithoutResponse) {
            GattWriteOption::WriteWithoutResponse
        } else if props.contains(GattCharacteristicProperties::Write) {
            GattWriteOption::WriteWithResponse
        } else {
            return Err(WinRtTransportError::WriteNotSupported);
        };

        // Tell the peripheral to start sending notifications.
        let cccd_result = characteristic
            .WriteClientCharacteristicConfigurationDescriptorWithResultAsync(cccd_mode)?
            .get()?;
        let cccd_status = cccd_result.Status()?;
        if cccd_status != GattCommunicationStatus::Success {
            let protocol_error = cccd_result
                .ProtocolError()
                .ok()
                .and_then(|value| value.Value().ok());
            return Err(WinRtTransportError::CccdWriteFailed {
                status: cccd_status,
                protocol_error,
            });
        }

        Ok(Self {
            _device: device,
            _service: service,
            characteristic,
            _notify_token: notify_token,
            read_rx,
            read_queue: VecDeque::new(),
            write_option,
            read_timeout: DEFAULT_READ_TIMEOUT,
        })
    }
}

/// Obtain a `BluetoothLEDevice`, verify the ZMK Studio service is present, and
/// return both service + RPC characteristic. Callers should keep these alive.
fn get_rpc_objects(
    addr_u64: u64,
) -> Result<
    (
        BluetoothLEDevice,
        GattDeviceService,
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    ),
    WinRtTransportError,
> {
    let device = BluetoothLEDevice::FromBluetoothAddressAsync(addr_u64)?
        .get()
        .map_err(|_| WinRtTransportError::DeviceNotFound)?;

    let services_result = device
        .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)?
        .get()?;

    if services_result.Status()? != GattCommunicationStatus::Success {
        return Err(WinRtTransportError::ServiceNotFound);
    }

    let services = services_result.Services()?;
    let zmk_service = (0..services.Size()?)
        .filter_map(|i| services.GetAt(i).ok())
        .find(
            |s: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService| {
                s.Uuid().ok().as_ref() == Some(&ZMK_SERVICE_GUID)
            },
        )
        .ok_or(WinRtTransportError::ServiceNotFound)?;

    let chars_result = zmk_service
        .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)?
        .get()?;

    if chars_result.Status()? != GattCommunicationStatus::Success {
        return Err(WinRtTransportError::CharacteristicNotFound);
    }

    let chars = chars_result.Characteristics()?;
    let characteristic = (0..chars.Size()?)
        .filter_map(|i| chars.GetAt(i).ok())
        .find(
            |c: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic| {
                c.Uuid().ok().as_ref() == Some(&ZMK_RPC_CHAR_GUID)
            },
        )
        .ok_or(WinRtTransportError::CharacteristicNotFound)?;

    Ok((device, zmk_service, characteristic))
}

/// Register a `ValueChanged` handler on `characteristic` that forwards decoded
/// bytes into `read_tx`.  Returns the event registration token.
fn register_notification_handler(
    characteristic: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    read_tx: SyncSender<Vec<u8>>,
) -> Result<i64, WinRtTransportError> {
    let handler = TypedEventHandler::new(
        move |_sender: Ref<
            windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        >,
              args: Ref<GattValueChangedEventArgs>| {
            if let Ok(args) = args.ok() {
                let value = args.CharacteristicValue()?;
                let reader = DataReader::FromBuffer(&value)?;
                let len = reader.UnconsumedBufferLength()? as usize;
                let mut buf = vec![0u8; len];
                reader.ReadBytes(&mut buf)?;
                let _ = read_tx.send(buf);
            }
            Ok(())
        },
    );

    characteristic
        .ValueChanged(&handler)
        .map_err(WinRtTransportError::SubscribeFailed)
}

impl Drop for WinRtGattTransport {
    fn drop(&mut self) {
        // Disable notifications (best-effort, ignore errors).
        let _ = self
            .characteristic
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::None,
            )
            .and_then(|op| op.get());
        let _ = self.characteristic.RemoveValueChanged(self._notify_token);
    }
}

impl Read for WinRtGattTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        if self.read_queue.is_empty() {
            let packet = self
                .read_rx
                .recv_timeout(self.read_timeout)
                .map_err(|err| match err {
                    mpsc::RecvTimeoutError::Timeout => std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Timed out waiting for GATT notification",
                    ),
                    mpsc::RecvTimeoutError::Disconnected => std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "WinRT GATT transport disconnected",
                    ),
                })?;
            self.read_queue.extend(packet);
        }

        let mut written = 0;
        while written < buf.len() {
            let Some(byte) = self.read_queue.pop_front() else {
                break;
            };
            buf[written] = byte;
            written += 1;
        }
        Ok(written)
    }
}

impl Write for WinRtGattTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let writer = DataWriter::new()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?;
        writer
            .WriteBytes(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?;
        self.characteristic
            .WriteValueWithResultAndOptionAsync(&buffer, self.write_option)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?
            .get()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))
            .and_then(|result| {
                let status = result.Status().map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string())
                })?;
                if status == GattCommunicationStatus::Success {
                    Ok(())
                } else {
                    let protocol_error = result
                        .ProtocolError()
                        .ok()
                        .and_then(|value| value.Value().ok());
                    Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        WinRtTransportError::WriteFailed {
                            status,
                            protocol_error,
                        }
                        .to_string(),
                    ))
                }
            })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
