use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path as AxumPath, State, WebSocketUpgrade};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};
use tower_http::services::ServeDir;
use tracing::{info, warn};

use crate::frontend_assets::FrontendAssets;
use crate::net::control_ws::{
    control_ws_handler, handle_event_rule_create, handle_event_rule_delete,
    handle_event_rule_export, handle_event_rule_list, handle_event_rule_promote, SourceInfo,
};
use crate::session::SessionManager;
use crate::sources::TxCommand;

/// Callback type for session export requests.
pub type ExportCallback = Arc<dyn Fn() -> Result<String, String> + Send + Sync>;
pub type RotateCallback =
    Arc<dyn Fn() -> Result<(serde_json::Value, serde_json::Value), String> + Send + Sync>;

#[derive(Default)]
pub struct SourceRuntimeStats {
    dequeued: AtomicU64,
    bytes: AtomicU64,
}

impl SourceRuntimeStats {
    pub fn record_dequeued(&self, bytes: usize) {
        self.dequeued.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    // ponytail: only the fields we actually track. Live queue depth/utilization
    // would need the producer side wired in too; add those back if a consumer
    // needs real backpressure telemetry.
    fn snapshot(&self, maxsize: usize) -> serde_json::Value {
        serde_json::json!({
            "maxsize": maxsize,
            "dequeued": self.dequeued.load(Ordering::Relaxed),
            "bytes": self.bytes.load(Ordering::Relaxed),
        })
    }
}

pub struct RuntimeStats {
    sources: HashMap<String, Arc<SourceRuntimeStats>>,
    queue_size: usize,
}

impl RuntimeStats {
    pub fn new<I>(source_names: I, queue_size: usize) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let sources = source_names
            .into_iter()
            .map(|name| (name, Arc::new(SourceRuntimeStats::default())))
            .collect();
        Self {
            sources,
            queue_size,
        }
    }

    pub fn empty() -> Self {
        Self {
            sources: HashMap::new(),
            queue_size: 0,
        }
    }

    pub fn source(&self, name: &str) -> Option<Arc<SourceRuntimeStats>> {
        self.sources.get(name).cloned()
    }

    fn snapshot(&self) -> serde_json::Value {
        let sources = self
            .sources
            .iter()
            .map(|(name, stats)| (name.clone(), stats.snapshot(self.queue_size)))
            .collect::<serde_json::Map<_, _>>();
        serde_json::Value::Object(sources)
    }
}

/// Shared state for the HTTP + WebSocket server.
#[derive(Clone)]
pub struct ServerState {
    /// Pre-serialized config message sent to every new WS client.
    ///
    /// This is mutable because session rotation changes the active session.
    /// New/reconnecting clients must receive the current session, not the
    /// process-start session.
    pub config_msg: Arc<Mutex<String>>,
    /// Broadcast channel for live log messages (JSON strings).
    pub broadcast_tx: broadcast::Sender<String>,
    /// Replay buffer for late-connecting clients (log entries).
    pub replay: Arc<Mutex<VecDeque<String>>>,
    /// Replay buffer for events, sent to late-connecting clients.
    pub events_replay: Arc<Mutex<VecDeque<String>>>,
    /// Callback to trigger HTML export.
    pub on_export: Option<ExportCallback>,
    /// Callback to rotate to a new session.
    pub on_rotate: Option<RotateCallback>,
    /// Current session artifact manager.
    pub session_manager: Option<Arc<Mutex<SessionManager>>>,
    /// Root directory where session folders live (e.g. `logs/`).
    pub logs_root: PathBuf,
    /// Live WebSocket client count.
    pub ws_client_count: Arc<AtomicUsize>,
    /// Generation used to cancel pending no-client exports when a new client connects.
    pub no_client_export_generation: Arc<AtomicU64>,
    /// Delay before exporting after the final WebSocket client disconnects.
    pub no_client_export_delay: Duration,
    /// Runtime queue/source counters.
    pub stats: Arc<RuntimeStats>,
    /// Per-source input channels for synthetic TX/inject entries (LogEntry pipeline).
    pub source_txs: Arc<HashMap<String, mpsc::Sender<crate::models::LogEntry>>>,
    /// Per-source TX command channels for writing bytes to writable sources (e.g. UART).
    pub source_tx_senders: Arc<HashMap<String, mpsc::Sender<TxCommand>>>,
    /// Source metadata for the control API (name → type, label, writable).
    pub source_metadata: Arc<HashMap<String, SourceInfo>>,
    /// Per-source line counters for stable `line_idx` in log entries.
    pub line_counters: Arc<HashMap<String, Arc<std::sync::atomic::AtomicU64>>>,
    /// Rules loaded from the companion event YAML file at server startup.
    pub static_event_rules: Arc<HashMap<String, Vec<crate::config::EventRule>>>,
    /// Preferred companion YAML path for persisting promoted runtime rules.
    pub event_rules_path: PathBuf,
    /// Event rules added for the lifetime of this server/session.
    pub runtime_event_rules: Arc<RwLock<HashMap<String, Vec<crate::config::EventRule>>>>,
    /// Whether the /api/v1/control WebSocket endpoint is enabled.
    pub control_api: bool,
}

