use std::collections::HashSet;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::StreamExt;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::warn;

use crate::models::LogEntry;
use crate::sources::TxCommand;

/// Source metadata reported in the `hello.result` response.
#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    #[serde(rename = "type")]
    pub source_type: String,
    pub label: String,
    pub writable: bool,
}

/// Handler for `GET /api/v1/control`.
pub async fn control_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<super::ServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_control_client(socket, state))
}

/// Per-client subscription state.
///
/// - `sources`: source names subscribed to for `log.entry` messages.
///   Empty set means no log entries are forwarded.
/// - `events`: whether `type: "event"` messages are forwarded.
#[derive(Clone, Default)]
pub struct ControlSubscription {
    pub sources: HashSet<String>,
    pub events: bool,
}

/// Determine whether a broadcast message should be forwarded to a control client.
pub fn should_forward_to_control_client(
    parsed: &serde_json::Value,
    sub: &ControlSubscription,
) -> bool {
    let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match msg_type {
        // Always forward these.
        "markers_update" | "session_info" => true,
        // Forward events only if subscribed.
        "event" => sub.events,
        // Forward rx/tx as log.entry only if source is subscribed.
        "rx" | "tx" => {
            let source_id = match parsed.get("source_id").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return false,
            };
            sub.sources.contains(source_id)
        }
        _ => false,
    }
}

impl ControlSubscription {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, source: &str) -> bool {
        self.sources.contains(source)
    }

    pub fn insert(&mut self, source: String) {
        self.sources.insert(source);
    }

    pub fn remove(&mut self, source: &str) {
        self.sources.remove(source);
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }
}

impl<const N: usize> From<[String; N]> for ControlSubscription {
    fn from(arr: [String; N]) -> Self {
        Self {
            sources: HashSet::from(arr),
            events: false,
        }
    }
}

