use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::traits::{LogSource, TxCommand};

/// Normalize UART TX for a Zephyr shell: CR-terminated lines (`\r`).
fn normalize_uart_shell_tx(data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    while matches!(v.last(), Some(b'\n' | b'\r')) {
        v.pop();
    }
    v.push(b'\r');
    v
}
use crate::config::models::ParserConfig;
use crate::models::LogEntry;
use crate::parsers::create_parser;

const UART_RECONNECT_DELAY: Duration = Duration::from_secs(1);
type SharedSerialPort = Arc<tokio::sync::Mutex<Option<Box<dyn serialport::SerialPort>>>>;

/// How the UART source obtains its serial port.
enum PortSource {
    /// Open by path + baud rate at startup.
    Path { port_path: String, baudrate: u32 },
    /// Use an already-opened serial port (e.g., for testing with PTYs).
    PreOpened { port: SharedSerialPort },
}

/// Reads bytes from a UART serial port, splits by newline, emits [`LogEntry`].
///
/// When a [`TxCommand`] channel is configured, the source will also accept TX
/// write requests: bytes are written to the serial port and a yellow
/// `TX::<origin>` log entry is recorded after successful write.
pub struct UartSource {
    name: String,
    parser: ParserConfig,
    port_source: PortSource,
    tx_rx: Option<mpsc::Receiver<TxCommand>>,
}

impl UartSource {
    pub fn new(name: impl Into<String>, port_path: impl Into<String>, baudrate: u32) -> Self {
        Self::new_with_parser(name, port_path, baudrate, "text")
    }

    pub fn new_with_parser(
        name: impl Into<String>,
        port_path: impl Into<String>,
        baudrate: u32,
        parser_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            parser: ParserConfig {
                parser_type: parser_type.into(),
                ..ParserConfig::default()
            },
            port_source: PortSource::Path {
                port_path: port_path.into(),
                baudrate,
            },
            tx_rx: None,
        }
    }

    /// Create a UART source from an already-opened serial port.
    ///
    /// Used in tests with PTY pairs; the caller provides a pre-configured
    /// `SerialPort` and the source takes ownership.
    pub fn from_port(
        name: impl Into<String>,
        port: Box<dyn serialport::SerialPort>,
        parser_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            parser: ParserConfig {
                parser_type: parser_type.into(),
                ..ParserConfig::default()
            },
            port_source: PortSource::PreOpened {
                port: Arc::new(tokio::sync::Mutex::new(Some(port))),
            },
            tx_rx: None,
        }
    }

    /// Attach a TX command receiver so this source can accept writes.
    pub fn with_tx_receiver(mut self, rx: mpsc::Receiver<TxCommand>) -> Self {
        self.tx_rx = Some(rx);
        self
    }

    pub fn with_parser(mut self, parser: ParserConfig) -> Self {
        self.parser = parser;
        self
    }

    /// Attach the `parser.database` path (used by e.g. `zephyr-dict`).
    pub fn with_parser_database(mut self, database: Option<String>) -> Self {
        self.parser.database = database;
        self
    }

    /// Open the configured path-backed serial port once.
    async fn open_path_port_once(&self) -> Result<Box<dyn serialport::SerialPort>> {
        match &self.port_source {
            PortSource::Path {
                port_path,
                baudrate,
            } => open_path_port(&self.name, port_path, *baudrate).await,
            PortSource::PreOpened { .. } => {
                anyhow::bail!("pre-opened UART source cannot be reopened by path")
            }
        }
    }

    /// Obtain the serial port — either by opening one or returning the pre-opened one.
    ///
    /// Path-backed ports retry forever so starting embed-log before plugging in a
    /// USB CDC device still works once the device appears.
    async fn get_port(&self) -> Result<SharedSerialPort> {
        match &self.port_source {
            PortSource::Path { .. } => loop {
                match self.open_path_port_once().await {
                    Ok(port) => return Ok(Arc::new(tokio::sync::Mutex::new(Some(port)))),
                    Err(e) => {
                        warn!(
                            "[{}] serial open failed: {e}; retrying in {:?}",
                            self.name, UART_RECONNECT_DELAY
                        );
                        tokio::time::sleep(UART_RECONNECT_DELAY).await;
                    }
                }
            },
            PortSource::PreOpened { port } => Ok(port.clone()),
        }
    }

    /// Reopen a path-backed UART and replace the shared port handle used by RX and TX.
    async fn reconnect_port(&self, port: &SharedSerialPort, reason: &str) -> Result<()> {
        match &self.port_source {
            PortSource::Path { .. } => {
                warn!(
                    "[{}] serial port disconnected ({reason}); reconnecting",
                    self.name
                );
                {
                    // Drop the stale file descriptor before trying to open the
                    // re-enumerated USB CDC device. This matters for exclusive
                    // serial opens and for devices that return with the same path.
                    let mut guard = port.lock().await;
                    *guard = None;
                }
                loop {
                    match self.open_path_port_once().await {
                        Ok(new_port) => {
                            let mut guard = port.lock().await;
                            *guard = Some(new_port);
                            info!("[{}] serial port reconnected", self.name);
                            return Ok(());
                        }
                        Err(e) => {
                            warn!(
                                "[{}] serial reconnect failed: {e}; retrying in {:?}",
                                self.name, UART_RECONNECT_DELAY
                            );
                            tokio::time::sleep(UART_RECONNECT_DELAY).await;
                        }
                    }
                }
            }
            PortSource::PreOpened { .. } => Ok(()),
        }
    }
}

