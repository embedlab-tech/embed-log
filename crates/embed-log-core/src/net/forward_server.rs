use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{info, warn};

/// TCP read-only forward server.
///
/// Connected clients receive a live raw line stream for this source only.  No
/// write capability — inbound bytes are ignored by the OS/socket layer until
/// disconnect.
pub struct ForwardServer {
    name: String,
    host: String,
    port: u16,
    broadcast_tx: broadcast::Sender<String>,
}

impl ForwardServer {
    pub fn new(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        broadcast_tx: broadcast::Sender<String>,
    ) -> Self {
        Self {
            name: name.into(),
            host: host.into(),
            port,
            broadcast_tx,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind((&*self.host, self.port)).await?;
        info!(
            "[{}] forward TCP listening on {}:{}",
            self.name, self.host, self.port
        );

        loop {
            let (mut stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    warn!("[{}] forward accept error: {e}", self.name);
                    continue;
                }
            };

            info!("[{}] forward client connected: {addr}", self.name);

            let name = self.name.clone();
            let mut rx = self.broadcast_tx.subscribe();

            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(payload) => {
                            let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload)
                            else {
                                continue;
                            };
                            if value.get("source_id").and_then(|v| v.as_str())
                                != Some(name.as_str())
                            {
                                continue;
                            }
                            let Some(data) = value.get("data").and_then(|v| v.as_str()) else {
                                continue;
                            };
                            if stream.write_all(data.as_bytes()).await.is_err()
                                || stream.write_all(b"\n").await.is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("[{name}] forward client lagged, skipped {n}");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                info!("[{name}] forward client disconnected: {addr}");
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::time::{timeout, Duration};

    fn free_tcp_port() -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn forwards_only_matching_source_raw_data() {
        let port = free_tcp_port();
        let (broadcast_tx, _rx) = broadcast::channel(8);
        let server = ForwardServer::new("dut", "127.0.0.1", port, broadcast_tx.clone());
        let handle = tokio::spawn(async move {
            let _ = server.run().await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
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
                "data": "hello",
            })
            .to_string(),
        );

        let line = timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(line, "hello");

        handle.abort();
    }
}