/// Run the control protocol for one connected client.
async fn handle_control_client(mut socket: WebSocket, state: super::ServerState) {
    let mut rx = state.broadcast_tx.subscribe();
    let mut subscribed_sources: ControlSubscription = ControlSubscription::new();

    loop {
        tokio::select! {
            // Incoming messages from the client
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let response = handle_control_command(&text, &state, &mut subscribed_sources).await;
                        if let Some(resp) = response {
                            if let Err(e) = socket.send(Message::Text(resp.into())).await {
                                warn!("control WS send error: {e}");
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }

            // Incoming log entries from the broadcast channel
            result = rx.recv() => {
                match result {
                    Ok(payload_str) => {
                        // Parse the broadcast message
                        let parsed: serde_json::Value = match serde_json::from_str(&payload_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        // Use the shared forward decision function.
                        if !should_forward_to_control_client(&parsed, &subscribed_sources) {
                            continue;
                        }

                        let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

                        // markers_update and session_info are forwarded as-is.
                        if msg_type == "markers_update" || msg_type == "session_info" {
                            if let Err(e) = socket.send(Message::Text(payload_str.clone().into())).await {
                                warn!("control WS send error: {e}");
                                break;
                            }
                            continue;
                        }

                        // Event messages are forwarded as-is.
                        if msg_type == "event" {
                            if let Err(e) = socket.send(Message::Text(payload_str.clone().into())).await {
                                warn!("control WS send error: {e}");
                                break;
                            }
                            continue;
                        }

                        // rx/tx messages are forwarded as structured log.entry events.
                        let source_id = match parsed.get("source_id").and_then(|v| v.as_str()) {
                            Some(s) => s.to_string(),
                            None => continue,
                        };
                        let entry = build_log_entry(&parsed, &source_id);
                        let entry_str = match serde_json::to_string(&entry) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };

                        if let Err(e) = socket.send(Message::Text(entry_str.into())).await {
                            warn!("control WS send error: {e}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // Notify the control client in-band so the gap isn't silent.
                        // ponytail: broadcast capacity is the tuning knob if lag is common.
                        warn!("control WS client lagged, skipped {n} messages");
                        let notice = format!("{{\"type\":\"stream_gap\",\"skipped\":{n}}}");
                        if let Err(e) = socket.send(Message::Text(notice.into())).await {
                            warn!("control WS send error: {e}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

/// Build a `log.entry` event from a broadcast payload.
///
/// The broadcast now carries these structured fields added by the writer:
/// - `origin` (String): the origin string ("SERIAL", "TX::<origin>", or injected origin)
/// - `color` (Option<String>): the color name or null
/// - `message` (String): the raw logical message (no ANSI wrapping)
/// - `line_idx` (u64): stable per-source line counter
fn build_log_entry(parsed: &serde_json::Value, source_id: &str) -> serde_json::Value {
    let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("rx");
    let is_tx = msg_type == "tx";

    // Use structured fields from the broadcast if available.
    let origin = parsed
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or(if is_tx { "ui" } else { "SERIAL" })
        .to_string();

    let message = parsed
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let timestamp_iso = parsed
        .get("timestamp_iso")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let line_idx = parsed.get("line_idx").and_then(|v| v.as_u64()).unwrap_or(0);

    let color = parsed
        .get("color")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "type": "log.entry",
        "source_id": source_id,
        "origin": origin,
        "message": message,
        "timestamp_iso": timestamp_iso,
        "line_idx": line_idx,
        "color": color,
        "is_tx": is_tx,
    })
}

/// Handle a single command from a control client.
async fn handle_control_command(
    text: &str,
    state: &super::ServerState,
    subscribed_sources: &mut ControlSubscription,
) -> Option<String> {
    let cmd: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            return Some(
                serde_json::json!({
                    "type": "error",
                    "error": format!("invalid JSON: {e}"),
                })
                .to_string(),
            );
        }
    };

    let cmd_type = match cmd.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return Some(
                serde_json::json!({
                    "type": "error",
                    "error": "missing 'type' field",
                })
                .to_string(),
            );
        }
    };

    let msg_id = cmd.get("id").and_then(|v| v.as_str());

    match cmd_type {
        "hello" => Some(handle_hello(state, msg_id)),
        "subscribe" => Some(handle_subscribe(&cmd, state, subscribed_sources, msg_id)),
        "unsubscribe" => Some(handle_unsubscribe(&cmd, subscribed_sources, msg_id)),
        "log.inject" => Some(handle_log_inject(&cmd, state, msg_id).await),
        "tx.write" => Some(handle_tx_write(&cmd, state, msg_id).await),
        "marker.create" => Some(handle_marker_create(&cmd, state, msg_id).await),
        _ => {
            let mut resp = serde_json::json!({
                "type": "error",
                "error": format!("unknown command: {cmd_type}"),
            });
            if let Some(id) = msg_id {
                resp["id"] = serde_json::Value::String(id.to_string());
            }
            Some(resp.to_string())
        }
    }
}

/// Build a success response with the given type and optional id/data.
fn make_response(resp_type: &str, msg_id: Option<&str>, data: serde_json::Value) -> String {
    let mut resp = serde_json::json!({});
    if let Some(id) = msg_id {
        resp["id"] = serde_json::Value::String(id.to_string());
    }
    resp["type"] = serde_json::Value::String(resp_type.to_string());
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            resp[k] = v.clone();
        }
    }
    resp.to_string()
}

/// Build a `type.result` error response for application-level failures.
/// These use the command's result type (e.g. `tx.result`) with `ok: false`
/// so SDK callers get a uniform response shape.
fn make_result_error(
    resp_type: &str,
    msg_id: Option<&str>,
    source_id: &str,
    error: &str,
) -> String {
    let mut resp = serde_json::json!({});
    if let Some(id) = msg_id {
        resp["id"] = serde_json::Value::String(id.to_string());
    }
    resp["type"] = serde_json::Value::String(format!("{}.result", resp_type));
    resp["ok"] = serde_json::Value::Bool(false);
    resp["source_id"] = serde_json::Value::String(source_id.to_string());
    resp["error"] = serde_json::Value::String(error.to_string());
    resp.to_string()
}

/// Handle `hello` — return source metadata and session info.
fn handle_hello(state: &super::ServerState, msg_id: Option<&str>) -> String {
    let sources: serde_json::Value = state
        .source_metadata
        .iter()
        .map(|(name, info)| {
            (
                name.clone(),
                serde_json::json!({
                    "type": info.source_type,
                    "label": info.label,
                    "writable": info.writable,
                }),
            )
        })
        .collect();

    let session_id = state
        .session_manager
        .as_ref()
        .and_then(|mgr| mgr.lock().ok())
        .map(|mgr| mgr.session_id().to_string())
        .unwrap_or_default();

    let data = serde_json::json!({
        "sources": sources,
        "session": {
            "id": session_id,
        },
    });

    make_response("hello.result", msg_id, data)
}

/// Handle `subscribe` — add sources to the subscription filter.
fn handle_subscribe(
    cmd: &serde_json::Value,
    state: &super::ServerState,
    subscribed_sources: &mut ControlSubscription,
    msg_id: Option<&str>,
) -> String {
    let wants_events = cmd.get("events").and_then(|v| v.as_bool()).unwrap_or(false);

    let source_names: Vec<String> = cmd
        .get("sources")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    // Require sources unless events-only subscription.
    if source_names.is_empty() && !wants_events {
        return serde_json::json!({
            "type": "error",
            "error": "missing or empty 'sources' array (provide sources or set events:true)",
        })
        .to_string();
    }

    // Validate that all sources exist
    for name in &source_names {
        if !state.source_metadata.contains_key(name) {
            return serde_json::json!({
                "type": "error",
                "error": format!("unknown source: {name}"),
            })
            .to_string();
        }
    }

    for name in source_names {
        subscribed_sources.insert(name);
    }

    if wants_events {
        subscribed_sources.events = true;
    }

    make_response("subscribe.result", msg_id, serde_json::json!({}))
}

/// Handle `unsubscribe` — remove sources from the subscription filter.
fn handle_unsubscribe(
    cmd: &serde_json::Value,
    subscribed_sources: &mut ControlSubscription,
    msg_id: Option<&str>,
) -> String {
    let sources = match cmd.get("sources").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing or invalid 'sources' array",
            })
            .to_string();
        }
    };

    let source_names: Vec<String> = sources
        .iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();

    for name in &source_names {
        subscribed_sources.remove(name);
    }

    // Unsubscribe from events if requested.
    if let Some(events) = cmd.get("events").and_then(|v| v.as_bool()) {
        if !events {
            subscribed_sources.events = false;
        }
    }

    make_response("unsubscribe.result", msg_id, serde_json::json!({}))
}

/// Handle `log.inject` — inject a log entry into the pipeline.
async fn handle_log_inject(
    cmd: &serde_json::Value,
    state: &super::ServerState,
    msg_id: Option<&str>,
) -> String {
    let source_id = match cmd.get("source_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing 'source_id'",
            })
            .to_string();
        }
    };

    let message = match cmd.get("message").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing 'message'",
            })
            .to_string();
        }
    };

    let origin = cmd.get("origin").and_then(|v| v.as_str()).unwrap_or("sdk");

    let color = cmd.get("color").and_then(|v| v.as_str());

    // Verify source exists
    if !state.source_metadata.contains_key(source_id) {
        return make_result_error(
            "log.inject",
            msg_id,
            source_id,
            &format!("unknown source: {source_id}"),
        );
    }

    // Find the entry_tx for this source
    if let Some(tx) = state.source_txs.get(source_id) {
        let entry = LogEntry::new(
            chrono::Local::now(),
            origin.to_string(),
            message.to_string(),
        );
        let entry = if let Some(c) = color {
            entry.with_color(c)
        } else {
            entry
        };

        match tx.send(entry).await {
            Ok(()) => make_response("log.inject.result", msg_id, serde_json::json!({})),
            Err(_) => make_result_error("log.inject", msg_id, source_id, "source queue closed"),
        }
    } else {
        make_result_error(
            "log.inject",
            msg_id,
            source_id,
            &format!("unknown source: {source_id}"),
        )
    }
}

