//! First-run onboarding shared by the Tauri desktop app and the CLI/browser UI.
//!
//! This module owns everything reusable about onboarding:
//!
//! - the quick-config data types exchanged with `frontend/onboarding.js`
//! - `build_quick_config_yaml` — turns the draft into validated YAML
//! - `save_quick_config` — writes that YAML to disk and validates it
//! - `list_serial_ports` — OS serial port discovery
//! - `OnboardingServer` — a tiny HTTP server that serves the onboarding page
//!   and the `serial_ports` / `server_status` / `save_config` endpoints
//!
//! Both the Tauri app and the CLI run the exact same `OnboardingServer`. The
//! only platform-specific behaviour is injected through a `SaveHandler`
//! closure, which lets Tauri start its `LogServer` at save time while the CLI
//! simply writes the config and proceeds to start its own server afterwards.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::load_config;
use crate::frontend_assets::FrontendAssets;

// ───────────────────────── Data types ─────────────────────────

/// Draft config sent from the onboarding UI.
#[derive(Debug, Deserialize)]
pub struct QuickConfigDraft {
    pub app_name: Option<String>,
    pub ws_port: Option<u16>,
    pub logs_dir: Option<String>,
    pub baudrate: Option<u32>,
    pub sources: Vec<QuickSourceDraft>,
    pub tabs: Vec<QuickTabDraft>,
}

#[derive(Debug, Deserialize)]
pub struct QuickSourceDraft {
    pub name: String,
    pub label: Option<String>,
    #[serde(rename = "source_type")]
    pub source_type: String,
    pub port: String,
    pub parser: Option<String>,
    pub baudrate: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct QuickTabDraft {
    pub label: String,
    pub panes: Vec<String>,
}

/// Result returned to the UI after saving.
#[derive(Debug, Clone, Serialize)]
pub struct QuickConfigResult {
    pub config_path: String,
    pub url: String,
    pub ws_port: u16,
}

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    pub running: bool,
    pub config_path: String,
    pub ws_port: u16,
    pub url: String,
}

/// Body of `POST /api/save_config`.
#[derive(Debug, Deserialize)]
pub struct SaveQuickConfigRequest {
    pub draft: QuickConfigDraft,
}

// ───────────────────────── Logic ─────────────────────────

/// Discover available serial ports on this machine.
pub fn list_serial_ports() -> Vec<String> {
    serialport::available_ports()
        .unwrap_or_default()
        .iter()
        .map(|p| p.port_name.clone())
        .collect()
}

/// Build a server status snapshot for `GET /api/server_status`.
///
/// `config_path` is the resolved path onboarding will write to. If a config
/// already exists there (e.g. on a later onboarding run), its `ws_port` is
/// used; otherwise the default `8080` is reported.
pub fn server_status(config_path: &Path) -> ServerStatus {
    let config = load_config(config_path).ok();
    let ws_port = config.as_ref().map(|c| c.server.ws_port).unwrap_or(8080);
    ServerStatus {
        running: false,
        config_path: config_path.display().to_string(),
        ws_port,
        url: format!("http://127.0.0.1:{ws_port}/"),
    }
}

