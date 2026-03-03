use std::time::Duration;

/// A discoverable ZMK Studio BLE device.
/// Defined at the transport level so it is available to both the btleplug and
/// bluest backends without a feature dependency.
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

/// Cross-platform BLE discovery intent.
///
/// Backends may not support all modes on every OS:
/// - Linux btleplug backend supports `Advertising` and `Any`.
/// - macOS/Windows bluest backend supports `Connected` and `Any`.
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
#[cfg(target_os = "linux")]
pub(crate) const DEFAULT_BLE_SCAN_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DEFAULT_BLE_READ_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DEFAULT_BLE_SETUP_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const DEFAULT_BLE_WRITE_QUEUE_CAPACITY: usize = 32;

#[cfg(all(feature = "ble", target_os = "linux"))]
pub mod ble;
#[cfg(feature = "ble")]
mod blocking_ble;
#[cfg(all(feature = "ble", any(target_os = "windows", target_os = "macos")))]
pub mod bluest_transport;
#[cfg(feature = "serial")]
pub mod serial;

#[cfg(all(feature = "ble", target_os = "linux"))]
pub type PlatformBleTransport = ble::BleTransport;
#[cfg(all(feature = "ble", any(target_os = "windows", target_os = "macos")))]
pub type PlatformBleTransport = bluest_transport::BluestTransport;

#[cfg(feature = "ble")]
#[derive(Debug)]
pub enum PlatformBleError {
    #[cfg(target_os = "linux")]
    Btleplug(ble::BleTransportError),
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Bluest(bluest_transport::BluestTransportError),
}

#[cfg(feature = "ble")]
impl std::fmt::Display for PlatformBleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(target_os = "linux")]
            Self::Btleplug(err) => err.fmt(f),
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            Self::Bluest(err) => err.fmt(f),
        }
    }
}

#[cfg(feature = "ble")]
impl std::error::Error for PlatformBleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Btleplug(err) => Some(err),
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            Self::Bluest(err) => Some(err),
        }
    }
}

#[cfg(all(feature = "ble", target_os = "linux"))]
impl From<ble::BleTransportError> for PlatformBleError {
    fn from(value: ble::BleTransportError) -> Self {
        Self::Btleplug(value)
    }
}

#[cfg(all(feature = "ble", any(target_os = "windows", target_os = "macos")))]
impl From<bluest_transport::BluestTransportError> for PlatformBleError {
    fn from(value: bluest_transport::BluestTransportError) -> Self {
        Self::Bluest(value)
    }
}

#[cfg(all(feature = "ble", target_os = "linux"))]
pub fn discover_platform_ble_devices(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
    Ok(ble::discover_devices_with_mode(mode)?)
}

#[cfg(all(feature = "ble", any(target_os = "windows", target_os = "macos")))]
pub fn discover_platform_ble_devices(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
    Ok(bluest_transport::discover_devices_with_mode(mode)?)
}
