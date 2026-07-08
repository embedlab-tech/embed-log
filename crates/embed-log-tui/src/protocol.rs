//! Typed models for the embed-log `/ws` protocol.
//!
//! These mirror the JSON shapes produced by `embed-log-core`:
//! - config message: `build_ws_config_message` (`runtime/server.rs`)
//! - live log payload: `run_writer` (`runtime/server.rs`)
//! - event payload: `run_writer` event branch
//! - session info: `SessionManager::build_session_info`
//! - markers_update / session_info / session_html_status / session_rotated / clear_logs
//!
//! The server sends `serde_json::Value`-based JSON; we deserialize into typed structs
//! where fields are stable, and keep a `serde_json::Value` escape hatch for fields we
//! don't yet care about (plugin scripts, frontend plugin definitions). Unknown `type`
//! variants fall through to [`ServerMessage::Unknown`] so the client never panics on a
//! new server message kind.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Top-level server→client message, tagged on the `type` field.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Initial config + replay handshake (first message on connect).
    Config(ConfigMessage),
    /// Live or replayed log line (`type: "rx"` or `"tx"`).
    #[serde(rename = "rx")]
    Rx(LogPayload),
    #[serde(rename = "tx")]
    Tx(LogPayload),
    /// Real-time or replayed event detection hit.
    Event(EventPayload),
    /// Session metadata update (first_log_at set, rotation, etc.).
    SessionInfo(SessionInfoMessage),
    /// Marker set replaced (user toggle or event-marker creation).
    MarkersUpdate(MarkersUpdate),
    /// HTML export status change.
    SessionHtmlStatus(SessionHtmlStatus),
    /// Session rotation completed; a fresh config message follows.
    SessionRotated(SessionRotated),
    /// Pane clear broadcast.
    ClearLogs(ClearLogs),
    /// Filter validation result (response to `set_filter`).
    FilterResult(Value),
    /// TX write result (response to `send_raw`).
    SendRawResult(Value),
    /// Anything else — kept as raw JSON so the client is forward-compatible.
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------
// Config message
// ---------------------------------------------------------------------------

/// `type: "config"` — the first message on `/ws` connect.
///
/// See `build_ws_config_message` in `runtime/server.rs`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigMessage {
    /// App name shown in the status bar.
    #[serde(default)]
    pub app_name: String,
    /// Default light/dark theme ids (applied on connect).
    #[serde(default)]
    pub theme_defaults: ThemeDefaults,
    /// Current session metadata (id, paths, html_status, first_log_at, …).
    #[serde(default)]
    pub session: Value,
    /// `pane_id → human label`.
    #[serde(default)]
    pub pane_labels: HashMap<String, String>,
    /// `pane_id → source type` (`"udp"`, `"uart"`, `"file"`, `"network_capture"`).
    #[serde(default)]
    pub pane_kinds: HashMap<String, String>,
    /// `pane_id → UART command suggestion list` (companion `.commands.yml`).
    #[serde(default)]
    pub pane_commands: HashMap<String, Vec<String>>,
    /// Tab definitions (label + panes + per-tab pane_labels).
    #[serde(default)]
    pub tabs: Vec<TabDef>,
    /// Frontend plugin definitions (kept raw — TUI only runs Rust builtins).
    #[serde(default)]
    pub frontend_plugins: Value,
    /// `pane_id → list of plugin entries`.
    #[serde(default)]
    pub pane_plugins: HashMap<String, Vec<PanePluginEntry>>,
    /// `plugin_name → JS source` (ignored by TUI; JS can't run here).
    #[serde(default)]
    pub plugin_scripts: Value,
    /// `source_id → list of rule summaries` (name + severity).
    #[serde(default)]
    pub event_rules: HashMap<String, Vec<EventRuleSummary>>,
    /// Existing markers (user + event).
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// Light/dark theme defaults from config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeDefaults {
    #[serde(default)]
    pub light: Option<String>,
    #[serde(default)]
    pub dark: Option<String>,
}

/// A tab in the config message: label + pane ids + per-tab pane labels.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TabDef {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub panes: Vec<String>,
    /// Per-tab label override (usually a subset of top-level pane_labels).
    #[serde(default)]
    pub pane_labels: HashMap<String, String>,
}

/// A plugin entry attached to a pane: bare name or `{name, options}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PanePluginEntry {
    Bare(String),
    Detailed {
        #[serde(default)]
        name: String,
        #[serde(default)]
        options: Value,
    },
}

