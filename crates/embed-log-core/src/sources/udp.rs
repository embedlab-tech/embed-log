use anyhow::Result;
use chrono::Local;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::traits::LogSource;
use crate::config::models::ParserConfig;
use crate::models::LogEntry;
use crate::parsers::create_parser;

/// Reads UDP datagrams, splits by newline, emits one [`LogEntry`] per line.
pub struct UdpSource {
    name: String,
    port: u16,
    parser: ParserConfig,
}

impl UdpSource {
    pub fn new(name: impl Into<String>, port: u16) -> Self {
        Self::new_with_parser(name, port, "text")
    }

    pub fn new_with_parser(
        name: impl Into<String>,
        port: u16,
        parser_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            port,
            parser: ParserConfig {
                parser_type: parser_type.into(),
                ..ParserConfig::default()
            },
        }
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
}

#[async_trait::async_trait]
impl LogSource for UdpSource {
    async fn run(self: Box<Self>, tx: mpsc::Sender<LogEntry>) -> Result<()> {
        let socket = UdpSocket::bind(("0.0.0.0", self.port)).await?;
        info!("[{}] UDP listening on :{}", self.name, self.port);

        let mut buf = vec![0u8; 65536];
        let mut parser = create_parser(&self.parser);
        let is_line_oriented_parser =
            matches!(self.parser.parser_type.as_str(), "text" | "hex-coap");

        loop {
            let (len, _addr) = match socket.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("[{}] UDP recv error: {e}", self.name);
                    continue;
                }
            };

            let lines = if is_line_oriented_parser {
                let mut datagram = Vec::with_capacity(len + 1);
                datagram.extend_from_slice(&buf[..len]);
                datagram.push(b'\n');
                parser.feed_entries(&datagram)
            } else {
                parser.feed_entries(&buf[..len])
            };

            for line in lines {
                let trimmed = line.display.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                let mut entry = LogEntry::new(Local::now(), self.name.clone(), trimmed.to_string());
                if let Some(raw) = line.raw {
                    entry = entry.with_raw_message(raw.trim_end().to_string());
                }
                if let Some(meta) = line.meta {
                    entry = entry.with_meta(meta);
                }
                if tx.send(entry).await.is_err() {
                    debug!("[{}] channel closed, stopping", self.name);
                    return Ok(());
                }
            }
        }
    }

    fn source_name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> &str {
        "udp"
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    fn free_udp_port() -> u16 {
        let socket = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        socket.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn text_udp_datagram_without_newline_emits_line() {
        let port = free_udp_port();
        let (tx, mut rx) = mpsc::channel(4);
        let handle = tokio::spawn(async move {
            let _ = Box::new(UdpSource::new("dut", port)).run(tx).await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let sender = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        sender
            .send_to(b"boot complete", ("127.0.0.1", port))
            .await
            .unwrap();

        let entry = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "dut");
        assert_eq!(entry.message, "boot complete");

        handle.abort();
    }

    #[tokio::test]
    async fn hex_coap_udp_datagram_without_newline_keeps_raw_and_decodes_display() {
        let port = free_udp_port();
        let (tx, mut rx) = mpsc::channel(4);
        let handle = tokio::spawn(async move {
            let source = UdpSource::new_with_parser("coap", port, "hex-coap");
            let _ = Box::new(source).run(tx).await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let sender = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        let raw = b"rx: 41 01 12 34 ab b4 74 65 73 74 rssi=-62";
        sender.send_to(raw, ("127.0.0.1", port)).await.unwrap();

        let entry = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(entry.message.contains("[COAP CON GET /test id=1234]"));
        assert_eq!(
            entry.raw_message.as_deref(),
            Some("rx: 41 01 12 34 ab b4 74 65 73 74 rssi=-62")
        );
        assert_eq!(entry.meta.as_ref().unwrap()["coap"]["uri"], "/test");

        handle.abort();
    }

    #[tokio::test]
    async fn cbor_udp_datagram_decodes_to_key_value_line() {
        let port = free_udp_port();
        let (tx, mut rx) = mpsc::channel(4);
        let handle = tokio::spawn(async move {
            let source = UdpSource::new_with_parser("sensors", port, "cbor-datagram");
            let _ = Box::new(source).run(tx).await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let value = ciborium::Value::Map(vec![(
            ciborium::Value::Text("temp".to_string()),
            ciborium::Value::Integer(25.into()),
        )]);
        let mut encoded = Vec::new();
        ciborium::into_writer(&value, &mut encoded).unwrap();

        let sender = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        sender.send_to(&encoded, ("127.0.0.1", port)).await.unwrap();

        let entry = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "sensors");
        assert!(entry.message.contains("temp=25"));

        handle.abort();
    }
}
