use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize},
    Arc, Mutex, RwLock,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde_json::json;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::clock::{format_relative_millis, SessionClock};
use crate::config::AppConfig;
use crate::models::{Ansi, LogEntry, TimestampMode};
use crate::naming::slugify;
use crate::net::control_ws::SourceInfo;
use crate::net::ws_server::{
    start_server, ExportCallback, RotateCallback, RuntimeStats, ServerState, SourceRuntimeStats,
};
use crate::session::manager::atomic_write_file;
use crate::session::{SessionExporter, SessionManager};
use crate::sources::{FileSource, LogSource, TxCommand, UartSource, UdpSource};

const REPLAY_BUFFER_SIZE: usize = 5000;

/// The main server orchestrator.
pub struct LogServer {
    config: AppConfig,
    frontend_dir: PathBuf,
    logs_root: PathBuf,
    config_path: Option<PathBuf>,
    export_on_shutdown: bool,
}

/// Resolved source information for the runtime.
struct ResolvedSource {
    name: String,
    source: Box<dyn LogSource>,
    label: String,
    source_type: String,
}

/// Loaded plugin data for the config message.
#[derive(Clone)]
struct LoadedPlugins {
    /// Optional custom plugin definitions sent to legacy config-v1 clients.
    definitions: serde_json::Value,
    /// Pane-plugin mappings: `{ "DUT_UART": [{ "name": "custom-line" }] }`
    pane_plugins: serde_json::Value,
    /// Custom plugin JS source code: `{ "custom-line": "..." }`
    scripts: serde_json::Value,
}

impl LogServer {
    pub fn new(config: AppConfig, frontend_dir: PathBuf, logs_root: PathBuf) -> Self {
        Self {
            config,
            frontend_dir,
            logs_root,
            config_path: None,
            export_on_shutdown: true,
        }
    }

    /// Set the config file path — enables resolving relative plugin paths.
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    /// Control whether a clean process shutdown automatically exports HTML.
    pub fn with_shutdown_export(mut self, enabled: bool) -> Self {
        self.export_on_shutdown = enabled;
        self
    }

    /// Run the server until the process is interrupted.
    pub async fn run(&self) -> Result<()> {
        // ── 1. Create session directory ──
        let session_id =
            make_session_id_for_root(&self.logs_root, self.config.server.job_id.as_deref())?;
        let session_dir = self.logs_root.join(&session_id);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("create session dir {}", session_dir.display()))?;
        info!("session: {session_id}  dir: {}", session_dir.display());

        // ── 2. Resolve sources ──
        let sources = self.resolve_sources()?;

        // ── 3. Build labels, kinds, and source_files ──
        let mut pane_labels: HashMap<String, String> = HashMap::new();
        let mut pane_kinds: HashMap<String, String> = HashMap::new();
        let mut source_files: HashMap<String, String> = HashMap::new();

        for src in &sources {
            pane_labels.insert(src.name.clone(), src.label.clone());
            pane_kinds.insert(src.name.clone(), src.source_type.clone());
        }
        for merge in &self.config.merges {
            pane_labels.insert(merge.name.clone(), merge_label(merge));
            pane_kinds.insert(merge.name.clone(), "merge".to_string());
        }

        // ── 3b. Load command suggestions from companion YAML ──
        let mut source_writability: HashMap<String, bool> = sources
            .iter()
            .map(|src| (src.name.clone(), src.source.writable()))
            .collect();
        for merge in &self.config.merges {
            source_writability.insert(merge.name.clone(), false);
        }
        let pane_commands = crate::config::load_command_suggestions(
            self.config_path.as_deref(),
            &source_writability,
        );

        // Temporary watches are process-local and independent of persisted sessions.
        let watches = Arc::new(RwLock::new(HashMap::new()));
        let watch_counter = Arc::new(AtomicU64::new(1));

        // Build source metadata for the control API.
        let mut source_metadata: HashMap<String, SourceInfo> = sources
            .iter()
            .map(|src| {
                let info = SourceInfo {
                    source_type: src.source_type.clone(),
                    label: src.label.clone(),
                    writable: src.source.writable(),
                };
                (src.name.clone(), info)
            })
            .collect();
        for merge in &self.config.merges {
            source_metadata.insert(
                merge.name.clone(),
                SourceInfo {
                    source_type: "merge".to_string(),
                    label: merge_label(merge),
                    writable: false,
                },
            );
        }

        // Build per-source line counters for stable line_idx.
        use std::sync::atomic::AtomicU64;
        let line_counters: HashMap<String, Arc<AtomicU64>> = sources
            .iter()
            .map(|src| (src.name.clone(), Arc::new(AtomicU64::new(0))))
            .collect();
        // ── 4. Compute log file paths ──
        let tab_label = self
            .config
            .tabs
            .first()
            .map(|t| t.label.as_str())
            .unwrap_or("session");
        let mut log_paths: HashMap<String, PathBuf> = HashMap::new();
        for src in &sources {
            let log_name = format!(
                "{}__{}__{}.log",
                slugify(tab_label),
                slugify(&src.name),
                slugify(&session_id),
            );
            let log_path = session_dir.join(&log_name);
            source_files.insert(src.name.clone(), log_path.display().to_string());
            log_paths.insert(src.name.clone(), log_path);
        }
        let writer_log_paths: HashMap<String, Arc<Mutex<PathBuf>>> = log_paths
            .iter()
            .map(|(name, path)| (name.clone(), Arc::new(Mutex::new(path.clone()))))
            .collect();
        let combined_path = session_dir.join("combined.jsonl");
        let shared_source_files = Arc::new(Mutex::new(source_files.clone()));
        let shared_html_path = Arc::new(Mutex::new(session_dir.join("session.html")));

        // ── 5. Load frontend plugins ──
        let plugins = self.load_plugins();

        // ── 6. Create shared first_log_at tracker ──
        let first_log_at: Arc<Mutex<Option<DateTime<Local>>>> = Arc::new(Mutex::new(None));

        // ── 7. Create SessionManager and write manifest ──
        let started_at = Local::now().to_rfc3339();
        let tabs_json: Vec<serde_json::Value> = self
            .config
            .tabs
            .iter()
            .map(|tab| {
                let panes: Vec<String> = tab
                    .panes
                    .iter()
                    .map(|p| p.source_name().to_string())
                    .collect();
                json!({ "label": tab.label, "panes": panes })
            })
            .collect();

        let session_mgr = SessionManager::new(
            &session_id,
            session_dir.clone(),
            &tabs_json,
            source_files.clone(),
            combined_path.display().to_string(),
            pane_labels.clone(),
            pane_kinds.clone(),
            pane_commands.clone(),
            plugins.definitions.clone(),
            plugins.pane_plugins.clone(),
            plugins.scripts.clone(),
            &started_at,
            &self.config.server.app_name,
            None,
            self.config.server.job_id.clone(),
            self.config.server.timestamp_mode.to_string(),
            None,
            serde_json::to_value(&self.config.merges).unwrap_or_else(|_| json!([])),
        );
        session_mgr.write_manifest()?;
        let markers = session_mgr.load_markers();
        let session_mgr = Arc::new(Mutex::new(session_mgr));

        // ── 8. Create broadcast channel ──
        let (broadcast_tx, _rx) = broadcast::channel::<String>(4096);
        let replay = Arc::new(Mutex::new(VecDeque::with_capacity(REPLAY_BUFFER_SIZE)));
        // One ordering point for sequence allocation, persistence, replay, and
        // live publication across all concurrent source writers.
        let commit_lock = Arc::new(Mutex::new(()));
        let stats = Arc::new(RuntimeStats::new(
            sources.iter().map(|src| src.name.clone()),
            self.config.server.queue_size,
        ));
        let mut source_txs: HashMap<String, mpsc::Sender<LogEntry>> = HashMap::new();
        let mut source_tx_senders: HashMap<String, mpsc::Sender<TxCommand>> = HashMap::new();