#[async_trait::async_trait]
impl LogSource for UartSource {
    async fn run(self: Box<Self>, tx: mpsc::Sender<LogEntry>) -> Result<()> {
        let mut this = self;
        let name = this.name.clone();
        let parser_cfg = this.parser.clone();

        // Obtain the serial port (open by path or use pre-opened).
        let port = this.get_port().await?;

        let mut tx_rx = this.tx_rx.take();

        // Spawn a TX writer task if a command receiver was configured.
        if let Some(mut cmd_rx) = tx_rx.take() {
            let tx_name = name.clone();
            let entry_tx = tx.clone();
            let tx_port = port.clone();
            tokio::spawn(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    let origin = cmd.origin;
                    let data = normalize_uart_shell_tx(&cmd.data);
                    let data_len = data.len();
                    let data_for_write = data.clone();

                    // Lock the port, clone it, then write in a blocking thread.
                    let cloned = {
                        let guard = tx_port.lock().await;
                        match guard.as_ref() {
                            Some(port) => port.try_clone().map_err(|e| format!("clone error: {e}")),
                            None => Err("serial port is reconnecting".to_string()),
                        }
                    };

                    match cloned {
                        Ok(mut wp) => {
                            let result =
                                tokio::task::spawn_blocking(move || wp.write_all(&data_for_write))
                                    .await;

                            let ack_result = match &result {
                                Ok(Ok(())) => Ok(()),
                                Ok(Err(e)) => Err(format!("write error: {e}")),
                                Err(e) => Err(format!("spawn error: {e}")),
                            };

                            // Send ack if requested (for SDK tx.write oneshot).
                            if let Some(ack) = cmd.ack {
                                let _ = ack.send(ack_result.map_err(|e| e.to_string()));
                            }

                            match result {
                                Ok(Ok(())) => {
                                    let text = String::from_utf8_lossy(&data);
                                    let entry = LogEntry::new(
                                        Local::now(),
                                        format!("TX::{}", origin),
                                        text.to_string(),
                                    )
                                    .with_color("yellow");
                                    let _ = entry_tx.send(entry).await;
                                    info!(
                                        "[{tx_name}] TX wrote {} bytes from '{}'",
                                        data_len, origin
                                    );
                                }
                                Ok(Err(e)) => {
                                    error!("[{tx_name}] TX write error from '{}': {e}", origin);
                                }
                                Err(e) => {
                                    error!("[{tx_name}] TX spawn error from '{}': {e}", origin);
                                }
                            }
                        }
                        Err(e) => {
                            error!("[{tx_name}] TX unavailable for '{}': {e}", origin);
                            if let Some(ack) = cmd.ack {
                                let _ = ack.send(Err(e));
                            }
                        }
                    }
                }
            });
        }

        let mut parser = create_parser(&parser_cfg);

        loop {
            // Clone the port under the lock, then release before the blocking read.
            let cloned = {
                let guard = port.lock().await;
                match guard.as_ref() {
                    Some(port) => port.try_clone().map_err(|e| format!("clone error: {e}")),
                    None => Err("serial port is reconnecting".to_string()),
                }
            };
            // buf must be owned by the closure and returned, otherwise the bytes
            // read into it are dropped with the closure (arrays are Copy).
            let read_result = match cloned {
                Ok(mut port_clone) => {
                    tokio::task::spawn_blocking(move || {
                        let mut buf = [0u8; 4096];
                        match port_clone.read(&mut buf) {
                            Ok(n) => Ok((buf, n)),
                            Err(e) => Err(e),
                        }
                    })
                    .await?
                }
                Err(e) => {
                    this.reconnect_port(&port, &format!("clone error: {e}"))
                        .await?;
                    parser = create_parser(&parser_cfg);
                    continue;
                }
            };

            let (buf, n) = match read_result {
                Ok((buf, n)) => (buf, n),
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    continue;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => {
                    this.reconnect_port(&port, &format!("read error: {e}"))
                        .await?;
                    parser = create_parser(&parser_cfg);
                    continue;
                }
            };

            if n == 0 {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }

            for line in parser.feed(&buf[..n]) {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                let entry = LogEntry::new(Local::now(), name.clone(), trimmed.to_string());
                if tx.send(entry).await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    fn source_name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> &str {
        "uart"
    }

    fn writable(&self) -> bool {
        true
    }

    fn set_tx_receiver(&mut self, rx: mpsc::Receiver<TxCommand>) {
        self.tx_rx = Some(rx);
    }
}

async fn open_path_port(
    name: &str,
    port_path: &str,
    baudrate: u32,
) -> Result<Box<dyn serialport::SerialPort>> {
    let path = port_path.to_string();
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        info!("[{name}] opening serial port {path} @ {baudrate}");
        open_serial_with_fallback(&path, baudrate, &name)
    })
    .await?
}