/// Handle `tx.write` — write bytes to a writable source (UART).
///
/// Uses a oneshot acknowledgement channel so the response reports
/// actual write success/failure, not just "queued".
///
/// All application-level failures return `tx.result` with `ok: false`
/// so SDK callers get a uniform response shape.
async fn handle_tx_write(
    cmd: &serde_json::Value,
    state: &super::ServerState,
    msg_id: Option<&str>,
) -> String {
    let source_id = match cmd.get("source_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing 'source_id'",
            })
            .to_string();
        }
    };

    let data = match cmd.get("data").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing 'data'",
            })
            .to_string();
        }
    };

    let origin = cmd.get("origin").and_then(|v| v.as_str()).unwrap_or("sdk");

    if data.is_empty() {
        return make_result_error("tx", msg_id, source_id, "empty data");
    }

    // Try TX sender (writable sources like UART).
    if let Some(tx_sender) = state.source_tx_senders.get(source_id) {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

        let cmd = TxCommand {
            data: data.as_bytes().to_vec(),
            origin: origin.to_string(),
            ack: Some(ack_tx),
        };

        match tx_sender.send(cmd).await {
            Ok(()) => {
                // Wait for the UART source to acknowledge the write.
                match ack_rx.await {
                    Ok(Ok(())) => {
                        let data = serde_json::json!({
                            "ok": true,
                            "source_id": source_id,
                            "bytes": data.len(),
                        });
                        make_response("tx.result", msg_id, data)
                    }
                    Ok(Err(e)) => make_result_error("tx", msg_id, source_id, &e),
                    Err(_) => make_result_error(
                        "tx",
                        msg_id,
                        source_id,
                        "source writer dropped without response",
                    ),
                }
            }
            Err(_) => make_result_error("tx", msg_id, source_id, "source write channel closed"),
        }
    } else if state.source_metadata.contains_key(source_id) {
        make_result_error("tx", msg_id, source_id, "source is not writable")
    } else {
        make_result_error(
            "tx",
            msg_id,
            source_id,
            &format!("unknown source: {source_id}"),
        )
    }
}

/// Handle `marker.create` — create a marker on the current session.
async fn handle_marker_create(
    cmd: &serde_json::Value,
    state: &super::ServerState,
    msg_id: Option<&str>,
) -> String {
    let source_id = match cmd.get("source_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing 'source_id'",
            })
            .to_string();
        }
    };

    let description = match cmd.get("description").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing 'description'",
            })
            .to_string();
        }
    };

    let line_idx = match cmd.get("line_idx").and_then(|v| v.as_i64()) {
        Some(n) if n >= 0 => n as u64,
        Some(_) => {
            return serde_json::json!({
                "type": "error",
                "error": "'line_idx' must be non-negative",
            })
            .to_string();
        }
        None => {
            return serde_json::json!({
                "type": "error",
                "error": "missing or invalid 'line_idx'",
            })
            .to_string();
        }
    };

    let origin = cmd
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or("watcher");

    // Validate source exists
    if !state.source_metadata.contains_key(source_id) {
        return make_result_error(
            "marker",
            msg_id,
            source_id,
            &format!("unknown source: {source_id}"),
        );
    }

    // Validate line_idx is within the known range for this source
    if let Some(counter) = state.line_counters.get(source_id) {
        let count = counter.load(std::sync::atomic::Ordering::Relaxed);
        if line_idx >= count {
            return make_result_error(
                "marker",
                msg_id,
                source_id,
                &format!(
                    "line_idx {line_idx} out of range (source '{source_id}' has {count} lines)"
                ),
            );
        }
    }

    // Resolve numTs (millisecond timestamp) for the marked line
    // Priority: 1) explicit timestamp_num in request, 2) lookup in replay buffer
    let num_ts = if let Some(ts) = cmd.get("timestamp_num").and_then(|v| v.as_f64()) {
        ts
    } else {
        match lookup_timestamp_in_replay(&state.replay, source_id, line_idx) {
            Some(ts) => ts,
            None => {
                return make_result_error(
                    "marker", msg_id, source_id,
                    "cannot resolve timestamp for this line; provide 'timestamp_num' or ensure the line is in the replay buffer",
                );
            }
        }
    };

    // Get session manager to persist markers
    let session_manager = match &state.session_manager {
        Some(mgr) => mgr,
        None => {
            return make_result_error("marker", msg_id, source_id, "session manager unavailable");
        }
    };

    let result = {
        let mgr = match session_manager.lock() {
            Ok(mgr) => mgr,
            Err(_) => {
                return make_result_error(
                    "marker",
                    msg_id,
                    source_id,
                    "session manager lock failed",
                );
            }
        };

        // Create new marker in frontend-compatible format.
        let now = chrono::Local::now();
        let new_marker = serde_json::json!({
            "paneId": source_id,
            "lineIdx": line_idx,
            "endIdx": line_idx,
            "numTs": num_ts,
            "description": description,
            "createdAt": now.to_rfc3339(),
            "origin": origin,
        });

        // Replace any existing marker at this (paneId, lineIdx) and persist.
        match mgr.replace_marker(source_id, line_idx, new_marker, false) {
            Ok(markers) => {
                let broadcast_payload = serde_json::json!({
                    "type": "markers_update",
                    "markers": markers,
                    "session": mgr.build_session_info(),
                });
                let _ = state.broadcast_tx.send(broadcast_payload.to_string());

                let resp_data = serde_json::json!({
                    "ok": true,
                    "source_id": source_id,
                });
                Ok(make_response("marker.result", msg_id, resp_data))
            }
            Err(e) => Err(make_result_error(
                "marker",
                msg_id,
                source_id,
                &format!("failed to save markers: {e}"),
            )),
        }
    };

    match result {
        Ok(resp) => resp,
        Err(resp) => resp,
    }
}

