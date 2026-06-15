use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize},
    Arc, Mutex,
};
use std::time::Duration;

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
use crate::net::forward_server::ForwardServer;
use crate::net::inject_server::InjectServer;
use crate::net::ws_server::{
    start_server, ExportCallback, RotateCallback, RuntimeStats, ServerState, SourceRuntimeStats,
};
use crate::session::{SessionExporter, SessionManager};
use crate::sources::{
    FileSource, LogSource, NetworkCaptureSource, TxCommand, UartSource, UdpSource,
};

const REPLAY_BUFFER_SIZE: usize = 5000;

/// The main server orchestrator.
pub struct LogServer {
    config: AppConfig,
    frontend_dir: PathBuf,
    logs_root: PathBuf,
    config_path: Option<PathBuf>,
}

/// Resolved source information for the runtime.
struct ResolvedSource {
    name: String,
    source: Box<dyn LogSource>,
    inject_port: Option<u16>,
    forward_ports: Vec<u16>,
    label: String,
    source_type: String,
}

/// Loaded plugin data for the config message.
struct LoadedPlugins {
    /// Plugin definitions: `{ "hex-coap": { "builtin": "hex-coap" } }`
    definitions: serde_json::Value,
    /// Pane-plugin mappings: `{ "DUT_UART": [{ "name": "hex-coap" }] }`
    pane_plugins: serde_json::Value,
    /// Plugin JS source code: `{ "hex-coap": "..." }`
    scripts: serde_json::Value,
}

impl LogServer {
    pub fn new(config: AppConfig, frontend_dir: PathBuf, logs_root: PathBuf) -> Self {
        Self {
            config,
            frontend_dir,
            logs_root,
            config_path: None,
        }
    }

    /// Set the config file path — enables resolving relative plugin paths.
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
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

        // ── 3b. Load command suggestions from companion YAML ──
        let source_writability: HashMap<String, bool> = sources
            .iter()
            .map(|src| (src.name.clone(), src.source.writable()))
            .collect();
        let pane_commands = crate::config::load_command_suggestions(
            self.config_path.as_deref(),
            &source_writability,
        );

        // ── 3c. Load event rules from companion YAML ──
        let source_names: Vec<String> = sources.iter().map(|src| src.name.clone()).collect();
        let event_matchers =
            crate::config::load_event_matchers(self.config_path.as_deref(), &source_names);

