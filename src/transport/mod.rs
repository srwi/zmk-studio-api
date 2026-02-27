/// A discoverable ZMK Studio BLE device.
/// Defined at the transport level so it is available to both the `ble` feature
/// module and the Windows-only WinRT transport without a feature dependency.
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

#[cfg(feature = "ble")]
pub mod ble;
#[cfg(all(feature = "ble", target_os = "windows"))]
pub mod bluest_transport;
#[cfg(feature = "serial")]
pub mod serial;