/// Open a serial port, falling back to non-exclusive mode on platforms where
/// the standard open fails (e.g., macOS with PTY slaves where TIOCEXCL returns
/// ENOTTY).
#[cfg(unix)]
fn open_serial_with_fallback(
    path: &str,
    baud: u32,
    name: &str,
) -> anyhow::Result<Box<dyn serialport::SerialPort>> {
    use std::os::unix::io::FromRawFd;

    // Standard open (exclusive mode by default).
    match serialport::new(path, baud)
        .timeout(Duration::from_millis(100))
        .open()
    {
        Ok(port) => return Ok(port),
        Err(e) => {
            tracing::warn!("[{name}] standard open failed for {path}: {e}; trying non-exclusive");
        }
    }

    // Fallback: open with raw fd so we can skip exclusive access.
    // This is needed on macOS where PTY slaves reject TIOCEXCL.
    let c_path = std::ffi::CString::new(path)
        .map_err(|_| anyhow::anyhow!("[{name}] path contains null byte: {path}"))?;
    let fd = unsafe {
        let fd = libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        if fd < 0 {
            return Err(anyhow::anyhow!(
                "[{name}] failed to open serial port {path}: {}",
                std::io::Error::last_os_error()
            ));
        }
        fd
    };

    // Configure termios for raw binary serial access.
    unsafe {
        let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
            libc::close(fd);
            return Err(anyhow::anyhow!("[{name}] tcgetattr failed for {path}"));
        }
        let mut termios = termios.assume_init();
        libc::cfmakeraw(&mut termios);
        termios.c_cflag |= libc::CREAD | libc::CLOCAL;
        if libc::tcsetattr(fd, libc::TCSANOW, &termios) != 0 {
            libc::close(fd);
            return Err(anyhow::anyhow!("[{name}] tcsetattr failed for {path}"));
        }
    }

    // Wrap in TTYPort via FromRawFd.  This calls TIOCEXCL on best-effort;
    // if it fails (as on macOS PTYs), exclusive stays false and the port
    // remains usable.
    let port = unsafe { serialport::TTYPort::from_raw_fd(fd) };
    Ok(Box::new(port))
}

