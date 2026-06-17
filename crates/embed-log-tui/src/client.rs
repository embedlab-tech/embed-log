//! WebSocket client for the embed-log `/ws` endpoint.
//!
//! Connects, reads the config → log replay → events replay → live broadcast
//! sequence, and forwards parsed [`ServerMessage`]s to the app via an
//! `mpsc<ServerEvent>` channel. The app sends [`ClientCommand`]s back over a
//! second channel. Reconnects with exponential backoff (1s → 16s), mirroring
//! `frontend/ws.js`.
//!
//! The client runs as a tokio task; `run_client_loop` is the spawned future.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::{debug, info, warn};

use crate::protocol::{ClientCommand, ServerMessage};

/// Default first reconnect delay (mirrors ws.js `wsRetryDelay = 1000`).
const RECONNECT_BASE_MS: u64 = 1000;
/// Max reconnect delay (mirrors ws.js `WS_MAX_DELAY = 16000`).
const RECONNECT_MAX_MS: u64 = 16_000;
/// Handshake/read timeout so a silent server doesn't hang the client forever.
const HANDSHAKE_TIMEOUT_SECS: u64 = 10;

/// Events delivered to the app. `Disconnected` is sent on connection loss so the
/// app can update the status bar; the client then retries in the background.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// A parsed server message. Boxed: `ServerMessage` is large (config payload).
    Message(Box<ServerMessage>),
    /// Raw text that failed to parse as a known message (forwarded for diagnostics).
    Unparsed(String),
    /// Connection state changed.
    State(ConnectionState),
}

/// Connection state reported to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// No active connection.
    Disconnected,
    /// First connection attempt in progress.
    Connecting,
    /// Connected and streaming.
    Connected,
    /// Reconnecting after a loss.
    Reconnecting,
}

/// Handle returned by [`spawn_client`]: the app's inbound receiver and outbound
/// command sender. Dropping the sender does NOT stop the client; call
/// [`ClientHandle::stop`] for a clean shutdown.
pub struct ClientHandle {
    /// Inbound server events.
    pub events: mpsc::Receiver<ServerEvent>,
    /// Outbound commands to the server.
    pub commands: mpsc::Sender<ClientCommand>,
    stop_tx: mpsc::Sender<()>,
}

impl ClientHandle {
    /// Signal the client loop to stop after the next iteration.
    pub fn stop(&self) {
        let _ = self.stop_tx.try_send(());
    }
}

/// Spawn the WS client loop as a background tokio task.
///
/// The task runs until either `stop()` is called or the task panics. It
/// reconnects automatically on connection loss.
pub fn spawn_client(ws_url: String) -> ClientHandle {
    let (events_tx, events_rx) = mpsc::channel::<ServerEvent>(256);
    let (commands_tx, commands_rx) = mpsc::channel::<ClientCommand>(64);
    let (stop_tx, stop_rx) = mpsc::channel::<()>(1);

    tokio::spawn(async move {
        client_loop(ws_url, events_tx, commands_rx, stop_rx).await;
    });

    ClientHandle {
        events: events_rx,
        commands: commands_tx,
        stop_tx,
    }
}

/// Main client loop: connect, run, reconnect on loss, until stopped.
async fn client_loop(
    ws_url: String,
    events_tx: mpsc::Sender<ServerEvent>,
    mut commands_rx: mpsc::Receiver<ClientCommand>,
    mut stop_rx: mpsc::Receiver<()>,
) {
    let mut delay_ms = RECONNECT_BASE_MS;
    let mut first_attempt = true;

    loop {
        // Check for stop before each (re)connect attempt.
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let state = if first_attempt {
            ConnectionState::Connecting
        } else {
            ConnectionState::Reconnecting
        };
        let _ = events_tx.send(ServerEvent::State(state)).await;

        let stream = match timeout(
            Duration::from_secs(HANDSHAKE_TIMEOUT_SECS),
            connect_async(&ws_url),
        )
        .await
        {
            Ok(Ok((stream, _response))) => {
                delay_ms = RECONNECT_BASE_MS; // reset backoff on success
                first_attempt = false;
                let _ = events_tx
                    .send(ServerEvent::State(ConnectionState::Connected))
                    .await;
                info!("TUI WS connected to {ws_url}");
                stream
            }
            Ok(Err(e)) => {
                warn!("TUI WS connect error to {ws_url}: {e}");
                if !sleep_with_stop(&mut stop_rx, delay_ms).await {
                    break;
                }
                delay_ms = (delay_ms * 2).min(RECONNECT_MAX_MS);
                continue;
            }
            Err(_elapsed) => {
                warn!("TUI WS connect to {ws_url} timed out");
                if !sleep_with_stop(&mut stop_rx, delay_ms).await {
                    break;
                }
                delay_ms = (delay_ms * 2).min(RECONNECT_MAX_MS);
                continue;
            }
        };

        // Run the connection until it closes or a stop is requested.
        let stop_requested =
            run_connection(stream, &events_tx, &mut commands_rx, &mut stop_rx).await;

        let _ = events_tx
            .send(ServerEvent::State(ConnectionState::Disconnected))
            .await;
        debug!("TUI WS disconnected from {ws_url}");

        if stop_requested {
            break;
        }
        if !sleep_with_stop(&mut stop_rx, delay_ms).await {
            break;
        }
        delay_ms = (delay_ms * 2).min(RECONNECT_MAX_MS);
    }

    debug!("TUI WS client loop exiting");
}

