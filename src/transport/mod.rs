use std::time::Duration;

/// A discoverable ZMK Studio BLE device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleDeviceInfo {
    pub device_id: String,
    pub local_name: Option<String>,
}

impl BleDeviceInfo {
    pub fn display_name(&self) -> String {
        match &self.local_name {
            Some(name) if !name.is_empty() => format!("{} [{}]", name, self.device_id),
            _ => self.device_id.clone(),
        }
    }
}

/// BLE discovery intent.
///
/// The bluest backend supports `Connected` and `Any` on all platforms.
/// `Advertising` is not supported and will return an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleDiscoveryMode {
    /// Discover devices currently advertising ZMK Studio BLE service data.
    Advertising,
    /// Discover devices already paired/connected in the OS BLE device store.
    Connected,
    /// Use the best available backend strategy for the current platform.
    Any,
}

pub(crate) const ZMK_SERVICE_UUID_STR: &str = "00000000-0196-6107-c967-c5cfb1c2482a";
pub(crate) const ZMK_RPC_CHAR_UUID_STR: &str = "00000001-0196-6107-c967-c5cfb1c2482a";
pub(crate) const DEFAULT_BLE_READ_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DEFAULT_BLE_SETUP_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const DEFAULT_BLE_WRITE_QUEUE_CAPACITY: usize = 32;

#[cfg(feature = "ble")]
pub mod ble;
#[cfg(feature = "ble")]
mod blocking_ble;
#[cfg(feature = "serial")]
pub mod serial;

#[cfg(feature = "ble")]
pub type PlatformBleTransport = ble::BluestTransport;

#[cfg(feature = "ble")]
pub type PlatformBleError = ble::BluestTransportError;

#[cfg(feature = "ble")]
pub fn discover_platform_ble_devices(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
    ble::discover_devices_with_mode(mode)
}
