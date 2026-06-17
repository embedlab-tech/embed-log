use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use embed_log_core::config::load_config;
use embed_log_core::demo::{prepare_demo_file_sources, spawn_demo_traffic};
use embed_log_core::runtime::LogServer;
use embed_log_core::session::SessionExporter;

const DEMO_CONFIG: &str = r#"version: 1
server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log demo
  open_browser: true
  timestamp_mode: absolute

logs:
  dir: logs/

baudrate: 115200

frontend_plugins:
  hex-coap:
    builtin: hex-coap

sources:
  - name: DUT
    label: DUT Device
    type: udp
    port: 6000

  - name: HOST
    label: Host Controller
    type: udp
    port: 6001

  - name: UART_DUT
    label: UART Main
    type: udp
    port: 6100

  - name: UART_DEBUG
    label: UART Debug
    type: udp
    port: 6101

  - name: COAP_RAW
    label: CoAP Raw Hex
    type: udp
    port: 6005

  - name: SENSORS
    label: Sensor CBOR
    type: udp
    port: 6002
    parser:
      type: cbor-datagram

  - name: NET_CAPTURE
    label: Network Mock
    type: network_capture
    network_backend: mock
    interface: mock0
    mock_interval: 1.0
    bpf_filter: udp or coap

  - name: FILE_WATCH
    label: Watched File
    type: file
    port: .tmp/demo-watch.log

tabs:
  - label: Device
    panes:
      - DUT
      - HOST

  - label: UART
    panes:
      - UART_DUT
      - UART_DEBUG

  - label: CoAP
    panes:
      - source: COAP_RAW
        plugins: [hex-coap]

  - label: Sensors
    panes:
      - SENSORS

  - label: Network
    panes:
      - NET_CAPTURE

  - label: File
    panes:
      - FILE_WATCH
"#;

#[derive(Parser)]
#[command(
    name = "embed-log",
    about = "Collect UART/UDP logs and view them in a browser UI",
    version
)]
struct Cli {
    /// YAML config file for default browser mode or --ui. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Path to the frontend directory for browser mode.
    #[arg(long, default_value = "frontend")]
    frontend_dir: PathBuf,

    /// Launch the terminal UI (ratatui) instead of the default browser UI.
    #[arg(long)]
    tui: bool,

    /// Launch the Tauri desktop UI instead of the default browser UI.
    #[arg(long)]
    ui: bool,

    /// Open the default browser after starting the web server.
    #[arg(long, conflicts_with = "no_open_browser")]
    open_browser: bool,

    /// Do not open the default browser after starting the web server.
    #[arg(long)]
    no_open_browser: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the log server from a config file
    Run {
        /// YAML config file. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Path to the frontend directory (default: ./frontend)
        #[arg(long, default_value = "frontend")]
        frontend_dir: PathBuf,

        /// Open the default browser after starting the web server.
        #[arg(long, conflicts_with = "no_open_browser")]
        open_browser: bool,

        /// Do not open the default browser after starting the web server.
        #[arg(long)]
        no_open_browser: bool,

        /// Override logs directory from config.
        #[arg(long)]
        log_dir: Option<PathBuf>,

        /// Launch the terminal UI (ratatui) instead of the browser UI.
        #[arg(long)]
        tui: bool,

        /// Override bind host from config.
        #[arg(long)]
        host: Option<String>,

        /// Override HTTP/WebSocket port from config.
        #[arg(long)]
        ws_port: Option<u16>,
    },

    /// Run first-run onboarding to build a config interactively in the browser.
    ///
    /// Starts a small setup page, lets you pick sources/tabs, saves the config
    /// to the resolved path, then launches the log server from it. Also runs
    /// automatically when `run` (or the default command) finds no config.
    Onboard {
        /// YAML config file to write. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Path to the frontend directory (default: ./frontend)
        #[arg(long, default_value = "frontend")]
        frontend_dir: PathBuf,

        /// Do not open the default browser for the setup page.
        #[arg(long)]
        no_open_browser: bool,
    },

    /// Show version and environment information
    Version {
        /// Config file to inspect
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },

    /// Show environment, config, and runtime diagnostics
    Doctor {
        /// Config file to inspect
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },

    /// List detected serial ports
    Ports {
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Inspect and export recorded sessions
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },

    /// Print a greeting (smoke-test target)
    Hello,

    /// Start the demo server with the embedded demo config.
    Demo {
        /// YAML config file (uses embedded demo config by default).
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Path to the frontend directory.
        #[arg(long, default_value = "frontend")]
        frontend_dir: PathBuf,

        /// Do not open the browser.
        #[arg(long)]
        no_open_browser: bool,

        /// Launch the terminal UI (ratatui) instead of the browser.
        #[arg(long)]
        tui: bool,
    },

    /// Generate a sample embed-log.yml config file.
    Init {
        /// Output path (default: embed-log.yml).
        #[arg(short, long, default_value = "embed-log.yml")]
        output: PathBuf,
    },

    /// Merge raw log files into a static HTML file.
    Merge {
        /// Repeatable: --tab "LABEL" PANE FILE [PANE FILE]
        #[arg(short, long = "tab", num_args = 1..)]
        tabs: Vec<String>,

        /// Output HTML path (default: merged.html).
        #[arg(short, long, default_value = "merged.html")]
        output: PathBuf,
        /// Timestamp mode for static replay.
        #[arg(long, default_value = "absolute")]
        timestamp_mode: String,

        /// Absolute timestamp origin used when replay logs contain relative timestamps.
        #[arg(long)]
        first_log_at: Option<String>,
    },

