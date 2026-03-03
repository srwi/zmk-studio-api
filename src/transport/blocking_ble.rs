use std::collections::VecDeque;
use std::future::Future;
use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use futures::{Stream, StreamExt};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{Receiver as TokioReceiver, Sender as TokioSender};

pub(crate) trait BleWorkerBackend: Send + 'static {
    type ConnectArg: Send + 'static;
    type Error: std::error::Error + Send + Sync + 'static;
    type Notifications<'a>: Stream<Item = Result<Vec<u8>, Self::Error>> + Send + Unpin + 'a
    where
        Self: 'a;

    fn connect(
        connect_arg: Self::ConnectArg,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Error>> + Send>>
    where
        Self: Sized;

    fn notifications<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Notifications<'a>, Self::Error>> + Send + 'a>>;

    fn write_packet<'a>(
        &'a self,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'a>>;

    fn shutdown<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

pub(crate) struct BlockingBleTransport {
    write_tx: TokioSender<Vec<u8>>,
    read_rx: Receiver<Vec<u8>>,
    read_queue: VecDeque<u8>,
    read_timeout: Duration,
}

impl BlockingBleTransport {
    pub(crate) fn connect<B>(
        connect_arg: B::ConnectArg,
        write_queue_capacity: usize,
        read_timeout: Duration,
        map_runtime_init: fn(std::io::Error) -> B::Error,
        map_setup_closed: fn() -> B::Error,
    ) -> Result<Self, B::Error>
    where
        B: BleWorkerBackend,
    {
        let (write_tx, write_rx) =
            tokio::sync::mpsc::channel::<Vec<u8>>(write_queue_capacity.max(1));
        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>();
        let (setup_tx, setup_rx) = mpsc::channel::<Result<(), B::Error>>();

        thread::spawn(move || {
            let runtime = match Runtime::new() {
                Ok(rt) => rt,
                Err(err) => {
                    let _ = setup_tx.send(Err(map_runtime_init(err)));
                    return;
                }
            };

            runtime.block_on(run_worker::<B>(connect_arg, write_rx, read_tx, setup_tx));
        });

        match setup_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                write_tx,
                read_rx,
                read_queue: VecDeque::new(),
                read_timeout,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(map_setup_closed()),
        }
    }
}

async fn run_worker<B: BleWorkerBackend>(
    connect_arg: B::ConnectArg,
    mut write_rx: TokioReceiver<Vec<u8>>,
    read_tx: mpsc::Sender<Vec<u8>>,
    setup_tx: mpsc::Sender<Result<(), B::Error>>,
) {
    let backend = match B::connect(connect_arg).await {
        Ok(backend) => backend,
        Err(err) => {
            let _ = setup_tx.send(Err(err));
            return;
        }
    };

    let mut notifications = match backend.notifications().await {
        Ok(notifications) => notifications,
        Err(err) => {
            backend.shutdown().await;
            let _ = setup_tx.send(Err(err));
            return;
        }
    };

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
                        if backend.write_packet(&data).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    backend.shutdown().await;
}

impl Read for BlockingBleTransport {
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
                        "Timed out waiting for BLE data",
                    ),
                    mpsc::RecvTimeoutError::Disconnected => std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "BLE transport disconnected",
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

impl Write for BlockingBleTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_tx.blocking_send(buf.to_vec()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "BLE transport worker is not running",
            )
        })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
