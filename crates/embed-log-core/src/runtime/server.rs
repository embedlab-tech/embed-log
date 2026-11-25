use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize},
    Arc, Mutex, RwLock,
};

use anyhow::{Context, Result};
use chrono::Local;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::AppConfig;
use crate::models::LogEntry;
use crate::naming::slugify;
use crate::net::control_ws::SourceInfo;
use crate::net::live::LiveMessage;
use crate::net::ws_server::{
    start_server, ExportCallback, RotateCallback, RuntimeStats, ServerState,
};
use crate::session::manager::atomic_write_file;
use crate::session::{SessionExporter, SessionManager};
use crate::sources::{FileSource, LogSource, TxCommand, UartSource, UdpSource};

use super::writer::{SourceRuntimeMeta, SourceSpec, Writer, WriterShared, REPLAY_BUFFER_SIZE};

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
        let combined_path = session_dir.join("combined.jsonl");

        // ── 5. Load frontend plugins ──
        let plugins = self.load_plugins();

        // ── 6. Create SessionManager and write manifest ──
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
        )
        .with_source_levels(crate::levels::source_levels_json(&self.config));
        session_mgr.write_manifest()?;
        let markers = session_mgr.load_markers();
        let session_mgr = Arc::new(Mutex::new(session_mgr));

        // ── 8. Create broadcast channel ──
        let (broadcast_tx, _rx) = broadcast::channel::<LiveMessage>(4096);
        let replay = Arc::new(Mutex::new(VecDeque::with_capacity(REPLAY_BUFFER_SIZE)));
        let stats = Arc::new(RuntimeStats::new(
            sources.iter().map(|src| src.name.clone()),
            self.config.server.queue_size,
        ));
        let mut source_txs: HashMap<String, mpsc::Sender<LogEntry>> = HashMap::new();
        let mut source_tx_senders: HashMap<String, mpsc::Sender<TxCommand>> = HashMap::new();
        let mut dictionaries = HashMap::new();

        let mut join_handles: Vec<JoinHandle<()>> = Vec::new();

        let source_tab_labels = build_source_tab_labels(&self.config.tabs, &self.config.merges);

        // ── 9. Start sources and the session writer ──
        let mut writer_sources = Vec::new();
        for mut src in sources {
            let (entry_tx, entry_rx) = mpsc::channel::<LogEntry>(self.config.server.queue_size);
            source_txs.insert(src.name.clone(), entry_tx.clone());

            // If this source is writable (e.g., UART), create a TX command channel
            // so the frontend/SDK can write bytes to it.
            if src.source.writable() {
                let (tx_sender, tx_receiver) = mpsc::channel::<TxCommand>(32);
                source_tx_senders.insert(src.name.clone(), tx_sender);
                src.source.set_tx_receiver(tx_receiver);
            }

            if self.config.server.control_api {
                if let Some(config) = src.source.parser_config() {
                    match crate::sources::dictionary::DictionaryControl::new(
                        &src.name,
                        config,
                        session_mgr.clone(),
                    ) {
                        Ok((control, runtime)) => {
                            dictionaries.insert(src.name.clone(), control);
                            src.source.set_dictionary_runtime(runtime);
                        }
                        Err(e) => warn!(
                            "[{}] runtime dictionary control unavailable (database {:?}): {e}; continuing with configured parser",
                            src.name, config.database
                        ),
                    }
                }
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

            writer_sources.push(SourceSpec {
                log_path: log_paths[&src.name].clone(),
                entries: entry_rx,
                stats: stats.source(&src.name),
                line_counter: line_counters[&src.name].clone(),
                level_detector: self.level_detector_for(&src.name),
                meta: SourceRuntimeMeta {
                    source_id: src.name.clone(),
                    source_label: src.label.clone(),
                    source_kind: src.source_type.clone(),
                    tab_labels: source_tab_labels
                        .get(&src.name)
                        .cloned()
                        .unwrap_or_default(),
                    app_name: self.config.server.app_name.clone(),
                    job_id: self.config.server.job_id.clone(),
                },
                name: src.name,
            });
        }

        let writer = Writer::new(
            WriterShared {
                broadcast_tx: broadcast_tx.clone(),
                replay: replay.clone(),
                session_manager: session_mgr.clone(),
                watches: watches.clone(),
                ts_mode: self.config.server.timestamp_mode,
            },
            writer_sources,
        );
        let (writer, writer_task) = writer.spawn();
        join_handles.push(writer_task);

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
            plugins: plugins.clone(),
            session_mgr: session_mgr.clone(),
            merges: serde_json::to_value(&self.config.merges).unwrap_or_else(|_| json!([])),
            source_levels: crate::levels::source_levels_json(&self.config),
        };
        let on_export: ExportCallback = Arc::new(move || export_session(&export_ctx));

        // ── 11b. Create rotation callback ──
        let rotation_ctx = RotationContext {
            logs_root: self.logs_root.clone(),
            tab_label: tab_label.to_string(),
            session_mgr: session_mgr.clone(),
            replay: replay.clone(),
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
            runtime_config: runtime_config_summary(&self.config, &self.logs_root),
            source_levels: crate::levels::source_levels_json(&self.config),
            default_light_theme: self.config.server.default_light_theme.clone(),
            default_dark_theme: self.config.server.default_dark_theme.clone(),
            export_on_rotate: self.export_on_shutdown,
            merges: serde_json::to_value(&self.config.merges).unwrap_or_else(|_| json!([])),
        };
        let rotation_ctx = Arc::new(rotation_ctx);
        let on_rotate: RotateCallback = Arc::new(move |title| {
            let ctx = rotation_ctx.clone();
            let writer = writer.clone();
            Box::pin(async move {
                writer
                    .exec(move |writer| rotate_session(&ctx, title, writer))
                    .await?
            })
        });
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
            dictionaries: Arc::new(dictionaries),
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

    /// Per-source level classifier. Sources not present in the config (should
    /// not happen) and `levels: off` get none.
    fn level_detector_for(&self, source_name: &str) -> Option<crate::levels::LevelDetector> {
        let source = self.config.sources.iter().find(|s| s.name == source_name)?;
        let profile = crate::levels::resolve_source_profile(&self.config, source)?;
        match crate::levels::LevelDetector::new(&profile, &format!("sources.{source_name}.levels"))
        {
            Ok(detector) => Some(detector),
            Err(error) => {
                warn!("[{source_name}] log level detection disabled: {error}");
                None
            }
        }
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
            runtime_config: runtime_config_summary(&self.config, &self.logs_root),
            source_levels: crate::levels::source_levels_json(&self.config),
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

// Deliberate allowlist: do not publish the YAML, plugin options, scripts or environment.
fn runtime_config_summary(config: &AppConfig, logs_root: &std::path::Path) -> serde_json::Value {
    let cwd = std::env::current_dir().ok();
    let absolute_path = |path: &Path| {
        path.canonicalize().unwrap_or_else(|_| {
            cwd.as_ref()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|| path.to_path_buf())
        })
    };
    json!({
        "server": { "host": config.server.host, "port": config.server.ws_port,
            "timestamp_mode": config.server.timestamp_mode.to_string() },
        "logs_dir": absolute_path(logs_root),
        "sources": config.sources.iter().map(|source| json!({
            "name": source.name, "label": source.label, "type": source.source_type,
            "port": match (source.source_type.as_str(), source.port.as_str()) {
                ("file", Some(path)) => json!(absolute_path(Path::new(path))),
                _ => json!(source.port),
            },
            "baudrate": if source.source_type == "uart" { Some(source.baudrate.unwrap_or(config.baudrate)) } else { None },
            "parser": { "type": source.parser.parser_type,
                "database": source.parser.database.as_deref().map(|path| absolute_path(Path::new(path))) },
        })).collect::<Vec<_>>(),
    })
}