impl PanePluginEntry {
    /// Plugin name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            Self::Bare(n) => n,
            Self::Detailed { name, .. } => name,
        }
    }
}

/// Rule summary from the config message (`event_rules[source][i]`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventRuleSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub severity: String,
}

// ---------------------------------------------------------------------------
// Live log payload
// ---------------------------------------------------------------------------

/// `type: "rx" | "tx"` — one log line.
///
/// The server sends both `data` (ANSI-wrapped) and `message` (raw). The TUI uses
/// `message` and applies its own styling, but keeps `data` available for parity.
/// Both absolute and relative timestamp variants are sent so the timestamp-mode
/// toggle needs no recompute round-trip.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogPayload {
    /// ANSI-wrapped message (server-applied color).
    #[serde(default)]
    pub data: String,
    /// Raw logical message (no ANSI).
    #[serde(default)]
    pub message: String,
    /// Display timestamp string (absolute): `"06-14 09:30:45.123"`.
    #[serde(default)]
    pub timestamp: String,
    /// RFC3339 timestamp.
    #[serde(default)]
    pub timestamp_iso: String,
    /// Epoch millis (absolute).
    #[serde(default)]
    pub timestamp_num: f64,
    /// Source / pane id.
    #[serde(default)]
    pub source_id: String,
    /// Stable per-source line counter.
    #[serde(default)]
    pub line_idx: u64,
    /// `"SERIAL"`, `"ui"` (TX), `"TX::<origin>"`, or injected origin.
    #[serde(default)]
    pub origin: String,
    /// Color name (`"cyan"`, …) or null.
    #[serde(default)]
    pub color: Option<String>,
    /// Absolute timestamp string (duplicate of `timestamp`).
    #[serde(default, rename = "absTs")]
    pub abs_ts: String,
    /// Absolute epoch millis (duplicate of `timestamp_num`).
    #[serde(default, rename = "absNum")]
    pub abs_num: f64,
    /// Relative timestamp string: `"00:00:45.123"`.
    #[serde(default, rename = "relTs")]
    pub rel_ts: String,
    /// Relative millis from first log.
    #[serde(default, rename = "relNum")]
    pub rel_num: f64,
}

// ---------------------------------------------------------------------------
// Event payload
// ---------------------------------------------------------------------------

/// `type: "event"` — one event-detection hit.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventPayload {
    #[serde(default)]
    pub event_id: String,
    #[serde(default)]
    pub source_id: String,
    /// `"info" | "warn" | "error" | "fatal"`.
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub timestamp_iso: String,
    #[serde(default)]
    pub timestamp_num: f64,
    #[serde(default)]
    pub rel_num: f64,
    #[serde(default)]
    pub line_idx: u64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub captures: Vec<String>,
}

// ---------------------------------------------------------------------------
// Session info
// ---------------------------------------------------------------------------

/// `type: "session_info"` — wraps a [`SessionInfo`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionInfoMessage {
    #[serde(default)]
    pub session: SessionInfo,
}

/// Session metadata (shape from `SessionManager::build_session_info`).
///
/// Kept loose: stable fields are typed, the rest live in [`Self::rest`] so the TUI
/// survives additive server changes without recompiles.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub html_ready: bool,
    #[serde(default)]
    pub html_status: String,
    #[serde(default)]
    pub html_updated_at: Option<String>,
    #[serde(default)]
    pub html_error: Option<String>,
    #[serde(default)]
    pub started_at: String,
    /// `"absolute" | "relative"`.
    #[serde(default)]
    pub timestamp_mode: String,
    /// RFC3339 of first log, or null before any log arrives.
    #[serde(default)]
    pub first_log_at: Option<String>,
    #[serde(default)]
    pub tabs: Vec<TabDef>,
    #[serde(default)]
    pub pane_labels: HashMap<String, String>,
    #[serde(default)]
    pub pane_kinds: HashMap<String, String>,
    #[serde(default)]
    pub sources: HashMap<String, String>,
    /// Catch-all for fields the TUI doesn't individually use (api map, plugin blobs).
    #[serde(flatten)]
    pub rest: HashMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

/// `type: "markers_update"` — full marker set replacement.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MarkersUpdate {
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub session: Value,
}

