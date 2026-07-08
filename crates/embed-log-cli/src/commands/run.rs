//! `embed-log run`, `embed-log demo`, and `embed-log onboard` — the commands
//! that start the log server.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use embed_log_core::config::{load_config, resolve_logs_root, AppConfig};
use embed_log_core::demo::{prepare_demo_file_sources, spawn_demo_traffic};
use embed_log_core::onboarding as ob;
use embed_log_core::runtime::LogServer;

use crate::config::resolve_config_path;
use crate::demo_config::DEMO_CONFIG;
use crate::util::{open_url_in_default_browser, schedule_browser_open};

/// `embed-log run` (and the default no-subcommand path): resolve config (running
/// onboarding first if none exists), start the server, optionally open a
/// browser or launch the in-process TUI.
pub(crate) async fn cmd_run(
    config_path: Option<&PathBuf>,
    frontend_dir: &Path,
    open_browser: bool,
    tui: bool,
    overrides: &RunOverrides,
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

    let mut config = load_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
    apply_server_overrides(&mut config, overrides);
    let frontend_dir = resolve_dir(frontend_dir)?;
    let logs_root = match overrides.log_dir.as_ref() {
        Some(dir) => resolve_dir(dir)?,
        None => resolve_logs_root(&config_path, &config.logs.dir),
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

/// `embed-log demo` — write the embedded demo config if none exists, start
/// synthetic traffic, then run the server.
pub(crate) async fn cmd_demo(
    config_path: Option<&PathBuf>,
    frontend_dir: &Path,
    open_browser: bool,
    tui: bool,
) -> Result<()> {
    let config_path = resolve_config_path(config_path);
    if !config_path.exists() {
        std::fs::write(&config_path, DEMO_CONFIG)
            .with_context(|| format!("write demo config {}", config_path.display()))?;
        println!("wrote demo config: {}", config_path.display());
    }
    let config = load_config(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let frontend_dir = resolve_dir(frontend_dir)?;
    let logs_root = resolve_logs_root(&config_path, &config.logs.dir);

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

/// Spawn the log server as a background task, then run the TUI in the
/// foreground. When the TUI quits, abort the server task and return.
///
/// The server's `run()` is the blocking serve loop (no graceful shutdown
/// signal), so we abort it on TUI exit. On-disk session artifacts are already
/// written by the server during the run; abort only drops in-memory state.
pub(crate) async fn run_server_with_tui(
    server: LogServer,
    ws_port: u16,
    app_name: &str,
) -> Result<()> {
    // Spawn the server serve loop in the background.
    let server_task = tokio::spawn(async move { server.run().await });

    // Wait for the server to bind the port before the TUI connects. The WS
    // client also retries on connect failure, so a timeout here is non-fatal.
    crate::util::wait_for_port(ws_port, std::time::Duration::from_secs(10)).await;

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
pub(crate) fn run_onboarding(config_path: &Path, open_browser: bool) -> Result<()> {
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
pub(crate) async fn cmd_onboard(
    config_path: Option<&PathBuf>,
    frontend_dir: &Path,
    open_browser: bool,
) -> Result<()> {
    let config_path = resolve_config_path(config_path);
    run_onboarding(&config_path, open_browser)?;
    cmd_run(
        Some(&config_path),
        frontend_dir,
        false,
        false,
        &RunOverrides::default(),
    )
    .await
}

/// Resolve a directory path relative to the cwd (absolute paths pass through).
fn resolve_dir(dir: &Path) -> Result<PathBuf> {
    if dir.is_absolute() {
        Ok(dir.to_path_buf())
    } else {
        Ok(std::env::current_dir().context("get cwd")?.join(dir))
    }
}

/// CLI overrides for `embed-log run` applied on top of the loaded config.
/// Each field is `None` when the corresponding flag was not passed.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunOverrides {
    pub log_dir: Option<PathBuf>,
    pub host: Option<String>,
    pub ws_port: Option<u16>,
}

/// Apply host and ws_port overrides to the loaded config in place.
/// `log_dir` is handled separately in `cmd_run` because it bypasses the
/// config-relative resolution (CLI paths are relative to cwd, not the config
/// file's directory).
fn apply_server_overrides(config: &mut AppConfig, overrides: &RunOverrides) {
    if let Some(ref host) = overrides.host {
        config.server.host = host.clone();
    }
    if let Some(ws_port) = overrides.ws_port {
        config.server.ws_port = ws_port;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_dir_absolute_passes_through() {
        let abs = if cfg!(windows) {
            PathBuf::from(r"C:\logs\frontend")
        } else {
            PathBuf::from("/srv/frontend")
        };
        assert_eq!(resolve_dir(&abs).unwrap(), abs);
    }

    #[test]
    fn apply_overrides_host_and_port() {
        let mut config = AppConfig::default();
        let original_host = config.server.host.clone();
        let original_port = config.server.ws_port;

        let overrides = RunOverrides {
            host: Some("0.0.0.0".to_string()),
            ws_port: Some(9090),
            ..Default::default()
        };
        apply_server_overrides(&mut config, &overrides);

        assert_ne!(config.server.host, original_host);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_ne!(config.server.ws_port, original_port);
        assert_eq!(config.server.ws_port, 9090);
    }

    #[test]
    fn apply_overrides_none_leaves_config_untouched() {
        let mut config = AppConfig::default();
        let original_host = config.server.host.clone();
        let original_port = config.server.ws_port;

        apply_server_overrides(&mut config, &RunOverrides::default());

        assert_eq!(config.server.host, original_host);
        assert_eq!(config.server.ws_port, original_port);
    }

    #[test]
    fn apply_overrides_partial_only_changes_provided_fields() {
        let mut config = AppConfig::default();
        let original_host = config.server.host.clone();

        let overrides = RunOverrides {
            ws_port: Some(3000),
            ..Default::default()
        };
        apply_server_overrides(&mut config, &overrides);

        // Host untouched, port changed.
        assert_eq!(config.server.host, original_host);
        assert_eq!(config.server.ws_port, 3000);
    }
}
