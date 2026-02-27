use std::future::Future;
use std::io::{Read, Write};
use std::pin::Pin;
use std::time::Duration;

use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::{Stream, StreamExt};
use tokio::runtime::Runtime;

use super::blocking_ble::{BleWorkerBackend, BlockingBleTransport};
use super::{
    BleDiscoveryMode, DEFAULT_BLE_READ_TIMEOUT, DEFAULT_BLE_SCAN_TIMEOUT,
    DEFAULT_BLE_WRITE_QUEUE_CAPACITY, ZMK_RPC_CHAR_UUID_STR, ZMK_SERVICE_UUID_STR,
};

#[derive(Debug, Clone)]
struct BleScanOptions {
    scan_timeout: Duration,
}

impl Default for BleScanOptions {
    fn default() -> Self {
        Self {
            scan_timeout: DEFAULT_BLE_SCAN_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
struct BleConnectOptions {
    scan_timeout: Duration,
    read_timeout: Duration,
    device_id: String,
}

impl BleConnectOptions {
    fn new(device_id: &str) -> Self {
        Self {
            scan_timeout: DEFAULT_BLE_SCAN_TIMEOUT,
            read_timeout: DEFAULT_BLE_READ_TIMEOUT,
            device_id: device_id.to_string(),
        }
    }
}

// Re-export from the transport root so callers can use either path.
pub use super::BleDeviceInfo;

/// Errors from BLE transport setup/operation.
#[derive(Debug)]
pub enum BleTransportError {
    RuntimeInit(std::io::Error),
    Btleplug(btleplug::Error),
    Uuid(uuid::Error),
    NoAdapter,
    UnsupportedDiscoveryMode(BleDiscoveryMode),
    DeviceNotFound(String),
    MissingRpcCharacteristic,
    SetupChannelClosed,
}

impl std::fmt::Display for BleTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeInit(err) => write!(f, "Failed to initialize runtime: {err}"),
            Self::Btleplug(err) => write!(f, "BLE error: {err}"),
            Self::Uuid(err) => write!(f, "UUID parse error: {err}"),
            Self::NoAdapter => write!(f, "No Bluetooth adapter available"),
            Self::UnsupportedDiscoveryMode(mode) => {
                write!(f, "Discovery mode not supported on this platform: {mode:?}")
            }
            Self::DeviceNotFound(device_id) => {
                write!(f, "BLE device not found for id: {device_id}")
            }
            Self::MissingRpcCharacteristic => write!(f, "ZMK Studio RPC characteristic not found"),
            Self::SetupChannelClosed => write!(f, "BLE worker initialization channel closed"),
        }
    }
}

impl std::error::Error for BleTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RuntimeInit(err) => Some(err),
            Self::Btleplug(err) => Some(err),
            Self::Uuid(err) => Some(err),
            Self::NoAdapter
            | Self::UnsupportedDiscoveryMode(_)
            | Self::DeviceNotFound(_)
            | Self::MissingRpcCharacteristic
            | Self::SetupChannelClosed => None,
        }
    }
}

impl From<btleplug::Error> for BleTransportError {
    fn from(value: btleplug::Error) -> Self {
        Self::Btleplug(value)
    }
}

impl From<uuid::Error> for BleTransportError {
    fn from(value: uuid::Error) -> Self {
        Self::Uuid(value)
    }
}

/// Discover ZMK Studio-capable BLE peripherals.
pub fn discover_devices() -> Result<Vec<BleDeviceInfo>, BleTransportError> {
    discover_devices_with_mode(BleDiscoveryMode::Any)
}