    /// Parse an exported session.html back into raw log files.
    Parse {
        /// Path to the session.html file.
        html: PathBuf,

        /// Output directory (default: parsed/).
        #[arg(short, long, default_value = "parsed")]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
enum SessionsCommand {
    /// List sessions under a log directory
    List {
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long = "with-markers")]
        with_markers: bool,
    },
    /// Show one session manifest
    Info {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Export a session as HTML or raw merged text
    Export {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ExportFormat::Html)]
        format: ExportFormat,
    },
    /// List markers in a session
    Marker {
        #[command(subcommand)]
        command: MarkerCommand,
    },
}

#[derive(Clone, Debug, Subcommand)]
enum MarkerCommand {
    /// List markers for a session
    List {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Show one marker by index (1-based)
    Show {
        session_id: String,
        marker_index: usize,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ExportFormat {
    Html,
    Raw,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    if cli.ui {
        return cmd_ui(cli.config.as_ref());
    }

    match cli.command {
        Some(Command::Run {
            config,
            frontend_dir,
            open_browser,
            no_open_browser,
            tui,
            ..
        }) => {
            let open_browser = browser_launch_enabled(open_browser, no_open_browser);
            cmd_run(config.as_ref(), &frontend_dir, open_browser, tui).await
        }
        Some(Command::Version { config, json }) => cmd_version(config.as_deref(), json),
        Some(Command::Doctor { config, json }) => cmd_doctor(config.as_deref(), json),
        Some(Command::Ports { json }) => cmd_ports(json),
        Some(Command::Hello) => cmd_hello(),
        Some(Command::Sessions { command }) => cmd_sessions(command),
        Some(Command::Demo {
            config,
            frontend_dir,
            no_open_browser,
            tui,
        }) => {
            let config_path = config.as_deref().map(PathBuf::from);
            cmd_demo(config_path.as_ref(), &frontend_dir, !no_open_browser, tui).await
        }

        Some(Command::Onboard {
            config,
            frontend_dir,
            no_open_browser,
        }) => cmd_onboard(config.as_ref(), &frontend_dir, !no_open_browser).await,

        Some(Command::Init { output }) => cmd_init(&output),
        Some(Command::Merge {
            tabs,
            output,
            timestamp_mode,
            first_log_at,
        }) => cmd_merge(&tabs, &output, &timestamp_mode, first_log_at),
        Some(Command::Parse { html, output }) => cmd_parse(&html, &output),
        None => {
            let open_browser = browser_launch_enabled(cli.open_browser, cli.no_open_browser);
            cmd_run(
                cli.config.as_ref(),
                &cli.frontend_dir,
                open_browser,
                cli.tui,
            )
            .await
        }
    }
}

async fn cmd_run(
    config_path: Option<&PathBuf>,
    frontend_dir: &PathBuf,
    open_browser: bool,
    tui: bool,
) -> Result<()> {
    let config_path = resolve_config_path(config_path);

    // First-run onboarding: if no config exists yet, run the browser setup
    // (same page the Tauri app uses), then proceed to start the real server.
    // The onboarding page's post-save redirect lands on the live server, so we
    // suppress the normal browser-open in that case to avoid a second tab.
    let onboarded = if !config_path.exists() {
        run_onboarding(&config_path, open_browser)?;
        true
    } else {
        false
    };

    let config = load_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Resolve frontend dir relative to cwd
    let frontend_dir = if frontend_dir.is_absolute() {
        frontend_dir.clone()
    } else {
        std::env::current_dir()
            .context("get cwd")?
            .join(frontend_dir)
    };

    // Resolve logs root relative to config file's directory
    let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
    let logs_root = if PathBuf::from(&config.logs.dir).is_absolute() {
        PathBuf::from(&config.logs.dir)
    } else {
        config_dir.join(&config.logs.dir)
    };

    println!("embed-log v{}", env!("CARGO_PKG_VERSION"));
    println!("  config:   {}", config_path.display());
    println!("  frontend: {}", frontend_dir.display());
    println!("  logs:     {}", logs_root.display());
    println!(
        "  sources:  {}",
        config
            .sources
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  tabs:     {}",
        config
            .tabs
            .iter()
            .map(|t| t.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  server:   http://{}:{}",
        config.server.host, config.server.ws_port
    );
    // In TUI mode, suppress the browser (the terminal is the UI).
    if open_browser && !onboarded && !tui {
        schedule_browser_open(config.server.host.clone(), config.server.ws_port);
    }

    let ws_port = config.server.ws_port;
    let app_name = config.server.app_name.clone();
    let server = LogServer::new(config, frontend_dir, logs_root).with_config_path(config_path);
    if tui {
        run_server_with_tui(server, ws_port, &app_name).await
    } else {
        server.run().await
    }
}

/// Spawn the log server as a background task, then run the TUI in the
/// foreground. When the TUI quits, abort the server task and return.
///
/// The server's `run()` is the blocking serve loop (no graceful shutdown
/// signal), so we abort it on TUI exit. On-disk session artifacts are already
/// written by the server during the run; abort only drops in-memory state.
async fn run_server_with_tui(server: LogServer, ws_port: u16, app_name: &str) -> Result<()> {
    // Spawn the server serve loop in the background.
    let server_task = tokio::spawn(async move { server.run().await });

    // Give the server a moment to bind the port before the TUI connects.
    // The WS client retries on connect failure, so this is a best-effort race
    // avoidance rather than a hard requirement.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Run the TUI on the current thread. It connects to ws://127.0.0.1:port/ws.
    #[cfg(feature = "tui")]
    let tui_result = embed_log_tui::run_in_process_async(ws_port, Some(app_name)).await;
    #[cfg(not(feature = "tui"))]
    let tui_result: Result<()> = {
        anyhow::bail!("embed-log was built without the `tui` feature; rebuild with --features tui")
    };

    // TUI exited → abort the server task.
    server_task.abort();
    tui_result
}

/// Run the interactive browser onboarding, blocking until the user saves a
/// config. Returns `Ok(())` once the config has been written to `config_path`.
///
/// Uses the exact same `OnboardingServer` + `frontend/onboarding.js` page as
/// the Tauri desktop app — no separate UI.
fn run_onboarding(config_path: &Path, open_browser: bool) -> Result<()> {
    use embed_log_core::onboarding as ob;

    println!("embed-log v{} — first-run setup", env!("CARGO_PKG_VERSION"));
    println!("  no config found at {}", config_path.display());

    let server = ob::OnboardingServer::start(config_path.to_path_buf(), ob::default_save_handler())
        .context("start onboarding server")?;
    println!("  setup page:  {}", server.base_url);
    println!("  waiting for you to finish setup…");

    if open_browser {
        // The setup server is already bound, so we can open immediately.
        if let Err(error) = open_url_in_default_browser(&server.base_url) {
            tracing::warn!("failed to open browser for onboarding: {error}");
        }
    }

    let result = server
        .wait_for_save()
        .map_err(|e| anyhow::anyhow!("onboarding did not complete: {e}"))?;
    println!("  config saved to {}", result.config_path);
    println!("  launching log server on port {}…", result.ws_port);
    println!();
    Ok(())
}

/// `embed-log onboard` — explicitly run onboarding, then start the log server
/// from the resulting config.
async fn cmd_onboard(
    config_path: Option<&PathBuf>,
    frontend_dir: &PathBuf,
    open_browser: bool,
) -> Result<()> {
    let config_path = resolve_config_path(config_path);
    run_onboarding(&config_path, open_browser)?;
    cmd_run(Some(&config_path), frontend_dir, false, false).await
}

fn browser_launch_enabled(_open_browser: bool, no_open_browser: bool) -> bool {
    !no_open_browser
}

fn schedule_browser_open(host: String, port: u16) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let url = format!("http://{host}:{port}/");
        if let Err(error) = open_url_in_default_browser(&url) {
            tracing::warn!("failed to open browser at {url}: {error}");
        }
    });
}

fn open_url_in_default_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = ProcessCommand::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = ProcessCommand::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = {
        let mut command = ProcessCommand::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .spawn()
        .with_context(|| format!("open default browser for {url}"))?;
    Ok(())
}

fn cmd_ui(config_path: Option<&PathBuf>) -> Result<()> {
    let plan = resolve_tauri_launch_plan(
        std::env::var_os("EMBED_LOG_TAURI_BIN").map(PathBuf::from),
        std::env::current_exe().ok(),
        config_path.map(PathBuf::from),
    )?;
    let mut command = tauri_command_from_plan(plan);
    let status = command.status().context("launch Tauri UI")?;
    if !status.success() {
        anyhow::bail!("Tauri UI exited with status {status}");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TauriLaunchPlan {
    Direct { program: PathBuf, args: Vec<String> },
    Cargo { args: Vec<String> },
}

fn resolve_tauri_launch_plan(
    env_bin: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    config_path: Option<PathBuf>,
) -> Result<TauriLaunchPlan> {
    let config_args = config_path
        .as_ref()
        .map(|path| vec!["--config".to_string(), path.display().to_string()])
        .unwrap_or_default();

    if let Some(program) = env_bin {
        return Ok(TauriLaunchPlan::Direct {
            program,
            args: config_args,
        });
    }

    if let Some(current_exe) = current_exe {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(if cfg!(windows) {
                "embed-log-tauri.exe"
            } else {
                "embed-log-tauri"
            });
            if candidate.exists() {
                return Ok(TauriLaunchPlan::Direct {
                    program: candidate,
                    args: config_args,
                });
            }
        }
    }

    if Path::new("Cargo.toml").exists() {
        let mut args = vec![
            "run".to_string(),
            "--quiet".to_string(),
            "--package".to_string(),
            "embed-log-tauri".to_string(),
            "--bin".to_string(),
            "embed-log-tauri".to_string(),
            "--".to_string(),
        ];
        args.extend(config_args);
        return Ok(TauriLaunchPlan::Cargo { args });
    }

    anyhow::bail!(
        "could not locate embed-log-tauri; set EMBED_LOG_TAURI_BIN or install the Tauri binary next to embed-log"
    )
}

fn tauri_command_from_plan(plan: TauriLaunchPlan) -> ProcessCommand {
    match plan {
        TauriLaunchPlan::Direct { program, args } => {
            let mut command = ProcessCommand::new(program);
            command.args(args);
            command
        }
        TauriLaunchPlan::Cargo { args } => {
            let mut command = ProcessCommand::new("cargo");
            command.args(args);
            command
        }
    }
}

fn resolve_config_path(config_path: Option<&PathBuf>) -> PathBuf {
    resolve_config_path_with_env(
        config_path,
        std::env::var_os("EMBED_LOG_CONFIG_YML_PATH").map(PathBuf::from),
    )
}

fn resolve_config_path_with_env(
    config_path: Option<&PathBuf>,
    env_path: Option<PathBuf>,
) -> PathBuf {
    config_path
        .cloned()
        .or(env_path)
        .unwrap_or_else(|| PathBuf::from("embed-log.yml"))
}

fn cmd_version(config_path: Option<&std::path::Path>, json: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    if json {
        let mut out = serde_json::json!({
            "version": version,
        });
        if let Some(path) = config_path {
            match load_config(path) {
                Ok(cfg) => {
                    out["config"] = serde_json::json!({
                        "path": path.display().to_string(),
                        "sources": cfg.sources.len(),
                        "tabs": cfg.tabs.len(),
                    });
                }
                Err(e) => {
                    out["config_error"] = serde_json::json!(e.to_string());
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("embed-log {version}");
        if let Some(path) = config_path {
            match load_config(path) {
                Ok(cfg) => {
                    println!("  config:   {}", path.display());
                    println!("  sources:  {}", cfg.sources.len());
                    println!("  tabs:     {}", cfg.tabs.len());
                }
                Err(e) => {
                    println!("  config error: {e}");
                }
            }
        }
    }
    Ok(())
}

fn cmd_doctor(config_path: Option<&std::path::Path>, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "status": "ok",
            }))?
        );
    } else {
        println!("embed-log doctor");
        println!("  version:  {}", env!("CARGO_PKG_VERSION"));
        println!("  status:   ok");
    }
    let _ = config_path;
    Ok(())
}