/// Start the axum HTTP + WebSocket server.
///
/// Routes:
/// - `GET /ws` — WebSocket for live log streaming
/// - `GET /api/session/current` — current session info
/// - `POST /api/session/export` — trigger HTML export
/// - `GET /api/sessions` — list known sessions
/// - `GET /api/health` — health probe
/// - `GET /sessions/{id}/{file}` — serve session artifacts (HTML, logs)
/// - `GET /*` — serve frontend static files
pub async fn start_server(
    host: &str,
    port: u16,
    frontend_dir: Option<PathBuf>,
    state: ServerState,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let local_addr = listener.local_addr()?;
    info!("UI ready at http://{local_addr}/  (WebSocket: ws://{local_addr}/ws)");

    let mut api = Router::new().route("/ws", axum::routing::get(ws_handler));

    if state.control_api {
        info!("Control API at ws://{local_addr}/api/v1/control");
        api = api.route("/api/v1/control", axum::routing::get(control_ws_handler));
    }

    api = api
        .route("/api/health", axum::routing::get(api_health_handler))
        .route(
            "/api/session/current",
            axum::routing::get(api_current_session_handler),
        )
        .route(
            "/api/session/export",
            axum::routing::post(api_export_handler),
        )
        .route(
            "/api/session/rotate",
            axum::routing::post(api_rotate_handler),
        )
        .route("/api/sessions", axum::routing::get(api_sessions_handler))
        .route("/api/stats", axum::routing::get(api_stats_handler))
        .route(
            "/sessions/{session_id}/{filename}",
            axum::routing::get(session_file_handler),
        );

    let app = if let Some(ref dir) = frontend_dir {
        if dir.join("index.html").exists() {
            api.fallback_service(ServeDir::new(dir.clone()))
        } else {
            api.fallback(embedded_fallback)
        }
    } else {
        api.fallback(embedded_fallback)
    }
    .layer(middleware::from_fn(no_cache_middleware))
    .with_state(state);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Serve static files from embedded frontend assets. Falls back to index.html for SPA routing.
async fn embedded_fallback(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    let content_type = if path.ends_with(".css") {
        "text/css; charset=utf-8".to_string()
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8".to_string()
    } else if path.contains('.') {
        // Binary/other assets (fonts, images, ...): guess from extension
        // rather than mislabeling them as HTML, which browsers reject for
        // e.g. @font-face loads.
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string()
    } else {
        "text/html; charset=utf-8".to_string()
    };
    let content_type = content_type.as_str();

    if let Some(file) = FrontendAssets::get(path) {
        return Response::builder()
            .header("content-type", content_type)
            .header("cache-control", "no-cache")
            .body(axum::body::Body::from(file.data))
            .unwrap();
    }

    // SPA fallback
    if let Some(file) = FrontendAssets::get("index.html") {
        return Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .header("cache-control", "no-cache")
            .body(axum::body::Body::from(file.data))
            .unwrap();
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Middleware that adds no-cache headers to all responses.
/// Prevents the browser from caching frontend JS/CSS files.
async fn no_cache_middleware(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-cache, no-store, must-revalidate".parse().unwrap(),
    );
    response
}

// ── WebSocket handler ──

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<ServerState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_client(socket, state))
}