/// Search the replay buffer for a broadcast payload matching `source_id` and `line_idx`.
/// Returns the `timestamp_num` value if found, or `None`.
fn lookup_timestamp_in_replay(
    replay: &std::sync::Mutex<std::collections::VecDeque<String>>,
    source_id: &str,
    line_idx: u64,
) -> Option<f64> {
    let buf = replay.lock().ok()?;
    for entry_str in buf.iter() {
        let parsed: serde_json::Value = serde_json::from_str(entry_str).ok()?;
        let same_source = parsed.get("source_id").and_then(|v| v.as_str()) == Some(source_id);
        let same_idx = parsed.get("line_idx").and_then(|v| v.as_u64()) == Some(line_idx);
        if same_source && same_idx {
            return parsed.get("timestamp_num").and_then(|v| v.as_f64());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::ws_server::ServerState;
    use crate::session::SessionManager;
    use crate::sources::TxCommand;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::{broadcast, mpsc};

    fn temp_session_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let dir = std::env::temp_dir().join(format!(
            "embed-log-control-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_control_state() -> ServerState {
        let (broadcast_tx, _) = broadcast::channel(64);
        let mut source_metadata = HashMap::new();
        source_metadata.insert(
            "DUT_UART".to_string(),
            SourceInfo {
                source_type: "uart".to_string(),
                label: "DUT".to_string(),
                writable: true,
            },
        );
        source_metadata.insert(
            "PYTEST".to_string(),
            SourceInfo {
                source_type: "udp".to_string(),
                label: "Pytest".to_string(),
                writable: false,
            },
        );
        let (entry_tx, _entry_rx) = mpsc::channel(8);
        let mut source_txs = HashMap::new();
        source_txs.insert("DUT_UART".to_string(), entry_tx);

        ServerState {
            config_msg: Arc::new(Mutex::new("{}".to_string())),
            broadcast_tx,
            replay: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            events_replay: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            on_export: None,
            on_rotate: None,
            session_manager: None,
            logs_root: std::path::PathBuf::from("/tmp"),
            ws_client_count: Arc::new(AtomicUsize::new(0)),
            no_client_export_generation: Arc::new(AtomicU64::new(0)),
            no_client_export_delay: Duration::from_secs(3600),
            stats: Arc::new(crate::net::ws_server::RuntimeStats::empty()),
            source_txs: Arc::new(source_txs),
            source_tx_senders: Arc::new(HashMap::new()),
            source_metadata: Arc::new(source_metadata),
            line_counters: Arc::new(HashMap::new()),
            control_api: true,
        }
    }

    #[tokio::test]
    async fn hello_returns_sources_and_session() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(r#"{"id":"h1","type":"hello"}"#, &state, &mut sub)
            .await
            .unwrap();

        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["id"], "h1");
        assert_eq!(resp["type"], "hello.result");
        assert!(resp["sources"].is_object());
        assert_eq!(resp["sources"]["DUT_UART"]["type"], "uart");
        assert_eq!(resp["sources"]["DUT_UART"]["label"], "DUT");
        assert_eq!(resp["sources"]["DUT_UART"]["writable"], true);
        assert_eq!(resp["sources"]["PYTEST"]["type"], "udp");
        assert_eq!(resp["sources"]["PYTEST"]["writable"], false);
        assert!(resp["session"]["id"].is_string());
    }

    #[tokio::test]
    async fn client_receives_nothing_before_subscribe() {
        let sub = ControlSubscription::new();
        assert!(sub.is_empty());
    }

    #[tokio::test]
    async fn subscribe_to_one_source() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":["DUT_UART"]}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert_eq!(sub.len(), 1);
        assert!(sub.contains("DUT_UART"));
    }

    #[tokio::test]
    async fn subscribe_to_multiple_sources() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":["DUT_UART","PYTEST"]}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert_eq!(sub.len(), 2);
    }

    #[tokio::test]
    async fn subscribe_unknown_source_returns_error() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":["NONEXISTENT"]}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "error");
        assert!(resp["error"].as_str().unwrap().contains("unknown source"));
    }

    #[tokio::test]
    async fn unsubscribe_stops_delivery() {
        let state = test_control_state();
        let mut sub = ControlSubscription::from(["DUT_UART".to_string(), "PYTEST".to_string()]);

        // Unsubscribe from PYTEST
        let response = handle_control_command(
            r#"{"id":"u1","type":"unsubscribe","sources":["PYTEST"]}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "unsubscribe.result");
        assert_eq!(sub.len(), 1);
        assert!(sub.contains("DUT_UART"));
    }

    #[tokio::test]
    async fn unsubscribe_last_source_empties_set() {
        let state = test_control_state();
        let mut sub = ControlSubscription::from(["DUT_UART".to_string()]);

        let response = handle_control_command(
            r#"{"id":"u1","type":"unsubscribe","sources":["DUT_UART"]}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "unsubscribe.result");
        assert!(
            sub.is_empty(),
            "after removing last source, set should be empty"
        );
    }

    #[tokio::test]
    async fn unknown_command_returns_error() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response =
            handle_control_command(r#"{"id":"x1","type":"unknown_cmd"}"#, &state, &mut sub)
                .await
                .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "error");
        assert!(resp["error"].as_str().unwrap().contains("unknown command"));
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(r#"{invalid json}"#, &state, &mut sub)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "error");
        assert!(resp["error"].as_str().unwrap().contains("invalid JSON"));
    }

    #[tokio::test]
    async fn log_inject_appends_to_source_pipeline() {
        let (entry_tx, _entry_rx) = mpsc::channel::<LogEntry>(8);
        let mut state = test_control_state();
        let mut source_txs = HashMap::new();
        source_txs.insert("DUT_UART".to_string(), entry_tx);
        state.source_txs = Arc::new(source_txs);

        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"li1","type":"log.inject","source_id":"DUT_UART","origin":"pytest","message":"test message","color":"cyan"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(
            resp["type"],
            "log.inject.result",
            "got error: {:?}",
            resp.get("error")
        );
    }

    #[tokio::test]
    async fn log_inject_unknown_source_returns_error() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"li1","type":"log.inject","source_id":"NONEXISTENT","message":"test"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "log.inject.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["source_id"], "NONEXISTENT");
        assert!(resp["error"].as_str().unwrap().contains("unknown source"));
    }

    #[tokio::test]
    async fn tx_write_to_non_writable_source_returns_error() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"tx1","type":"tx.write","source_id":"PYTEST","data":"version\r\n","origin":"pytest"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "tx.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["source_id"], "PYTEST");
        assert_eq!(resp["error"], "source is not writable");
    }

    #[tokio::test]
    async fn tx_write_sends_command_with_ack() {
        let mut sub = ControlSubscription::new();

        let (tx_sender, mut tx_rx) = mpsc::channel::<TxCommand>(4);
        let mut source_tx_senders = HashMap::new();
        source_tx_senders.insert("DUT_UART".to_string(), tx_sender);

        let mut state_with_tx = test_control_state();
        state_with_tx.source_tx_senders = Arc::new(source_tx_senders);

        // Spawn a task that receives the TxCommand and sends ack.
        let ack_handle = tokio::spawn(async move {
            let mut cmd = tx_rx.recv().await.unwrap();
            assert_eq!(cmd.data, b"version\r\n");
            assert_eq!(cmd.origin, "pytest");
            // Send ack success
            if let Some(ack) = cmd.ack.take() {
                let _ = ack.send(Ok(()));
            }
        });

        let response = handle_control_command(
            r#"{"id":"tx1","type":"tx.write","source_id":"DUT_UART","data":"version\r\n","origin":"pytest"}"#,
            &state_with_tx,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "tx.result");
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["source_id"], "DUT_UART");
        assert_eq!(resp["bytes"], 9);

        ack_handle.await.unwrap();
    }

    #[tokio::test]
    async fn tx_write_closed_channel_returns_error() {
        let mut state = test_control_state();
        let mut sub = ControlSubscription::new();

        let (tx_sender, _tx_rx) = mpsc::channel::<TxCommand>(4);
        drop(_tx_rx);
        let mut source_tx_senders = HashMap::new();
        source_tx_senders.insert("DUT_UART".to_string(), tx_sender);
        state.source_tx_senders = Arc::new(source_tx_senders);

        let response = handle_control_command(
            r#"{"id":"tx1","type":"tx.write","source_id":"DUT_UART","data":"hello"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "tx.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["source_id"], "DUT_UART");
        assert_eq!(resp["error"], "source write channel closed");
    }

    #[test]
    fn build_log_entry_uses_structured_fields() {
        let payload = serde_json::json!({
            "type": "rx",
            "data": "\u{001b}[32mboot complete\u{001b}[0m",
            "message": "boot complete",
            "origin": "SERIAL",
            "color": "green",
            "timestamp": "06-14 12:00:00.123",
            "timestamp_iso": "2026-06-14T12:00:00.123Z",
            "source_id": "DUT_UART",
            "line_idx": 42,
        });

        let entry = build_log_entry(&payload, "DUT_UART");
        assert_eq!(entry["type"], "log.entry");
        assert_eq!(entry["source_id"], "DUT_UART");
        assert_eq!(entry["origin"], "SERIAL");
        assert_eq!(entry["message"], "boot complete");
        assert_eq!(entry["timestamp_iso"], "2026-06-14T12:00:00.123Z");
        assert_eq!(entry["line_idx"], 42);
        assert_eq!(entry["color"], "green");
        assert_eq!(entry["is_tx"], false);

        // TX entry with custom origin
        let tx_payload = serde_json::json!({
            "type": "tx",
            "data": "help\r\n",
            "message": "help\r\n",
            "origin": "pytest",
            "color": "yellow",
            "source_id": "DUT_UART",
            "line_idx": 43,
        });
        let tx_entry = build_log_entry(&tx_payload, "DUT_UART");
        assert_eq!(tx_entry["type"], "log.entry");
        assert_eq!(tx_entry["origin"], "pytest");
        assert_eq!(tx_entry["is_tx"], true);
        assert_eq!(tx_entry["color"], "yellow");
    }

    #[tokio::test]
    async fn tx_write_failure_ack_returns_error() {
        let mut sub = ControlSubscription::new();

        let (tx_sender, mut tx_rx) = mpsc::channel::<TxCommand>(4);
        let mut source_tx_senders = HashMap::new();
        source_tx_senders.insert("DUT_UART".to_string(), tx_sender);

        let mut state_with_tx = test_control_state();
        state_with_tx.source_tx_senders = Arc::new(source_tx_senders);

        // Simulate write failure by sending Err ack.
        let ack_handle = tokio::spawn(async move {
            let mut cmd = tx_rx.recv().await.unwrap();
            if let Some(ack) = cmd.ack.take() {
                let _ = ack.send(Err("serial port disconnected".to_string()));
            }
        });

        let response = handle_control_command(
            r#"{"id":"tx1","type":"tx.write","source_id":"DUT_UART","data":"version\r\n","origin":"pytest"}"#,
            &state_with_tx,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "tx.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["source_id"], "DUT_UART");
        assert!(
            resp["error"]
                .as_str()
                .unwrap()
                .contains("serial port disconnected"),
            "got error: {:?}",
            resp["error"]
        );

        ack_handle.await.unwrap();
    }

    #[tokio::test]
    async fn build_log_entry_from_writer_payload_preserves_origin() {
        // Simulate a writer broadcast for a normal RX entry with origin SERIAL.
        let rx_payload = serde_json::json!({
            "type": "rx",
            "source_id": "DUT_UART",
            "origin": "SERIAL",
            "message": "boot complete",
            "color": serde_json::Value::Null,
            "timestamp_iso": "2026-06-14T12:00:00.123Z",
            "line_idx": 1,
        });
        let entry = build_log_entry(&rx_payload, "DUT_UART");
        assert_eq!(entry["origin"], "SERIAL");
        assert_eq!(entry["is_tx"], false);

        // Simulate a writer broadcast for an injected entry with custom origin.
        let inject_payload = serde_json::json!({
            "type": "rx",
            "source_id": "DUT_UART",
            "origin": "pytest",
            "message": "test: assertion passed",
            "color": "cyan",
            "timestamp_iso": "2026-06-14T12:00:00.456Z",
            "line_idx": 2,
        });
        let entry = build_log_entry(&inject_payload, "DUT_UART");
        assert_eq!(entry["origin"], "pytest");
        assert_eq!(entry["is_tx"], false);
        assert_eq!(entry["color"], "cyan");

        // Simulate a TX entry with custom origin.
        let tx_payload = serde_json::json!({
            "type": "tx",
            "source_id": "DUT_UART",
            "origin": "pytest",
            "message": "help\r\n",
            "color": "yellow",
            "timestamp_iso": "2026-06-14T12:00:00.789Z",
            "line_idx": 3,
        });
        let entry = build_log_entry(&tx_payload, "DUT_UART");
        assert_eq!(entry["origin"], "pytest");
        assert_eq!(entry["is_tx"], true);
        assert_eq!(entry["color"], "yellow");
    }

    /// Build a ServerState with a real session manager and temp directory.
    fn test_state_with_session(name: &str) -> (ServerState, std::path::PathBuf) {
        let dir = temp_session_dir(name);
        let mut source_files = HashMap::new();
        source_files.insert(
            "DUT_UART".to_string(),
            dir.join("dut.log").display().to_string(),
        );

        let mut pane_labels = HashMap::new();
        pane_labels.insert("DUT_UART".to_string(), "DUT".to_string());

        let mut pane_kinds = HashMap::new();
        pane_kinds.insert("DUT_UART".to_string(), "uart".to_string());

        let mgr = SessionManager::new(
            "session-1",
            dir.clone(),
            &[serde_json::json!({"label": "Main", "panes": ["DUT_UART"]})],
            source_files,
            pane_labels,
            pane_kinds,
            serde_json::json!({}),
            serde_json::json!({}),
            serde_json::json!({}),
            serde_json::json!({}),
            "2026-06-14T12:00:00+00:00",
            "embed-log",
            None,
            None,
            "absolute",
            None,
        );
        mgr.write_manifest().unwrap();

        let (broadcast_tx, _rx) = broadcast::channel(64);

        // Populate the replay buffer with a fake log entry at line_idx 42
        let replay = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        {
            let mut buf = replay.lock().unwrap();
            buf.push_back(
                serde_json::json!({
                    "source_id": "DUT_UART",
                    "line_idx": 42,
                    "timestamp_num": 1234567890.789,
                })
                .to_string(),
            );
        }

        let mut source_metadata = HashMap::new();
        source_metadata.insert(
            "DUT_UART".to_string(),
            SourceInfo {
                source_type: "uart".to_string(),
                label: "DUT".to_string(),
                writable: true,
            },
        );

        // Populate line_counters so line_idx validation works
        use std::sync::atomic::AtomicU64;
        let dut_counter = Arc::new(AtomicU64::new(100));
        let mut line_counters = HashMap::new();
        line_counters.insert("DUT_UART".to_string(), dut_counter);

        let (entry_tx, _entry_rx) = mpsc::channel::<LogEntry>(8);
        let mut source_txs = HashMap::new();
        source_txs.insert("DUT_UART".to_string(), entry_tx);

        let state = ServerState {
            config_msg: Arc::new(Mutex::new("{}".to_string())),
            broadcast_tx,
            replay,
            events_replay: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            on_export: None,
            on_rotate: None,
            session_manager: Some(Arc::new(Mutex::new(mgr))),
            logs_root: dir.clone(),
            ws_client_count: Arc::new(AtomicUsize::new(0)),
            no_client_export_generation: Arc::new(AtomicU64::new(0)),
            no_client_export_delay: Duration::from_secs(3600),
            stats: Arc::new(crate::net::ws_server::RuntimeStats::empty()),
            source_txs: Arc::new(source_txs),
            source_tx_senders: Arc::new(HashMap::new()),
            source_metadata: Arc::new(source_metadata),
            line_counters: Arc::new(line_counters),
            control_api: true,
        };
        (state, dir)
    }

    #[tokio::test]
    async fn marker_create_writes_markers_json() {
        let (state, dir) = test_state_with_session("write-json");
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"DUT_UART","line_idx":42,"description":"fatal-error: ZEPHYR FATAL ERROR","origin":"watcher"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "marker.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["source_id"], "DUT_UART");

        // Verify markers.json was written
        let markers_path = dir.join("markers.json");
        assert!(markers_path.exists());
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&markers_path).unwrap()).unwrap();
        assert_eq!(raw["session_id"], "session-1");
        assert_eq!(raw["markers"].as_array().unwrap().len(), 1);
        assert_eq!(raw["markers"][0]["paneId"], "DUT_UART");
        assert_eq!(raw["markers"][0]["lineIdx"], 42);
        assert_eq!(
            raw["markers"][0]["description"],
            "fatal-error: ZEPHYR FATAL ERROR"
        );
        assert!(raw["markers"][0]["createdAt"].is_string());

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn marker_create_rejects_unknown_source() {
        let (state, dir) = test_state_with_session("unknown");
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"NONEXISTENT","line_idx":1,"description":"test"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "marker.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["source_id"], "NONEXISTENT");
        assert!(resp["error"].as_str().unwrap().contains("unknown source"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn marker_create_rejects_negative_line_idx() {
        let (state, dir) = test_state_with_session("neg-idx");
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"DUT_UART","line_idx":-1,"description":"test"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "error");
        assert!(resp["error"].as_str().unwrap().contains("non-negative"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn marker_create_multiple_markers_preserved() {
        let (state, dir) = test_state_with_session("multi-marker");
        let mut sub = ControlSubscription::new();

        // Create first marker with explicit timestamp_num
        let _ = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"DUT_UART","line_idx":10,"description":"first","timestamp_num":1000.0}"#,
            &state,
            &mut sub,
        )
        .await;

        // Create second marker at a different line
        let _ = handle_control_command(
            r#"{"id":"m2","type":"marker.create","source_id":"DUT_UART","line_idx":20,"description":"second","timestamp_num":2000.0}"#,
            &state,
            &mut sub,
        )
        .await;

        // Verify both markers exist
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("markers.json")).unwrap())
                .unwrap();
        let markers = raw["markers"].as_array().unwrap();
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0]["description"], "first");
        assert_eq!(markers[1]["description"], "second");
        assert!((markers[0]["numTs"].as_f64().unwrap() - 1000.0).abs() < 0.001);
        assert!((markers[1]["numTs"].as_f64().unwrap() - 2000.0).abs() < 0.001);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn marker_create_replaces_existing_marker_at_same_line() {
        let (state, dir) = test_state_with_session("replacement");
        let mut sub = ControlSubscription::new();

        // Create first marker
        let _ = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"DUT_UART","line_idx":42,"description":"original","timestamp_num":5000.0}"#,
            &state,
            &mut sub,
        )
        .await;

        // Create second marker at the same line → should replace
        let _ = handle_control_command(
            r#"{"id":"m2","type":"marker.create","source_id":"DUT_UART","line_idx":42,"description":"replacement","timestamp_num":5000.0}"#,
            &state,
            &mut sub,
        )
        .await;

        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("markers.json")).unwrap())
                .unwrap();
        let markers = raw["markers"].as_array().unwrap();
        assert_eq!(markers.len(), 1, "should replace, not duplicate");
        assert_eq!(markers[0]["description"], "replacement");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn marker_create_rejects_line_idx_out_of_range() {
        let (state, dir) = test_state_with_session("out-of-range");
        let mut sub = ControlSubscription::new();

        // line_counters has DUT_UART = 100, so line_idx 100 is out of range
        let response = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"DUT_UART","line_idx":100,"description":"bad","timestamp_num":6000.0}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "marker.result", "got: {:?}", resp);
        assert_eq!(resp["ok"], false);
        assert!(resp["error"].as_str().unwrap().contains("out of range"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn marker_create_broadcasts_markers_update() {
        let dir = temp_session_dir("marker-broadcast");
        let (broadcast_tx, mut rx) = broadcast::channel::<String>(8);

        // Build a minimal state with broadcast and session
        let replay = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        {
            let mut buf = replay.lock().unwrap();
            buf.push_back(
                serde_json::json!({
                    "source_id": "DUT_UART",
                    "line_idx": 42,
                    "timestamp_num": 7000.0,
                })
                .to_string(),
            );
        }

        let session_mgr = {
            let mut source_files = HashMap::new();
            source_files.insert(
                "DUT_UART".to_string(),
                dir.join("dut.log").display().to_string(),
            );
            let mut pane_labels = HashMap::new();
            pane_labels.insert("DUT_UART".to_string(), "DUT".to_string());
            let mut pane_kinds = HashMap::new();
            pane_kinds.insert("DUT_UART".to_string(), "uart".to_string());
            let mgr = SessionManager::new(
                "session-bc",
                dir.clone(),
                &[serde_json::json!({"label": "Main", "panes": ["DUT_UART"]})],
                source_files,
                pane_labels,
                pane_kinds,
                serde_json::json!({}),
                serde_json::json!({}),
                serde_json::json!({}),
                serde_json::json!({}),
                "2026-06-14T12:00:00+00:00",
                "embed-log",
                None,
                None,
                "absolute",
                None,
            );
            mgr.write_manifest().unwrap();
            Arc::new(Mutex::new(mgr))
        };

        let mut source_metadata = HashMap::new();
        source_metadata.insert(
            "DUT_UART".to_string(),
            SourceInfo {
                source_type: "uart".to_string(),
                label: "DUT".to_string(),
                writable: true,
            },
        );

        use std::sync::atomic::AtomicU64;
        let mut line_counters = HashMap::new();
        line_counters.insert("DUT_UART".to_string(), Arc::new(AtomicU64::new(100)));

        let (entry_tx, _entry_rx) = mpsc::channel::<LogEntry>(8);
        let mut source_txs = HashMap::new();
        source_txs.insert("DUT_UART".to_string(), entry_tx);

        let state = ServerState {
            config_msg: Arc::new(Mutex::new("{}".to_string())),
            broadcast_tx,
            replay,
            events_replay: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            on_export: None,
            on_rotate: None,
            session_manager: Some(session_mgr),
            logs_root: dir.clone(),
            ws_client_count: Arc::new(AtomicUsize::new(0)),
            no_client_export_generation: Arc::new(AtomicU64::new(0)),
            no_client_export_delay: Duration::from_secs(3600),
            stats: Arc::new(crate::net::ws_server::RuntimeStats::empty()),
            source_txs: Arc::new(source_txs),
            source_tx_senders: Arc::new(HashMap::new()),
            source_metadata: Arc::new(source_metadata),
            line_counters: Arc::new(line_counters),
            control_api: true,
        };

        let mut sub = ControlSubscription::new();
        let response = handle_control_command(
            r#"{"id":"m1","type":"marker.create","source_id":"DUT_UART","line_idx":42,"description":"broadcast-test"}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "marker.result");
        assert_eq!(resp["ok"], true);

        // Verify broadcast was emitted
        let broadcast_msg = rx.try_recv().ok();
        assert!(broadcast_msg.is_some(), "expected markers_update broadcast");
        if let Some(msg) = broadcast_msg {
            let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
            assert_eq!(parsed["type"], "markers_update");
            assert_eq!(parsed["markers"].as_array().unwrap().len(), 1);
        }

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn subscribe_with_events_true_enables_events() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();
        assert!(!sub.events);

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":["DUT_UART"],"events":true}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert!(
            sub.events,
            "events flag should be true after subscribe with events:true"
        );
    }

    #[tokio::test]
    async fn subscribe_without_events_does_not_enable_events() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();
        assert!(!sub.events);

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":["DUT_UART"]}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert!(
            !sub.events,
            "events flag should remain false when not requested"
        );
    }

    #[tokio::test]
    async fn subscribe_with_events_false_leaves_events_unchanged() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();
        sub.events = true;

        // subscribe with events:false should NOT disable events
        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":["DUT_UART"],"events":false}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert!(
            sub.events,
            "events flag should remain true; only unsubscribe with events:false disables it"
        );
    }

    #[tokio::test]
    async fn unsubscribe_events_disables_events_flag() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();
        sub.events = true;

        let response = handle_control_command(
            r#"{"id":"u1","type":"unsubscribe","sources":[],"events":false}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "unsubscribe.result");
        assert!(
            !sub.events,
            "events flag should be false after unsubscribe with events:false"
        );
    }

    #[test]
    fn should_forward_event_when_events_subscribed() {
        let sub = ControlSubscription {
            events: true,
            ..Default::default()
        };
        let parsed = serde_json::json!({"type": "event", "event_id": "boot"});
        assert!(should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn should_not_forward_event_when_events_not_subscribed() {
        let sub = ControlSubscription::new();
        let parsed = serde_json::json!({"type": "event", "event_id": "boot"});
        assert!(!should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn should_forward_log_entry_when_source_subscribed() {
        let sub = ControlSubscription::from(["DUT_UART".to_string()]);
        let parsed = serde_json::json!({"type": "rx", "source_id": "DUT_UART"});
        assert!(should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn should_not_forward_log_entry_when_source_not_subscribed() {
        let sub = ControlSubscription::new();
        let parsed = serde_json::json!({"type": "rx", "source_id": "DUT_UART"});
        assert!(!should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn should_forward_markers_update_always() {
        let sub = ControlSubscription::new();
        let parsed = serde_json::json!({"type": "markers_update"});
        assert!(should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn should_forward_session_info_always() {
        let sub = ControlSubscription::new();
        let parsed = serde_json::json!({"type": "session_info"});
        assert!(should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn should_not_forward_unknown_type() {
        let sub = ControlSubscription::new();
        let parsed = serde_json::json!({"type": "unknown"});
        assert!(!should_forward_to_control_client(&parsed, &sub));
    }

    #[test]
    fn event_and_log_entry_interleave() {
        let sub = ControlSubscription {
            sources: HashSet::from(["DUT_UART".to_string()]),
            events: true,
        };

        let event = serde_json::json!({"type": "event", "event_id": "boot"});
        let log_entry = serde_json::json!({"type": "rx", "source_id": "DUT_UART"});
        let unknown = serde_json::json!({"type": "unknown"});

        // Both event and subscribed log entry are forwarded.
        assert!(should_forward_to_control_client(&event, &sub));
        assert!(should_forward_to_control_client(&log_entry, &sub));
        // Unknown type is not.
        assert!(!should_forward_to_control_client(&unknown, &sub));
    }

    #[tokio::test]
    async fn subscribe_events_only_without_sources() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","events":true}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert!(sub.events, "events-only subscribe should enable events");
        assert!(sub.is_empty(), "no sources should be subscribed");
    }

    #[tokio::test]
    async fn subscribe_events_only_with_empty_sources() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response = handle_control_command(
            r#"{"id":"s1","type":"subscribe","sources":[],"events":true}"#,
            &state,
            &mut sub,
        )
        .await
        .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "subscribe.result");
        assert!(
            sub.events,
            "events-only subscribe with empty sources should enable events"
        );
    }

    #[tokio::test]
    async fn subscribe_missing_sources_without_events_returns_error() {
        let state = test_control_state();
        let mut sub = ControlSubscription::new();

        let response =
            handle_control_command(r#"{"id":"s1","type":"subscribe"}"#, &state, &mut sub)
                .await
                .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(resp["type"], "error");
        assert!(!sub.events);
    }
}