fn cmd_ports(json: bool) -> Result<()> {
    let ports = serialport::available_ports().unwrap_or_default();

    if json {
        let port_list: Vec<serde_json::Value> = ports
            .iter()
            .map(|p| {
                let port_type = match &p.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        serde_json::json!({
                            "type": "usb",
                            "vid": info.vid,
                            "pid": info.pid,
                            "product": info.product,
                            "manufacturer": info.manufacturer,
                        })
                    }
                    _ => serde_json::json!({"type": "other"}),
                };
                serde_json::json!({
                    "name": p.port_name,
                    "port_type": port_type,
                })
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ports": port_list,
            }))?
        );
    } else if ports.is_empty() {
        println!("No serial ports detected.");
    } else {
        println!("Detected serial ports:");
        for p in &ports {
            match &p.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    let product = info.product.as_deref().unwrap_or("unknown");
                    let mfr = info.manufacturer.as_deref().unwrap_or("unknown");
                    println!(
                        "  {}  USB {:04x}:{:04x}  {} ({})",
                        p.port_name, info.vid, info.pid, product, mfr
                    );
                }
                _ => {
                    println!("  {}", p.port_name);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct SessionRecord {
    id: String,
    dir: PathBuf,
    manifest: serde_json::Value,
}

fn cmd_sessions(command: SessionsCommand) -> Result<()> {
    match command {
        SessionsCommand::List {
            dir,
            json,
            limit,
            with_markers,
        } => {
            let mut sessions = load_sessions(&dir)?;
            if let Some(limit) = limit {
                sessions.truncate(limit);
            }
            // Apply --with-markers filter before any output (JSON or human)
            if with_markers {
                sessions.retain(|s| count_markers_in_session(&s.dir) > 0);
            }

            if json {
                let rows: Vec<_> = sessions
                    .iter()
                    .map(|session| {
                        let marker_count = count_markers_in_session(&session.dir);
                        let mut entry = serde_json::json!({
                            "id": session.id,
                            "dir": session.dir,
                            "manifest": session.manifest,
                        });
                        entry["marker_count"] = serde_json::json!(marker_count);
                        entry
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "sessions": rows }))?
                );
            } else {
                for session in sessions {
                    let marker_count = count_markers_in_session(&session.dir);
                    let started_at = session
                        .manifest
                        .get("started_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    println!(
                        "{}\t{}\t{}\t{} marker(s)",
                        session.id,
                        started_at,
                        session.dir.display(),
                        marker_count
                    );
                }
            }
            Ok(())
        }
        SessionsCommand::Marker { command } => cmd_session_marker(command),
        SessionsCommand::Info {
            session_id,
            dir,
            json,
        } => {
            let session = resolve_session(&dir, &session_id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&session.manifest)?);
            } else {
                println!("session: {}", session.id);
                println!("dir:     {}", session.dir.display());
                if let Some(started_at) =
                    session.manifest.get("started_at").and_then(|v| v.as_str())
                {
                    println!("started: {started_at}");
                }
                if let Some(status) = session.manifest.get("html_status").and_then(|v| v.as_str()) {
                    println!("html:    {status}");
                }
                if let Some(source_files) = session
                    .manifest
                    .get("source_files")
                    .and_then(|v| v.as_object())
                {
                    println!("sources: {}", source_files.len());
                    for (name, path) in source_files {
                        println!("  {name}: {}", path.as_str().unwrap_or(""));
                    }
                }
            }
            Ok(())
        }
        SessionsCommand::Export {
            session_id,
            dir,
            output,
            format,
        } => {
            let session = resolve_session(&dir, &session_id)?;
            match format {
                ExportFormat::Html => {
                    let output = output.unwrap_or_else(|| session.dir.join("session.html"));
                    export_session_html(&session, output)?;
                }
                ExportFormat::Raw => {
                    let output = output.unwrap_or_else(|| session.dir.join("session.raw.log"));
                    export_session_raw(&session, output)?;
                }
            }
            Ok(())
        }
    }
}