/// A marker on a log line (user or event).
///
/// Shape matches `save_event_marker` and the frontend `save_markers` command.
/// Missing `kind` defaults to `"user"` (backward compatible with older sessions).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    #[serde(default)]
    pub pane_id: String,
    #[serde(default)]
    pub line_idx: u64,
    #[serde(default)]
    pub end_idx: u64,
    #[serde(default)]
    pub num_ts: f64,
    #[serde(default)]
    pub description: String,
    /// `"user"` (default if missing) or `"event"`.
    #[serde(default = "default_user_kind")]
    pub kind: String,
    /// Severity for event markers; empty for user markers.
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub created_at: String,
}

fn default_user_kind() -> String {
    "user".to_string()
}

impl Marker {
    /// Whether this is an event-detected marker (vs a user-placed one).
    pub fn is_event(&self) -> bool {
        self.kind == "event"
    }
}

// ---------------------------------------------------------------------------
// Status / rotation / clear
// ---------------------------------------------------------------------------

/// `type: "session_html_status"` — export progress/result.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionHtmlStatus {
    /// `"ready" | "updating" | "error" | "pending"`.
    #[serde(default)]
    pub html_status: String,
    #[serde(default)]
    pub html_path: Option<String>,
    #[serde(default)]
    pub html_error: Option<String>,
    /// Why the export happened (`"no_clients"`, `"rotate"`, …).
    #[serde(default)]
    pub reason: Option<String>,
    /// Fresh session info sometimes attached by the server.
    #[serde(default)]
    pub session: Option<Value>,
}

/// `type: "session_rotated"` — old session closed, new one started.
///
/// The server follows this with a fresh `type: "config"` message; the client
/// should tear down its layout and rebuild from the new config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionRotated {
    #[serde(default)]
    pub old_session: Value,
    #[serde(default)]
    pub session: Value,
}

/// `type: "clear_logs"` — clear one pane or all panes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClearLogs {
    /// Pane id, or null/absent meaning "all panes".
    #[serde(default)]
    pub pane: Option<String>,
    /// Effective scope echoed by the server: the pane id or `"all"`.
    #[serde(default)]
    pub scope: String,
}

// ---------------------------------------------------------------------------
// Client → server commands
// ---------------------------------------------------------------------------

/// Commands the TUI sends to the server over `/ws` (same `handle_client_command`
/// shapes the browser frontend uses). Serialized with `serde_json::to_string`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    ExportSessionHtml,
    SaveMarkers {
        markers: Vec<Marker>,
    },
    ClearLogs {
        #[serde(skip_serializing_if = "Option::is_none")]
        pane: Option<String>,
    },
    SetFilter {
        /// Pane/source id.
        id: String,
        /// Regex string (empty clears the filter).
        filter: String,
    },
    SendRaw {
        /// Pane/source id.
        id: String,
        /// Bytes to write (the TUI appends `\n` for interactive TX).
        data: String,
    },
}

