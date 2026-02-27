/// A discoverable ZMK Studio BLE device.
/// Defined at the transport level so it is available to both the `ble` feature
/// module and the Windows-only bluest transport without a feature dependency.
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
/// - Linux/macOS btleplug backend supports `Advertising` and `Any`.
/// - Windows bluest backend supports `Connected` and `Any`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleDiscoveryMode {
    /// Discover devices currently advertising ZMK Studio BLE service data.
    Advertising,
    /// Discover devices already paired/connected in the OS BLE device store.
    Connected,
    /// Use the best available backend strategy for the current platform.
    Any,
}

pub(crate) fn read_from_queue(
    read_queue: &mut std::collections::VecDeque<u8>,
    buf: &mut [u8],
) -> usize {
    let mut written = 0;
    while written < buf.len() {
        let Some(byte) = read_queue.pop_front() else {
            break;
        };
        buf[written] = byte;
        written += 1;
    }
    written
}

#[cfg(feature = "ble")]
pub mod ble;
#[cfg(all(feature = "ble", target_os = "windows"))]
pub mod bluest_transport;
#[cfg(feature = "serial")]
pub mod serial;

#[cfg(all(feature = "ble", not(target_os = "windows")))]
pub type PlatformBleTransport = ble::BleTransport;
#[cfg(all(feature = "ble", target_os = "windows"))]
pub type PlatformBleTransport = bluest_transport::BluestTransport;

#[cfg(feature = "ble")]
#[derive(Debug)]
pub enum PlatformBleError {
    #[cfg(not(target_os = "windows"))]
    Btleplug(ble::BleTransportError),
    #[cfg(target_os = "windows")]
    Bluest(bluest_transport::BluestTransportError),
}

#[cfg(feature = "ble")]
impl std::fmt::Display for PlatformBleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(not(target_os = "windows"))]
            Self::Btleplug(err) => err.fmt(f),
            #[cfg(target_os = "windows")]
            Self::Bluest(err) => err.fmt(f),
        }
    }
}

#[cfg(feature = "ble")]
impl std::error::Error for PlatformBleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(not(target_os = "windows"))]
            Self::Btleplug(err) => Some(err),
            #[cfg(target_os = "windows")]
            Self::Bluest(err) => Some(err),
        }
    }
}

#[cfg(all(feature = "ble", not(target_os = "windows")))]
impl From<ble::BleTransportError> for PlatformBleError {
    fn from(value: ble::BleTransportError) -> Self {
        Self::Btleplug(value)
    }
}

#[cfg(all(feature = "ble", target_os = "windows"))]
impl From<bluest_transport::BluestTransportError> for PlatformBleError {
    fn from(value: bluest_transport::BluestTransportError) -> Self {
        Self::Bluest(value)
    }
}

#[cfg(all(feature = "ble", not(target_os = "windows")))]
pub fn discover_platform_ble_devices(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
    Ok(ble::discover_devices_with_mode(mode)?)
}

#[cfg(all(feature = "ble", target_os = "windows"))]
pub fn discover_platform_ble_devices(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
    Ok(bluest_transport::discover_devices_with_mode(mode)?)
}