fn load_sessions(log_dir: &std::path::Path) -> Result<Vec<SessionRecord>> {
    let mut sessions = Vec::new();
    if !log_dir.exists() {
        return Ok(sessions);
    }

    for entry in
        std::fs::read_dir(log_dir).with_context(|| format!("read {}", log_dir.display()))?
    {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&manifest_path)
                .with_context(|| format!("read {}", manifest_path.display()))?,
        )
        .with_context(|| format!("parse {}", manifest_path.display()))?;
        let id = manifest
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or_else(|| {
                dir.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        sessions.push(SessionRecord { id, dir, manifest });
    }

    sessions.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(sessions)
}

fn resolve_session(log_dir: &std::path::Path, session_id: &str) -> Result<SessionRecord> {
    let matches: Vec<_> = load_sessions(log_dir)?
        .into_iter()
        .filter(|session| session.id == session_id || session.id.starts_with(session_id))
        .collect();

    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => anyhow::bail!("session not found: {session_id}"),
        _ => anyhow::bail!("ambiguous session id prefix: {session_id}"),
    }
}

/// Load markers from a session directory's markers.json.
/// Extract markers from parsed JSON, supporting both wrapper and bare-array formats.
fn extract_markers(parsed: &serde_json::Value) -> Vec<serde_json::Value> {
    // 1) Top-level array  [ {...}, ... ]
    if let Some(arr) = parsed.as_array() {
        return arr.clone();
    }
    // 2) Wrapper object  { "session_id": "...", "markers": [...] }
    if let Some(arr) = parsed.get("markers").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    Vec::new()
}