impl ClientCommand {
    /// Serialize to the JSON string the `/ws` handler expects.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_message_minimal() {
        let json = r#"{"type":"config","app_name":"demo","tabs":[]}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        match msg {
            ServerMessage::Config(c) => {
                assert_eq!(c.app_name, "demo");
                assert!(c.tabs.is_empty());
            }
            other => panic!("expected Config, got {other:?}"),
        }
    }

    #[test]
    fn parses_full_config_message() {
        let json = r#"{
            "type":"config",
            "app_name":"embed-log demo",
            "theme_defaults":{"light":"gruvbox-light","dark":"gruvbox-dark"},
            "session":{"id":"2026-06-14_09-30-00","first_log_at":null,"timestamp_mode":"absolute"},
            "pane_labels":{"DUT":"DUT Device","UART_DUT":"UART Main"},
            "pane_kinds":{"DUT":"udp","UART_DUT":"uart"},
            "pane_commands":{"UART_DUT":["help\r\n","version\r\n"]},
            "tabs":[{"label":"Device","panes":["DUT","HOST"],"pane_labels":{"DUT":"DUT Device"}}],
            "frontend_plugins":{"hex-coap":{"builtin":"hex-coap"}},
            "pane_plugins":{"COAP_RAW":[{"name":"hex-coap"}]},
            "plugin_scripts":{"hex-coap":"/* js */"},
            "event_rules":{"DUT":[{"name":"fatal_error","severity":"error"}]},
            "markers":[{"paneId":"DUT","lineIdx":3,"endIdx":3,"numTs":5000.0,"description":"note","kind":"user"}]
        }"#;
        let ServerMessage::Config(c) = serde_json::from_str(json).unwrap() else {
            panic!("not config");
        };
        assert_eq!(c.app_name, "embed-log demo");
        assert_eq!(c.theme_defaults.dark.as_deref(), Some("gruvbox-dark"));
        assert_eq!(c.pane_kinds.get("UART_DUT").unwrap(), "uart");
        assert_eq!(
            c.pane_commands.get("UART_DUT").unwrap(),
            &["help\r\n", "version\r\n"]
        );
        assert_eq!(c.tabs.len(), 1);
        assert_eq!(c.tabs[0].label, "Device");
        assert_eq!(c.tabs[0].panes, ["DUT", "HOST"]);
        assert_eq!(c.event_rules.get("DUT").unwrap()[0].severity, "error");
        assert_eq!(c.markers.len(), 1);
        assert_eq!(c.markers[0].pane_id, "DUT");
        assert!(!c.markers[0].is_event());
        // pane_plugins entry: detailed variant
        let entry = &c.pane_plugins.get("COAP_RAW").unwrap()[0];
        assert_eq!(entry.name(), "hex-coap");
    }

    #[test]
    fn parses_rx_payload() {
        let json = r#"{
            "type":"rx","data":"\u001b[36mboot\u001b[0m","message":"boot",
            "timestamp":"06-14 09:30:45.123","timestamp_iso":"2026-06-14T09:30:45.123Z",
            "timestamp_num":1718347845123.0,"source_id":"DUT","line_idx":42,
            "origin":"SERIAL","color":"cyan",
            "absTs":"06-14 09:30:45.123","absNum":1718347845123.0,
            "relTs":"00:00:45.123","relNum":45123.0
        }"#;
        let ServerMessage::Rx(p) = serde_json::from_str(json).unwrap() else {
            panic!("not rx");
        };
        assert_eq!(p.source_id, "DUT");
        assert_eq!(p.line_idx, 42);
        assert_eq!(p.origin, "SERIAL");
        assert_eq!(p.color.as_deref(), Some("cyan"));
        assert!((p.rel_num - 45123.0).abs() < 1e-6);
    }

    #[test]
    fn parses_tx_payload() {
        let json = r#"{"type":"tx","data":"version","message":"version",
            "timestamp":"06-14 09:30:46.000","timestamp_iso":"","timestamp_num":1718347846000.0,
            "source_id":"UART_DUT","line_idx":7,"origin":"ui","color":null,
            "absTs":"","absNum":0.0,"relTs":"00:00:46.000","relNum":46000.0}"#;
        let ServerMessage::Tx(p) = serde_json::from_str(json).unwrap() else {
            panic!("not tx");
        };
        assert_eq!(p.source_id, "UART_DUT");
        assert_eq!(p.origin, "ui");
        assert!(p.color.is_none());
    }

    #[test]
    fn parses_event_payload() {
        let json = r#"{"type":"event","event_id":"fatal_error","source_id":"DUT",
            "severity":"error","timestamp":"06-14 09:30:45.123","timestamp_iso":"",
            "timestamp_num":1718347845123.0,"rel_num":45123.0,"line_idx":42,
            "message":"ZEPHYR FATAL ERROR","origin":"SERIAL","captures":["FATAL ERROR"]}"#;
        let ServerMessage::Event(e) = serde_json::from_str(json).unwrap() else {
            panic!("not event");
        };
        assert_eq!(e.event_id, "fatal_error");
        assert_eq!(e.severity, "error");
        assert_eq!(e.captures, ["FATAL ERROR"]);
    }

    #[test]
    fn parses_markers_update() {
        let json = r#"{"type":"markers_update","markers":[
            {"paneId":"DUT","lineIdx":1,"endIdx":1,"numTs":100.0,"description":"u","kind":"user"},
            {"paneId":"DUT","lineIdx":2,"endIdx":2,"numTs":200.0,"description":"e: boom","kind":"event","severity":"error"}
        ],"session":null}"#;
        let ServerMessage::MarkersUpdate(m) = serde_json::from_str(json).unwrap() else {
            panic!("not markers_update");
        };
        assert_eq!(m.markers.len(), 2);
        assert!(!m.markers[0].is_event());
        assert!(m.markers[1].is_event());
        assert_eq!(m.markers[1].severity, "error");
    }

    #[test]
    fn parses_session_info_and_first_log_at() {
        let json = r#"{"type":"session_info","session":{
            "id":"s1","app_name":"demo","dir":"/tmp/s","html":"/sessions/s1/session.html",
            "html_ready":false,"html_status":"pending","started_at":"2026-06-14T09:30:00Z",
            "timestamp_mode":"absolute","first_log_at":"2026-06-14T09:30:45.123Z",
            "tabs":[],"pane_labels":{},"pane_kinds":{},"sources":{}
        }}"#;
        let ServerMessage::SessionInfo(s) = serde_json::from_str(json).unwrap() else {
            panic!("not session_info");
        };
        assert_eq!(s.session.id, "s1");
        assert_eq!(
            s.session.first_log_at.as_deref(),
            Some("2026-06-14T09:30:45.123Z")
        );
        assert_eq!(s.session.timestamp_mode, "absolute");
    }

    #[test]
    fn parses_session_html_status() {
        let json = r#"{"type":"session_html_status","html_status":"ready",
            "html_path":"/tmp/s/session.html","reason":"no_clients"}"#;
        let ServerMessage::SessionHtmlStatus(s) = serde_json::from_str(json).unwrap() else {
            panic!("not session_html_status");
        };
        assert_eq!(s.html_status, "ready");
        assert_eq!(s.reason.as_deref(), Some("no_clients"));
    }

    #[test]
    fn parses_session_rotated() {
        let json =
            r#"{"type":"session_rotated","old_session":{"id":"old"},"session":{"id":"new"}}"#;
        let ServerMessage::SessionRotated(r) = serde_json::from_str(json).unwrap() else {
            panic!("not session_rotated");
        };
        assert_eq!(r.session.get("id").and_then(|v| v.as_str()), Some("new"));
    }

    #[test]
    fn parses_clear_logs() {
        let json = r#"{"type":"clear_logs","pane":"DUT","scope":"DUT"}"#;
        let ServerMessage::ClearLogs(c) = serde_json::from_str(json).unwrap() else {
            panic!("not clear_logs");
        };
        assert_eq!(c.pane.as_deref(), Some("DUT"));
        assert_eq!(c.scope, "DUT");
    }

    #[test]
    fn parses_clear_logs_all_scope() {
        let json = r#"{"type":"clear_logs","scope":"all"}"#;
        let ServerMessage::ClearLogs(c) = serde_json::from_str(json).unwrap() else {
            panic!("not clear_logs");
        };
        assert!(c.pane.is_none());
        assert_eq!(c.scope, "all");
    }

    #[test]
    fn unknown_type_does_not_error() {
        let json = r#"{"type":"some_future_message","payload":42}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ServerMessage::Unknown));
    }

    #[test]
    fn client_command_export_serializes_to_legacy_shape() {
        // The server's handle_client_command matches on "type":"export_session_html".
        let cmd = ClientCommand::ExportSessionHtml;
        assert_eq!(cmd.to_json(), r#"{"type":"export_session_html"}"#);
    }

    #[test]
    fn client_command_send_raw_serializes_with_id_and_data() {
        let cmd = ClientCommand::SendRaw {
            id: "UART_DUT".into(),
            data: "version\n".into(),
        };
        let s = cmd.to_json();
        assert!(s.contains(r#""type":"send_raw""#));
        assert!(s.contains(r#""id":"UART_DUT""#));
        assert!(s.contains(r#""data":"version\n""#));
    }

    #[test]
    fn client_command_clear_logs_omits_pane_when_none() {
        let cmd = ClientCommand::ClearLogs { pane: None };
        let s = cmd.to_json();
        assert!(!s.contains(r#""pane""#));
    }

    #[test]
    fn client_command_set_filter_serializes() {
        let cmd = ClientCommand::SetFilter {
            id: "DUT".into(),
            filter: "FATAL".into(),
        };
        let s = cmd.to_json();
        assert!(s.contains(r#""id":"DUT""#));
        assert!(s.contains(r#""filter":"FATAL""#));
    }

    #[test]
    fn marker_without_kind_defaults_to_user() {
        // Older sessions/markers may omit `kind`; the TUI must treat them as user markers.
        let json = r#"{"paneId":"DUT","lineIdx":1,"endIdx":1,"numTs":100.0,"description":"old"}"#;
        let m: Marker = serde_json::from_str(json).unwrap();
        assert_eq!(m.kind, "user");
        assert!(!m.is_event());
    }
}
