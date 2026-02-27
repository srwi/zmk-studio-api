use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use bluest::{Adapter, Uuid};
use futures::StreamExt;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{BleDeviceInfo, BleDiscoveryMode};

const SETUP_TIMEOUT: Duration = Duration::from_secs(15);
const ZMK_SERVICE_UUID_STR: &str = "00000000-0196-6107-c967-c5cfb1c2482a";
const ZMK_RPC_CHAR_UUID_STR: &str = "00000001-0196-6107-c967-c5cfb1c2482a";
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

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

    // Discover ZMK Studio keyboards that are already connected as BLE HID devices.
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
    write_tx: UnboundedSender<Vec<u8>>,
    read_rx: Receiver<Vec<u8>>,
    read_queue: VecDeque<u8>,
    read_timeout: Duration,
}

impl BluestTransport {
    pub fn connect_device(device_id_json: &str) -> Result<Self, BluestTransportError> {
        let device_id: bluest::DeviceId =
            serde_json::from_str(device_id_json).map_err(BluestTransportError::Json)?;

        let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>();
        let (setup_tx, setup_rx) = mpsc::channel::<Result<(), BluestTransportError>>();

        thread::spawn(move || {
            let rt = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = setup_tx.send(Err(BluestTransportError::Runtime(e)));
                    return;
                }
            };
            let _ = rt.block_on(run_worker(device_id, write_rx, read_tx, setup_tx));
        });

        match setup_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                write_tx,
                read_rx,
                read_queue: VecDeque::new(),
                read_timeout: DEFAULT_READ_TIMEOUT,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(BluestTransportError::SetupFailed(
                "worker thread closed channel without signalling".into(),
            )),
        }
    }
}

async fn run_worker(
    device_id: bluest::DeviceId,
    mut write_rx: UnboundedReceiver<Vec<u8>>,
    read_tx: mpsc::Sender<Vec<u8>>,
    setup_tx: mpsc::Sender<Result<(), BluestTransportError>>,
) -> Result<(), BluestTransportError> {
    let (service_uuid, rpc_uuid) = zmk_uuids();
    let adapter = open_adapter().await?;

    let device = adapter.open_device(&device_id).await?;

    let services = tokio::time::timeout(
        SETUP_TIMEOUT,
        device.discover_services_with_uuid(service_uuid),
    )
    .await
    .map_err(|_| {
        BluestTransportError::SetupFailed(format!(
            "Timed out after {SETUP_TIMEOUT:?} waiting for GATT service discovery"
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
    let use_write_without_response = props.write_without_response;

    let mut notifications = tokio::time::timeout(SETUP_TIMEOUT, characteristic.notify())
        .await
        .map_err(|_| {
            BluestTransportError::SetupFailed(format!(
                "Timed out after {SETUP_TIMEOUT:?} waiting to subscribe to notifications"
            ))
        })??;

    let _ = setup_tx.send(Ok(()));

    loop {
        tokio::select! {
            maybe_notification = notifications.next() => {
                match maybe_notification {
                    Some(Ok(data)) => {
                        if read_tx.send(data).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            maybe_write = write_rx.recv() => {
                match maybe_write {
                    Some(data) => {
                        if use_write_without_response {
                            if characteristic.write_without_response(&data).await.is_err() {
                                break;
                            }
                        } else if characteristic.write(&data).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

impl Read for BluestTransport {
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
                        "Timed out waiting for BLE notification",
                    ),
                    mpsc::RecvTimeoutError::Disconnected => std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "BLE worker disconnected",
                    ),
                })?;
            self.read_queue.extend(packet);
        }

        let mut written = 0;
        written += super::read_from_queue(&mut self.read_queue, buf);
        Ok(written)
    }
}

impl Write for BluestTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_tx.send(buf.to_vec()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "BLE worker thread has terminated",
            )
        })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