pub fn discover_devices_with_mode(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, BleTransportError> {
    match mode {
        BleDiscoveryMode::Advertising | BleDiscoveryMode::Any => {
            discover_devices_with_options(BleScanOptions::default())
        }
        BleDiscoveryMode::Connected => Err(BleTransportError::UnsupportedDiscoveryMode(mode)),
    }
}

/// Blocking BLE transport adapter for [`crate::StudioClient`].
pub struct BleTransport {
    inner: BlockingBleTransport,
}

impl BleTransport {
    /// Connects to a specific BLE peripheral using a deterministic device ID.
    pub fn connect_device(device_id: &str) -> Result<Self, BleTransportError> {
        Self::connect_with_options(BleConnectOptions::new(device_id))
    }

    fn connect_with_options(options: BleConnectOptions) -> Result<Self, BleTransportError> {
        let read_timeout = options.read_timeout;
        let inner = BlockingBleTransport::connect::<BtleplugBackend>(
            options,
            DEFAULT_BLE_WRITE_QUEUE_CAPACITY,
            read_timeout,
            BleTransportError::RuntimeInit,
            || BleTransportError::SetupChannelClosed,
        )?;
        Ok(Self { inner })
    }
}

impl Read for BleTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for BleTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn discover_devices_with_options(
    options: BleScanOptions,
) -> Result<Vec<BleDeviceInfo>, BleTransportError> {
    let runtime = Runtime::new().map_err(BleTransportError::RuntimeInit)?;
    runtime.block_on(discover_devices_async(options))
}

async fn discover_devices_async(
    options: BleScanOptions,
) -> Result<Vec<BleDeviceInfo>, BleTransportError> {
    let service_uuid = uuid::Uuid::parse_str(ZMK_SERVICE_UUID_STR)?;

    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or(BleTransportError::NoAdapter)?;

    // Use an empty ScanFilter so the OS-level watcher receives all
    // advertisement events; we filter by service UUID using discovered props.
    adapter.start_scan(ScanFilter::default()).await?;
    tokio::time::sleep(options.scan_timeout).await;

    let peripherals = adapter.peripherals().await?;
    let mut devices = Vec::new();

    for peripheral in peripherals {
        let Some(props) = peripheral.properties().await? else {
            continue;
        };

        if props.services.contains(&service_uuid) {
            devices.push(BleDeviceInfo {
                device_id: peripheral.id().to_string(),
                local_name: props.local_name,
            });
        }
    }

    Ok(devices)
}

struct BtleplugBackend {
    peripheral: Peripheral,
    characteristic: Characteristic,
    write_type: WriteType,
}

impl BleWorkerBackend for BtleplugBackend {
    type ConnectArg = BleConnectOptions;
    type Error = BleTransportError;
    type Notifications<'a> = Pin<Box<dyn Stream<Item = Result<Vec<u8>, Self::Error>> + Send + 'a>>;

    fn connect(
        options: Self::ConnectArg,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Error>> + Send>> {
        Box::pin(async move {
            let rpc_uuid = uuid::Uuid::parse_str(ZMK_RPC_CHAR_UUID_STR)?;
            let (peripheral, characteristic, write_type) = connect_peripheral(rpc_uuid, &options).await?;
            peripheral.subscribe(&characteristic).await?;
            Ok(Self {
                peripheral,
                characteristic,
                write_type,
            })
        })
    }

    fn notifications<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Notifications<'a>, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let characteristic_uuid = self.characteristic.uuid;
            let notifications = self.peripheral.notifications().await?;
            let notifications = notifications.filter_map(move |notification| {
                let matches_characteristic = notification.uuid == characteristic_uuid;
                async move {
                    if matches_characteristic {
                        Some(Ok(notification.value))
                    } else {
                        None
                    }
                }
            });
            let notifications: Self::Notifications<'a> = Box::pin(notifications);
            Ok(notifications)
        })
    }

    fn write_packet<'a>(
        &'a self,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            self.peripheral
                .write(&self.characteristic, data, self.write_type)
                .await?;
            Ok(())
        })
    }

    fn shutdown<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let _ = self.peripheral.disconnect().await;
        })
    }
}

async fn connect_peripheral(
    rpc_uuid: uuid::Uuid,
    options: &BleConnectOptions,
) -> Result<(Peripheral, Characteristic, WriteType), BleTransportError> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or(BleTransportError::NoAdapter)?;

    let peripheral = if let Some(peripheral) = select_peripheral(&adapter, &options.device_id).await?
    {
        peripheral
    } else {
        adapter.start_scan(ScanFilter::default()).await?;
        tokio::time::sleep(options.scan_timeout).await;
        select_peripheral(&adapter, &options.device_id)
            .await?
            .ok_or_else(|| BleTransportError::DeviceNotFound(options.device_id.clone()))?
    };
    peripheral.connect().await?;
    peripheral.discover_services().await?;

    let characteristic = peripheral
        .characteristics()
        .into_iter()
        .find(|ch| ch.uuid == rpc_uuid)
        .ok_or(BleTransportError::MissingRpcCharacteristic)?;

    let write_type = if characteristic
        .properties
        .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
    {
        WriteType::WithoutResponse
    } else {
        WriteType::WithResponse
    };

    Ok((peripheral, characteristic, write_type))
}

async fn select_peripheral(
    adapter: &Adapter,
    device_id: &str,
) -> Result<Option<Peripheral>, BleTransportError> {
    let peripherals = adapter.peripherals().await?;
    for peripheral in peripherals {
        if peripheral.id().to_string() == device_id {
            return Ok(Some(peripheral));
        }
    }

    Ok(None)
}
