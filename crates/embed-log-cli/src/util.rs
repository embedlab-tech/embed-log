//! Small process/browser helpers shared across command modules.

use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};

/// Decide whether the browser should auto-open. The `--open-browser` flag is
/// accepted for symmetry / future use but currently a no-op; only
/// `--no-open-browser` suppresses the launch.
pub(crate) fn browser_launch_enabled(_open_browser: bool, no_open_browser: bool) -> bool {
    !no_open_browser
}

/// Open `http://{host}:{port}/` in the default browser after a short delay, so
/// the server has had time to bind. Fire-and-forget on a background task.
pub(crate) fn schedule_browser_open(host: String, port: u16) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let url = format!("http://{host}:{port}/");
        if let Err(error) = open_url_in_default_browser(&url) {
            tracing::warn!("failed to open browser at {url}: {error}");
        }
    });
}

/// Open `url` in the platform default browser. Spawns detached; errors only on
/// spawn failure.
pub(crate) fn open_url_in_default_browser(url: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_launch_enabled_only_no_flag_matters() {
        // --no-open-browser suppresses; --open-browser is a no-op override.
        assert!(browser_launch_enabled(false, false));
        assert!(!browser_launch_enabled(false, true));
        assert!(!browser_launch_enabled(true, true));
        assert!(browser_launch_enabled(true, false));
    }
}