        // Build source metadata for the control API.
        let source_metadata: HashMap<String, SourceInfo> = sources
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
        );
        session_mgr.write_manifest()?;
        let markers = session_mgr.load_markers();
        let session_mgr = Arc::new(Mutex::new(session_mgr));

        // ── 8. Create broadcast channel ──
        let (broadcast_tx, _rx) = broadcast::channel::<String>(4096);
        let replay = Arc::new(Mutex::new(VecDeque::with_capacity(REPLAY_BUFFER_SIZE)));
        let events_replay = Arc::new(Mutex::new(VecDeque::with_capacity(REPLAY_BUFFER_SIZE)));
        let stats = Arc::new(RuntimeStats::new(
            sources.iter().map(|src| src.name.clone()),
            self.config.server.queue_size,
        ));
        let mut source_txs: HashMap<String, mpsc::Sender<LogEntry>> = HashMap::new();
        let mut source_tx_senders: HashMap<String, mpsc::Sender<TxCommand>> = HashMap::new();

        let mut join_handles: Vec<JoinHandle<()>> = Vec::new();

        // ── 9. Start sources + writers + inject/forward ──
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

            // Spawn source reader.
            let reader_name = src.name.clone();
            let reader_entry_tx = entry_tx.clone();
            let reader_handle = tokio::spawn(async move {
                if let Err(e) = src.source.run(reader_entry_tx).await {
                    error!("[{reader_name}] source error: {e}");
                }
            });
            join_handles.push(reader_handle);

            // Spawn inject server if configured.
            if let Some(inject_port) = src.inject_port {
                let inject_name = src.name.clone();
                let host = self.config.server.host.clone();
                let inject_entry_tx = entry_tx.clone();
                let inject_broadcast = broadcast_tx.clone();
                let inject_handle = tokio::spawn(async move {
                    let server = InjectServer::new(
                        inject_name,
                        host,
                        inject_port,
                        inject_entry_tx,
                        inject_broadcast,
                    );
                    if let Err(e) = server.run().await {
                        error!("[inject:{inject_port}] error: {e}");
                    }
                });
                join_handles.push(inject_handle);
            }

            // Spawn forward servers if configured.
            for &fwd_port in &src.forward_ports {
                let fwd_name = src.name.clone();
                let host = self.config.server.host.clone();
                let fwd_broadcast = broadcast_tx.clone();
                let fwd_handle = tokio::spawn(async move {
                    let server = ForwardServer::new(fwd_name, host, fwd_port, fwd_broadcast);
                    if let Err(e) = server.run().await {
                        error!("[forward:{fwd_port}] error: {e}");
                    }
                });
                join_handles.push(fwd_handle);
            }

            // Spawn writer task.
            let writer_name = src.name.clone();
            let writer_event_matcher = event_matchers.get(&src.name).cloned();
            let writer_runtime = WriterRuntime {
                broadcast_tx: broadcast_tx.clone(),
                replay: replay.clone(),
                events_replay: events_replay.clone(),
                first_log_at: first_log_at.clone(),
                session_manager: session_mgr.clone(),
                stats: stats.source(&src.name),
                ts_mode: self.config.server.timestamp_mode,
                line_counter: line_counters.get(&src.name).cloned(),
                event_matcher: writer_event_matcher,
            };
            let writer_handle = tokio::spawn(async move {
                run_writer(writer_name, log_path, entry_rx, writer_runtime).await;
            });
            join_handles.push(writer_handle);
        }

        // ── 10. Build config message ──
        let session_info = session_mgr.lock().unwrap().build_session_info();
        let event_rules_meta: serde_json::Value = event_matchers
            .iter()
            .map(|(source_name, matcher)| {
                let rules: Vec<serde_json::Value> = matcher
                    .rules()
                    .iter()
                    .map(|r| {
                        json!({
                            "name": r.name,
                            "severity": r.severity,
                        })
                    })
                    .collect();
                (source_name.clone(), serde_json::Value::Array(rules))
            })
            .collect::<serde_json::Map<_, _>>()
            .into();
        let event_rules_meta_clone = event_rules_meta.clone();
        let config_msg = self.build_config_message(
            &pane_labels,
            &pane_kinds,
            &plugins,
            session_info,
            markers,
            event_rules_meta_clone,
        );
        let shared_config_msg = Arc::new(Mutex::new(config_msg.to_string()));

        // ── 11. Create export callback ──
        let export_tabs = tabs_json.clone();
        let export_labels = pane_labels.clone();
        let export_frontend = self.frontend_dir.clone();
        let export_ts_mode = self.config.server.timestamp_mode.to_string();
        let export_source_files = shared_source_files.clone();
        let export_html_path = shared_html_path.clone();
        let export_plugins_definitions = plugins.definitions.clone();
        let export_pane_plugins = plugins.pane_plugins.clone();
        let export_plugin_scripts = plugins.scripts.clone();

        let session_mgr_for_export = session_mgr.clone();
        let export_first_log_at = first_log_at.clone();

        let on_export: ExportCallback = Arc::new(move || {
            let fla = export_first_log_at
                .lock()
                .unwrap()
                .map(|dt| dt.to_rfc3339());
            let export_html = export_html_path.lock().unwrap().clone();
            let export_sources = export_source_files.lock().unwrap().clone();
            let (export_markers, marker_paths) = session_mgr_for_export
                .lock()
                .map(|mgr| {
                    (
                        mgr.load_markers(),
                        vec![mgr.session_dir().join("markers.json")],
                    )
                })
                .unwrap_or_default();

            if session_html_is_current(&export_html, &export_sources, &marker_paths) {
                if let Ok(mut mgr) = session_mgr_for_export.lock() {
                    let _ = mgr.mark_html_exported(&export_html);
                }
                return Ok(export_html.display().to_string());
            }

            let exporter = SessionExporter::new(
                export_html.clone(),
                export_sources,
                export_tabs.clone(),
                export_labels.clone(),
                export_frontend.clone(),
                export_ts_mode.clone(),
                fla,
            )
            .with_plugins(
                export_plugins_definitions.clone(),
                export_pane_plugins.clone(),
                export_plugin_scripts.clone(),
            )
            .with_markers(export_markers);

            match exporter.export() {
                Ok(path) => {
                    if let Ok(mut mgr) = session_mgr_for_export.lock() {
                        let _ = mgr.mark_html_exported(&path);
                    }
                    Ok(path.display().to_string())
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    if let Ok(mut mgr) = session_mgr_for_export.lock() {
                        let _ = mgr.mark_html_error(&err_msg);
                    }
                    Err(err_msg)
                }
            }
        });

        // ── 11b. Create rotation callback ──
        let rotate_logs_root = self.logs_root.clone();
        let rotate_tab_label = tab_label.to_string();
        let rotate_source_names: Vec<String> = writer_log_paths.keys().cloned().collect();
        let rotate_writer_paths = writer_log_paths.clone();
        let rotate_source_files = shared_source_files.clone();
        let rotate_html_path = shared_html_path.clone();
        let rotate_session_mgr = session_mgr.clone();
        let rotate_first_log_at = first_log_at.clone();
        let rotate_replay = replay.clone();
        let rotate_frontend = self.frontend_dir.clone();
        let rotate_tabs = tabs_json.clone();
        let rotate_pane_labels = pane_labels.clone();
        let rotate_pane_kinds = pane_kinds.clone();
        let rotate_pane_commands = pane_commands.clone();
        let rotate_frontend_plugins = plugins.definitions.clone();
        let rotate_pane_plugins = plugins.pane_plugins.clone();
        let rotate_plugin_scripts = plugins.scripts.clone();
        let rotate_app_name = self.config.server.app_name.clone();
        let rotate_job_id = self.config.server.job_id.clone();
        let rotate_timestamp_mode = self.config.server.timestamp_mode.to_string();
        let rotate_config_msg = shared_config_msg.clone();
        let rotate_default_light_theme = self.config.server.default_light_theme.clone();
        let rotate_default_dark_theme = self.config.server.default_dark_theme.clone();
        let rotate_event_rules = event_rules_meta.clone();

        let on_rotate: RotateCallback = Arc::new(move || {
            let (old_session, old_markers) = {
                let manager = rotate_session_mgr
                    .lock()
                    .map_err(|_| "session manager lock failed".to_string())?;
                (manager.build_session_info(), manager.load_markers())
            };
            let old_source_files = rotate_source_files.lock().unwrap().clone();
            let old_html_path = rotate_html_path.lock().unwrap().clone();
            let old_first_log_at = rotate_first_log_at
                .lock()
                .unwrap()
                .map(|dt| dt.to_rfc3339());

            let new_session_id =
                make_session_id_for_root(&rotate_logs_root, rotate_job_id.as_deref())
                    .map_err(|err| err.to_string())?;
            let new_session_dir = rotate_logs_root.join(&new_session_id);
            std::fs::create_dir_all(&new_session_dir).map_err(|err| err.to_string())?;

            let mut new_source_files = HashMap::new();
            for source_name in &rotate_source_names {
                let log_name = format!(
                    "{}__{}__{}.log",
                    slugify(&rotate_tab_label),
                    slugify(source_name),
                    slugify(&new_session_id),
                );
                let log_path = new_session_dir.join(log_name);
                new_source_files.insert(source_name.clone(), log_path.display().to_string());
                if let Some(shared_path) = rotate_writer_paths.get(source_name) {
                    *shared_path.lock().unwrap() = log_path;
                }
            }

            let started_at = Local::now().to_rfc3339();
            let new_manager = SessionManager::new(
                &new_session_id,
                new_session_dir.clone(),
                &rotate_tabs,
                new_source_files.clone(),
                rotate_pane_labels.clone(),
                rotate_pane_kinds.clone(),
                rotate_pane_commands.clone(),
                rotate_frontend_plugins.clone(),
                rotate_pane_plugins.clone(),
                rotate_plugin_scripts.clone(),
                &started_at,
                &rotate_app_name,
                None,
                rotate_job_id.clone(),
                rotate_timestamp_mode.clone(),
                None,
            );
            new_manager
                .write_manifest()
                .map_err(|err| err.to_string())?;

            {
                let mut source_files = rotate_source_files.lock().unwrap();
                *source_files = new_source_files;
            }
            *rotate_html_path.lock().unwrap() = new_session_dir.join("session.html");
            *rotate_first_log_at.lock().unwrap() = None;
            rotate_replay.lock().unwrap().clear();

            let new_session = new_manager.build_session_info();
            *rotate_session_mgr
                .lock()
                .map_err(|_| "session manager lock failed".to_string())? = new_manager;

            let new_config_msg = build_ws_config_message(WsConfigParts {
                app_name: &rotate_app_name,
                default_light_theme: &rotate_default_light_theme,
                default_dark_theme: &rotate_default_dark_theme,
                session_info: new_session.clone(),
                pane_labels: &rotate_pane_labels,
                pane_kinds: &rotate_pane_kinds,
                pane_commands: rotate_pane_commands.clone(),
                tabs: &rotate_tabs,
                frontend_plugins: rotate_frontend_plugins.clone(),
                pane_plugins: rotate_pane_plugins.clone(),
                plugin_scripts: rotate_plugin_scripts.clone(),
                markers: Vec::new(),
                event_rules: rotate_event_rules.clone(),
            });
            *rotate_config_msg
                .lock()
                .map_err(|_| "config message lock failed".to_string())? =
                new_config_msg.to_string();

            let old_export_tabs = rotate_tabs.clone();
            let old_export_labels = rotate_pane_labels.clone();
            let old_export_frontend = rotate_frontend.clone();
            let old_export_ts_mode = rotate_timestamp_mode.clone();
            let old_export_plugin_definitions = rotate_frontend_plugins.clone();
            let old_export_pane_plugins = rotate_pane_plugins.clone();
            let old_export_plugin_scripts = rotate_plugin_scripts.clone();
            if let Some(old_manifest_path) =
                old_html_path.parent().map(|dir| dir.join("manifest.json"))
            {
                let _ = update_manifest_file(
                    &old_manifest_path,
                    &json!({
                        "html_status": "updating",
                        "html_error": serde_json::Value::Null,
                        "last_export_reason": "rotate",
                    }),
                );
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
                    .with_plugins(
                        old_export_plugin_definitions,
                        old_export_pane_plugins,
                        old_export_plugin_scripts,
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

            Ok((old_session, new_session))
        });
        let shutdown_export = on_export.clone();

        // ── 12. Start HTTP + WS server ──
        let state = ServerState {
            config_msg: shared_config_msg,
            broadcast_tx: broadcast_tx.clone(),
            replay: replay.clone(),
            events_replay,
            on_export: Some(on_export.clone()),
            on_rotate: Some(on_rotate),
            session_manager: Some(session_mgr.clone()),
            logs_root: self.logs_root.clone(),
            ws_client_count: Arc::new(AtomicUsize::new(0)),
            no_client_export_generation: Arc::new(AtomicU64::new(0)),
            no_client_export_delay: Duration::from_secs(2),
            stats: stats.clone(),
            source_txs: Arc::new(source_txs),
            source_tx_senders: Arc::new(source_tx_senders),
            source_metadata: Arc::new(source_metadata),
            line_counters: Arc::new(line_counters),
            control_api: self.config.server.control_api,
        };

        let host = self.config.server.host.clone();
        let port = self.config.server.ws_port;
        let frontend_dir = self.frontend_dir.clone();

        let server_handle = tokio::spawn(async move {
            if let Err(e) = start_server(&host, port, Some(frontend_dir), state).await {
                error!("HTTP/WS server error: {e}");
            }
        });
        join_handles.push(server_handle);

        // ── 13. Wait for shutdown ──
        tokio::signal::ctrl_c().await?;
        info!("shutting down…");

        // Export current session HTML on shutdown.
        info!("exporting session HTML before exit…");
        match shutdown_export() {
            Ok(path) => info!("session HTML exported: {path}"),
            Err(e) => error!("session HTML export failed: {e}"),
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
                // Read the builtin plugin JS from frontend/plugin-<name>.js
                let js_path = self.frontend_dir.join(format!("plugin-{builtin}.js"));
                match std::fs::read_to_string(&js_path) {
                    Ok(js) => {
                        scripts.insert(name.clone(), json!(js));
                        info!("loaded builtin plugin: {name} from {}", js_path.display());
                    }
                    Err(e) => {
                        warn!("failed to load builtin plugin {name}: {e}");
                    }
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

        let mut inject_map: HashMap<String, u16> = HashMap::new();
        let mut forward_map: HashMap<String, Vec<u16>> = HashMap::new();

        for src_cfg in &self.config.sources {
            if let Some(port) = src_cfg.inject_port {
                inject_map.insert(src_cfg.name.clone(), port);
            }
            let mut fwd_ports = Vec::new();
            if let Some(port) = src_cfg.forward_port {
                fwd_ports.push(port);
            }
            if let Some(ref ports) = src_cfg.forward_ports {
                fwd_ports.extend(ports);
            }
            if !fwd_ports.is_empty() {
                forward_map.insert(src_cfg.name.clone(), fwd_ports);
            }
        }

        for src_cfg in &self.config.sources {
            let stype = src_cfg.source_type.to_lowercase();
            let label = src_cfg
                .label
                .clone()
                .unwrap_or_else(|| src_cfg.name.clone());

            let parser_type = src_cfg.parser.parser_type.clone();

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
                    Box::new(UartSource::new_with_parser(
                        &src_cfg.name,
                        port_path,
                        baudrate,
                        &parser_type,
                    ))
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
                    Box::new(UdpSource::new_with_parser(
                        &src_cfg.name,
                        port,
                        &parser_type,
                    ))
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
                    Box::new(FileSource::new_with_parser(
                        &src_cfg.name,
                        file_path,
                        &parser_type,
                    ))
                }
                "network_capture" => {
                    let interface = src_cfg.interface.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "source {}: network_capture interface is required",
                            src_cfg.name
                        )
                    })?;
                    let backend = src_cfg
                        .network_backend
                        .clone()
                        .unwrap_or_else(|| "scapy".to_string());
                    Box::new(NetworkCaptureSource::new(
                        &src_cfg.name,
                        interface,
                        src_cfg.bpf_filter.clone(),
                        backend,
                        src_cfg.mock_interval,
                    ))
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
                inject_port: inject_map.get(&src_cfg.name).copied(),
                forward_ports: forward_map.get(&src_cfg.name).cloned().unwrap_or_default(),
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
        event_rules_meta: serde_json::Value,
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
            event_rules: event_rules_meta,
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
    event_rules: serde_json::Value,
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
        "event_rules": parts.event_rules,
    })
}

