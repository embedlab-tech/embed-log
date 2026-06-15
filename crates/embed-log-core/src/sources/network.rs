use anyhow::{bail, Result};
use chrono::Local;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::debug;

use super::traits::LogSource;
use crate::models::LogEntry;

/// Deterministic network-capture source.
///
/// The mock backend is intentionally available without privileges for tests and
/// demos. Real packet capture is not implemented in this Rust backend yet and
/// fails during startup instead of silently doing nothing.
pub struct NetworkCaptureSource {
    name: String,
    interface: String,
    bpf_filter: String,
    backend: String,
    interval: Duration,
}

impl NetworkCaptureSource {
    pub fn new(
        name: impl Into<String>,
        interface: impl Into<String>,
        bpf_filter: impl Into<String>,
        backend: impl Into<String>,
        mock_interval_secs: Option<f64>,
    ) -> Self {
        let interval = Duration::from_secs_f64(mock_interval_secs.unwrap_or(1.0).max(0.001));
        Self {
            name: name.into(),
            interface: interface.into(),
            bpf_filter: bpf_filter.into(),
            backend: backend.into(),
            interval,
        }
    }
}

#[async_trait::async_trait]
impl LogSource for NetworkCaptureSource {
    async fn run(self: Box<Self>, tx: mpsc::Sender<LogEntry>) -> Result<()> {
        if self.backend != "mock" {
            bail!(
                "network_capture backend {:?} is not implemented; use network_backend: mock",
                self.backend
            );
        }

        let mut seq: u64 = 0;
        let mut ticker = time::interval(self.interval);
        loop {
            ticker.tick().await;
            seq += 1;
            let message = format!(
                "network interface={} backend=mock seq={} filter={}",
                self.interface, seq, self.bpf_filter
            );
            let entry = LogEntry::new(Local::now(), self.name.clone(), message);
            if tx.send(entry).await.is_err() {
                debug!("[{}] channel closed, stopping", self.name);
                return Ok(());
            }
        }
    }

    fn source_name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> &str {
        "network_capture"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn mock_backend_emits_deterministic_network_events() {
        let (tx, mut rx) = mpsc::channel(2);
        let source = NetworkCaptureSource::new("net", "lo0", "udp", "mock", Some(0.001));
        let handle = tokio::spawn(async move {
            let _ = Box::new(source).run(tx).await;
        });

        let entry = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "net");
        assert!(entry.message.contains("interface=lo0"));
        assert!(entry.message.contains("backend=mock"));
        assert!(entry.message.contains("filter=udp"));

        handle.abort();
    }
}