struct WsConfigParts<'a> {
    app_name: &'a str,
    default_light_theme: &'a Option<String>,
    default_dark_theme: &'a Option<String>,
    session_info: serde_json::Value,
    runtime_config: serde_json::Value,
    source_levels: serde_json::Value,
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
        "runtime_config": parts.runtime_config,
        "source_levels": parts.source_levels,
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
    plugins: LoadedPlugins,
    session_mgr: Arc<Mutex<SessionManager>>,
    merges: serde_json::Value,
    source_levels: serde_json::Value,
}

/// Export the current session to HTML from the latest complete canonical
/// combined stream. SessionExporter serializes producers and publishes atomically.
fn export_session(ctx: &ExportContext) -> Result<String, String> {
    let (fla, export_html, export_sources, export_markers, combined_file) = {
        let mgr = ctx
            .session_mgr
            .lock()
            .map_err(|_| "session manager lock failed".to_string())?;
        (
            mgr.first_log_at().map(str::to_string),
            mgr.html_path(),
            mgr.source_files().clone(),
            mgr.load_markers(),
            mgr.combined_file(),
        )
    };

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
    .with_source_levels(ctx.source_levels.clone())
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
    session_mgr: Arc<Mutex<SessionManager>>,
    replay: Arc<Mutex<VecDeque<LiveMessage>>>,
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
    runtime_config: serde_json::Value,
    source_levels: serde_json::Value,
    default_light_theme: Option<String>,
    default_dark_theme: Option<String>,
    export_on_rotate: bool,
    merges: serde_json::Value,
}