async fn handle_ws_client(mut socket: WebSocket, state: ServerState) {
    state.ws_client_count.fetch_add(1, Ordering::Relaxed);
    state
        .no_client_export_generation
        .fetch_add(1, Ordering::Relaxed);
    let _client_count_guard = WsClientCountGuard {
        count: state.ws_client_count.clone(),
        generation: state.no_client_export_generation.clone(),
        delay: state.no_client_export_delay,
        on_export: state.on_export.clone(),
        broadcast_tx: state.broadcast_tx.clone(),
    };

    // 1. Send the config message immediately.
    let config_msg = state.config_msg.lock().unwrap().clone();
    if let Err(e) = socket.send(Message::Text(config_msg.into())).await {
        warn!("WS: failed to send config: {e}");
        return;
    }

    // 2. Send replay buffer so the client catches up.
    let replay_msgs = drain_replay(&state.replay);
    for msg in replay_msgs {
        if socket.send(Message::Text(msg.into())).await.is_err() {
            return;
        }
    }

    // 2b. Send events replay buffer so late-connecting clients catch up.
    let events_replay_msgs = drain_replay(&state.events_replay);
    for msg in events_replay_msgs {
        if socket.send(Message::Text(msg.into())).await.is_err() {
            return;
        }
    }

    // 3. Subscribe to the broadcast channel.
    let mut rx = state.broadcast_tx.subscribe();

    // 4. Forward broadcast messages to the client; drain incoming messages.
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(data) => {
                        if socket.send(Message::Text(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // A slow client fell behind the broadcast buffer. Tell it
                        // in-band so the gap isn't silent — the frontend can flag
                        // missing lines / resync rather than show a seamless feed.
                        // ponytail: broadcast capacity is the tuning knob if lag is common.
                        warn!("WS client lagged, skipped {n} messages");
                        let notice = format!("{{\"type\":\"stream_gap\",\"skipped\":{n}}}");
                        if socket.send(Message::Text(notice.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let response = handle_client_command(&text, &state).await;
                        if let Some(resp) = response {
                            if socket.send(Message::Text(resp.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

struct WsClientCountGuard {
    count: Arc<AtomicUsize>,
    generation: Arc<AtomicU64>,
    delay: Duration,
    on_export: Option<ExportCallback>,
    broadcast_tx: broadcast::Sender<String>,
}

impl Drop for WsClientCountGuard {
    fn drop(&mut self) {
        let previous = self.count.fetch_sub(1, Ordering::Relaxed);
        if previous != 1 {
            return;
        }

        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation_counter = self.generation.clone();
        let count = self.count.clone();
        let delay = self.delay;
        let on_export = self.on_export.clone();
        let broadcast_tx = self.broadcast_tx.clone();

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if count.load(Ordering::Relaxed) != 0
                || generation_counter.load(Ordering::Relaxed) != generation
            {
                return;
            }

            let Some(export_fn) = on_export else {
                return;
            };
            let payload = match export_fn() {
                Ok(path) => serde_json::json!({
                    "type": "session_html_status",
                    "html_status": "ready",
                    "html_path": path,
                    "reason": "no_clients",
                }),
                Err(error) => serde_json::json!({
                    "type": "session_html_status",
                    "html_status": "error",
                    "html_error": error,
                    "reason": "no_clients",
                }),
            };
            let _ = broadcast_tx.send(payload.to_string());
        });
    }
}

/// Handle incoming WebSocket commands from the frontend.
async fn handle_client_command(text: &str, state: &ServerState) -> Option<String> {
    let cmd: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let cmd_type = cmd
        .get("cmd")
        .or_else(|| cmd.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    info!("WS command: {cmd_type}");

    match cmd_type {
        "export_session_html" => {
            let status = do_export(state);
            let status_str = status.to_string();
            let _ = state.broadcast_tx.send(status_str.clone());
            Some(status_str)
        }
        "save_markers" => Some(handle_save_markers(&cmd, state).to_string()),
        "clear_logs" => Some(handle_clear_logs(&cmd, state).to_string()),
        "set_filter" => Some(handle_set_filter(&cmd).to_string()),
        "send_raw" => Some(handle_send_raw(&cmd, state).await.to_string()),
        "event_rule.create" => Some(handle_event_rule_create(
            &cmd,
            state,
            cmd.get("id").and_then(|value| value.as_str()),
        )),
        "event_rule.list" => Some(handle_event_rule_list(
            state,
            cmd.get("id").and_then(|value| value.as_str()),
        )),
        "event_rule.export" => Some(handle_event_rule_export(
            state,
            cmd.get("id").and_then(|value| value.as_str()),
        )),
        "event_rule.promote" => Some(handle_event_rule_promote(
            &cmd,
            state,
            cmd.get("id").and_then(|value| value.as_str()),
        )),
        "event_rule.delete" => Some(handle_event_rule_delete(
            &cmd,
            state,
            cmd.get("id").and_then(|value| value.as_str()),
        )),
        _ => None,
    }
}

fn do_export(state: &ServerState) -> serde_json::Value {
    if let Some(ref export_fn) = state.on_export {
        match export_fn() {
            Ok(path) => {
                info!("session HTML exported: {path}");
                serde_json::json!({
                    "type": "session_html_status",
                    "html_status": "ready",
                    "html_path": path,
                    "session": current_session_value(state),
                })
            }
            Err(err) => {
                warn!("export failed: {err}");
                serde_json::json!({
                    "type": "session_html_status",
                    "html_status": "error",
                    "html_error": err,
                    "session": current_session_value(state),
                })
            }
        }
    } else {
        serde_json::json!({
            "type": "session_html_status",
            "html_status": "error",
            "html_error": "export not available",
            "session": current_session_value(state),
        })
    }
}

fn handle_save_markers(cmd: &serde_json::Value, state: &ServerState) -> serde_json::Value {
    let markers = cmd
        .get("markers")
        .or_else(|| cmd.get("payload").and_then(|p| p.get("markers")))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let result = if let Some(manager) = &state.session_manager {
        manager
            .lock()
            .map_err(|_| "session manager lock failed".to_string())
            .and_then(|mgr| mgr.save_markers(&markers).map_err(|e| e.to_string()))
    } else {
        Err("session manager unavailable".to_string())
    };

    match result {
        Ok(()) => {
            let payload = serde_json::json!({
                "type": "markers_update",
                "markers": markers,
                "session": current_session_value(state),
            });
            let _ = state.broadcast_tx.send(payload.to_string());
            payload
        }
        Err(error) => serde_json::json!({
            "type": "markers_update",
            "ok": false,
            "error": error,
        }),
    }
}

fn handle_clear_logs(cmd: &serde_json::Value, state: &ServerState) -> serde_json::Value {
    let pane = cmd
        .get("pane")
        .or_else(|| cmd.get("source_id"))
        .and_then(|v| v.as_str());
    let payload = serde_json::json!({
        "type": "clear_logs",
        "pane": pane,
        "scope": pane.unwrap_or("all"),
        "message": "[SYSTEM] logs cleared",
    });
    let _ = state.broadcast_tx.send(payload.to_string());
    payload
}

async fn handle_send_raw(cmd: &serde_json::Value, state: &ServerState) -> serde_json::Value {
    let source = cmd
        .get("source_id")
        .or_else(|| cmd.get("source"))
        .or_else(|| cmd.get("pane"))
        .or_else(|| cmd.get("id"))
        .and_then(|v| v.as_str());
    let data = cmd
        .get("data")
        .or_else(|| cmd.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let origin = cmd.get("origin").and_then(|v| v.as_str()).unwrap_or("ui");

    // Validate: empty data
    if data.is_empty() {
        return serde_json::json!({
            "type": "send_raw_result",
            "ok": false,
            "error": "empty data",
        });
    }

    // Validate: source must be provided
    let Some(source) = source else {
        return serde_json::json!({
            "type": "send_raw_result",
            "ok": false,
            "error": "no source specified",
        });
    };

    // Try TX sender first (writable sources like UART).
    if let Some(tx_sender) = state.source_tx_senders.get(source) {
        let cmd = TxCommand {
            data: data.as_bytes().to_vec(),
            origin: origin.to_string(),
            ack: None,
        };
        match tx_sender.send(cmd).await {
            Ok(()) => serde_json::json!({
                "type": "send_raw_result",
                "ok": true,
                "source_id": source,
                "bytes": data.len(),
            }),
            Err(_) => serde_json::json!({
                "type": "send_raw_result",
                "ok": false,
                "source_id": source,
                "error": "source write channel closed",
            }),
        }
    } else if state.source_txs.contains_key(source) {
        // Source exists but is not writable (e.g., UDP).
        serde_json::json!({
            "type": "send_raw_result",
            "ok": false,
            "source_id": source,
            "error": "source is not writable",
        })
    } else {
        serde_json::json!({
            "type": "send_raw_result",
            "ok": false,
            "source_id": source,
            "error": "unknown source",
        })
    }
}

fn handle_set_filter(cmd: &serde_json::Value) -> serde_json::Value {
    let source = cmd
        .get("source_id")
        .or_else(|| cmd.get("source"))
        .and_then(|v| v.as_str());
    let filter = cmd.get("filter").and_then(|v| v.as_str()).unwrap_or("");
    // Empty/clear filter means unfiltered.
    if filter.is_empty() || filter == "null" || filter == "undefined" {
        return serde_json::json!({
            "type": "filter_result",
            "ok": true,
            "id": source,
        });
    }
    // Validate the regex and return result.
    match regex::Regex::new(filter) {
        Ok(_) => serde_json::json!({
            "type": "filter_result",
            "ok": true,
            "id": source,
        }),
        Err(e) => serde_json::json!({
            "type": "filter_result",
            "ok": false,
            "id": source,
            "error": e.to_string(),
        }),
    }
}

fn current_session_value(state: &ServerState) -> serde_json::Value {
    state
        .session_manager
        .as_ref()
        .and_then(|manager| manager.lock().ok().map(|mgr| mgr.build_session_info()))
        .unwrap_or(serde_json::Value::Null)
}

// ── HTTP API handlers ──

async fn api_health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

async fn api_current_session_handler(State(state): State<ServerState>) -> impl IntoResponse {
    match state
        .session_manager
        .as_ref()
        .and_then(|manager| manager.lock().ok().map(|mgr| mgr.build_session_info()))
    {
        Some(session) => (StatusCode::OK, axum::Json(session)),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({ "error": "session unavailable" })),
        ),
    }
}

/// POST /api/session/export — trigger HTML export (HTTP alternative to WS command).
async fn api_export_handler(State(state): State<ServerState>) -> impl IntoResponse {
    let status_payload = do_export(&state);
    let ok = status_payload.get("html_status").and_then(|s| s.as_str()) == Some("ready");
    let body = serde_json::json!({
        "ok": ok,
        "session": current_session_value(&state),
        "html_status": status_payload.get("html_status").cloned().unwrap_or(serde_json::Value::Null),
        "html_path": status_payload.get("html_path").cloned().unwrap_or(serde_json::Value::Null),
        "html_error": status_payload.get("html_error").cloned().unwrap_or(serde_json::Value::Null),
    });
    let _ = state.broadcast_tx.send(status_payload.to_string());
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, axum::Json(body))
}

async fn api_stats_handler(State(state): State<ServerState>) -> impl IntoResponse {
    let session_id = state
        .session_manager
        .as_ref()
        .and_then(|manager| manager.lock().ok().map(|mgr| mgr.session_id().to_string()));

    let replay_depth = state
        .replay
        .lock()
        .map(|replay| replay.len())
        .unwrap_or_default();

    axum::Json(serde_json::json!({
        "session_id": session_id,
        "ws_clients": state.ws_client_count.load(Ordering::Relaxed),
        "replay_depth": replay_depth,
        "sources": state.stats.snapshot(),
        "totals": {
            "sources": state.stats.sources.len(),
        },
    }))
}

async fn api_rotate_handler(State(state): State<ServerState>) -> impl IntoResponse {
    let Some(rotate_fn) = &state.on_rotate else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "ok": false,
                "error": "rotation not available",
                "old_session": current_session_value(&state),
                "session": current_session_value(&state),
            })),
        );
    };

    match rotate_fn() {
        Ok((old_session, session)) => {
            let payload = serde_json::json!({
                "type": "session_rotated",
                "old_session": old_session,
                "session": session,
            });
            let _ = state.broadcast_tx.send(payload.to_string());
            (
                StatusCode::OK,
                axum::Json(serde_json::json!({
                    "ok": true,
                    "old_session": old_session,
                    "session": session,
                })),
            )
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "ok": false,
                "error": error,
                "old_session": current_session_value(&state),
                "session": current_session_value(&state),
            })),
        ),
    }
}

