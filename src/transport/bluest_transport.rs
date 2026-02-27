//! Bluest-backed BLE GATT transport for Windows.
//!
//! Replaces the hand-rolled WinRT transport. Bluest wraps the same Windows BLE
//! APIs but exposes a high-level async interface. The key API used here is
//! [`bluest::Adapter::connected_devices_with_services`], which uses the Windows
//! device-association store to find already-paired BLE peripherals that expose a
//! given GATT service — no advertisement scan required.  This is the common
//! Windows scenario for a ZMK keyboard that has been previously paired and now
//! appears as a system HID device.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use bluest::{Adapter, Uuid};
use futures::StreamExt;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

const SETUP_TIMEOUT: Duration = Duration::from_secs(15);

use super::BleDeviceInfo;

const ZMK_SERVICE_UUID_STR: &str = "00000000-0196-6107-c967-c5cfb1c2482a";
const ZMK_RPC_CHAR_UUID_STR: &str = "00000001-0196-6107-c967-c5cfb1c2482a";
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Discover ZMK Studio keyboards that are already Connected as BLE HID devices.
///
/// Uses [`Adapter::connected_devices_with_services`] which queries the Windows
/// device store rather than starting an advertisement scan.  Works for any
/// ZMK keyboard that has been previously paired.
pub fn discover_devices() -> Result<Vec<BleDeviceInfo>, BluestTransportError> {
    let rt = Runtime::new().map_err(BluestTransportError::Runtime)?;
    rt.block_on(async {
        let service_uuid: Uuid = ZMK_SERVICE_UUID_STR
            .parse()
            .expect("ZMK service UUID is valid");

        let adapter = Adapter::default()
            .await
            .ok_or(BluestTransportError::NoAdapter)?;
        adapter.wait_available().await?;

        let devices = adapter
            .connected_devices_with_services(&[service_uuid])
            .await?;

        let mut result = Vec::new();
        for device in devices {
            let name = device.name().ok();
            // Serialize the platform-specific DeviceId to a JSON string so that
            // it can be stored as a plain String and later deserialized by
            // BluestTransport::connect_device.
            let device_id = serde_json::to_string(&device.id())
                .map_err(BluestTransportError::Json)?;
            result.push(BleDeviceInfo {
                device_id,
                local_name: name,
            });
        }
        Ok(result)
    })
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum BluestTransportError {
    Runtime(std::io::Error),
    Ble(bluest::Error),
    Json(serde_json::Error),
    NoAdapter,
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

// ── BluestTransport ───────────────────────────────────────────────────────────

/// Blocking [`Read`] + [`Write`] BLE transport backed by bluest.
///
/// Internally a Tokio worker thread handles the async BLE work and
/// communicates with the synchronous caller via channels — the same pattern
/// used by [`super::ble::BleTransport`].
pub struct BluestTransport {
    write_tx: UnboundedSender<Vec<u8>>,
    read_rx: Receiver<Vec<u8>>,
    read_queue: VecDeque<u8>,
    read_timeout: Duration,
}

impl BluestTransport {
    /// Connect to the device whose ID was returned by [`discover_devices`].
    ///
    /// `device_id` must be the JSON-serialized bluest `DeviceId` string
    /// produced by `serde_json::to_string(&device.id())` during discovery.
    pub fn connect_device(device_id_json: &str) -> Result<Self, BluestTransportError> {
        let device_id: bluest::DeviceId =
            serde_json::from_str(device_id_json).map_err(BluestTransportError::Json)?;

        let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>();
        let (setup_tx, setup_rx) = mpsc::channel::<Result<(), String>>();

        thread::spawn(move || {
            let rt = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = setup_tx.send(Err(e.to_string()));
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
            Ok(Err(msg)) => Err(BluestTransportError::SetupFailed(msg)),
            Err(_) => Err(BluestTransportError::SetupFailed(
                "worker thread closed channel without signalling".into(),
            )),
        }
    }
}

// ── Worker ────────────────────────────────────────────────────────────────────

async fn run_worker(
    device_id: bluest::DeviceId,
    write_rx: UnboundedReceiver<Vec<u8>>,
    read_tx: mpsc::Sender<Vec<u8>>,
    setup_tx: mpsc::Sender<Result<(), String>>,
) {
    if let Err(e) = run_worker_inner(device_id, write_rx, &read_tx, &setup_tx).await {
        // If setup already succeeded we can't send again; the error is only
        // visible to the caller through connection drops.
        let _ = setup_tx.send(Err(e.to_string()));
    }
}

async fn run_worker_inner(
    device_id: bluest::DeviceId,
    mut write_rx: UnboundedReceiver<Vec<u8>>,
    read_tx: &mpsc::Sender<Vec<u8>>,
    setup_tx: &mpsc::Sender<Result<(), String>>,
) -> Result<(), BluestTransportError> {
    let service_uuid: Uuid = ZMK_SERVICE_UUID_STR
        .parse()
        .expect("ZMK service UUID is valid");
    let rpc_uuid: Uuid = ZMK_RPC_CHAR_UUID_STR
        .parse()
        .expect("ZMK RPC characteristic UUID is valid");

    let adapter = Adapter::default()
        .await
        .ok_or(BluestTransportError::NoAdapter)?;
    adapter.wait_available().await?;

    let device = adapter.open_device(&device_id).await?;
    // The OS manages the BLE connection for paired devices; Windows opens the
    // GATT session lazily when we discover services, so no explicit
    // connect_device call is needed here.

    let services = tokio::time::timeout(
        SETUP_TIMEOUT,
        device.discover_services_with_uuid(service_uuid),
    )
    .await
    .map_err(|_| BluestTransportError::SetupFailed(
        format!("Timed out after {SETUP_TIMEOUT:?} waiting for GATT service discovery"),
    ))??;
    let service = services
        .into_iter()
        .next()
        .ok_or(BluestTransportError::ServiceNotFound)?;

    let chars = service
        .discover_characteristics_with_uuid(rpc_uuid)
        .await?;
    let characteristic = chars
        .into_iter()
        .next()
        .ok_or(BluestTransportError::CharacteristicNotFound)?;

    let props = characteristic.properties().await?;
    let use_write_without_response = props.write_without_response;

    // Both notify() and write()/write_without_response() take &self, so we
    // can hold the notification stream and still write to the same
    // characteristic without needing to clone it.
    let mut notifications = tokio::time::timeout(SETUP_TIMEOUT, characteristic.notify())
        .await
        .map_err(|_| BluestTransportError::SetupFailed(
            format!("Timed out after {SETUP_TIMEOUT:?} waiting to subscribe to notifications"),
        ))??;

    // Signal successful setup before entering the I/O loop.
    let _ = setup_tx.send(Ok(()));

    loop {
        tokio::select! {
            maybe_notification = notifications.next() => {
                match maybe_notification {
                    Some(Ok(data)) => {
                        if read_tx.send(data).is_err() {
                            break; // Main thread dropped its receiver
                        }
                    }
                    _ => break, // Stream ended or errored
                }
            }
            maybe_write = write_rx.recv() => {
                match maybe_write {
                    Some(data) => {
                        if use_write_without_response {
                            let _ = characteristic.write_without_response(&data).await;
                        } else {
                            let _ = characteristic.write(&data).await;
                        }
                    }
                    None => break, // Main thread dropped the sender
                }
            }
        }
    }

    Ok(())
}

// ── Read + Write impls ────────────────────────────────────────────────────────

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