        let mut join_handles: Vec<JoinHandle<()>> = Vec::new();

        let source_tab_labels = build_source_tab_labels(&self.config.tabs, &self.config.merges);

        // ── 9. Start sources + writers ──
        for mut src in sources {
            let log_path = writer_log_paths[&src.name].clone();

            let (entry_tx, entry_rx) = mpsc::channel::<LogEntry>(self.config.server.queue_size);
            source_txs.insert(src.name.clone(), entry_tx.clone());

            // If this source is writable (e.g., UART), create a TX command channel
            // so the frontend/SDK can write bytes to it.
            if src.source.writable() {
                let (tx_sender, tx_receiver) = mpsc::channel::<TxCommand>(32);
                source_tx_senders.insert(src.name.clone(), tx_sender);
                src.source.set_tx_receiver(tx_receiver);
            }

            // Spawn the physical source reader directly into its sole writer.
            let reader_name = src.name.clone();
            let reader_entry_tx = entry_tx.clone();
            let reader_handle = tokio::spawn(async move {
                if let Err(e) = src.source.run(reader_entry_tx).await {
                    error!("[{reader_name}] source error: {e}");
                }
            });
            join_handles.push(reader_handle);

            // Spawn writer task.
            let writer_name = src.name.clone();
            let writer_runtime = WriterRuntime {
                broadcast_tx: broadcast_tx.clone(),
                replay: replay.clone(),
                first_log_at: first_log_at.clone(),
                session_manager: session_mgr.clone(),
                stats: stats.source(&src.name),
                ts_mode: self.config.server.timestamp_mode,
                line_counter: line_counters.get(&src.name).cloned(),
                watches: watches.clone(),
                commit_lock: commit_lock.clone(),
                source_meta: SourceRuntimeMeta {
                    source_id: src.name.clone(),
                    source_label: src.label.clone(),
                    source_kind: src.source_type.clone(),
                    tab_labels: source_tab_labels
                        .get(&src.name)
                        .cloned()
                        .unwrap_or_default(),
                    session_id: session_id.clone(),
                    app_name: self.config.server.app_name.clone(),
                    job_id: self.config.server.job_id.clone(),
                },
            };
            let writer_handle = tokio::spawn(async move {
                run_writer(writer_name, log_path, entry_rx, writer_runtime).await;
            });
            join_handles.push(writer_handle);
        }

        // ── 10. Build config message ──
        let session_info = session_mgr.lock().unwrap().build_session_info();
        let config_msg =
            self.build_config_message(&pane_labels, &pane_kinds, &plugins, session_info, markers);
        let shared_config_msg = Arc::new(Mutex::new(config_msg.to_string()));

        // ── 11. Create export callback ──
        let export_ctx = ExportContext {
            tabs: tabs_json.clone(),
            labels: pane_labels.clone(),
            frontend: self.frontend_dir.clone(),
            ts_mode: self.config.server.timestamp_mode.to_string(),
            source_files: shared_source_files.clone(),
            html_path: shared_html_path.clone(),
            plugins: plugins.clone(),
            session_mgr: session_mgr.clone(),
            first_log_at: first_log_at.clone(),
            merges: serde_json::to_value(&self.config.merges).unwrap_or_else(|_| json!([])),
        };
        let on_export: ExportCallback = Arc::new(move || export_session(&export_ctx));

        // ── 11b. Create rotation callback ──
        let rotation_ctx = RotationContext {
            logs_root: self.logs_root.clone(),
            tab_label: tab_label.to_string(),
            source_names: writer_log_paths.keys().cloned().collect(),
            writer_paths: writer_log_paths.clone(),
            source_files: shared_source_files.clone(),
            html_path: shared_html_path.clone(),
            session_mgr: session_mgr.clone(),
            first_log_at: first_log_at.clone(),
            replay: replay.clone(),
            commit_lock: commit_lock.clone(),
            line_counters: line_counters.clone(),
            frontend: self.frontend_dir.clone(),
            tabs: tabs_json.clone(),
            pane_labels: pane_labels.clone(),
            pane_kinds: pane_kinds.clone(),
            pane_commands: pane_commands.clone(),
            plugins: plugins.clone(),
            app_name: self.config.server.app_name.clone(),
            job_id: self.config.server.job_id.clone(),
            timestamp_mode: self.config.server.timestamp_mode.to_string(),
            config_msg: shared_config_msg.clone(),
            default_light_theme: self.config.server.default_light_theme.clone(),
            default_dark_theme: self.config.server.default_dark_theme.clone(),
            export_on_rotate: self.export_on_shutdown,
            merges: serde_json::to_value(&self.config.merges).unwrap_or_else(|_| json!([])),
        };
        let on_rotate: RotateCallback = Arc::new(move |title| rotate_session(&rotation_ctx, title));
        let shutdown_export = on_export.clone();

        // ── 12. Start HTTP + WS server ──
        let state = ServerState {
            config_msg: shared_config_msg,
            broadcast_tx: broadcast_tx.clone(),
            replay: replay.clone(),
            on_export: Some(on_export.clone()),
            on_rotate: Some(on_rotate),
            session_manager: Some(session_mgr.clone()),
            logs_root: self.logs_root.clone(),
            ws_client_count: Arc::new(AtomicUsize::new(0)),
            stats: stats.clone(),
            source_txs: Arc::new(source_txs),
            source_tx_senders: Arc::new(source_tx_senders),
            source_metadata: Arc::new(source_metadata),
            line_counters: Arc::new(line_counters),
            watches,
            watch_counter,
            virtual_sources: Arc::new(
                self.config
                    .merges
                    .iter()
                    .map(|merge| (merge.name.clone(), merge.of.clone()))
                    .collect(),
            ),
            control_api: self.config.server.control_api,
        };

        let host = self.config.server.host.clone();
        let port = self.config.server.ws_port;
        let frontend_dir = self.frontend_dir.clone();

        let mut server_handle =
            tokio::spawn(async move { start_server(&host, port, Some(frontend_dir), state).await });