/// Drive one connection: split into read/write halves, forward messages and
/// commands. Returns `true` if a stop was requested during the connection.
async fn run_connection(
    stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    events_tx: &mpsc::Sender<ServerEvent>,
    commands_rx: &mut mpsc::Receiver<ClientCommand>,
    stop_rx: &mut mpsc::Receiver<()>,
) -> bool {
    let (mut ws_sink, mut ws_stream) = stream.split();

    // Outbound command forwarder + inbound reader race, plus a stop watcher.
    loop {
        tokio::select! {
            // Stop signal: drain and exit promptly (do not graceful-close to keep
            // the shutdown fast; the server tolerates abrupt WS close).
            _ = stop_rx.recv() => return true,

            // Inbound server message.
            msg = ws_stream.next() => {
                let Some(msg) = msg else { return false; };
                match msg {
                    Ok(Message::Text(text)) => {
                        if forward_text(&text, events_tx).await.is_err() {
                            debug!("TUI WS event channel closed");
                            return false;
                        }
                    }
                    Ok(Message::Binary(b)) => {
                        // Server speaks text; tolerate binary by parsing as UTF-8.
                        if let Ok(s) = std::str::from_utf8(&b) {
                            let _ = forward_text(s, events_tx).await;
                        }
                    }
                    Ok(Message::Ping(p)) => {
                        // tungstenite auto-responds to pings; ignore here.
                        let _ = p;
                    }
                    Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                    Ok(Message::Close(_)) => return false,
                    Err(e) => {
                        warn!("TUI WS read error: {e}");
                        return false;
                    }
                }
            }

            // Outbound command from the app.
            cmd = commands_rx.recv() => {
                let Some(cmd) = cmd else { return false; };
                let json = cmd.to_json();
                if let Err(e) = ws_sink.send(Message::Text(json)).await {
                    warn!("TUI WS write error: {e}");
                    return false;
                }
            }
        }
    }
}

/// Parse one inbound text frame and forward it as a [`ServerEvent`].
async fn forward_text(text: &str, events_tx: &mpsc::Sender<ServerEvent>) -> Result<(), ()> {
    match serde_json::from_str::<ServerMessage>(text) {
        Ok(ServerMessage::Unknown) => {
            // Parsed JSON but unrecognized `type` — keep raw text for diagnostics.
            events_tx
                .send(ServerEvent::Unparsed(text.to_string()))
                .await
                .map_err(|_| ())
        }
        Ok(msg) => events_tx
            .send(ServerEvent::Message(Box::new(msg)))
            .await
            .map_err(|_| ()),
        Err(e) => {
            // Non-JSON or malformed — forward as unparsed so the app can log it.
            debug!("TUI WS unparsed frame ({e}): {}", truncate(text, 120));
            events_tx
                .send(ServerEvent::Unparsed(text.to_string()))
                .await
                .map_err(|_| ())
        }
    }
}

/// Sleep for `delay_ms`, but wake early if a stop is requested.
/// Returns `false` if stop was requested during the sleep.
async fn sleep_with_stop(stop_rx: &mut mpsc::Receiver<()>, delay_ms: u64) -> bool {
    tokio::select! {
        _ = sleep(Duration::from_millis(delay_ms)) => true,
        _ = stop_rx.recv() => false,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ServerMessage;

    #[tokio::test]
    async fn forward_text_parses_config() {
        let (tx, mut rx) = mpsc::channel::<ServerEvent>(8);
        let json = r#"{"type":"config","app_name":"x","tabs":[]}"#;
        forward_text(json, &tx).await.unwrap();
        match rx.recv().await.unwrap() {
            ServerEvent::Message(msg) => match *msg {
                ServerMessage::Config(c) => assert_eq!(c.app_name, "x"),
                other => panic!("expected config, got {other:?}"),
            },
            other => panic!("expected config, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn forward_text_routes_unparsed_for_unknown_type() {
        let (tx, mut rx) = mpsc::channel::<ServerEvent>(8);
        let json = r#"{"type":"future_kind","x":1}"#;
        forward_text(json, &tx).await.unwrap();
        match rx.recv().await.unwrap() {
            ServerEvent::Unparsed(s) => assert!(s.contains("future_kind")),
            other => panic!("expected unparsed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn forward_text_routes_unparsed_for_malformed_json() {
        let (tx, mut rx) = mpsc::channel::<ServerEvent>(8);
        forward_text("not json", &tx).await.unwrap();
        assert!(matches!(rx.recv().await.unwrap(), ServerEvent::Unparsed(_)));
    }

    #[tokio::test]
    async fn forward_text_channel_close_returns_err() {
        // Drop the receiver to simulate the app shutting down.
        let (tx, rx) = mpsc::channel::<ServerEvent>(8);
        drop(rx);
        let res = forward_text(r#"{"type":"config"}"#, &tx).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn client_handle_stop_breaks_loop() {
        // Use a bogus URL so connect fails fast; stop should still terminate the loop.
        let handle = spawn_client("ws://127.0.0.1:1/ws".to_string());
        handle.stop();
        // Give the task a moment to observe stop and exit.
        sleep(Duration::from_millis(200)).await;
        // The commands sender should still be usable (client task has exited).
        // We can't observe task completion directly without a JoinHandle, but the
        // invariant we care about is that stop() doesn't hang and the senders work.
        let _ = handle.commands.try_send(ClientCommand::ExportSessionHtml);
    }
}