/// Roll over to a fresh session without restarting source tasks or releasing
/// UART ownership. Runs inside the session writer, between two records.
/// Returns `(old_session_info, new_session_info)`.
fn rotate_session(
    ctx: &RotationContext,
    title: Option<String>,
    writer: &mut Writer,
) -> Result<(serde_json::Value, serde_json::Value), String> {
    let title = normalize_session_title(title)?;
    let (old_session, old_markers, old_source_files, old_html_path) = {
        let manager = ctx
            .session_mgr
            .lock()
            .map_err(|_| "session manager lock failed".to_string())?;
        (
            manager.build_session_info(),
            manager.load_markers(),
            manager.source_files().clone(),
            manager.html_path(),
        )
    };
    let old_first_log_at = writer.first_log_at().map(|dt| dt.to_rfc3339());

    let new_session_id =
        make_titled_session_id_for_root(&ctx.logs_root, ctx.job_id.as_deref(), title.as_deref())
            .map_err(|err| err.to_string())?;
    let new_session_dir = ctx.logs_root.join(&new_session_id);
    std::fs::create_dir_all(&new_session_dir).map_err(|err| err.to_string())?;

    let mut new_source_files = HashMap::new();
    let mut new_log_paths = HashMap::new();
    for source_name in writer.source_names() {
        let log_name = format!(
            "{}__{}__{}.log",
            slugify(&ctx.tab_label),
            slugify(source_name),
            slugify(&new_session_id),
        );
        let log_path = new_session_dir.join(log_name);
        new_source_files.insert(source_name.to_string(), log_path.display().to_string());
        new_log_paths.insert(source_name.to_string(), log_path);
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
    .with_title(title)
    .with_source_levels(ctx.source_levels.clone());
    new_manager
        .write_manifest()
        .map_err(|err| err.to_string())?;

    writer.start_session(&new_log_paths);
    // Hold the replay lock until the new config is in place, so a client that
    // connects during rotation sees either the old session or the new one.
    let mut replay = ctx.replay.lock().unwrap();
    replay.clear();

    let new_session = new_manager.build_session_info();
    *ctx.session_mgr
        .lock()
        .map_err(|_| "session manager lock failed".to_string())? = new_manager;

    let new_config_msg = build_ws_config_message(WsConfigParts {
        app_name: &ctx.app_name,
        default_light_theme: &ctx.default_light_theme,
        default_dark_theme: &ctx.default_dark_theme,
        session_info: new_session.clone(),
        runtime_config: ctx.runtime_config.clone(),
        source_levels: ctx.source_levels.clone(),
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
    drop(replay);

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
            let old_export_source_levels = ctx.source_levels.clone();
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
                .with_source_levels(old_export_source_levels)
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

#[cfg(test)]
mod tests {
    use super::super::writer::test_support::*;
    use super::*;

    #[test]
    fn runtime_summary_uses_effective_values_without_plugin_contents() {
        let root = std::env::temp_dir().join("embed-log-runtime-summary");
        let database = root.join("database.json");
        let logs = root.join("logs");
        let config: AppConfig = serde_json::from_value(json!({
            "baudrate": 230400,
            "sources": [{"name":"DUT", "type":"uart", "port":"/dev/ttyUSB0",
                "parser":{"type":"zephyr-dict", "database":database}},
                {"name":"HOST", "type":"uart", "port":"/dev/ttyUSB1", "baudrate":9600}],
            "frontend_plugins":{"private":{"path":"/private/not-for-publication.js"}},
            "logs":{"dir":"not-the-effective-directory"}
        }))
        .unwrap();
        let summary = runtime_config_summary(&config, &logs);
        assert_eq!(summary["logs_dir"], json!(logs));
        assert_eq!(summary["sources"][0]["baudrate"], 230400);
        assert_eq!(summary["sources"][1]["baudrate"], 9600);
        assert_eq!(summary["sources"][0]["parser"]["database"], json!(database));
        assert!(!summary.to_string().contains("not-for-publication"));
        assert_eq!(summary.as_object().unwrap().len(), 3);
    }
    #[test]
    fn runtime_summary_resolves_relative_paths_without_rewriting_device_names_or_ports() {
        let config: AppConfig = serde_json::from_value(json!({
            "sources": [
                {"name":"FILE", "type":"file", "port":"missing-input.log",
                    "parser":{"type":"zephyr-dict", "database":"missing-dictionary.json"}},
                {"name":"UART", "type":"uart", "port":"COM4"},
                {"name":"UDP", "type":"udp", "port":9999}
            ]
        }))
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let summary = runtime_config_summary(&config, Path::new("missing-log-directory"));
        assert_eq!(
            summary["logs_dir"],
            json!(cwd.join("missing-log-directory"))
        );
        assert_eq!(
            summary["sources"][0]["port"],
            json!(cwd.join("missing-input.log"))
        );
        assert_eq!(
            summary["sources"][0]["parser"]["database"],
            json!(cwd.join("missing-dictionary.json"))
        );
        assert_eq!(summary["sources"][1]["port"], "COM4");
        assert_eq!(summary["sources"][2]["port"], 9999);
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

    fn empty_plugins() -> LoadedPlugins {
        LoadedPlugins {
            definitions: json!({}),
            pane_plugins: json!({}),
            scripts: json!({}),
        }
    }

    #[tokio::test]
    async fn rotate_session_starts_a_new_session_in_the_writer() {
        let root = temp_dir("rotate-session");
        let first_dir = root.join("session-1");
        std::fs::create_dir_all(&first_dir).unwrap();
        let first_log = first_dir.join("main__dut__session-1.log");
        let mgr = test_manager("session-1", first_dir.clone(), first_log.clone());
        mgr.write_manifest().unwrap();
        let rig = TestWriter::spawn(mgr, &[("dut", first_log.clone())], 16, None);
        rig.send("dut", "before rotation").await;
        rig.send("dut", "before rotation 2").await;
        wait_file_contains(&first_log, "before rotation 2").await;

        let config_msg = Arc::new(Mutex::new("old-config".to_string()));
        let ctx = Arc::new(RotationContext {
            logs_root: root.clone(),
            tab_label: "main".to_string(),
            session_mgr: rig.manager.clone(),
            replay: rig.replay.clone(),
            frontend: root.join("frontend"),
            tabs: vec![json!({ "label": "Main", "panes": ["dut"] })],
            pane_labels: HashMap::from([("dut".to_string(), "DUT".to_string())]),
            pane_kinds: HashMap::from([("dut".to_string(), "udp".to_string())]),
            pane_commands: json!({}),
            runtime_config: json!({"sources": [{"name": "dut", "type": "uart"}]}),
            source_levels: json!({"dut": crate::levels::profile_summary(&crate::levels::builtin_zephyr_profile())}),
            plugins: empty_plugins(),
            app_name: "embed-log".to_string(),
            job_id: None,
            timestamp_mode: "absolute".to_string(),
            config_msg: config_msg.clone(),
            default_light_theme: None,
            default_dark_theme: None,
            export_on_rotate: false,
            merges: json!([]),
        });

        let title = "EDHOC reconnect attempt 3".to_string();
        let rotation = ctx.clone();
        let rotation_title = title.clone();
        let (old_session, new_session) = rig
            .writer
            .exec(move |writer| rotate_session(&rotation, Some(rotation_title), writer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(old_session["id"], "session-1");
        assert_eq!(new_session["title"], title);
        let new_id = new_session["id"].as_str().unwrap().to_string();
        assert!(new_id.contains("_edhoc-reconnect-attempt-3"));
        let new_dir = root.join(&new_id);

        // The session manager, replay and config now describe the new session.
        let new_log = PathBuf::from(&rig.manager.lock().unwrap().source_files()["dut"]);
        assert_eq!(new_log.parent().unwrap(), new_dir);
        assert!(
            rig.replay.lock().unwrap().is_empty(),
            "replay buffer cleared"
        );
        assert_eq!(
            rig.counters["dut"].load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        let rotated_config: serde_json::Value =
            serde_json::from_str(&config_msg.lock().unwrap()).unwrap();
        assert_eq!(rotated_config["session"]["id"], new_id);
        assert_eq!(rotated_config["runtime_config"], ctx.runtime_config);
        assert_eq!(rotated_config["source_levels"], ctx.source_levels);
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(new_dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["title"], "EDHOC reconnect attempt 3");
        assert!(!first_dir.join("session.html").exists());

        // The next record starts the new session's files and numbering.
        rig.send("dut", "after rotation").await;
        wait_file_contains(&new_log, "after rotation").await;
        let replayed = rig.replay.lock().unwrap().len();
        rig.finish().await;
        assert!(!std::fs::read_to_string(&first_log)
            .unwrap()
            .contains("after rotation"));
        let records = read_jsonl(&new_dir.join("combined.jsonl"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["message"], "after rotation");
        assert_eq!(records[0]["sequence"], 1);
        assert_eq!(records[0]["line_idx"], 0);
        assert_eq!(records[0]["session_id"], new_id);
        assert_eq!(read_jsonl(&first_dir.join("combined.jsonl")).len(), 2);
        assert_eq!(replayed, 1);

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
            plugins: empty_plugins(),
            session_mgr: Arc::new(Mutex::new(mgr)),
            merges: json!([]),
            source_levels: json!({}),
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
