use std::io::{Read, Write};
use std::time::Duration;

const DEFAULT_BAUD_RATE: u32 = 12_500;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Debug)]
pub enum SerialTransportError {
    Open(serialport::Error),
    NoMatchingPort,
}

impl std::fmt::Display for SerialTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(err) => write!(f, "Failed to open serial port: {err}"),
            Self::NoMatchingPort => write!(f, "No matching serial port found"),
        }
    }
}

impl std::error::Error for SerialTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open(err) => Some(err),
            Self::NoMatchingPort => None,
        }
    }
}

impl From<serialport::Error> for SerialTransportError {
    fn from(value: serialport::Error) -> Self {
        Self::Open(value)
    }
}

pub struct SerialTransport {
    inner: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    pub fn open(path: &str) -> Result<Self, SerialTransportError> {
        Self::open_with(path, DEFAULT_BAUD_RATE, DEFAULT_READ_TIMEOUT)
    }

    fn open_with(
        path: &str,
        baud_rate: u32,
        timeout: Duration,
    ) -> Result<Self, SerialTransportError> {
        // Assert DTR on open: POSIX raises it as a side effect of opening the
        // tty, but Windows leaves it deasserted, and the reference client (Web
        // Serial) asserts it. Keeping the modem lines identical on every OS
        // removes a whole class of platform-specific firmware behavior.
        let mut port = serialport::new(path, baud_rate)
            .timeout(timeout)
            .dtr_on_open(true)
            .open()?;
        let _ = port.write_request_to_send(true);

        // Discard anything buffered by the OS from a previous session.
        let _ = port.clear(serialport::ClearBuffer::All);

        Ok(Self { inner: port })
    }
}

impl Read for SerialTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for SerialTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