        // ── 13. Wait for shutdown or propagate an HTTP bind/server failure. ──
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal?,
            result = &mut server_handle => {
                return match result {
                    Ok(Ok(())) => Err(anyhow::anyhow!("HTTP/WebSocket server stopped unexpectedly")),
                    Ok(Err(error)) => Err(error.context("HTTP/WebSocket server failed")),
                    Err(error) => Err(anyhow::anyhow!("HTTP/WebSocket server task failed: {error}")),
                };
            }
        }
        server_handle.abort();
        info!("shutting down…");

        if self.export_on_shutdown {
            info!("exporting session HTML before exit…");
            match shutdown_export() {
                Ok(path) => info!("session HTML exported: {path}"),
                Err(e) => error!("session HTML export failed: {e}"),
            }
        } else {
            info!("skipping automatic HTML export for daemon shutdown");
        }

        Ok(())
    }

    /// Load frontend plugins from the config and frontend directory.
    fn load_plugins(&self) -> LoadedPlugins {
        let mut definitions = serde_json::Map::new();
        let mut scripts = serde_json::Map::new();
        let mut pane_plugins: HashMap<String, Vec<serde_json::Value>> = HashMap::new();

        // Load plugin definitions from config.
        for (name, plugin_def) in &self.config.frontend_plugins {
            let mut meta = serde_json::Map::new();

            if let Some(ref builtin) = plugin_def.builtin {
                meta.insert("builtin".to_string(), json!(builtin));
                // Read the builtin plugin JS from frontend/plugin-<name>.js on disk,
                // falling back to the compiled-in copy (same as embedded_fallback in
                // net::ws_server) when no on-disk frontend dir is present.
                let asset_name = format!("plugin-{builtin}.js");
                let js_path = self.frontend_dir.join(&asset_name);
                match std::fs::read_to_string(&js_path) {
                    Ok(js) => {
                        scripts.insert(name.clone(), json!(js));
                        info!("loaded builtin plugin: {name} from {}", js_path.display());
                    }
                    Err(e) => match crate::frontend_assets::FrontendAssets::get(&asset_name) {
                        Some(file) => {
                            let js = String::from_utf8_lossy(&file.data).into_owned();
                            scripts.insert(name.clone(), json!(js));
                            info!("loaded builtin plugin: {name} from embedded assets");
                        }
                        None => {
                            warn!("failed to load builtin plugin {name}: {e}");
                        }
                    },
                }
            } else if let Some(ref rel_path) = plugin_def.path {
                meta.insert("path".to_string(), json!(rel_path));
                // Resolve relative plugin paths against the config file directory.
                let resolved = if std::path::Path::new(rel_path).is_absolute() {
                    std::path::PathBuf::from(rel_path)
                } else if let Some(ref cfg_path) = self.config_path {
                    cfg_path
                        .parent()
                        .map(|p| p.join(rel_path))
                        .unwrap_or_else(|| std::path::PathBuf::from(rel_path))
                } else {
                    std::path::PathBuf::from(rel_path)
                };
                match std::fs::read_to_string(&resolved) {
                    Ok(js) => {
                        scripts.insert(name.clone(), json!(js));
                        info!("loaded custom plugin: {name} from {}", resolved.display());
                    }
                    Err(e) => {
                        warn!(
                            "failed to load custom plugin {name} from {}: {e}",
                            resolved.display()
                        );
                    }
                }
            }

            definitions.insert(name.clone(), serde_json::Value::Object(meta));
        }

        // Build pane_plugins from tab config.
        for tab in &self.config.tabs {
            for pane in &tab.panes {
                let pane_source = pane.source_name().to_string();
                if let crate::config::PaneConfig::Detailed { plugins, .. } = pane {
                    let refs: Vec<serde_json::Value> = plugins
                        .iter()
                        .map(|p| match p {
                            crate::config::PanePluginEntry::Name(name) => {
                                json!({ "name": name })
                            }
                            crate::config::PanePluginEntry::Detailed(cfg) => {
                                json!({ "name": cfg.name, "options": cfg.options })
                            }
                        })
                        .collect();
                    if !refs.is_empty() {
                        pane_plugins.insert(pane_source, refs);
                    }
                }
            }
        }

        LoadedPlugins {
            definitions: serde_json::Value::Object(definitions),
            pane_plugins: json!(pane_plugins),
            scripts: serde_json::Value::Object(scripts),
        }
    }

    /// Resolve config source definitions into runtime sources.
    fn resolve_sources(&self) -> Result<Vec<ResolvedSource>> {
        let mut sources = Vec::new();

        for src_cfg in &self.config.sources {
            let stype = src_cfg.source_type.to_lowercase();
            let label = src_cfg
                .label
                .clone()
                .unwrap_or_else(|| src_cfg.name.clone());

            let parser = src_cfg.parser.clone();

            let source: Box<dyn LogSource> = match stype.as_str() {
                "uart" => {
                    let port_path = match &src_cfg.port {
                        serde_yaml::Value::String(s) => s.clone(),
                        _ => {
                            error!(
                                "source {}: uart port must be a string, skipping",
                                src_cfg.name
                            );
                            continue;
                        }
                    };
                    let baudrate = src_cfg.baudrate.unwrap_or(self.config.baudrate);
                    Box::new(
                        UartSource::new_with_parser(
                            &src_cfg.name,
                            port_path,
                            baudrate,
                            &parser.parser_type,
                        )
                        .with_parser(parser),
                    )
                }
                "udp" => {
                    let port = match &src_cfg.port {
                        serde_yaml::Value::Number(n) => n.as_u64().unwrap_or(0) as u16,
                        _ => {
                            error!(
                                "source {}: udp port must be an integer, skipping",
                                src_cfg.name
                            );
                            continue;
                        }
                    };
                    Box::new(
                        UdpSource::new_with_parser(&src_cfg.name, port, &parser.parser_type)
                            .with_parser(parser),
                    )
                }
                "file" => {
                    let file_path = match &src_cfg.port {
                        serde_yaml::Value::String(s) => s.clone(),
                        _ => {
                            error!(
                                "source {}: file port must be a string, skipping",
                                src_cfg.name
                            );
                            continue;
                        }
                    };
                    Box::new(
                        FileSource::new_with_parser(&src_cfg.name, file_path, &parser.parser_type)
                            .with_parser(parser),
                    )
                }
                other => {
                    error!(
                        "source {}: type {other:?} not yet implemented, skipping",
                        src_cfg.name
                    );
                    continue;
                }
            };

            sources.push(ResolvedSource {
                name: src_cfg.name.clone(),
                source,
                label,
                source_type: stype,
            });
        }

        Ok(sources)
    }

    fn build_config_message(
        &self,
        pane_labels: &HashMap<String, String>,
        pane_kinds: &HashMap<String, String>,
        plugins: &LoadedPlugins,
        session_info: serde_json::Value,
        markers: Vec<serde_json::Value>,
    ) -> serde_json::Value {
        let tabs_json: Vec<serde_json::Value> = self
            .config
            .tabs
            .iter()
            .map(|tab| {
                let panes: Vec<String> = tab
                    .panes
                    .iter()
                    .map(|p| p.source_name().to_string())
                    .collect();
                json!({
                    "label": tab.label,
                    "panes": panes,
                })
            })
            .collect();

        let pane_commands = session_info
            .get("pane_commands")
            .cloned()
            .unwrap_or_else(|| json!({}));

        build_ws_config_message(WsConfigParts {
            app_name: &self.config.server.app_name,
            default_light_theme: &self.config.server.default_light_theme,
            default_dark_theme: &self.config.server.default_dark_theme,
            session_info,
            pane_labels,
            pane_kinds,
            pane_commands,
            tabs: &tabs_json,
            frontend_plugins: plugins.definitions.clone(),
            pane_plugins: plugins.pane_plugins.clone(),
            plugin_scripts: plugins.scripts.clone(),
            markers,
            merges: serde_json::to_value(&self.config.merges).unwrap_or_else(|_| json!([])),
        })
    }
}

struct WsConfigParts<'a> {
    app_name: &'a str,
    default_light_theme: &'a Option<String>,
    default_dark_theme: &'a Option<String>,
    session_info: serde_json::Value,
    pane_labels: &'a HashMap<String, String>,
    pane_kinds: &'a HashMap<String, String>,
    pane_commands: serde_json::Value,
    tabs: &'a [serde_json::Value],
    frontend_plugins: serde_json::Value,
    pane_plugins: serde_json::Value,
    plugin_scripts: serde_json::Value,
    markers: Vec<serde_json::Value>,
    merges: serde_json::Value,
}

