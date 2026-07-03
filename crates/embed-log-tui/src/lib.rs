//! Terminal UI frontend for embed-log.
//!
//! The TUI is a WebSocket client to the unchanged `embed-log-core` server
//! (`/ws` + `/api/*`), exactly like the browser frontend and the Tauri
//! webview. It supports live viewing, tabs/panes, scrolling, selection/copy,
//! markers, events, clear, relative/absolute timestamps, and UART TX with
//! ratatui + crossterm. It does not execute browser JavaScript plugins or
//! provide onboarding; use `embed-log run --tui` after creating a config.
//!
//! Two entry points:
//! - [`run_in_process`] — used by `embed-log run --tui` / `demo --tui` when
//!   the server is already running in the same process on loopback.
//! - [`run_client`] — connect the standalone binary to any running server.
//!
//! See `tui-frontend-plan.md` for the full architecture and phase plan.

pub mod app;
pub mod client;
pub mod draw;
pub mod events;
pub mod input;
pub mod keys;
pub mod lines;
pub mod protocol;
pub mod selection;
pub mod state;

use anyhow::Result;

/// Run the TUI against a server already running in-process on loopback.
///
/// `ws_port` is the server's HTTP/WebSocket port. `app_name` is shown in the
/// status bar (falls back to "embed-log" if empty). Synchronous: builds its
/// own tokio runtime. Used by the standalone binary and any non-async caller.
pub fn run_in_process(ws_port: u16, app_name: Option<&str>) -> Result<()> {
    let app_name = app_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("embed-log");
    let url = format!("ws://127.0.0.1:{ws_port}/ws");
    run_client_with_url(&url, app_name)
}

/// Run the TUI against a server already running in-process on loopback,
/// **async** variant for callers already inside a tokio runtime (e.g. the
/// `embed-log` CLI's `#[tokio::main]`).
///
/// This is the entry point `embed-log run --tui` / `demo --tui` use: the CLI
/// spawns `LogServer::run()` as a background task, then `.await`s this, then
/// tears down the server when the TUI quits.
pub async fn run_in_process_async(ws_port: u16, app_name: Option<&str>) -> Result<()> {
    let app_name = app_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("embed-log");
    let url = format!("ws://127.0.0.1:{ws_port}/ws");
    run_client_async(&url, app_name).await
}

/// Run the TUI as a standalone client connected to an arbitrary server URL.
///
/// Accepts a full `ws://host:port/ws` URL. Used by the `embed-log-tui` binary
/// and by [`run_in_process`] (which just builds the loopback URL).
pub fn run_client(ws_url: &str) -> Result<()> {
    run_client_with_url(ws_url, "embed-log")
}

/// Shared client startup (sync): build a current-thread runtime, then run the
/// app loop. For the standalone binary and other non-async callers.
fn run_client_with_url(ws_url: &str, app_name: &str) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_client_async(ws_url, app_name))
}

/// Shared client startup (async): spawn the WS client, run the ratatui app
/// loop until the user quits, then stop the client. For callers already
/// inside a tokio runtime (the CLI's `--tui` path, via
/// [`run_in_process_async`]).
async fn run_client_async(ws_url: &str, app_name: &str) -> Result<()> {
    let handle = client::spawn_client(ws_url.to_string());
    // Run the (ratatui) app loop on the current thread; the WS client and
    // input tasks run as tokio tasks on the same runtime.
    app::run(handle, app_name).await
}