fn session_html_is_current(
    html_path: &Path,
    source_files: &HashMap<String, String>,
    extra_paths: &[PathBuf],
) -> bool {
    let Ok(html_meta) = std::fs::metadata(html_path) else {
        return false;
    };
    let Ok(html_mtime) = html_meta.modified() else {
        return false;
    };

    for path in source_files
        .values()
        .map(PathBuf::from)
        .chain(extra_paths.iter().cloned())
    {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            return false;
        };
        if mtime > html_mtime {
            return false;
        }
    }

    true
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

    std::fs::write(path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("update manifest {}", path.display()))
}

fn make_session_id_for_root(logs_root: &Path, job_id: Option<&str>) -> Result<String> {
    let base_time = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let base = if let Some(job_id) = job_id {
        format!("{base_time}__{}", slugify(job_id))
    } else {
        base_time
    };

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

struct WriterRuntime {
    broadcast_tx: broadcast::Sender<String>,
    replay: Arc<Mutex<VecDeque<String>>>,
    events_replay: Arc<Mutex<VecDeque<String>>>,
    first_log_at: Arc<Mutex<Option<DateTime<Local>>>>,
    session_manager: Arc<Mutex<SessionManager>>,
    stats: Option<Arc<SourceRuntimeStats>>,
    ts_mode: TimestampMode,
    line_counter: Option<Arc<AtomicU64>>,
    event_matcher: Option<crate::config::PatternMatcher>,
}

/// Create an event marker and broadcast a markers_update.
///
/// Replaces any existing event marker at the same (paneId, lineIdx),
/// but preserves user markers (kind != "event").
#[allow(clippy::too_many_arguments)]
fn save_event_marker(
    session_manager: &Arc<Mutex<SessionManager>>,
    broadcast_tx: &broadcast::Sender<String>,
    source_name: &str,
    line_idx: u64,
    num_ts: f64,
    rule_name: &str,
    severity: &str,
    message: &str,
) {
    if let Ok(mgr) = session_manager.lock() {
        let mut markers: Vec<serde_json::Value> = mgr.load_markers();

        // Remove only event markers at the same (paneId, lineIdx).
        // User markers (kind != "event") are preserved.
        markers.retain(|m| {
            let same_pane = m.get("paneId").and_then(|v| v.as_str()) == Some(source_name);
            let same_idx = m.get("lineIdx").and_then(|v| v.as_u64()) == Some(line_idx);
            let is_event = m.get("kind").and_then(|v| v.as_str()).unwrap_or("user") == "event";
            !(same_pane && same_idx && is_event)
        });

        let now = chrono::Local::now();
        let event_marker = serde_json::json!({
            "paneId": source_name,
            "lineIdx": line_idx,
            "endIdx": line_idx,
            "numTs": num_ts,
            "description": format!("{}: {}", rule_name, message),
            "kind": "event",
            "severity": severity,
            "createdAt": now.to_rfc3339(),
        });
        markers.push(event_marker);

        if let Err(e) = mgr.save_markers(&markers) {
            error!("[{source_name}] failed to save event marker: {e}");
        }

        let markers_update = serde_json::json!({
            "type": "markers_update",
            "markers": markers,
            "session": mgr.build_session_info(),
        });
        let _ = broadcast_tx.send(markers_update.to_string());
    }
}

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
    use std::io::Write;

    while let Some(entry) = entry_rx.recv().await {
        let desired_log_path = log_path.lock().unwrap().clone();
        if desired_log_path != current_log_path {
            let _ = file.flush();
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
        }
        let _ = file.flush();

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

        let payload = json!({
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

        let payload_str = payload.to_string();

        // ── Event detection ──
        // Check message against compiled event rules for this source.
        if let Some(ref matcher) = runtime.event_matcher {
            for event_match in matcher.check(&message) {
                let event_payload = json!({
                    "type": "event",
                    "event_id": event_match.rule_name,
                    "source_id": source_name,
                    "severity": event_match.severity,
                    "timestamp": display_ts,
                    "timestamp_iso": ts_iso,
                    "timestamp_num": abs_num,
                    "rel_num": rel_num,
                    "line_idx": line_idx,
                    "message": message,
                    "origin": origin,
                    "captures": event_match.captures,
                });

                // Broadcast the event to all WS clients.
                let _ = runtime.broadcast_tx.send(event_payload.to_string());

                // Persist event to events.jsonl.
                if let Ok(mgr) = runtime.session_manager.lock() {
                    if let Err(e) = mgr.append_event(&event_payload) {
                        error!("[{source_name}] failed to append event: {e}");
                    }
                }

                // Push to events replay buffer.
                {
                    let mut buf = runtime.events_replay.lock().unwrap();
                    if buf.len() >= REPLAY_BUFFER_SIZE {
                        buf.pop_front();
                    }
                    buf.push_back(event_payload.to_string());
                }

                // Create an event marker (replaces previous event markers at this line,
                // preserves user markers).
                save_event_marker(
                    &runtime.session_manager,
                    &runtime.broadcast_tx,
                    &source_name,
                    line_idx,
                    abs_num,
                    &event_match.rule_name,
                    &event_match.severity,
                    &message,
                );
            }
        }

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

        let mut pane_labels = HashMap::new();
        pane_labels.insert("dut".to_string(), "DUT".to_string());

        let mut pane_kinds = HashMap::new();
        pane_kinds.insert("dut".to_string(), "udp".to_string());

        SessionManager::new(
            session_id,
            dir,
            &[json!({ "label": "Main", "panes": ["dut"] })],
            source_files,
            pane_labels,
            pane_kinds,
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
        )
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
    fn collision_safe_session_id_appends_suffix() {
        let root = temp_dir("collision");
        let id = make_session_id_for_root(&root, Some("job")).unwrap();
        std::fs::create_dir_all(root.join(&id)).unwrap();
        let next = make_session_id_for_root(&root, Some("job")).unwrap();
        assert_ne!(id, next);
        assert!(next.starts_with(&id));
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
            events_replay: Arc::new(Mutex::new(VecDeque::new())),
            first_log_at: first_log_at.clone(),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            event_matcher: None,
        };
        let handle = tokio::spawn(run_writer(
            "dut".to_string(),
            path.clone(),
            entry_rx,
            runtime,
        ));

        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "first".to_string(),
            ))
            .await
            .unwrap();
        wait_file_contains(&first_log, "first").await;

        *path.lock().unwrap() = second_log.clone();
        *first_log_at.lock().unwrap() = None;
        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "second".to_string(),
            ))
            .await
            .unwrap();
        wait_file_contains(&second_log, "second").await;

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn event_match_broadcasts_event_and_creates_marker() {
        let root = temp_dir("event-match");
        std::fs::create_dir_all(&root).unwrap();
        let session_dir = root.join("session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let log_path = session_dir.join("main__dut__session-1.log");

        let path = Arc::new(Mutex::new(log_path.clone()));
        let (entry_tx, entry_rx) = mpsc::channel(4);
        let (broadcast_tx, mut broadcast_rx) = broadcast::channel(32);
        let replay = Arc::new(Mutex::new(VecDeque::new()));
        let first_log_at = Arc::new(Mutex::new(None));
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            session_dir.clone(),
            log_path.clone(),
        )));

        // Build a PatternMatcher with a rule matching "ERROR".
        let matcher = crate::config::PatternMatcher::new(vec![crate::config::EventRule {
            name: "fatal_error".to_string(),
            pattern: "ERROR".to_string(),
            severity: "error".to_string(),
            regex: regex::Regex::new("ERROR").unwrap(),
        }]);

        let runtime = WriterRuntime {
            broadcast_tx: broadcast_tx.clone(),
            replay,
            events_replay: Arc::new(Mutex::new(VecDeque::new())),
            first_log_at: first_log_at.clone(),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            event_matcher: Some(matcher),
        };
        let handle = tokio::spawn(run_writer("dut".to_string(), path, entry_rx, runtime));

        // Send a matching log entry.
        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "FATAL ERROR: overflow".to_string(),
            ))
            .await
            .unwrap();

        // Wait for the event on broadcast.
        let mut found_event = false;
        let mut found_marker = false;
        for _ in 0..50 {
            match broadcast_rx.try_recv() {
                Ok(msg) => {
                    let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
                    if parsed["type"] == "event" {
                        assert_eq!(parsed["event_id"], "fatal_error");
                        assert_eq!(parsed["source_id"], "dut");
                        assert_eq!(parsed["severity"], "error");
                        assert_eq!(parsed["message"], "FATAL ERROR: overflow");
                        assert_eq!(parsed["captures"][0], "ERROR");
                        assert!(parsed["line_idx"].as_u64().is_some());
                        assert!(parsed["timestamp_num"].as_f64().is_some());
                        found_event = true;
                    }
                    if parsed["type"] == "markers_update" {
                        let markers = parsed["markers"].as_array().unwrap();
                        let event_marker = markers
                            .iter()
                            .find(|m| m["kind"] == "event" && m["severity"] == "error");
                        assert!(event_marker.is_some(), "no event marker found");
                        assert_eq!(
                            event_marker.unwrap()["description"],
                            "fatal_error: FATAL ERROR: overflow"
                        );
                        found_marker = true;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    sleep(Duration::from_millis(10)).await;
                }
                Err(_) => break,
            }
            if found_event && found_marker {
                break;
            }
        }

        assert!(found_event, "expected event broadcast");
        assert!(found_marker, "expected marker_update broadcast");

        // Verify marker persisted in markers.json.
        let markers_path = session_dir.join("markers.json");
        let markers_text = std::fs::read_to_string(&markers_path).unwrap();
        assert!(markers_text.contains(r#""kind": "event""#));
        assert!(markers_text.contains(r#""severity": "error""#));

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn event_match_multiple_rules() {
        let root = temp_dir("event-multi");
        std::fs::create_dir_all(&root).unwrap();
        let session_dir = root.join("session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let log_path = session_dir.join("main__dut__session-1.log");

        let path = Arc::new(Mutex::new(log_path.clone()));
        let (entry_tx, entry_rx) = mpsc::channel(4);
        let (broadcast_tx, mut broadcast_rx) = broadcast::channel(32);
        let replay = Arc::new(Mutex::new(VecDeque::new()));
        let first_log_at = Arc::new(Mutex::new(None));
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            session_dir.clone(),
            log_path.clone(),
        )));

        let rule1 = crate::config::EventRule {
            name: "error".to_string(),
            pattern: "ERROR".to_string(),
            severity: "error".to_string(),
            regex: regex::Regex::new("ERROR").unwrap(),
        };
        let rule2 = crate::config::EventRule {
            name: "warn".to_string(),
            pattern: "WARN".to_string(),
            severity: "warn".to_string(),
            regex: regex::Regex::new("WARN").unwrap(),
        };
        let matcher = crate::config::PatternMatcher::new(vec![rule1, rule2]);

        let runtime = WriterRuntime {
            broadcast_tx: broadcast_tx.clone(),
            replay,
            events_replay: Arc::new(Mutex::new(VecDeque::new())),
            first_log_at: first_log_at.clone(),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            event_matcher: Some(matcher),
        };
        let handle = tokio::spawn(run_writer("dut".to_string(), path, entry_rx, runtime));

        // Send a log entry matching BOTH rules.
        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "ERROR: something, WARN: caution".to_string(),
            ))
            .await
            .unwrap();

        let mut event_count = 0;
        for _ in 0..50 {
            match broadcast_rx.try_recv() {
                Ok(msg) => {
                    let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
                    if parsed["type"] == "event" {
                        event_count += 1;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    sleep(Duration::from_millis(10)).await;
                }
                Err(_) => break,
            }
            if event_count >= 2 {
                break;
            }
        }

        assert_eq!(event_count, 2, "expected 2 events for 2 matching rules");

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn non_matching_line_produces_no_event() {
        let root = temp_dir("event-no-match");
        std::fs::create_dir_all(&root).unwrap();
        let session_dir = root.join("session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let log_path = session_dir.join("main__dut__session-1.log");

        let path = Arc::new(Mutex::new(log_path.clone()));
        let (entry_tx, entry_rx) = mpsc::channel(4);
        let (broadcast_tx, mut broadcast_rx) = broadcast::channel(32);
        let replay = Arc::new(Mutex::new(VecDeque::new()));
        let first_log_at = Arc::new(Mutex::new(None));
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            session_dir.clone(),
            log_path.clone(),
        )));

        let rule = crate::config::EventRule {
            name: "fatal".to_string(),
            pattern: "FATAL".to_string(),
            severity: "fatal".to_string(),
            regex: regex::Regex::new("FATAL").unwrap(),
        };
        let matcher = crate::config::PatternMatcher::new(vec![rule]);

        let runtime = WriterRuntime {
            broadcast_tx: broadcast_tx.clone(),
            replay,
            events_replay: Arc::new(Mutex::new(VecDeque::new())),
            first_log_at: first_log_at.clone(),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            event_matcher: Some(matcher),
        };
        let handle = tokio::spawn(run_writer("dut".to_string(), path, entry_rx, runtime));

        // Send a non-matching line.
        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "boot complete".to_string(),
            ))
            .await
            .unwrap();

        // Small delay to let the writer process.
        sleep(Duration::from_millis(50)).await;

        // Check that no event was broadcast.
        let events_received: Vec<String> =
            std::iter::from_fn(|| broadcast_rx.try_recv().ok()).collect();

        let has_event = events_received.iter().any(|m| {
            serde_json::from_str::<serde_json::Value>(m)
                .map(|v| v["type"] == "event")
                .unwrap_or(false)
        });
        assert!(!has_event, "non-matching line should not produce event");

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn source_without_rules_produces_no_events() {
        let root = temp_dir("event-no-rules");
        std::fs::create_dir_all(&root).unwrap();
        let session_dir = root.join("session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let log_path = session_dir.join("main__dut__session-1.log");

        let path = Arc::new(Mutex::new(log_path.clone()));
        let (entry_tx, entry_rx) = mpsc::channel(4);
        let (broadcast_tx, mut broadcast_rx) = broadcast::channel(32);
        let replay = Arc::new(Mutex::new(VecDeque::new()));
        let first_log_at = Arc::new(Mutex::new(None));
        let manager = Arc::new(Mutex::new(test_manager(
            "session-1",
            session_dir.clone(),
            log_path.clone(),
        )));

        // Empty event_matchers — no rules for any source.
        let runtime = WriterRuntime {
            broadcast_tx: broadcast_tx.clone(),
            replay,
            events_replay: Arc::new(Mutex::new(VecDeque::new())),
            first_log_at: first_log_at.clone(),
            session_manager: manager,
            stats: None,
            ts_mode: TimestampMode::Absolute,
            line_counter: None,
            event_matcher: None,
        };
        let handle = tokio::spawn(run_writer("dut".to_string(), path, entry_rx, runtime));

        entry_tx
            .send(LogEntry::new(
                Local::now(),
                "dut".to_string(),
                "ERROR: this would match if rules existed".to_string(),
            ))
            .await
            .unwrap();

        sleep(Duration::from_millis(50)).await;

        let has_event = std::iter::from_fn(|| broadcast_rx.try_recv().ok()).any(|m| {
            serde_json::from_str::<serde_json::Value>(&m)
                .map(|v| v["type"] == "event")
                .unwrap_or(false)
        });
        assert!(!has_event, "source without rules should not produce events");

        drop(entry_tx);
        handle.await.unwrap();
        std::fs::remove_dir_all(root).ok();
    }
}