fn load_markers_file(session_dir: &Path) -> Result<Vec<serde_json::Value>> {
    let path = session_dir.join("markers.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(extract_markers(&parsed))
}

/// Count markers in a session without loading the full array.
fn count_markers_in_session(session_dir: &Path) -> usize {
    let path = session_dir.join("markers.json");
    if !path.exists() {
        return 0;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return 0,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    extract_markers(&parsed).len()
}

/// Handle `sessions marker list/show` subcommands.
fn cmd_session_marker(command: MarkerCommand) -> Result<()> {
    match command {
        MarkerCommand::List {
            session_id,
            dir,
            json,
            search,
            pane,
        } => {
            let session = resolve_session(&dir, &session_id)?;
            let all_markers = load_markers_file(&session.dir)?;

            if json && search.is_none() && pane.is_none() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "session_id": session.id,
                        "markers": all_markers,
                    }))?
                );
                return Ok(());
            }

            // Apply filters while preserving original 1-based indexes.
            // Missing fields do NOT match (no false positives).
            // --search is case-insensitive.
            let search_lower = search.as_ref().map(|s| s.to_lowercase());
            let filtered: Vec<(usize, &serde_json::Value)> = all_markers
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    if let Some(ref pat) = search_lower {
                        match m.get("description").and_then(|v| v.as_str()) {
                            Some(desc) => {
                                if !desc.to_lowercase().contains(pat) {
                                    return false;
                                }
                            }
                            None => return false, // missing field doesn't match
                        }
                    }
                    if let Some(ref pane_filter) = pane {
                        match m.get("paneId").and_then(|v| v.as_str()) {
                            Some(pid) => {
                                if pid != pane_filter.as_str() {
                                    return false;
                                }
                            }
                            None => return false, // missing field doesn't match
                        }
                    }
                    true
                })
                .collect();

            if json {
                let json_markers: Vec<serde_json::Value> = filtered
                    .iter()
                    .map(|(idx, m)| {
                        let mut entry = serde_json::json!({
                            "index": idx + 1,
                        });
                        if let Some(obj) = m.as_object() {
                            for (k, v) in obj {
                                entry[k] = v.clone();
                            }
                        }
                        entry
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "session_id": session.id,
                        "markers": json_markers,
                    }))?
                );
            } else {
                println!("Session: {}", session.id);
                println!("Markers: {}", filtered.len());
                println!();
                for (orig_idx, m) in &filtered {
                    let pane_id = m.get("paneId").and_then(|v| v.as_str()).unwrap_or("?");
                    let line = m.get("lineIdx").and_then(|v| v.as_u64()).unwrap_or(0);
                    let end_idx = m.get("endIdx").and_then(|v| v.as_u64());
                    let desc = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    let num_ts = m.get("numTs").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let lines_str = match end_idx {
                        Some(end) if end != line => format!("lines {}-{}", line, end),
                        _ => format!("line {}", line),
                    };
                    println!("  {}. [{}] {}", orig_idx + 1, pane_id, lines_str);
                    println!("     {}", desc);
                    println!("     numTs={}", num_ts);
                    println!();
                }
            }
            Ok(())
        }
        MarkerCommand::Show {
            session_id,
            marker_index,
            dir,
            json,
        } => {
            let session = resolve_session(&dir, &session_id)?;
            let all_markers = load_markers_file(&session.dir)?;

            if marker_index == 0 || marker_index > all_markers.len() {
                anyhow::bail!(
                    "marker index {marker_index} out of range (session has {} markers)",
                    all_markers.len()
                );
            }

            let m = &all_markers[marker_index - 1];

            if json {
                println!("{}", serde_json::to_string_pretty(m)?);
            } else {
                let pane_id = m.get("paneId").and_then(|v| v.as_str()).unwrap_or("?");
                let line = m.get("lineIdx").and_then(|v| v.as_u64()).unwrap_or(0);
                let end_idx = m.get("endIdx").and_then(|v| v.as_u64());
                let desc = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let num_ts = m.get("numTs").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let created = m.get("createdAt").and_then(|v| v.as_str()).unwrap_or("");
                let lines_str = match end_idx {
                    Some(end) if end != line => format!("{}-{}", line, end),
                    _ => format!("{}", line),
                };
                println!("Marker {}", marker_index);
                println!("  Pane:        {}", pane_id);
                println!("  Lines:       {}", lines_str);
                println!("  Description: {}", desc);
                println!("  Timestamp:   {}", num_ts);
                println!("  Created:     {}", created);
            }
            Ok(())
        }
    }
}

fn manifest_source_files(
    session: &SessionRecord,
) -> Result<std::collections::HashMap<String, String>> {
    let source_files = session
        .manifest
        .get("source_files")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("manifest missing source_files"))?;

    Ok(source_files
        .iter()
        .filter_map(|(name, path)| path.as_str().map(|path| (name.clone(), path.to_string())))
        .collect())
}

