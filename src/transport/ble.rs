use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

use crate::client::{BLE_RPC_CHARACTERISTIC_UUID, BLE_SERVICE_UUID};

pub const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct BleConnectOptions {
    pub scan_timeout: Duration,
    pub read_timeout: Duration,
    pub name_contains: Option<String>,
}

impl Default for BleConnectOptions {
    fn default() -> Self {
        Self {
            scan_timeout: DEFAULT_SCAN_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            name_contains: None,
        }
    }
}

#[derive(Debug)]
pub enum BleTransportError {
    RuntimeInit(std::io::Error),
    Btleplug(btleplug::Error),
    Uuid(uuid::Error),
    NoAdapter,
    NoMatchingPeripheral,
    MissingRpcCharacteristic,
    SetupChannelClosed,
}

impl std::fmt::Display for BleTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeInit(err) => write!(f, "failed to initialize runtime: {err}"),
            Self::Btleplug(err) => write!(f, "ble error: {err}"),
            Self::Uuid(err) => write!(f, "uuid parse error: {err}"),
            Self::NoAdapter => write!(f, "no bluetooth adapter available"),
            Self::NoMatchingPeripheral => write!(f, "no matching zmk studio peripheral found"),
            Self::MissingRpcCharacteristic => write!(f, "zmk studio rpc characteristic not found"),
            Self::SetupChannelClosed => write!(f, "ble worker initialization channel closed"),
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
            | Self::NoMatchingPeripheral
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

pub struct BleTransport {
    write_tx: UnboundedSender<Vec<u8>>,
    read_rx: Receiver<Vec<u8>>,
    read_queue: VecDeque<u8>,
    read_timeout: Duration,
}

impl BleTransport {
    pub fn connect_first() -> Result<Self, BleTransportError> {
        Self::connect_with_options(BleConnectOptions::default())
    }

    pub fn connect_with_options(options: BleConnectOptions) -> Result<Self, BleTransportError> {
        let read_timeout = options.read_timeout;
        let worker_options = options.clone();
        let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>();
        let (setup_tx, setup_rx) = mpsc::channel::<Result<(), BleTransportError>>();

        thread::spawn(move || {
            let runtime = match Runtime::new() {
                Ok(rt) => rt,
                Err(err) => {
                    let _ = setup_tx.send(Err(BleTransportError::RuntimeInit(err)));
                    return;
                }
            };

            let _ = runtime.block_on(run_ble_worker(write_rx, read_tx, setup_tx, worker_options));
        });

        match setup_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                write_tx,
                read_rx,
                read_queue: VecDeque::new(),
                read_timeout,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(BleTransportError::SetupChannelClosed),
        }
    }
}

impl Read for BleTransport {
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
                        "timed out waiting for BLE data",
                    ),
                    mpsc::RecvTimeoutError::Disconnected => std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "ble transport disconnected",
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

impl Write for BleTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_tx.send(buf.to_vec()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "ble transport worker is not running",
            )
        })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn run_ble_worker(
    mut write_rx: UnboundedReceiver<Vec<u8>>,
    read_tx: mpsc::Sender<Vec<u8>>,
    setup_tx: mpsc::Sender<Result<(), BleTransportError>>,
    options: BleConnectOptions,
) -> Result<(), BleTransportError> {
    let service_uuid = Uuid::parse_str(BLE_SERVICE_UUID)?;
    let rpc_uuid = Uuid::parse_str(BLE_RPC_CHARACTERISTIC_UUID)?;

    let (peripheral, characteristic, write_type) =
        match connect_peripheral(service_uuid, rpc_uuid, &options).await {
            Ok(v) => v,
            Err(err) => {
                let _ = setup_tx.send(Err(err));
                return Ok(());
            }
        };

    if let Err(err) = peripheral.subscribe(&characteristic).await {
        let _ = setup_tx.send(Err(err.into()));
        return Ok(());
    }
    let mut notifications = match peripheral.notifications().await {
        Ok(stream) => stream,
        Err(err) => {
            let _ = setup_tx.send(Err(err.into()));
            return Ok(());
        }
    };
    let _ = setup_tx.send(Ok(()));

    loop {
        tokio::select! {
            maybe_notification = notifications.next() => {
                let Some(notification) = maybe_notification else {
                    break;
                };
                if notification.uuid == characteristic.uuid && read_tx.send(notification.value).is_err() {
                    break;
                }
            }
            maybe_write = write_rx.recv() => {
                let Some(data) = maybe_write else {
                    break;
                };
                if let Err(err) = peripheral.write(&characteristic, &data, write_type).await {
                    return Err(err.into());
                }
            }
        }
    }

    let _ = peripheral.disconnect().await;
    Ok(())
}

async fn connect_peripheral(
    service_uuid: Uuid,
    rpc_uuid: Uuid,
    options: &BleConnectOptions,
) -> Result<(Peripheral, Characteristic, WriteType), BleTransportError> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or(BleTransportError::NoAdapter)?;

    adapter
        .start_scan(ScanFilter {
            services: vec![service_uuid],
        })
        .await?;
    tokio::time::sleep(options.scan_timeout).await;

    let peripheral = select_peripheral(&adapter, service_uuid, options).await?;
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
    service_uuid: Uuid,
    options: &BleConnectOptions,
) -> Result<Peripheral, BleTransportError> {
    let peripherals = adapter.peripherals().await?;
    for peripheral in peripherals {
        let Some(props) = peripheral.properties().await? else {
            continue;
        };

        if !props.services.contains(&service_uuid) {
            continue;
        }

        if let Some(needle) = &options.name_contains {
            let Some(local_name) = props.local_name.as_deref() else {
                continue;
            };
            if !local_name.contains(needle) {
                continue;
            }
        }

        return Ok(peripheral);
    }

    Err(BleTransportError::NoMatchingPeripheral)
}
