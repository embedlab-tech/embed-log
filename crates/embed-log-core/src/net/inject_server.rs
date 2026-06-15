use chrono::Local;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};

use crate::models::LogEntry;

/// TCP server for bidirectional log inject/stream.
///
/// Clients can:
/// - Send JSON lines to inject log entries or TX commands
/// - Receive a live raw-line stream for this source
pub struct InjectServer {
    name: String,
    host: String,
    port: u16,
    entry_tx: mpsc::Sender<LogEntry>,
    broadcast_tx: broadcast::Sender<String>,
}

impl InjectServer {
    pub fn new(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        entry_tx: mpsc::Sender<LogEntry>,
        broadcast_tx: broadcast::Sender<String>,
    ) -> Self {
        Self {
            name: name.into(),
            host: host.into(),
            port,
            entry_tx,
            broadcast_tx,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind((&*self.host, self.port)).await?;
        info!(
            "[{}] inject TCP listening on {}:{}",
            self.name, self.host, self.port
        );

        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    warn!("[{}] inject accept error: {e}", self.name);
                    continue;
                }
            };

            info!("[{}] inject client connected: {addr}", self.name);

            let name = self.name.clone();
            let entry_tx = self.entry_tx.clone();
            let rx = self.broadcast_tx.subscribe();

            tokio::spawn(async move {
                if let Err(e) = handle_inject_client(stream, name, entry_tx, rx).await {
                    error!("[inject] client error: {e}");
                }
            });
        }
    }
}

async fn handle_inject_client(
    stream: TcpStream,
    name: String,
    entry_tx: mpsc::Sender<LogEntry>,
    mut rx: broadcast::Receiver<String>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let stream_name = name.clone();

    let writer_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(payload) => {
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else {
                        continue;
                    };
                    if value.get("source_id").and_then(|v| v.as_str()) != Some(stream_name.as_str())
                    {
                        continue;
                    }
                    let Some(data) = value.get("data").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if writer.write_all(data.as_bytes()).await.is_err()
                        || writer.write_all(b"\n").await.is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Try to parse as JSON command.
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("log");
            match msg_type {
                "tx" => {
                    let data = msg.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    if data.is_empty() {
                        continue;
                    }
                    let entry = LogEntry::new(Local::now(), "TX::UI".to_string(), data.to_string())
                        .with_color("yellow");
                    let _ = entry_tx.send(entry).await;
                }
                _ => {
                    let source = msg
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&name)
                        .to_string();
                    let message = msg
                        .get("message")
                        .or_else(|| msg.get("data"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(trimmed)
                        .to_string();
                    let color = msg
                        .get("color")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let mut entry = LogEntry::new(Local::now(), source, message);
                    if let Some(c) = color {
                        entry = entry.with_color(c);
                    }
                    let _ = entry_tx.send(entry).await;
                }
            }
        } else {
            // Plain text — inject as a line for this source.
            let entry = LogEntry::new(Local::now(), name.clone(), trimmed.to_string());
            let _ = entry_tx.send(entry).await;
        }
    }

    writer_task.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::{timeout, Duration};

    fn free_tcp_port() -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn json_tx_records_yellow_ui_tx_entry() {
        let port = free_tcp_port();
        let (entry_tx, mut entry_rx) = mpsc::channel(4);
        let (broadcast_tx, _rx) = broadcast::channel(4);
        let server = InjectServer::new("dut", "127.0.0.1", port, entry_tx, broadcast_tx);
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream
            .write_all(
                br#"{"type":"tx","data":"help"}
"#,
            )
            .await
            .unwrap();

        let entry = timeout(Duration::from_secs(2), entry_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "TX::UI");
        assert_eq!(entry.message, "help");
        assert_eq!(entry.color.as_deref(), Some("yellow"));

        handle.abort();
    }

    #[tokio::test]
    async fn client_receives_raw_stream_for_source_only() {
        let port = free_tcp_port();
        let (entry_tx, _entry_rx) = mpsc::channel(4);
        let (broadcast_tx, _rx) = broadcast::channel(8);
        let server = InjectServer::new("dut", "127.0.0.1", port, entry_tx, broadcast_tx.clone());
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut lines = BufReader::new(stream).lines();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = broadcast_tx.send(
            serde_json::json!({
                "type": "rx",
                "source_id": "other",
                "data": "skip",
            })
            .to_string(),
        );
        let _ = broadcast_tx.send(
            serde_json::json!({
                "type": "rx",
                "source_id": "dut",
                "data": "boot",
            })
            .to_string(),
        );

        let line = timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(line, "boot");

        handle.abort();
    }
}