fn build_ws_config_message(parts: WsConfigParts<'_>) -> serde_json::Value {
    let tabs_json: Vec<serde_json::Value> = parts
        .tabs
        .iter()
        .map(|tab| {
            let panes: Vec<String> = tab
                .get("panes")
                .and_then(|panes| panes.as_array())
                .map(|panes| {
                    panes
                        .iter()
                        .filter_map(|pane| pane.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let mut tab_pane_labels = HashMap::new();
            for pane_name in &panes {
                if let Some(label) = parts.pane_labels.get(pane_name) {
                    tab_pane_labels.insert(pane_name.clone(), label.clone());
                }
            }
            json!({
                "label": tab.get("label").and_then(|label| label.as_str()).unwrap_or("Tab"),
                "panes": panes,
                "pane_labels": tab_pane_labels,
            })
        })
        .collect();

    json!({
        "type": "config",
        "app_name": parts.app_name,
        "theme_defaults": {
            "light": parts.default_light_theme,
            "dark": parts.default_dark_theme,
        },
        "session": parts.session_info,
        "pane_labels": parts.pane_labels,
        "pane_kinds": parts.pane_kinds,
        "pane_commands": parts.pane_commands,
        "tabs": tabs_json,
        "frontend_plugins": parts.frontend_plugins,
        "pane_plugins": parts.pane_plugins,
        "plugin_scripts": parts.plugin_scripts,
        "markers": parts.markers,
        "merges": parts.merges,
    })
}

/// Everything the export callback needs, captured once when the server starts.
struct ExportContext {
    tabs: Vec<serde_json::Value>,
    labels: HashMap<String, String>,
    frontend: PathBuf,
    ts_mode: String,
    source_files: Arc<Mutex<HashMap<String, String>>>,
    html_path: Arc<Mutex<PathBuf>>,
    plugins: LoadedPlugins,
    session_mgr: Arc<Mutex<SessionManager>>,
    first_log_at: Arc<Mutex<Option<DateTime<Local>>>>,
    merges: serde_json::Value,
}

/// Export the current session to HTML from the latest complete canonical
/// combined stream. SessionExporter serializes producers and publishes atomically.
fn export_session(ctx: &ExportContext) -> Result<String, String> {
    let fla = ctx.first_log_at.lock().unwrap().map(|dt| dt.to_rfc3339());
    let export_html = ctx.html_path.lock().unwrap().clone();
    let export_sources = ctx.source_files.lock().unwrap().clone();
    let (export_markers, combined_file) = ctx
        .session_mgr
        .lock()
        .map(|mgr| (mgr.load_markers(), mgr.combined_file()))
        .unwrap_or_default();

    let exporter = SessionExporter::new(
        export_html.clone(),
        export_sources,
        ctx.tabs.clone(),
        ctx.labels.clone(),
        ctx.frontend.clone(),
        ctx.ts_mode.clone(),
        fla,
    )
    .with_combined_file(combined_file)
    .with_merges(ctx.merges.clone())
    .with_plugins(
        ctx.plugins.definitions.clone(),
        ctx.plugins.pane_plugins.clone(),
        ctx.plugins.scripts.clone(),
    )
    .with_markers(export_markers);

    match exporter.export() {
        Ok(path) => {
            if let Ok(mut mgr) = ctx.session_mgr.lock() {
                let _ = mgr.mark_html_exported(&path);
            }
            Ok(path.display().to_string())
        }
        Err(e) => {
            let err_msg = e.to_string();
            if let Ok(mut mgr) = ctx.session_mgr.lock() {
                let _ = mgr.mark_html_error(&err_msg);
            }
            Err(err_msg)
        }
    }
}

/// Everything the rotation callback needs, captured once when the server starts.
struct RotationContext {
    logs_root: PathBuf,
    tab_label: String,
    source_names: Vec<String>,
    writer_paths: HashMap<String, Arc<Mutex<PathBuf>>>,
    source_files: Arc<Mutex<HashMap<String, String>>>,
    html_path: Arc<Mutex<PathBuf>>,
    session_mgr: Arc<Mutex<SessionManager>>,
    first_log_at: Arc<Mutex<Option<DateTime<Local>>>>,
    replay: Arc<Mutex<VecDeque<String>>>,
    commit_lock: Arc<Mutex<()>>,
    line_counters: HashMap<String, Arc<AtomicU64>>,
    frontend: PathBuf,
    tabs: Vec<serde_json::Value>,
    pane_labels: HashMap<String, String>,
    pane_kinds: HashMap<String, String>,
    pane_commands: serde_json::Value,
    plugins: LoadedPlugins,
    app_name: String,
    job_id: Option<String>,
    timestamp_mode: String,
    config_msg: Arc<Mutex<String>>,
    default_light_theme: Option<String>,
    default_dark_theme: Option<String>,
    export_on_rotate: bool,
    merges: serde_json::Value,
}

/// Roll over to a fresh session without restarting source tasks or releasing
/// UART ownership. Returns `(old_session_info, new_session_info)`.
fn rotate_session(
    ctx: &RotationContext,
    title: Option<String>,
) -> Result<(serde_json::Value, serde_json::Value), String> {
    let title = normalize_session_title(title)?;
    let commit_guard = ctx
        .commit_lock
        .lock()
        .map_err(|_| "combined commit lock failed".to_string())?;
    let (old_session, old_markers) = {
        let manager = ctx
            .session_mgr
            .lock()
            .map_err(|_| "session manager lock failed".to_string())?;
        (manager.build_session_info(), manager.load_markers())
    };
    let old_source_files = ctx.source_files.lock().unwrap().clone();
    let old_html_path = ctx.html_path.lock().unwrap().clone();
    let old_first_log_at = ctx.first_log_at.lock().unwrap().map(|dt| dt.to_rfc3339());

    let new_session_id =
        make_titled_session_id_for_root(&ctx.logs_root, ctx.job_id.as_deref(), title.as_deref())
            .map_err(|err| err.to_string())?;
    let new_session_dir = ctx.logs_root.join(&new_session_id);
    std::fs::create_dir_all(&new_session_dir).map_err(|err| err.to_string())?;

    let mut new_source_files = HashMap::new();
    for source_name in &ctx.source_names {
        let log_name = format!(
            "{}__{}__{}.log",
            slugify(&ctx.tab_label),
            slugify(source_name),
            slugify(&new_session_id),
        );
        let log_path = new_session_dir.join(log_name);
        new_source_files.insert(source_name.clone(), log_path.display().to_string());
        if let Some(shared_path) = ctx.writer_paths.get(source_name) {
            *shared_path.lock().unwrap() = log_path;
        }
    }

    let started_at = Local::now().to_rfc3339();
    let new_manager = SessionManager::new(
        &new_session_id,
        new_session_dir.clone(),
        &ctx.tabs,
        new_source_files.clone(),
        new_session_dir.join("combined.jsonl").display().to_string(),
        ctx.pane_labels.clone(),
        ctx.pane_kinds.clone(),
        ctx.pane_commands.clone(),
        ctx.plugins.definitions.clone(),
        ctx.plugins.pane_plugins.clone(),
        ctx.plugins.scripts.clone(),
        &started_at,
        &ctx.app_name,
        None,
        ctx.job_id.clone(),
        ctx.timestamp_mode.clone(),
        None,
        ctx.merges.clone(),
    )
    .with_title(title);
    new_manager
        .write_manifest()
        .map_err(|err| err.to_string())?;

    {
        let mut source_files = ctx.source_files.lock().unwrap();
        *source_files = new_source_files;
    }
    *ctx.html_path.lock().unwrap() = new_session_dir.join("session.html");
    *ctx.first_log_at.lock().unwrap() = None;
    for counter in ctx.line_counters.values() {
        counter.store(0, std::sync::atomic::Ordering::Relaxed);
    }
    ctx.replay.lock().unwrap().clear();

    let new_session = new_manager.build_session_info();
    *ctx.session_mgr
        .lock()
        .map_err(|_| "session manager lock failed".to_string())? = new_manager;

    let new_config_msg = build_ws_config_message(WsConfigParts {
        app_name: &ctx.app_name,
        default_light_theme: &ctx.default_light_theme,
        default_dark_theme: &ctx.default_dark_theme,
        session_info: new_session.clone(),
        pane_labels: &ctx.pane_labels,
        pane_kinds: &ctx.pane_kinds,
        pane_commands: ctx.pane_commands.clone(),
        tabs: &ctx.tabs,
        frontend_plugins: ctx.plugins.definitions.clone(),
        pane_plugins: ctx.plugins.pane_plugins.clone(),
        plugin_scripts: ctx.plugins.scripts.clone(),
        markers: Vec::new(),
        merges: ctx.merges.clone(),
    });
    *ctx.config_msg
        .lock()
        .map_err(|_| "config message lock failed".to_string())? = new_config_msg.to_string();
    drop(commit_guard);

    // Export the old foreground session's HTML off the hot path. Daemons keep
    // raw artifacts only unless an explicit export is requested.
    if ctx.export_on_rotate {
        if let Some(old_manifest_path) = old_html_path.parent().map(|dir| dir.join("manifest.json"))
        {
            let _ = update_manifest_file(
                &old_manifest_path,
                &json!({
                    "html_status": "updating",
                    "html_error": serde_json::Value::Null,
                    "last_export_reason": "rotate",
                }),
            );
            let old_export_tabs = ctx.tabs.clone();
            let old_export_labels = ctx.pane_labels.clone();
            let old_export_frontend = ctx.frontend.clone();
            let old_export_ts_mode = ctx.timestamp_mode.clone();
            let old_export_plugins = ctx.plugins.clone();
            let old_export_merges = ctx.merges.clone();
            std::thread::spawn(move || {
                let exporter = SessionExporter::new(
                    old_html_path.clone(),
                    old_source_files,
                    old_export_tabs,
                    old_export_labels,
                    old_export_frontend,
                    old_export_ts_mode,
                    old_first_log_at,
                )
                .with_combined_file(old_html_path.parent().unwrap().join("combined.jsonl"))
                .with_merges(old_export_merges)
                .with_plugins(
                    old_export_plugins.definitions,
                    old_export_plugins.pane_plugins,
                    old_export_plugins.scripts,
                )
                .with_markers(old_markers);

                match exporter.export() {
                    Ok(path) => {
                        let now = Local::now().to_rfc3339();
                        let _ = update_manifest_file(
                            &old_manifest_path,
                            &json!({
                                "session_html": path.display().to_string(),
                                "html_status": "ready",
                                "html_updated_at": now,
                                "html_error": serde_json::Value::Null,
                                "last_export_reason": "rotate",
                            }),
                        );
                    }
                    Err(error) => {
                        let now = Local::now().to_rfc3339();
                        let _ = update_manifest_file(
                            &old_manifest_path,
                            &json!({
                                "html_status": "error",
                                "html_error": error.to_string(),
                                "html_updated_at": now,
                                "last_export_reason": "rotate",
                            }),
                        );
                    }
                }
            });
        }
    }

    Ok((old_session, new_session))
}

fn update_manifest_file(path: &Path, updates: &serde_json::Value) -> Result<()> {
    let mut manifest = if path.exists() {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if let (Some(obj), Some(updates_obj)) = (manifest.as_object_mut(), updates.as_object()) {
        for (key, value) in updates_obj {
            obj.insert(key.clone(), value.clone());
        }
    }

    atomic_write_file(path, serde_json::to_string_pretty(&manifest)?.as_bytes())
        .with_context(|| format!("update manifest {}", path.display()))
}

fn normalize_session_title(title: Option<String>) -> Result<Option<String>, String> {
    let Some(title) = title else {
        return Ok(None);
    };
    if title.trim().is_empty() {
        return Err("session title must not be empty".to_string());
    }
    if title.chars().count() > 120 {
        return Err("session title must not exceed 120 characters".to_string());
    }
    if slugify(title.trim()).is_empty() {
        return Err("session title must contain a letter or number".to_string());
    }
    Ok(Some(title))
}

fn make_session_id_for_root(logs_root: &Path, job_id: Option<&str>) -> Result<String> {
    make_titled_session_id_for_root(logs_root, job_id, None)
}

fn make_titled_session_id_for_root(
    logs_root: &Path,
    job_id: Option<&str>,
    title: Option<&str>,
) -> Result<String> {
    let base_time = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let mut base = base_time;
    if let Some(title) = title {
        base.push('_');
        base.push_str(&slugify(title.trim()));
    }
    if let Some(job_id) = job_id {
        base.push_str("__");
        base.push_str(&slugify(job_id));
    }

    for suffix in 0..1000 {
        let candidate = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}_{suffix}")
        };
        if !logs_root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "could not allocate unique session id under {}",
        logs_root.display()
    )
}

#[derive(Clone, Default)]
struct SourceRuntimeMeta {
    source_id: String,
    source_label: String,
    source_kind: String,
    tab_labels: Vec<String>,
    session_id: String,
    app_name: String,
    job_id: Option<String>,
}

struct WriterRuntime {
    broadcast_tx: broadcast::Sender<String>,
    replay: Arc<Mutex<VecDeque<String>>>,
    first_log_at: Arc<Mutex<Option<DateTime<Local>>>>,
    session_manager: Arc<Mutex<SessionManager>>,
    stats: Option<Arc<SourceRuntimeStats>>,
    ts_mode: TimestampMode,
    line_counter: Option<Arc<AtomicU64>>,
    watches: Arc<RwLock<HashMap<String, crate::net::watch::TemporaryWatch>>>,
    commit_lock: Arc<Mutex<()>>,
    source_meta: SourceRuntimeMeta,
}

fn merge_label(merge: &crate::config::MergeConfig) -> String {
    merge.label.clone().unwrap_or_else(|| merge.name.clone())
}

fn build_source_tab_labels(
    tabs: &[crate::config::TabConfig],
    merges: &[crate::config::MergeConfig],
) -> HashMap<String, Vec<String>> {
    let merge_members: HashMap<&str, &[String]> = merges
        .iter()
        .map(|merge| (merge.name.as_str(), merge.of.as_slice()))
        .collect();
    let mut by_source: HashMap<String, Vec<String>> = HashMap::new();
    for tab in tabs {
        for pane in &tab.panes {
            let pane_name = pane.source_name();
            if let Some(members) = merge_members.get(pane_name) {
                for member in *members {
                    by_source
                        .entry(member.clone())
                        .or_default()
                        .push(tab.label.clone());
                }
            } else {
                by_source
                    .entry(pane_name.to_string())
                    .or_default()
                    .push(tab.label.clone());
            }
        }
    }
    by_source
}

fn build_combined_log_entry(
    payload: &serde_json::Value,
    source_meta: &SourceRuntimeMeta,
) -> serde_json::Value {
    let mut combined = payload.clone();
    if let Some(obj) = combined.as_object_mut() {
        obj.insert("source_id".to_string(), json!(source_meta.source_id));
        obj.insert("session_id".to_string(), json!(source_meta.session_id));
        obj.insert("app_name".to_string(), json!(source_meta.app_name));
        obj.insert("job_id".to_string(), json!(source_meta.job_id));
        obj.insert("source_label".to_string(), json!(source_meta.source_label));
        obj.insert("source_kind".to_string(), json!(source_meta.source_kind));
        obj.insert("tab_labels".to_string(), json!(source_meta.tab_labels));
    }
    combined
}

/// Flush the buffered source log periodically rather than once per line.
/// This keeps the file reasonably durable while preserving BufWriter batching
/// under high-volume capture.
const LOG_FLUSH_EVERY_LINES: usize = 256;

/// Writer task: receives log entries, writes to file, broadcasts to WS clients.
///
/// Tracks `first_log_at` and sends a `session_info` update when it's first set.
async fn run_writer(
    source_name: String,
    log_path: Arc<Mutex<PathBuf>>,
    mut entry_rx: mpsc::Receiver<LogEntry>,
    runtime: WriterRuntime,
) {
    let mut current_log_path = log_path.lock().unwrap().clone();
    let mut file = match open_log_file(&source_name, &current_log_path) {
        Some(file) => file,
        None => return,
    };

    let clock = SessionClock::new(runtime.ts_mode);
    let mut lines_since_flush = 0usize;
    use std::io::Write;

    while let Some(entry) = entry_rx.recv().await {
        let _commit_guard = match runtime.commit_lock.lock() {
            Ok(guard) => guard,
            Err(_) => {
                error!("[{source_name}] combined commit lock poisoned");
                break;
            }
        };
        let desired_log_path = log_path.lock().unwrap().clone();
        if desired_log_path != current_log_path {
            let _ = file.flush();
            lines_since_flush = 0;
            match open_log_file(&source_name, &desired_log_path) {
                Some(next_file) => {
                    current_log_path = desired_log_path;
                    file = next_file;
                }
                None => continue,
            }
        }

        // Set origin on first entry for the current session and broadcast session_info update.
        let set_first_log_at = {
            let mut fla = runtime.first_log_at.lock().unwrap();
            if fla.is_none() {
                *fla = Some(entry.timestamp);
                true
            } else {
                false
            }
        };
        if set_first_log_at {
            let session = if let Ok(mut manager) = runtime.session_manager.lock() {
                let _ = manager.mark_first_log_at(entry.timestamp);
                manager.build_session_info()
            } else {
                json!({
                    "first_log_at": entry.timestamp.to_rfc3339(),
                    "timestamp_mode": runtime.ts_mode.to_string(),
                })
            };
            let session_info = json!({
                "type": "session_info",
                "session": session,
            });
            let _ = runtime.broadcast_tx.send(session_info.to_string());
        }

        // Format for log file: [timestamp] message
        let file_ts = clock.file_timestamp(entry.timestamp);
        let file_line = format!("[{file_ts}] {}\n", entry.message);
        if let Some(stats) = &runtime.stats {
            stats.record_dequeued(file_line.len());
        }

        if let Err(e) = file.write_all(file_line.as_bytes()) {
            error!("[{source_name}] log write error: {e}");
        } else {
            lines_since_flush += 1;
            // Flush a partial batch when the queue goes idle so low-volume
            // sources remain observable without sacrificing batching during
            // sustained capture.
            if lines_since_flush >= LOG_FLUSH_EVERY_LINES || entry_rx.is_empty() {
                let _ = file.flush();
                lines_since_flush = 0;
            }
        }

        // Build WS payload with BOTH absolute and relative timestamps.
        // The frontend needs both so the timestamp toggle button works.
        let abs_ts = entry.timestamp.format("%m-%d %H:%M:%S%.3f").to_string();
        let abs_num = entry.timestamp.timestamp_millis() as f64;

        // Calculate relative timestamp from origin (first log).
        let (rel_ts, rel_num) = {
            let origin = runtime.first_log_at.lock().unwrap();
            match *origin {
                Some(origin_ts) => {
                    let delta = entry.timestamp - origin_ts;
                    let total_ms = delta.num_milliseconds().max(0) as u64;
                    let rel_ts = format_relative_millis(total_ms);
                    (rel_ts, total_ms as f64)
                }
                None => (format_relative_millis(0), 0.0),
            }
        };

        // Always send absolute as the display timestamp.
        // The frontend recalculates using metadata when the mode changes.
        let display_ts = abs_ts.clone();
        let ts_iso = entry.timestamp.to_rfc3339();

        let data = if let Some(ref color) = entry.color {
            if let Some(ansi) = Ansi::code(color) {
                format!("{ansi}{}{}", entry.message, Ansi::RESET)
            } else {
                entry.message.clone()
            }
        } else {
            entry.message.clone()
        };

        let is_tx = entry.source.starts_with("TX::");

        // Derive origin:
        // - TX::<origin> → the part after TX:: (pytest, ui, etc.)
        // - Normal RX where entry.source == source_name → "SERIAL"
        // - Injected where entry.source != source_name → the caller-provided origin
        let origin = if is_tx {
            entry
                .source
                .strip_prefix("TX::")
                .unwrap_or("ui")
                .to_string()
        } else if entry.source == source_name {
            "SERIAL".to_string()
        } else {
            entry.source.clone()
        };

        // Raw message without ANSI wrapping (for SDK / structured API).
        let message = entry.message.clone();

        // Increment per-source line counter.
        let line_idx = runtime
            .line_counter
            .as_ref()
            .map(|c| c.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0);

        let mut payload = json!({
            "type": if is_tx { "tx" } else { "rx" },
            "data": data,
            "timestamp": display_ts,
            "timestamp_iso": ts_iso,
            "timestamp_num": abs_num,
            "source_id": source_name,
            "line_idx": line_idx,
            "origin": origin,
            "color": entry.color,
            "message": message,
            // Send both absolute and relative so the frontend can toggle.
            "absTs": abs_ts,
            "absNum": abs_num,
            "relTs": rel_ts,
            "relNum": rel_num,
        });
        if let (Some(payload_obj), Some(meta_obj)) = (
            payload.as_object_mut(),
            entry.meta.as_ref().and_then(|m| m.as_object()),
        ) {
            for (key, value) in meta_obj {
                payload_obj.insert(key.clone(), value.clone());
            }
        }

        let mut combined_entry = build_combined_log_entry(&payload, &runtime.source_meta);
        let (sequence, active_session_id) = match runtime.session_manager.lock() {
            Ok(mut manager) => match manager.append_combined_entry(&mut combined_entry) {
                Ok(sequence) => (sequence, manager.session_id().to_string()),
                Err(error) => {
                    error!("[{source_name}] failed to append combined entry: {error}");
                    continue;
                }
            },
            Err(_) => {
                error!("[{source_name}] session manager lock poisoned");
                continue;
            }
        };
        if let Some(object) = payload.as_object_mut() {
            object.insert("sequence".to_string(), json!(sequence));
            object.insert("session_id".to_string(), json!(active_session_id));
        }

        let payload_str = payload.to_string();

        crate::net::watch::retain_matching_watches(
            &runtime.watches,
            &source_name,
            is_tx,
            &message,
            &payload,
        );

        // Store in replay buffer
        {
            let mut buf = runtime.replay.lock().unwrap();
            if buf.len() >= REPLAY_BUFFER_SIZE {
                buf.pop_front();
            }
            buf.push_back(payload_str.clone());
        }

        // Broadcast to WS clients (ignore if no receivers)
        let _ = runtime.broadcast_tx.send(payload_str);
    }

    let _ = file.flush();
}

fn open_log_file(
    source_name: &str,
    log_path: &PathBuf,
) -> Option<std::io::BufWriter<std::fs::File>> {
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(file) => Some(std::io::BufWriter::new(file)),
        Err(e) => {
            error!(
                "[{source_name}] cannot open log file {}: {e}",
                log_path.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = Local::now().timestamp_nanos_opt().unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "embed-log-runtime-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_manager(session_id: &str, dir: PathBuf, source_file: PathBuf) -> SessionManager {
        let mut source_files = HashMap::new();
        source_files.insert("dut".to_string(), source_file.display().to_string());
        let combined_file = dir.join("combined.jsonl");
        SessionManager::new(
            session_id,
            dir,
            &[json!({ "label": "Main", "panes": ["dut"] })],
            source_files,
            combined_file.display().to_string(),
            HashMap::from([("dut".to_string(), "DUT".to_string())]),
            HashMap::from([("dut".to_string(), "udp".to_string())]),
            json!({}),
            json!({}),
            json!({}),
            json!({}),
            Local::now().to_rfc3339(),
            "embed-log",
            None,
            None,
            "absolute",
            None,
            json!([]),
        )
    }

    fn test_source_meta(session_id: &str) -> SourceRuntimeMeta {
        SourceRuntimeMeta {
            source_id: "dut".to_string(),
            source_label: "DUT".to_string(),
            source_kind: "udp".to_string(),
            tab_labels: vec!["Main".to_string()],
            session_id: session_id.to_string(),
            app_name: "embed-log".to_string(),
            job_id: None,
        }
    }

    async fn wait_file_contains(path: &PathBuf, needle: &str) {
        for _ in 0..100 {
            if std::fs::read_to_string(path)
                .map(|text| text.contains(needle))
                .unwrap_or(false)
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("{} did not contain {needle:?}", path.display());
    }

    #[test]
    fn virtual_merge_tabs_are_attributed_to_physical_members() {
        let tabs = vec![crate::config::TabConfig {
            label: "Link".to_string(),
            panes: vec![crate::config::PaneConfig::Simple("MCU_LINK".to_string())],
        }];
        let merges = vec![crate::config::MergeConfig {
            name: "MCU_LINK".to_string(),
            label: Some("MCU Link".to_string()),
            of: vec!["MCU_TX".to_string(), "MCU_RX".to_string()],
        }];
        let labels = build_source_tab_labels(&tabs, &merges);
        assert_eq!(labels["MCU_TX"], ["Link"]);
        assert_eq!(labels["MCU_RX"], ["Link"]);
        assert!(!labels.contains_key("MCU_LINK"));
    }

    #[test]
    fn collision_safe_session_id_appends_suffix() {
        let root = temp_dir("collision");
        let id = make_session_id_for_root(&root, Some("job")).unwrap();
        std::fs::create_dir_all(root.join(&id)).unwrap();
        let next = make_session_id_for_root(&root, Some("job")).unwrap();
        assert_ne!(id, next);
        assert!(next.starts_with(&id));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn titled_session_id_preserves_job_and_rejects_invalid_titles() {
        let root = temp_dir("titled-id");
        let id = make_titled_session_id_for_root(
            &root,
            Some("nightly job"),
            Some("Reconnect Attempt #3"),
        )
        .unwrap();
        assert!(id.contains("_reconnect-attempt-3__nightly-job"));
        assert!(normalize_session_title(Some("   ".to_string())).is_err());
        assert!(normalize_session_title(Some("***".to_string())).is_err());
        assert!(normalize_session_title(Some("x".repeat(121))).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn writer_reopens_log_file_after_rotation_path_update() {
        let root = temp_dir("rotate-writer");
        let first_dir = root.join("session-1");
        let second_dir = root.join("session-2");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();

        let first_log = first_dir.join("main__dut__session-1.log");
        let second_log = second_dir.join("main__dut__session-2.log");
        let path = Arc::new(Mutex::new(first_log.clone()));
        let (entry_tx, entry_rx) = mpsc::channel(4);
        let (broadcast_tx, _rx) = broadcast::channel(16);
        let replay = Arc::new(Mutex::new(VecDeque::new()));
        let first_log_at = Arc::new(Mutex::new(None));
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            first_dir,
            first_log.clone(),
        )));

        let runtime = WriterRuntime {
            broadcast_tx,
            replay,
            first_log_at: first_log_at.clone(),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            watches: Arc::new(RwLock::new(HashMap::new())),
            commit_lock: Arc::new(Mutex::new(())),
            source_meta: test_source_meta("session-1"),
        };
        let handle = tokio::spawn(run_writer(
            "dut".to_string(),
            path.clone(),
            entry_rx,
            runtime,
        ));

        for index in 0..LOG_FLUSH_EVERY_LINES {
            entry_tx
                .send(LogEntry::new(
                    Local::now(),
                    "dut".to_string(),
                    if index + 1 == LOG_FLUSH_EVERY_LINES {
                        "first".to_string()
                    } else {
                        format!("warmup-{index}")
                    },
                ))
                .await
                .unwrap();
        }
        wait_file_contains(&first_log, "first").await;

        *path.lock().unwrap() = second_log.clone();
        *first_log_at.lock().unwrap() = None;
        for index in 0..LOG_FLUSH_EVERY_LINES {
            entry_tx
                .send(LogEntry::new(
                    Local::now(),
                    "dut".to_string(),
                    if index + 1 == LOG_FLUSH_EVERY_LINES {
                        "second".to_string()
                    } else {
                        format!("rotated-warmup-{index}")
                    },
                ))
                .await
                .unwrap();
        }
        wait_file_contains(&second_log, "second").await;

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn writer_appends_combined_jsonl_with_source_metadata() {
        let root = temp_dir("combined-jsonl");
        let session_dir = root.join("session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let log_path = session_dir.join("main__dut__session-1.log");
        let combined_path = session_dir.join("combined.jsonl");

        let path = Arc::new(Mutex::new(log_path.clone()));
        let (entry_tx, entry_rx) = mpsc::channel(4);
        let (broadcast_tx, _broadcast_rx) = broadcast::channel(16);
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            session_dir.clone(),
            log_path.clone(),
        )));

        let runtime = WriterRuntime {
            broadcast_tx,
            replay: Arc::new(Mutex::new(VecDeque::new())),
            first_log_at: Arc::new(Mutex::new(None)),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            watches: Arc::new(RwLock::new(HashMap::new())),
            commit_lock: Arc::new(Mutex::new(())),
            source_meta: test_source_meta("session-1"),
        };
        let handle = tokio::spawn(run_writer("dut".to_string(), path, entry_rx, runtime));

        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "boot complete".to_string(),
            ))
            .await
            .unwrap();
        wait_file_contains(&combined_path, "\"source_label\":\"DUT\"").await;
        let line = std::fs::read_to_string(&combined_path).unwrap();
        let first = line.lines().next().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(parsed["source_id"], "dut");
        assert_eq!(parsed["source_label"], "DUT");
        assert_eq!(parsed["source_kind"], "udp");
        assert_eq!(parsed["tab_labels"], json!(["Main"]));
        assert_eq!(parsed["session_id"], "session-1");
        assert_eq!(parsed["app_name"], "embed-log");
        assert_eq!(parsed["message"], "boot complete");
        assert_eq!(parsed["sequence"], 1);

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn concurrent_writers_commit_combined_and_broadcast_in_sequence_order() {
        let root = temp_dir("concurrent-sequence");
        let session_dir = root.join("session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let dut_log = session_dir.join("dut.log");
        let host_log = session_dir.join("host.log");
        let combined_path = session_dir.join("combined.jsonl");
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            session_dir.clone(),
            dut_log.clone(),
        )));
        let (broadcast_tx, mut broadcast_rx) = broadcast::channel(256);
        let replay = Arc::new(Mutex::new(VecDeque::new()));
        let first_log_at = Arc::new(Mutex::new(None));
        let watches = Arc::new(RwLock::new(HashMap::new()));
        let commit_lock = Arc::new(Mutex::new(()));
        let (dut_tx, dut_rx) = mpsc::channel(64);
        let (host_tx, host_rx) = mpsc::channel(64);
        let make_runtime = |source_id: &str, line_counter: Arc<AtomicU64>| WriterRuntime {
            broadcast_tx: broadcast_tx.clone(),
            replay: replay.clone(),
            first_log_at: first_log_at.clone(),
            session_manager: manager.clone(),
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: Some(line_counter),
            watches: watches.clone(),
            commit_lock: commit_lock.clone(),
            source_meta: SourceRuntimeMeta {
                source_id: source_id.to_string(),
                source_label: source_id.to_string(),
                source_kind: "file".to_string(),
                tab_labels: vec!["Main".to_string()],
                session_id: "session-1".to_string(),
                app_name: "embed-log".to_string(),
                job_id: None,
            },
        };
        let dut_handle = tokio::spawn(run_writer(
            "DUT".to_string(),
            Arc::new(Mutex::new(dut_log)),
            dut_rx,
            make_runtime("DUT", Arc::new(AtomicU64::new(0))),
        ));
        let host_handle = tokio::spawn(run_writer(
            "HOST".to_string(),
            Arc::new(Mutex::new(host_log)),
            host_rx,
            make_runtime("HOST", Arc::new(AtomicU64::new(0))),
        ));
        let dut_sender = tokio::spawn(async move {
            for index in 0..50 {
                dut_tx
                    .send(LogEntry::new(Local::now(), "DUT", format!("dut-{index}")))
                    .await
                    .unwrap();
            }
        });
        let host_sender = tokio::spawn(async move {
            for index in 0..50 {
                host_tx
                    .send(LogEntry::new(Local::now(), "HOST", format!("host-{index}")))
                    .await
                    .unwrap();
            }
        });
        dut_sender.await.unwrap();
        host_sender.await.unwrap();
        dut_handle.await.unwrap();
        host_handle.await.unwrap();

        let combined = std::fs::read_to_string(&combined_path).unwrap();
        let sequences = combined
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["sequence"]
                    .as_u64()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(sequences, (1..=100).collect::<Vec<_>>());

        let mut broadcast_sequences = Vec::new();
        while let Ok(message) = broadcast_rx.try_recv() {
            let value: serde_json::Value = serde_json::from_str(&message).unwrap();
            if let Some(sequence) = value.get("sequence").and_then(|value| value.as_u64()) {
                broadcast_sequences.push(sequence);
            }
        }
        assert_eq!(broadcast_sequences, (1..=100).collect::<Vec<_>>());
        std::fs::remove_dir_all(root).ok();
    }

    fn empty_plugins() -> LoadedPlugins {
        LoadedPlugins {
            definitions: json!({}),
            pane_plugins: json!({}),
            scripts: json!({}),
        }
    }

    #[test]
    fn rotate_session_creates_new_session_and_repoints_writers() {
        let root = temp_dir("rotate-session");
        let first_dir = root.join("session-1");
        std::fs::create_dir_all(&first_dir).unwrap();
        let first_log = first_dir.join("main__dut__session-1.log");

        let writer_path = Arc::new(Mutex::new(first_log.clone()));
        let mut writer_paths = HashMap::new();
        writer_paths.insert("dut".to_string(), writer_path.clone());

        let source_files = Arc::new(Mutex::new(HashMap::from([(
            "dut".to_string(),
            first_log.display().to_string(),
        )])));
        let html_path = Arc::new(Mutex::new(first_dir.join("session.html")));
        let mgr = test_manager("session-1", first_dir.clone(), first_log.clone());
        mgr.write_manifest().unwrap();
        let session_mgr = Arc::new(Mutex::new(mgr));

        let replay = Arc::new(Mutex::new(VecDeque::from(["stale".to_string()])));
        let config_msg = Arc::new(Mutex::new("old-config".to_string()));
        let line_counter = Arc::new(AtomicU64::new(42));

        let mut pane_labels = HashMap::new();
        pane_labels.insert("dut".to_string(), "DUT".to_string());
        let mut pane_kinds = HashMap::new();
        pane_kinds.insert("dut".to_string(), "udp".to_string());

        let ctx = RotationContext {
            logs_root: root.clone(),
            tab_label: "main".to_string(),
            source_names: vec!["dut".to_string()],
            writer_paths,
            source_files: source_files.clone(),
            html_path: html_path.clone(),
            session_mgr,
            first_log_at: Arc::new(Mutex::new(Some(Local::now()))),
            replay: replay.clone(),
            commit_lock: Arc::new(Mutex::new(())),
            line_counters: HashMap::from([("dut".to_string(), line_counter.clone())]),
            frontend: root.join("frontend"),
            tabs: vec![json!({ "label": "Main", "panes": ["dut"] })],
            pane_labels,
            pane_kinds,
            pane_commands: json!({}),
            plugins: empty_plugins(),
            app_name: "embed-log".to_string(),
            job_id: None,
            timestamp_mode: "absolute".to_string(),
            config_msg: config_msg.clone(),
            default_light_theme: None,
            default_dark_theme: None,
            export_on_rotate: false,
            merges: json!([]),
        };

        let title = "EDHOC reconnect attempt 3".to_string();
        let (old_session, new_session) = rotate_session(&ctx, Some(title.clone())).unwrap();
        assert_ne!(old_session, new_session, "session info should change");
        assert_eq!(new_session["title"], title);
        assert!(new_session["id"]
            .as_str()
            .unwrap()
            .contains("_edhoc-reconnect-attempt-3"));

        // Writer now points into a brand-new session directory under the root.
        let new_writer = writer_path.lock().unwrap().clone();
        assert_ne!(new_writer, first_log);
        let new_dir = new_writer.parent().unwrap();
        assert!(new_dir.exists() && new_dir != first_dir);

        // Shared state was swapped to the new session.
        assert_eq!(
            source_files.lock().unwrap()["dut"],
            new_writer.display().to_string()
        );
        assert!(replay.lock().unwrap().is_empty(), "replay buffer cleared");
        assert_eq!(line_counter.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_ne!(*config_msg.lock().unwrap(), "old-config");
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(new_dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["title"], "EDHOC reconnect attempt 3");
        assert!(!first_dir.join("session.html").exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn explicit_export_regenerates_and_replaces_existing_html() {
        let dir = temp_dir("export-replace");
        let log = dir.join("dut.log");
        std::fs::write(&log, "[t] hello\n").unwrap();
        let html = dir.join("session.html");
        std::fs::write(&html, "<html>stale</html>").unwrap();

        let mgr = test_manager("session-1", dir.clone(), log.clone());
        mgr.write_manifest().unwrap();

        let ctx = ExportContext {
            tabs: vec![json!({ "label": "Main", "panes": ["dut"] })],
            labels: HashMap::from([("dut".to_string(), "DUT".to_string())]),
            frontend: dir.join("frontend"),
            ts_mode: "absolute".to_string(),
            source_files: Arc::new(Mutex::new(HashMap::from([(
                "dut".to_string(),
                log.display().to_string(),
            )]))),
            html_path: Arc::new(Mutex::new(html.clone())),
            plugins: empty_plugins(),
            session_mgr: Arc::new(Mutex::new(mgr)),
            first_log_at: Arc::new(Mutex::new(None)),
            merges: json!([]),
        };

        let path = export_session(&ctx).unwrap();
        assert_eq!(path, html.display().to_string());
        let exported = std::fs::read_to_string(&html).unwrap();
        assert!(exported.starts_with("<!DOCTYPE html>"));
        assert!(exported.ends_with("</html>\n"));
        assert!(!exported.contains("<html>stale</html>"));

        std::fs::remove_dir_all(dir).ok();
    }
}