async fn api_sessions_handler(State(state): State<ServerState>) -> impl IntoResponse {
    let current_id = state
        .session_manager
        .as_ref()
        .and_then(|manager| manager.lock().ok().map(|mgr| mgr.session_id().to_string()));

    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&state.logs_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("manifest.json");
            let manifest = std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            let id = manifest
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .or_else(|| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            sessions.push(serde_json::json!({
                "id": id,
                "dir": path.display().to_string(),
                "manifest": format!("/sessions/{}/manifest.json", id),
                "current": current_id.as_deref() == Some(path.file_name().and_then(|n| n.to_str()).unwrap_or(""))
                    || current_id.as_deref() == manifest.get("session_id").and_then(|v| v.as_str()),
                "html_ready": manifest.get("html_status").and_then(|v| v.as_str()) == Some("ready"),
                "html": format!("/sessions/{}/session.html", id),
                "html_status": manifest.get("html_status").and_then(|v| v.as_str()).unwrap_or("pending"),
                "started_at": manifest.get("started_at").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
    }
    let current = state
        .session_manager
        .as_ref()
        .and_then(|mgr| mgr.lock().ok().map(|mgr| mgr.session_id().to_string()));

    sessions.sort_by(|a, b| {
        b.get("id")
            .and_then(|v| v.as_str())
            .cmp(&a.get("id").and_then(|v| v.as_str()))
    });
    axum::Json(serde_json::json!({
        "sessions": sessions,
        "current": current,
    }))
}

/// GET /sessions/{session_id}/{filename} — serve session artifacts.
///
/// This is what the frontend uses to download exported HTML and raw log files.
/// Path traversal is prevented by checking the resolved path stays within `logs_root`.
async fn session_file_handler(
    AxumPath((session_id, filename)): AxumPath<(String, String)>,
    State(state): State<ServerState>,
) -> impl IntoResponse {
    // Sanitize: no slashes or dots in components.
    if session_id.contains("..")
        || session_id.contains('/')
        || filename.contains("..")
        || filename.contains('/')
    {
        return (StatusCode::FORBIDDEN, "forbidden".to_string()).into_response();
    }

    let file_path = state.logs_root.join(&session_id).join(&filename);

    // Verify the resolved path is under logs_root.
    let canonical_root = match std::fs::canonicalize(&state.logs_root) {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "bad root".to_string()).into_response()
        }
    };
    let canonical_file = match std::fs::canonicalize(&file_path) {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "not found".to_string()).into_response(),
    };
    if !canonical_file.starts_with(&canonical_root) {
        return (StatusCode::FORBIDDEN, "forbidden".to_string()).into_response();
    }

    if !canonical_file.is_file() {
        return (StatusCode::NOT_FOUND, "not found".to_string()).into_response();
    }

    // Determine content type from extension.
    let content_type = if filename.ends_with(".html") || filename.ends_with(".htm") {
        "text/html; charset=utf-8"
    } else if filename.ends_with(".json") {
        "application/json"
    } else if filename.ends_with(".log") || filename.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    };

    match std::fs::read_to_string(&canonical_file) {
        Ok(body) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, content_type)],
            body,
        )
            .into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "read error".to_string()).into_response(),
    }
}