/// Turn an onboarding draft into validated YAML.
///
/// This is the single source of truth for the generated config shape; both the
/// CLI and Tauri use it.
pub fn build_quick_config_yaml(draft: &QuickConfigDraft) -> Result<String, String> {
    use serde_yaml::{Mapping, Number, Value};
    use std::collections::HashSet;

    if draft.sources.is_empty() {
        return Err("add at least one source".to_string());
    }
    if draft.tabs.is_empty() {
        return Err("add at least one tab".to_string());
    }

    fn key(name: &str) -> Value {
        Value::String(name.to_string())
    }
    fn string(value: impl Into<String>) -> Value {
        Value::String(value.into())
    }
    fn number(value: impl Into<i64>) -> Value {
        Value::Number(Number::from(value.into()))
    }

    let app_name = draft
        .app_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("embed-log");
    let ws_port = draft.ws_port.unwrap_or(8080);
    let logs_dir = draft
        .logs_dir
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("logs/");
    let default_baudrate = draft.baudrate.unwrap_or(115200);

    let mut source_names = HashSet::new();
    let mut sources = Vec::new();
    for src in &draft.sources {
        let name = src.name.trim();
        if name.is_empty() {
            return Err("every source needs a name".to_string());
        }
        if !source_names.insert(name.to_string()) {
            return Err(format!("duplicate source name: {name}"));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!(
                "source name '{name}' can only contain letters, numbers, '-' and '_'"
            ));
        }

        let source_type = src.source_type.trim();
        if !matches!(source_type, "uart" | "udp" | "file" | "network_capture") {
            return Err(format!("unsupported source type: {source_type}"));
        }
        let port = src.port.trim();
        if source_type != "network_capture" && port.is_empty() {
            return Err(format!("source '{name}' needs a port/path"));
        }

        let mut map = Mapping::new();
        map.insert(key("name"), string(name));
        map.insert(key("type"), string(source_type));
        if source_type == "udp" {
            let udp_port: u16 = port
                .parse()
                .map_err(|_| format!("source '{name}' needs a numeric UDP port"))?;
            map.insert(key("port"), number(i64::from(udp_port)));
        } else if source_type == "network_capture" {
            if !port.is_empty() {
                map.insert(key("interface"), string(port));
            }
        } else {
            map.insert(key("port"), string(port));
        }
        if source_type == "uart" {
            map.insert(
                key("baudrate"),
                number(i64::from(src.baudrate.unwrap_or(default_baudrate))),
            );
        }
        if let Some(label) = src
            .label
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            map.insert(key("label"), string(label));
        }
        let parser_type = src
            .parser
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("text");
        let parser_type = if parser_type == "cbor" {
            "cbor-datagram"
        } else {
            parser_type
        };
        if parser_type == "cbor-datagram" && source_type != "udp" {
            return Err(format!(
                "source '{name}' uses CBOR parser, which is only supported for UDP sources"
            ));
        }
        if !matches!(parser_type, "text" | "cbor-datagram") {
            return Err(format!(
                "unsupported parser type for source '{name}': {parser_type}"
            ));
        }
        let mut parser = Mapping::new();
        parser.insert(key("type"), string(parser_type));
        map.insert(key("parser"), Value::Mapping(parser));
        sources.push(Value::Mapping(map));
    }

    let mut tabs = Vec::new();
    for tab in &draft.tabs {
        let label = tab.label.trim();
        if label.is_empty() {
            return Err("every tab needs a label".to_string());
        }
        if tab.panes.is_empty() || tab.panes.len() > 2 {
            return Err(format!("tab '{label}' needs one or two panes"));
        }
        let mut panes = Vec::new();
        for pane in &tab.panes {
            let pane = pane.trim();
            if !source_names.contains(pane) {
                return Err(format!("tab '{label}' references unknown source '{pane}'"));
            }
            panes.push(string(pane));
        }
        let mut map = Mapping::new();
        map.insert(key("label"), string(label));
        map.insert(key("panes"), Value::Sequence(panes));
        tabs.push(Value::Mapping(map));
    }

    let mut server = Mapping::new();
    server.insert(key("app_name"), string(app_name));
    server.insert(key("ws_port"), number(i64::from(ws_port)));

    let mut logs = Mapping::new();
    logs.insert(key("dir"), string(logs_dir));

    let mut root = Mapping::new();
    root.insert(key("version"), number(1));
    root.insert(key("baudrate"), number(i64::from(default_baudrate)));
    root.insert(key("sources"), Value::Sequence(sources));
    root.insert(key("tabs"), Value::Sequence(tabs));
    root.insert(key("server"), Value::Mapping(server));
    root.insert(key("logs"), Value::Mapping(logs));

    serde_yaml::to_string(&Value::Mapping(root)).map_err(|e| e.to_string())
}

/// Write the onboarding draft to `config_path` and validate it by reloading.
///
/// Returns a [`QuickConfigResult`] describing where the config landed and the
/// URL the browser should navigate to next.
pub fn save_quick_config(
    config_path: &Path,
    draft: &QuickConfigDraft,
) -> Result<QuickConfigResult, String> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }

    let yaml = build_quick_config_yaml(draft)?;
    std::fs::write(config_path, yaml).map_err(|e| format!("write config: {e}"))?;

    let config =
        load_config(config_path).map_err(|e| format!("generated config is invalid: {e}"))?;
    let ws_port = config.server.ws_port;
    Ok(QuickConfigResult {
        config_path: config_path.display().to_string(),
        url: format!("http://127.0.0.1:{ws_port}/"),
        ws_port,
    })
}

/// The raw onboarding JavaScript (the contents of `frontend/onboarding.js`).
/// Used by the Tauri webview eval fallback.
pub fn onboarding_script() -> String {
    FrontendAssets::get("onboarding.js")
        .map(|file| String::from_utf8_lossy(&file.data).into_owned())
        .unwrap_or_default()
}

/// The onboarding page HTML — the embedded `onboarding.js` wrapped so it runs
/// as a standalone document in any browser or webview.
pub fn onboarding_html() -> String {
    let js = onboarding_script();
    if js.is_empty() {
        return "<!doctype html><html><body><h1>embed-log setup</h1>\
                <p>onboarding.js is missing from this build.</p></body></html>"
            .to_string();
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>embed-log setup</title></head><body><script>{js}</script></body></html>"
    )
}

// ───────────────────────── Reusable onboarding HTTP server ─────────────────────────

