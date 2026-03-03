use std::future::Future;
use std::io::{Read, Write};
use std::pin::Pin;

use bluest::{Adapter, Characteristic, Uuid};
use futures::{Stream, StreamExt};
use tokio::runtime::Runtime;

use super::blocking_ble::{BleWorkerBackend, BlockingBleTransport};
use super::{
    BleDeviceInfo, BleDiscoveryMode, DEFAULT_BLE_READ_TIMEOUT, DEFAULT_BLE_SETUP_TIMEOUT,
    DEFAULT_BLE_WRITE_QUEUE_CAPACITY, ZMK_RPC_CHAR_UUID_STR, ZMK_SERVICE_UUID_STR,
};

fn zmk_uuids() -> (Uuid, Uuid) {
    (
        ZMK_SERVICE_UUID_STR
            .parse()
            .expect("ZMK service UUID is valid"),
        ZMK_RPC_CHAR_UUID_STR
            .parse()
            .expect("ZMK RPC characteristic UUID is valid"),
    )
}

async fn open_adapter() -> Result<Adapter, BluestTransportError> {
    let adapter = Adapter::default()
        .await
        .ok_or(BluestTransportError::NoAdapter)?;
    adapter.wait_available().await?;
    Ok(adapter)
}

pub fn discover_devices() -> Result<Vec<BleDeviceInfo>, BluestTransportError> {
    discover_devices_with_mode(BleDiscoveryMode::Any)
}

pub fn discover_devices_with_mode(
    mode: BleDiscoveryMode,
) -> Result<Vec<BleDeviceInfo>, BluestTransportError> {
    if mode == BleDiscoveryMode::Advertising {
        return Err(BluestTransportError::UnsupportedDiscoveryMode(mode));
    }

    let rt = Runtime::new().map_err(BluestTransportError::Runtime)?;
    rt.block_on(async {
        let (service_uuid, _) = zmk_uuids();
        let adapter = open_adapter().await?;

        let devices = adapter
            .connected_devices_with_services(&[service_uuid])
            .await?;

        devices
            .into_iter()
            .map(|device| {
                let local_name = device.name().ok();
                let device_id =
                    serde_json::to_string(&device.id()).map_err(BluestTransportError::Json)?;
                Ok(BleDeviceInfo {
                    device_id,
                    local_name,
                })
            })
            .collect()
    })
}

#[derive(Debug)]
pub enum BluestTransportError {
    Runtime(std::io::Error),
    Ble(bluest::Error),
    Json(serde_json::Error),
    NoAdapter,
    UnsupportedDiscoveryMode(BleDiscoveryMode),
    ServiceNotFound,
    CharacteristicNotFound,
    SetupFailed(String),
}

impl std::fmt::Display for BluestTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(e) => write!(f, "Tokio runtime init failed: {e}"),
            Self::Ble(e) => write!(f, "BLE error: {e}"),
            Self::Json(e) => write!(f, "Device ID (de)serialization error: {e}"),
            Self::NoAdapter => write!(f, "No Bluetooth adapter found"),
            Self::UnsupportedDiscoveryMode(mode) => {
                write!(f, "Discovery mode not supported on this platform: {mode:?}")
            }
            Self::ServiceNotFound => write!(f, "ZMK Studio GATT service not found on device"),
            Self::CharacteristicNotFound => {
                write!(f, "ZMK Studio RPC GATT characteristic not found")
            }
            Self::SetupFailed(msg) => write!(f, "BLE worker setup failed: {msg}"),
        }
    }
}

impl std::error::Error for BluestTransportError {}

impl From<bluest::Error> for BluestTransportError {
    fn from(e: bluest::Error) -> Self {
        Self::Ble(e)
    }
}

impl From<serde_json::Error> for BluestTransportError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub struct BluestTransport {
    inner: BlockingBleTransport,
}

impl BluestTransport {
    pub fn connect_device(device_id_json: &str) -> Result<Self, BluestTransportError> {
        let device_id: bluest::DeviceId =
            serde_json::from_str(device_id_json).map_err(BluestTransportError::Json)?;

        let inner = BlockingBleTransport::connect::<BluestBackend>(
            device_id,
            DEFAULT_BLE_WRITE_QUEUE_CAPACITY,
            DEFAULT_BLE_READ_TIMEOUT,
            BluestTransportError::Runtime,
            || {
                BluestTransportError::SetupFailed(
                    "worker thread closed channel without signalling".into(),
                )
            },
        )?;

        Ok(Self { inner })
    }
}

impl Read for BluestTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for BluestTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct BluestBackend {
    _adapter: Adapter,
    characteristic: Characteristic,
    use_write_without_response: bool,
}

impl BleWorkerBackend for BluestBackend {
    type ConnectArg = bluest::DeviceId;
    type Error = BluestTransportError;
    type Notifications<'a> = Pin<Box<dyn Stream<Item = Result<Vec<u8>, Self::Error>> + Send + 'a>>;

    fn connect(
        device_id: Self::ConnectArg,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Error>> + Send>> {
        Box::pin(async move {
            let (service_uuid, rpc_uuid) = zmk_uuids();
            let adapter = open_adapter().await?;
            let device = adapter.open_device(&device_id).await?;
            adapter.connect_device(&device).await?;

            let services = tokio::time::timeout(
                DEFAULT_BLE_SETUP_TIMEOUT,
                device.discover_services_with_uuid(service_uuid),
            )
            .await
            .map_err(|_| {
                BluestTransportError::SetupFailed(format!(
                    "Timed out after {DEFAULT_BLE_SETUP_TIMEOUT:?} waiting for GATT service discovery"
                ))
            })??;
            let service = services
                .into_iter()
                .next()
                .ok_or(BluestTransportError::ServiceNotFound)?;

            let chars = service.discover_characteristics_with_uuid(rpc_uuid).await?;
            let characteristic = chars
                .into_iter()
                .next()
                .ok_or(BluestTransportError::CharacteristicNotFound)?;

            let props = characteristic.properties().await?;
            Ok(Self {
                _adapter: adapter,
                characteristic,
                use_write_without_response: props.write_without_response,
            })
        })
    }

    fn notifications<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Notifications<'a>, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let notifications = tokio::time::timeout(
                DEFAULT_BLE_SETUP_TIMEOUT,
                self.characteristic.notify(),
            )
                .await
                .map_err(|_| {
                    BluestTransportError::SetupFailed(format!(
                        "Timed out after {DEFAULT_BLE_SETUP_TIMEOUT:?} waiting to subscribe to notifications"
                    ))
                })??;
            let notifications = notifications.map(|packet| packet.map_err(BluestTransportError::from));
            let notifications: Self::Notifications<'a> = Box::pin(notifications);
            Ok(notifications)
        })
    }

    fn write_packet<'a>(
        &'a self,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            if self.use_write_without_response {
                self.characteristic.write_without_response(data).await?;
            } else {
                self.characteristic.write(data).await?;
            }
            Ok(())
        })
    }

    fn shutdown<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
}