/// Collect all messages from a replay buffer, leaving it intact.
pub fn drain_replay(replay: &Arc<Mutex<VecDeque<String>>>) -> Vec<String> {
    let buf = replay.lock().unwrap();
    buf.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use axum::response::IntoResponse;
    use chrono::Local;
    use serde_json::json;

    #[tokio::test]
    async fn embedded_fallback_serves_font_with_font_mime_type_not_html() {
        let uri: axum::http::Uri = "/fonts/JetBrainsMono-Regular.woff2".parse().unwrap();
        let response = embedded_fallback(uri).await.into_response();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(content_type, "font/woff2");
    }

    #[tokio::test]
    async fn embedded_fallback_keeps_css_and_js_content_types() {
        let uri: axum::http::Uri = "/viewer.css".parse().unwrap();
        let response = embedded_fallback(uri).await.into_response();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(content_type, "text/css; charset=utf-8");
    }

    fn temp_session_dir(name: &str) -> PathBuf {
        let nanos = Local::now().timestamp_nanos_opt().unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "embed-log-ws-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_manager(dir: PathBuf) -> Arc<Mutex<SessionManager>> {
        let mut source_files = HashMap::new();
        source_files.insert("dut".to_string(), dir.join("dut.log").display().to_string());

        let mut pane_labels = HashMap::new();
        pane_labels.insert("dut".to_string(), "DUT".to_string());

        let mut pane_kinds = HashMap::new();
        pane_kinds.insert("dut".to_string(), "udp".to_string());

        Arc::new(Mutex::new(SessionManager::new(
            "session-1",
            dir.clone(),
            &[json!({ "label": "Main", "panes": ["dut"] })],
            source_files,
            dir.join("combined.jsonl").display().to_string(),
            pane_labels,
            pane_kinds,
            json!({}),
            json!({}),
            json!({}),
            json!({}),
            "2026-06-13T00:00:00+00:00",
            "embed-log",
            None,
            None,
            "absolute",
            None,
        )))
    }

    fn test_state(dir: PathBuf) -> (ServerState, broadcast::Receiver<String>) {
        let (broadcast_tx, rx) = broadcast::channel::<String>(16);
        let manager = test_manager(dir.clone());
        let state = ServerState {
            config_msg: Arc::new(Mutex::new(json!({ "type": "config" }).to_string())),
            broadcast_tx,
            replay: Arc::new(Mutex::new(VecDeque::new())),
            events_replay: Arc::new(Mutex::new(VecDeque::new())),
            on_export: Some(Arc::new(|| Ok("/tmp/session.html".to_string()))),
            on_rotate: None,
            session_manager: Some(manager),
            logs_root: dir,
            ws_client_count: Arc::new(AtomicUsize::new(0)),
            no_client_export_generation: Arc::new(AtomicU64::new(0)),
            no_client_export_delay: Duration::from_secs(3600),
            stats: Arc::new(RuntimeStats::empty()),
            source_txs: Arc::new(HashMap::new()),
            source_tx_senders: Arc::new(HashMap::new()),
            source_metadata: Arc::new(HashMap::new()),
            line_counters: Arc::new(HashMap::new()),
            static_event_rules: Arc::new(HashMap::new()),
            event_rules_path: std::env::temp_dir().join("embed-log.events.yml"),
            runtime_event_rules: Arc::new(std::sync::RwLock::new(HashMap::new())),
            control_api: true,
        };
        (state, rx)
    }

    #[tokio::test]
    async fn save_markers_accepts_cmd_shape_persists_and_broadcasts_update() {
        let dir = temp_session_dir("markers");
        let (state, mut rx) = test_state(dir.clone());

        let response = handle_client_command(
            r#"{"cmd":"save_markers","markers":[{"pane":"dut","line":7}]}"#,
            &state,
        )
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "markers_update");
        assert_eq!(response["markers"][0]["line"], 7);

        let broadcast: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(broadcast["type"], "markers_update");

        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("markers.json")).unwrap())
                .unwrap();
        assert_eq!(raw["session_id"], "session-1");
        assert_eq!(raw["markers"][0]["line"], 7);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn export_command_accepts_legacy_type_and_broadcasts_status() {
        let dir = temp_session_dir("export");
        let (state, mut rx) = test_state(dir.clone());

        let response = handle_client_command(r#"{"type":"export_session_html"}"#, &state)
            .await
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "session_html_status");
        assert_eq!(response["html_status"], "ready");

        let broadcast: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(broadcast["type"], "session_html_status");
        assert_eq!(broadcast["html_status"], "ready");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn clear_logs_accepts_cmd_shape_and_broadcasts_scope() {
        let dir = temp_session_dir("clear");
        let (state, mut rx) = test_state(dir.clone());

        let response = handle_client_command(r#"{"cmd":"clear_logs","pane":"dut"}"#, &state)
            .await
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "clear_logs");
        assert_eq!(response["scope"], "dut");

        let broadcast: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(broadcast["type"], "clear_logs");
        assert_eq!(broadcast["scope"], "dut");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_raw_to_writable_source_sends_tx_command() {
        let dir = temp_session_dir("send-raw-tx");
        let (mut state, _rx) = test_state(dir.clone());
        // Set up a TX sender channel for a writable source.
        let (tx_sender, mut tx_rx) = mpsc::channel::<super::TxCommand>(2);
        let mut source_tx_senders = HashMap::new();
        source_tx_senders.insert("dut".to_string(), tx_sender);
        state.source_tx_senders = Arc::new(source_tx_senders);

        let response = handle_client_command(
            r#"{"cmd":"send_raw","source_id":"dut","data":"help"}"#,
            &state,
        )
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "send_raw_result");
        assert_eq!(response["ok"], true);
        assert_eq!(response["source_id"], "dut");
        assert_eq!(response["bytes"], 4);

        // Verify the TxCommand was sent.
        let cmd = tx_rx.recv().await.unwrap();
        assert_eq!(cmd.data, b"help");
        assert_eq!(cmd.origin, "ui");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_raw_to_non_writable_source_returns_error() {
        let dir = temp_session_dir("send-raw-non-writable");
        let (mut state, _rx) = test_state(dir.clone());
        // Source exists in source_txs but not in source_tx_senders => non-writable.
        let (tx, _rx) = mpsc::channel::<crate::models::LogEntry>(2);
        let mut source_txs = HashMap::new();
        source_txs.insert("udp_source".to_string(), tx);
        state.source_txs = Arc::new(source_txs);

        let response = handle_client_command(
            r#"{"cmd":"send_raw","source_id":"udp_source","data":"help"}"#,
            &state,
        )
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "send_raw_result");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"], "source is not writable");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_raw_to_unknown_source_returns_error() {
        let dir = temp_session_dir("send-raw-unknown");
        let (state, _rx) = test_state(dir.clone());

        let response = handle_client_command(
            r#"{"cmd":"send_raw","source_id":"nonexistent","data":"help"}"#,
            &state,
        )
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "send_raw_result");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"], "unknown source");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_raw_empty_data_returns_error() {
        let dir = temp_session_dir("send-raw-empty");
        let (state, _rx) = test_state(dir.clone());

        let response =
            handle_client_command(r#"{"cmd":"send_raw","source_id":"dut","data":""}"#, &state)
                .await
                .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "send_raw_result");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"], "empty data");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_raw_accepts_frontend_id_field() {
        let dir = temp_session_dir("send-raw-frontend");
        let (mut state, _rx) = test_state(dir.clone());
        // The frontend sends { cmd: "send_raw", id: paneId, data: text + "\n" }
        let (tx_sender, mut tx_rx) = mpsc::channel::<crate::sources::TxCommand>(2);
        let mut source_tx_senders = HashMap::new();
        source_tx_senders.insert("DUT_UART".to_string(), tx_sender);
        state.source_tx_senders = Arc::new(source_tx_senders);

        let response = handle_client_command(
            r#"{"cmd":"send_raw","id":"DUT_UART","data":"version\n"}"#,
            &state,
        )
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "send_raw_result");
        assert_eq!(response["ok"], true);
        assert_eq!(response["source_id"], "DUT_UART");
        assert_eq!(response["bytes"], 8);

        let cmd = tx_rx.recv().await.unwrap();
        assert_eq!(cmd.data, b"version\n");
        assert_eq!(cmd.origin, "ui");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn send_raw_closed_channel_returns_error() {
        let dir = temp_session_dir("send-raw-closed");
        let (mut state, _rx) = test_state(dir.clone());
        // Create a channel and drop the receiver immediately to simulate a closed
        // write channel (e.g., UART source task crashed / port disconnected).
        let (tx_sender, _tx_rx) = mpsc::channel::<crate::sources::TxCommand>(2);
        // Drop the receiver so send fails.
        drop(_tx_rx);
        let mut source_tx_senders = HashMap::new();
        source_tx_senders.insert("dut".to_string(), tx_sender);
        state.source_tx_senders = Arc::new(source_tx_senders);

        let response = handle_client_command(
            r#"{"cmd":"send_raw","source_id":"dut","data":"help"}"#,
            &state,
        )
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "send_raw_result");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"], "source write channel closed");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn runtime_stats_snapshot_reports_per_source_counters() {
        let stats = RuntimeStats::new(["dut".to_string()], 32);
        let dut = stats.source("dut").unwrap();
        dut.record_dequeued(12);
        dut.record_dequeued(30);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot["dut"]["maxsize"], 32);
        assert_eq!(snapshot["dut"]["dequeued"], 2);
        assert_eq!(snapshot["dut"]["bytes"], 42);
    }

    #[tokio::test]
    async fn final_client_disconnect_schedules_no_client_export() {
        let count = Arc::new(AtomicUsize::new(1));
        let generation = Arc::new(AtomicU64::new(0));
        let exports = Arc::new(AtomicUsize::new(0));
        let (broadcast_tx, mut rx) = broadcast::channel(4);
        let exports_for_callback = exports.clone();

        {
            let _guard = WsClientCountGuard {
                count: count.clone(),
                generation: generation.clone(),
                delay: Duration::from_millis(1),
                on_export: Some(Arc::new(move || {
                    exports_for_callback.fetch_add(1, Ordering::Relaxed);
                    Ok("/tmp/session.html".to_string())
                })),
                broadcast_tx,
            };
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(exports.load(Ordering::Relaxed), 1);
        let payload: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(payload["type"], "session_html_status");
        assert_eq!(payload["reason"], "no_clients");
    }

    #[tokio::test]
    async fn reconnect_generation_cancels_pending_no_client_export() {
        let count = Arc::new(AtomicUsize::new(1));
        let generation = Arc::new(AtomicU64::new(0));
        let exports = Arc::new(AtomicUsize::new(0));
        let (broadcast_tx, _rx) = broadcast::channel(4);
        let exports_for_callback = exports.clone();

        {
            let _guard = WsClientCountGuard {
                count: count.clone(),
                generation: generation.clone(),
                delay: Duration::from_millis(20),
                on_export: Some(Arc::new(move || {
                    exports_for_callback.fetch_add(1, Ordering::Relaxed);
                    Ok("/tmp/session.html".to_string())
                })),
                broadcast_tx,
            };
        }

        count.fetch_add(1, Ordering::Relaxed);
        generation.fetch_add(1, Ordering::Relaxed);

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(exports.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn events_replay_drain_returns_messages_in_order() {
        let buf = Arc::new(Mutex::new(VecDeque::new()));
        {
            let mut b = buf.lock().unwrap();
            b.push_back(r#"{"type":"rx","data":"boot complete","line_idx":0}"#.to_string());
            b.push_back(r#"{"type":"event","event_id":"boot","severity":"info"}"#.to_string());
            b.push_back(r#"{"type":"rx","data":"FATAL ERROR","line_idx":1}"#.to_string());
        }

        let msgs = drain_replay(&buf);
        assert_eq!(msgs.len(), 3);
        assert_eq!(
            msgs[0],
            r#"{"type":"rx","data":"boot complete","line_idx":0}"#
        );
        assert_eq!(
            msgs[1],
            r#"{"type":"event","event_id":"boot","severity":"info"}"#
        );
        assert_eq!(
            msgs[2],
            r#"{"type":"rx","data":"FATAL ERROR","line_idx":1}"#
        );
    }

    #[test]
    fn events_replay_drain_empty_returns_empty() {
        let buf = Arc::new(Mutex::new(VecDeque::new()));
        let msgs = drain_replay(&buf);
        assert!(msgs.is_empty());
    }

    #[test]
    fn events_replay_drain_does_not_clear_buffer() {
        let buf = Arc::new(Mutex::new(VecDeque::new()));
        {
            let mut b = buf.lock().unwrap();
            b.push_back(r#"{"type":"event","event_id":"e1"}"#.to_string());
        }

        let first = drain_replay(&buf);
        let second = drain_replay(&buf);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1, "buffer should be intact after drain");
        assert_eq!(first[0], second[0]);
    }
}