/// Platform-specific behaviour executed when the user saves a config.
///
/// The closure receives the resolved config path and the validated draft. It
/// must persist the config (via [`save_quick_config`] or otherwise) and return
/// a [`QuickConfigResult`]. The Tauri app uses this hook to also start its
/// `LogServer` at save time; the CLI uses [`default_save_handler`] and starts
/// its own server after the save is reported.
pub type SaveHandler =
    Arc<dyn Fn(PathBuf, QuickConfigDraft) -> Result<QuickConfigResult, String> + Send + Sync>;

/// The default save handler: just writes + validates the config. Used by the
/// CLI, which starts its `LogServer` after [`OnboardingServer::wait_for_save`].
pub fn default_save_handler() -> SaveHandler {
    Arc::new(|path: PathBuf, draft: QuickConfigDraft| save_quick_config(&path, &draft))
}

struct OnboardingState {
    config_path: PathBuf,
    save_handler: SaveHandler,
    result_tx: Mutex<Option<mpsc::Sender<QuickConfigResult>>>,
    /// Set to true after a successful save so the accept loop exits.
    shutdown: Arc<AtomicBool>,
}

/// A lightweight onboarding HTTP server.
///
/// Serves:
/// - `GET /` — the onboarding page
/// - `GET /api/serial_ports` — discovered serial ports
/// - `GET /api/server_status` — resolved config path + ws port
/// - `POST /api/save_config` — persist the draft via the `save_handler`
///
/// The server runs on a random localhost port on a dedicated thread. Call
/// [`OnboardingServer::wait_for_save`] (CLI) to block until the user saves.
pub struct OnboardingServer {
    pub base_url: String,
    result_rx: mpsc::Receiver<QuickConfigResult>,
}

impl OnboardingServer {
    /// Bind a random localhost port and start serving.
    ///
    /// `save_handler` performs the actual save and any platform-specific
    /// follow-up (e.g. starting the Tauri `LogServer`).
    pub fn start(config_path: PathBuf, save_handler: SaveHandler) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        // Non-blocking so the accept loop can poll the shutdown flag.
        listener.set_nonblocking(true)?;
        let (tx, rx) = mpsc::channel();
        let html = Arc::new(onboarding_html());
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(OnboardingState {
            config_path,
            save_handler,
            result_tx: Mutex::new(Some(tx)),
            shutdown: shutdown.clone(),
        });

        std::thread::spawn(move || loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let html = html.clone();
                    if let Err(error) = handle_request(stream, state, html) {
                        eprintln!("onboarding request failed: {error}");
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("onboarding accept error: {e}");
                    break;
                }
            }
        });

        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}/"),
            result_rx: rx,
        })
    }

    /// Block until the user saves a config, returning the result.
    ///
    /// Intended for the CLI/browser flow. The Tauri app ignores this (its save
    /// handler starts the `LogServer` directly).
    pub fn wait_for_save(self) -> Result<QuickConfigResult, String> {
        self.result_rx
            .recv()
            .map_err(|_| "onboarding did not produce a config".to_string())
    }
}

fn handle_request(
    mut stream: TcpStream,
    state: Arc<OnboardingState>,
    html: Arc<String>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut data = Vec::new();
    let mut buf = [0_u8; 8192];
    let mut expected_len = None;

    loop {
        let read = stream.read(&mut buf)?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buf[..read]);
        if let Some(header_end) = find_header_end(&data) {
            if expected_len.is_none() {
                let headers = String::from_utf8_lossy(&data[..header_end]);
                let content_len = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        if name.eq_ignore_ascii_case("content-length") {
                            value.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                expected_len = Some(header_end + 4 + content_len);
            }
            if data.len() >= expected_len.unwrap_or(header_end + 4) {
                break;
            }
        }
        if data.len() > 2 * 1024 * 1024 {
            return write_text_response(&mut stream, "413 Payload Too Large", "request too large");
        }
    }

    let Some(header_end) = find_header_end(&data) else {
        return write_text_response(&mut stream, "400 Bad Request", "missing headers");
    };
    let headers = String::from_utf8_lossy(&data[..header_end]);
    let request_line = headers.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");
    let body = &data[header_end + 4..];

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => write_html_response(&mut stream, &html),
        ("GET", "/api/serial_ports") => {
            let ports = list_serial_ports();
            let body = serde_json::to_string(&ports).unwrap_or_else(|_| "[]".to_string());
            write_json_response(&mut stream, "200 OK", &body)
        }
        ("GET", "/api/server_status") => {
            let status = server_status(&state.config_path);
            let body = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
            write_json_response(&mut stream, "200 OK", &body)
        }
        ("POST", "/api/save_config") => {
            let request: SaveQuickConfigRequest = match serde_json::from_slice(body) {
                Ok(request) => request,
                Err(error) => {
                    return write_text_response(
                        &mut stream,
                        "400 Bad Request",
                        &format!("invalid request: {error}"),
                    );
                }
            };

            match (state.save_handler)(state.config_path.clone(), request.draft) {
                Ok(result) => {
                    // Deliver the result to wait_for_save() (ignored by Tauri).
                    if let Some(tx) = state.result_tx.lock().unwrap().take() {
                        let _ = tx.send(result.clone());
                    }
                    let body = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                    let response = write_json_response(&mut stream, "200 OK", &body);
                    // Onboarding is done — tell the accept loop to exit so the
                    // thread and its port don't leak past the save.
                    state.shutdown.store(true, Ordering::Relaxed);
                    response
                }
                Err(error) => write_text_response(&mut stream, "400 Bad Request", &error),
            }
        }
        ("GET", "/favicon.ico") => write_response(&mut stream, "204 No Content", "text/plain", &[]),
        _ => write_text_response(&mut stream, "404 Not Found", "not found"),
    }
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_html_response(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    write_response(
        stream,
        "200 OK",
        "text/html; charset=utf-8",
        body.as_bytes(),
    )
}