fn export_session_html(session: &SessionRecord, output: PathBuf) -> Result<()> {
    let source_files = manifest_source_files(session)?;
    let tabs = session
        .manifest
        .get("tabs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let pane_labels = session
        .manifest
        .get("pane_labels")
        .and_then(|v| v.as_object())
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let timestamp_mode = session
        .manifest
        .get("timestamp_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("absolute")
        .to_string();
    let first_log_at = session
        .manifest
        .get("first_log_at")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let frontend_dir = std::env::current_dir()?.join("frontend");

    let exporter = SessionExporter::new(
        output.clone(),
        source_files,
        tabs,
        pane_labels,
        frontend_dir,
        timestamp_mode,
        first_log_at,
    );
    exporter.export()?;
    println!("{}", output.display());
    Ok(())
}

fn export_session_raw(session: &SessionRecord, output: PathBuf) -> Result<()> {
    let source_files = manifest_source_files(session)?;
    let mut merged = String::new();
    for (source, path) in source_files {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        for line in content.lines() {
            merged.push_str(&source);
            merged.push('\t');
            merged.push_str(line);
            merged.push('\n');
        }
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, merged)?;
    println!("{}", output.display());
    Ok(())
}
fn cmd_hello() -> Result<()> {
    println!("Hello from embed-log!");
    Ok(())
}

async fn cmd_demo(
    config_path: Option<&PathBuf>,
    frontend_dir: &PathBuf,
    open_browser: bool,
    tui: bool,
) -> Result<()> {
    // Use embedded demo config if no explicit config provided.
    let config_path = resolve_config_path(config_path);
    if !config_path.exists() {
        // Write a temporary demo config from embedded template.
        let demo_yml = DEMO_CONFIG;

        std::fs::write(&config_path, demo_yml)
            .with_context(|| format!("write demo config {}", config_path.display()))?;
        println!("wrote demo config: {}", config_path.display());
    }
    let config = load_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let frontend_dir = if frontend_dir.is_absolute() {
        frontend_dir.clone()
    } else {
        std::env::current_dir()
            .context("get cwd")?
            .join(frontend_dir)
    };
    let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
    let logs_root = if PathBuf::from(&config.logs.dir).is_absolute() {
        PathBuf::from(&config.logs.dir)
    } else {
        config_dir.join(&config.logs.dir)
    };

    println!("embed-log v{} (demo)", env!("CARGO_PKG_VERSION"));
    println!("  config:   {}", config_path.display());
    println!(
        "  server:   http://{}:{}",
        config.server.host, config.server.ws_port
    );
    println!(
        "  tabs:     {}",
        config
            .tabs
            .iter()
            .map(|t| t.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if open_browser && !tui {
        schedule_browser_open(config.server.host.clone(), config.server.ws_port);
    }
    prepare_demo_file_sources(&config)?;
    spawn_demo_traffic(&config);
    let ws_port = config.server.ws_port;
    let app_name = config.server.app_name.clone();
    let server = LogServer::new(config, frontend_dir, logs_root).with_config_path(config_path);
    if tui {
        run_server_with_tui(server, ws_port, &app_name).await
    } else {
        server.run().await
    }
}

fn cmd_init(output: &PathBuf) -> Result<()> {
    let sample = DEMO_CONFIG;
    std::fs::write(output, sample).with_context(|| format!("write {}", output.display()))?;
    println!("wrote {}", output.display());
    println!("edit it and run: embed-log --config {}", output.display());
    Ok(())
}

fn cmd_merge(
    tabs: &[String],
    output: &Path,
    timestamp_mode: &str,
    first_log_at: Option<String>,
) -> Result<()> {
    // Parse tab specs: flat list of "LABEL PANE FILE [PANE FILE]..." grouped by --tab flags.
    // Current callers use one --tab group at a time; this parser keeps the existing flat contract.
    let mut groups: Vec<Vec<String>> = Vec::new();
    for arg in tabs {
        if groups.is_empty() {
            groups.push(vec![arg.clone()]);
        } else if arg.contains(".log") || arg.contains(".txt") {
            groups.last_mut().unwrap().push(arg.clone());
        } else if groups
            .last()
            .map(|g| g.len() > 1 && g.len() % 2 == 1)
            .unwrap_or(false)
        {
            groups.push(vec![arg.clone()]);
        } else {
            groups.last_mut().unwrap().push(arg.clone());
        }
    }

    let mut tab_configs: Vec<serde_json::Value> = Vec::new();
    let mut source_files: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut pane_labels: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for group in &groups {
        if group.len() < 3 {
            anyhow::bail!("each --tab needs LABEL PANE FILE [PANE FILE]");
        }
        let label = &group[0];
        let mut panes: Vec<String> = Vec::new();
        let mut i = 1;
        while i < group.len() {
            if i + 1 >= group.len() {
                anyhow::bail!("each pane needs FILE after PANE name in --tab {}", label);
            }
            let pane_spec = &group[i];
            let file = group[i + 1].clone();
            let (pane_id, pane_label) = pane_spec
                .split_once('=')
                .map(|(id, label)| (id.to_string(), label.to_string()))
                .unwrap_or_else(|| (pane_spec.clone(), pane_spec.clone()));
            source_files.insert(pane_id.clone(), file);
            pane_labels.insert(pane_id.clone(), pane_label);
            panes.push(pane_id);
            i += 2;
        }
        tab_configs.push(serde_json::json!({
            "label": label,
            "panes": panes,
        }));
    }

    let frontend_dir = std::env::current_dir()?.join("frontend");
    let exporter = SessionExporter::new(
        output.to_path_buf(),
        source_files,
        tab_configs,
        pane_labels,
        frontend_dir,
        timestamp_mode.to_string(),
        first_log_at,
    );
    exporter.export()?;
    println!("{}", output.display());
    Ok(())
}

fn cmd_parse(html_path: &PathBuf, output_dir: &PathBuf) -> Result<()> {
    let html = std::fs::read_to_string(html_path)
        .with_context(|| format!("read {}", html_path.display()))?;

    // Extract logData JSON array from the HTML.
    let marker = "const logData = ";
    let start = html
        .find(marker)
        .ok_or_else(|| anyhow::anyhow!("not an embed-log session HTML: missing logData"))?;
    let data_start = start + marker.len();
    let end = html[data_start..]
        .find(";\n")
        .ok_or_else(|| anyhow::anyhow!("malformed logData in HTML"))?;
    let json_str = &html[data_start..data_start + end];
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(json_str).with_context(|| "parse logData JSON")?;

    std::fs::create_dir_all(output_dir)?;

    // Group entries by source_id.
    let mut by_source: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for entry in &entries {
        let source_id = entry
            .get("source_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let data = entry.get("data").and_then(|v| v.as_str()).unwrap_or("");
        by_source
            .entry(source_id.to_string())
            .or_default()
            .push(data.to_string());
    }

    for (source, lines) in &by_source {
        let path = output_dir.join(format!("{}.log", source));
        std::fs::write(&path, lines.join("\n") + "\n")?;
        println!("  {}  {} lines", path.display(), lines.len());
    }
    println!(
        "parsed {} sources → {}",
        by_source.len(),
        output_dir.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_config_resolution_uses_flag_then_env_then_default() {
        let flag = PathBuf::from("flag.yml");
        let env = Some(PathBuf::from("env.yml"));

        assert_eq!(
            resolve_config_path_with_env(Some(&flag), env.clone()),
            PathBuf::from("flag.yml")
        );
        assert_eq!(
            resolve_config_path_with_env(None, env),
            PathBuf::from("env.yml")
        );
        assert_eq!(
            resolve_config_path_with_env(None, None),
            PathBuf::from("embed-log.yml")
        );
    }

    #[test]
    fn run_config_flag_is_optional_for_env_default_compatibility() {
        let cli = Cli::parse_from(["embed-log", "run"]);
        match cli.command {
            Some(Command::Run { config, .. }) => assert!(config.is_none()),
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn no_subcommand_is_default_browser_run_mode() {
        let cli = Cli::parse_from([
            "embed-log",
            "--config",
            "embed-log.yml",
            "--frontend-dir",
            "frontend",
            "--no-open-browser",
        ]);

        assert!(cli.command.is_none());
        assert!(!cli.ui);
        assert_eq!(cli.config, Some(PathBuf::from("embed-log.yml")));
        assert!(cli.no_open_browser);
    }

    #[test]
    fn ui_flag_uses_tauri_launch_plan_with_config() {
        let cli = Cli::parse_from(["embed-log", "--ui", "--config", "desktop.yml"]);
        assert!(cli.ui);
        assert_eq!(cli.config, Some(PathBuf::from("desktop.yml")));

        let plan = resolve_tauri_launch_plan(
            Some(PathBuf::from("/tmp/embed-log-tauri")),
            None,
            cli.config,
        )
        .unwrap();

        assert_eq!(
            plan,
            TauriLaunchPlan::Direct {
                program: PathBuf::from("/tmp/embed-log-tauri"),
                args: vec!["--config".to_string(), "desktop.yml".to_string()],
            }
        );
    }

    #[test]
    fn run_alias_accepts_browser_launch_flags() {
        let cli = Cli::parse_from([
            "embed-log",
            "run",
            "--config",
            "run.yml",
            "--no-open-browser",
        ]);
        match cli.command {
            Some(Command::Run {
                config,
                no_open_browser,
                ..
            }) => {
                assert_eq!(config, Some(PathBuf::from("run.yml")));
                assert!(no_open_browser);
            }
            _ => panic!("expected run command"),
        }
    }

    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn temp_log_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "embed-log-cli-sessions-{}-{nanos}-{counter}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_markers(root: &Path, session_id: &str, markers: &[serde_json::Value]) {
        let dir = root.join(session_id);
        std::fs::create_dir_all(&dir).unwrap();
        let body = serde_json::json!({
            "session_id": session_id,
            "markers": markers,
        });
        std::fs::write(
            dir.join("markers.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
    }

    // ------------------  Marker CLI tests  ------------------

    #[test]
    fn marker_list_prints_all_markers() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 10, "description": "boot started"}),
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 42, "description": "fatal error"}),
            ],
        );
        assert_eq!(load_markers_file(&root.join("s1")).unwrap().len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_no_file_returns_empty() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        assert!(load_markers_file(&root.join("s1")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_empty_array_returns_empty() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(&root, "s1", &[]);
        assert!(load_markers_file(&root.join("s1")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_unknown_session_is_error() {
        let root = temp_log_dir();
        let err = resolve_session(&root, "nonexistent").unwrap_err();
        assert!(err.to_string().contains("not found"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_malformed_json_is_error() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        std::fs::write(root.join("s1").join("markers.json"), "not valid json {{").unwrap();
        assert!(load_markers_file(&root.join("s1")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_search_case_insensitive_and_missing_excluded() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1, "description": "Boot Started"}),
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 2, "description": "fatal error: PANIC"}),
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 3}), // no description
            ],
        );
        let all = load_markers_file(&root.join("s1")).unwrap();
        // Case-insensitive "fatal" matches marker at idx 2
        let pat = "fatal".to_lowercase();
        let f: Vec<_> = all
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.get("description")
                    .and_then(|v| v.as_str())
                    .map(|d| d.to_lowercase().contains(&pat))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].1["lineIdx"], 2);
        // "boot" matches only marker 0 (idx 3 has no description -> excluded)
        let pat = "boot".to_lowercase();
        let f: Vec<_> = all
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.get("description")
                    .and_then(|v| v.as_str())
                    .map(|d| d.to_lowercase().contains(&pat))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].1["lineIdx"], 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_pane_missing_field_excluded_and_index_preserved() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1, "description": "a"}),
                serde_json::json!({"lineIdx": 2, "description": "b"}), // no paneId
            ],
        );
        let all = load_markers_file(&root.join("s1")).unwrap();
        let f: Vec<_> = all
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.get("paneId")
                    .and_then(|v| v.as_str())
                    .map(|p| p == "DUT_UART")
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].0, 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_show_with_range_display() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 10, "description": "single"}),
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 42, "endIdx": 45, "description": "range"}),
            ],
        );
        let all = load_markers_file(&root.join("s1")).unwrap();
        let m = &all[1];
        let line = m["lineIdx"].as_u64().unwrap();
        let end = m["endIdx"].as_u64();
        let range_str = match end {
            Some(e) if e != line => format!("lines {}-{}", line, e),
            _ => format!("line {}", line),
        };
        assert_eq!(range_str, "lines 42-45");
        assert_eq!(m["description"], "range");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_show_out_of_range_is_error() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1})],
        );
        let all = load_markers_file(&root.join("s1")).unwrap();
        assert_eq!(all.len(), 1);
        assert!(std::panic::catch_unwind(|| {
            let _ = &all[2];
        })
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sessions_list_marker_count() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1})],
        );
        write_test_session(&root, "s2");
        assert_eq!(count_markers_in_session(&root.join("s1")), 1);
        assert_eq!(count_markers_in_session(&root.join("s2")), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_json_output() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[serde_json::json!({"paneId": "DUT_UART", "lineIdx": 5, "description": "json test"})],
        );
        let all = load_markers_file(&root.join("s1")).unwrap();
        let json_out =
            serde_json::to_string_pretty(&serde_json::json!({"session_id": "s1", "markers": all}))
                .unwrap();
        assert!(json_out.contains("json test"));
        assert!(json_out.contains("DUT_UART"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_load_bare_array_format() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        std::fs::write(
            root.join("s1").join("markers.json"),
            serde_json::to_string_pretty(&serde_json::json!([
                {"paneId": "DUT_UART", "lineIdx": 1, "description": "bare"}
            ]))
            .unwrap(),
        )
        .unwrap();
        let markers = load_markers_file(&root.join("s1")).unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0]["description"], "bare");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sessions_list_json_with_markers_filter() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1})],
        );
        write_test_session(&root, "s2");
        let sessions = load_sessions(&root).unwrap();
        let with_markers: Vec<_> = sessions
            .iter()
            .filter(|s| count_markers_in_session(&s.dir) > 0)
            .collect();
        assert_eq!(with_markers.len(), 1);
        assert_eq!(with_markers[0].id, "s1");
        std::fs::remove_dir_all(root).unwrap();
    }

    // ---------------------------------------------------------

    fn write_test_session(root: &std::path::Path, id: &str) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("main__dut__session.log");
        std::fs::write(&log_path, "[2026-06-13 00:00:00.000] boot\n").unwrap();
        let manifest = serde_json::json!({
            "session_id": id,
            "session_dir": dir.display().to_string(),
            "started_at": "2026-06-13T00:00:00+00:00",
            "timestamp_mode": "absolute",
            "tabs": [{ "label": "Main", "panes": ["dut"] }],
            "pane_labels": { "dut": "DUT" },
            "source_files": { "dut": log_path.display().to_string() },
            "html_status": "pending",
            "snippets": [],
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn sessions_command_surface_parses_core_subcommands() {
        for args in [
            ["embed-log", "sessions", "list"].as_slice(),
            ["embed-log", "sessions", "info", "abc"].as_slice(),
            ["embed-log", "sessions", "export", "abc", "--format", "raw"].as_slice(),
        ] {
            Cli::parse_from(args);
        }
    }

    #[test]
    fn sessions_resolve_prefix_and_raw_export() {
        let root = temp_log_dir();
        let session_dir = write_test_session(&root, "2026-06-13_00-00-00");
        let session = resolve_session(&root, "2026-06-13").unwrap();
        assert_eq!(session.id, "2026-06-13_00-00-00");

        let output = session_dir.join("merged.raw.log");
        export_session_raw(&session, output.clone()).unwrap();
        let merged = std::fs::read_to_string(output).unwrap();
        assert!(merged.contains("dut\t[2026-06-13 00:00:00.000] boot"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_commands_parse() {
        Cli::parse_from(["embed-log", "init"]);
        Cli::parse_from(["embed-log", "init", "-o", "my.yml"]);
        Cli::parse_from(["embed-log", "demo"]);
        Cli::parse_from(["embed-log", "demo", "--no-open-browser"]);
        Cli::parse_from(["embed-log", "merge", "--tab", "DevA", "SENSOR_A", "a.log"]);
        Cli::parse_from([
            "embed-log",
            "merge",
            "-t",
            "DevA",
            "SENSOR_A",
            "a.log",
            "-o",
            "out.html",
        ]);
        Cli::parse_from(["embed-log", "parse", "session.html"]);
        Cli::parse_from(["embed-log", "parse", "session.html", "-o", "my-parsed"]);
    }

    #[test]
    fn run_with_override_flags_parses() {
        let cli = Cli::parse_from([
            "embed-log",
            "run",
            "--log-dir",
            "/tmp/logs",
            "--host",
            "0.0.0.0",
            "--ws-port",
            "9090",
        ]);
        match cli.command {
            Some(Command::Run {
                log_dir,
                host,
                ws_port,
                ..
            }) => {
                assert_eq!(log_dir, Some(PathBuf::from("/tmp/logs")));
                assert_eq!(host, Some("0.0.0.0".to_string()));
                assert_eq!(ws_port, Some(9090));
            }
            _ => panic!("expected run"),
        }
    }
}