#[cfg(not(unix))]
fn open_serial_with_fallback(
    path: &str,
    baud: u32,
    name: &str,
) -> anyhow::Result<Box<dyn serialport::SerialPort>> {
    let _ = name;
    serialport::new(path, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| anyhow::anyhow!("[{name}] failed to open serial port {path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::TTYPort;
    use std::io::Read;
    use std::time::Duration;
    use tokio::time::timeout;

    /// Create a PTY pair using `serialport::TTYPort::pair()`.
    ///
    /// Returns (master TTYPort, slave TTYPort). The slave is passed to
    /// `UartSource::from_port()` so both sides stay open for the test.
    fn create_pty_pair() -> (TTYPort, TTYPort) {
        TTYPort::pair().expect("TTYPort::pair failed")
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn uart_tx_normalizes_lf_to_cr() {
        assert_eq!(super::normalize_uart_shell_tx(b"help\n"), b"help\r");
        assert_eq!(super::normalize_uart_shell_tx(b"version\r\n"), b"version\r");
        assert_eq!(super::normalize_uart_shell_tx(b"cmd"), b"cmd\r");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn uart_tx_writes_exact_bytes_to_pty_and_emits_yellow_tx_entry() {
        let (mut master, slave) = create_pty_pair();

        let (entry_tx, mut entry_rx) = mpsc::channel::<LogEntry>(8);
        let (tx_sender, tx_receiver) = mpsc::channel::<TxCommand>(4);

        let source =
            UartSource::from_port("dut", Box::new(slave), "text").with_tx_receiver(tx_receiver);

        let handle = tokio::spawn(async move {
            let _ = Box::new(source).run(entry_tx).await;
        });

        // Give the UART source time to start running.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Send a TX command with origin "ui".
        tx_sender
            .send(TxCommand {
                data: b"version\r\n".to_vec(),
                origin: "ui".to_string(),
                ack: None,
            })
            .await
            .expect("TX receiver dropped before send");

        // Give the TX writer time to process and write to the PTY slave.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Read from master side to verify exact bytes were written.
        let result = tokio::task::spawn_blocking(move || {
            let mut buf = [0u8; 32];
            let n = master.read(&mut buf);
            (buf, n)
        })
        .await
        .unwrap();
        let (master_buf, read_result) = result;
        let n = read_result.unwrap_or(0);
        assert_eq!(&master_buf[..n], b"version\r");

        // Verify a yellow TX::ui LogEntry was emitted.
        let entry = timeout(Duration::from_secs(2), entry_rx.recv())
            .await
            .expect("timeout waiting for TX log entry")
            .expect("channel closed before TX entry");
        assert_eq!(entry.source, "TX::ui");
        assert_eq!(entry.message, "version\r");
        assert_eq!(entry.color.as_deref(), Some("yellow"));

        handle.abort();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn uart_rx_reads_bytes_from_pty_and_emits_log_entry() {
        use std::io::Write;

        let (mut master, slave) = create_pty_pair();

        let (entry_tx, mut entry_rx) = mpsc::channel::<LogEntry>(8);
        let source = UartSource::from_port("dut", Box::new(slave), "text");

        let handle = tokio::spawn(async move {
            let _ = Box::new(source).run(entry_tx).await;
        });

        tokio::time::sleep(Duration::from_millis(300)).await;

        // Write a line into the master; the source reads it from the slave.
        // Keep master alive until after the entry arrives — dropping it closes
        // the PTY and can discard the slave's unread bytes.
        let master = tokio::task::spawn_blocking(move || {
            master.write_all(b"hello world\r\n").unwrap();
            master
        })
        .await
        .unwrap();

        let entry = timeout(Duration::from_secs(2), entry_rx.recv())
            .await
            .expect("timeout waiting for RX log entry")
            .expect("channel closed before RX entry");
        assert_eq!(entry.source, "dut");
        assert_eq!(entry.message, "hello world");

        drop(master);
        handle.abort();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn uart_tx_with_custom_origin_emits_tx_origin_entry() {
        let (mut master, slave) = create_pty_pair();

        let (entry_tx, mut entry_rx) = mpsc::channel::<LogEntry>(8);
        let (tx_sender, tx_receiver) = mpsc::channel::<TxCommand>(4);

        let source =
            UartSource::from_port("dut", Box::new(slave), "text").with_tx_receiver(tx_receiver);

        let handle = tokio::spawn(async move {
            let _ = Box::new(source).run(entry_tx).await;
        });

        tokio::time::sleep(Duration::from_millis(300)).await;

        tx_sender
            .send(TxCommand {
                data: b"status\n".to_vec(),
                origin: "pytest".to_string(),
                ack: None,
            })
            .await
            .expect("TX receiver dropped before send");

        // Give TX writer time to process
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Verify bytes on master
        let result = tokio::task::spawn_blocking(move || {
            let mut buf = [0u8; 32];
            let n = master.read(&mut buf);
            (buf, n)
        })
        .await
        .unwrap();
        let (master_buf, read_result) = result;
        let n = read_result.unwrap_or(0);
        assert_eq!(&master_buf[..n], b"status\r");

        // Verify TX::pytest log entry
        let entry = timeout(Duration::from_secs(2), entry_rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");
        assert_eq!(entry.source, "TX::pytest");
        assert_eq!(entry.message, "status\r");
        assert_eq!(entry.color.as_deref(), Some("yellow"));

        handle.abort();
    }
}
