use tokio::sync::mpsc;

use crate::models::LogEntry;

/// A command to write data to a writable source (e.g., UART TX).
#[derive(Debug)]
pub struct TxCommand {
    pub data: Vec<u8>,
    pub origin: String,
    /// Optional oneshot channel to acknowledge the write result.
    /// When set, the source sends `Ok(())` after a successful write
    /// or `Err(reason)` on failure.
    pub ack: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
}

/// A source of log entries (UART, UDP, file, etc.).
///
/// Implementors spawn their own I/O task and send [`LogEntry`] values
/// through the provided channel.  When the channel is closed the source
/// should exit cleanly.
#[async_trait::async_trait]
pub trait LogSource: Send + 'static {
    /// Read from the source and send log entries through `tx`.
    ///
    /// Returns `Ok(())` when the source is exhausted or the channel is closed.
    async fn run(self: Box<Self>, tx: mpsc::Sender<LogEntry>) -> anyhow::Result<()>;

    /// Config-level name of this source (e.g. "DUT_UART").
    fn source_name(&self) -> &str;

    /// Transport type (e.g. "uart", "udp", "file").
    fn source_type(&self) -> &str;

    /// Whether this source can accept TX writes (e.g., UART).
    fn writable(&self) -> bool {
        false
    }

    /// Attach a TX command receiver so this source can accept writes.
    /// Default implementation is a no-op for non-writable sources.
    fn set_tx_receiver(&mut self, _rx: mpsc::Receiver<TxCommand>) {}
}