fn write_json_response(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write_response(
        stream,
        status,
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

fn write_text_response(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write_response(stream, status, "text/plain; charset=utf-8", body.as_bytes())
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_config_builder_supports_multi_tab_multi_source_setup() {
        let yaml = build_quick_config_yaml(&QuickConfigDraft {
            app_name: Some("embed-log".to_string()),
            ws_port: Some(8080),
            logs_dir: Some("logs/".to_string()),
            baudrate: Some(115200),
            sources: vec![
                QuickSourceDraft {
                    name: "dev_a".to_string(),
                    label: Some("Device A".to_string()),
                    source_type: "uart".to_string(),
                    port: "/dev/tty.a".to_string(),
                    parser: Some("text".to_string()),
                    baudrate: Some(115200),
                },
                QuickSourceDraft {
                    name: "dev_b".to_string(),
                    label: Some("Device B".to_string()),
                    source_type: "uart".to_string(),
                    port: "/dev/tty.b".to_string(),
                    parser: Some("text".to_string()),
                    baudrate: Some(115200),
                },
                QuickSourceDraft {
                    name: "udp".to_string(),
                    label: None,
                    source_type: "udp".to_string(),
                    port: "9000".to_string(),
                    parser: Some("text".to_string()),
                    baudrate: None,
                },
                QuickSourceDraft {
                    name: "file".to_string(),
                    label: None,
                    source_type: "file".to_string(),
                    port: "/tmp/app.log".to_string(),
                    parser: Some("text".to_string()),
                    baudrate: None,
                },
            ],
            tabs: vec![
                QuickTabDraft {
                    label: "Devices".to_string(),
                    panes: vec!["dev_a".to_string(), "dev_b".to_string()],
                },
                QuickTabDraft {
                    label: "UDP".to_string(),
                    panes: vec!["udp".to_string()],
                },
                QuickTabDraft {
                    label: "File".to_string(),
                    panes: vec!["file".to_string()],
                },
            ],
        })
        .unwrap();

        let config: crate::config::AppConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.sources.len(), 4);
        assert_eq!(config.tabs.len(), 3);
        assert_eq!(config.tabs[0].panes.len(), 2);
        assert_eq!(config.sources[2].port.as_i64(), Some(9000));
    }

    #[test]
    fn quick_config_builder_writes_cbor_datagram_parser() {
        let yaml = build_quick_config_yaml(&QuickConfigDraft {
            app_name: None,
            ws_port: None,
            logs_dir: Some("logs/".to_string()),
            baudrate: None,
            sources: vec![QuickSourceDraft {
                name: "sensors".to_string(),
                label: None,
                source_type: "udp".to_string(),
                port: "6002".to_string(),
                parser: Some("cbor".to_string()),
                baudrate: None,
            }],
            tabs: vec![QuickTabDraft {
                label: "Sensors".to_string(),
                panes: vec!["sensors".to_string()],
            }],
        })
        .unwrap();

        let config: crate::config::AppConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.sources[0].parser.parser_type, "cbor-datagram");
    }

    #[test]
    fn onboarding_html_contains_setup_script() {
        let html = onboarding_html();
        assert!(html.contains("quick-setup-root"));
        assert!(html.contains("<script>"));
    }

    #[test]
    fn server_status_reports_default_port_without_config() {
        let tmp = std::env::temp_dir().join(format!(
            "embed-log-onboarding-status-{}.yml",
            std::process::id()
        ));
        let status = server_status(&tmp);
        assert_eq!(status.ws_port, 8080);
        assert_eq!(status.config_path, tmp.display().to_string());
    }
}
