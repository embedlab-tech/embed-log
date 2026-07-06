//! Small process/browser helpers shared across command modules.

use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};

/// Poll `127.0.0.1:port` until it accepts a connection or `timeout` elapses.
/// Returns `true` once the server is listening, `false` on timeout. Replaces
/// fixed startup sleeps so callers proceed as soon as the port is actually up.
pub(crate) async fn wait_for_port(port: u16, timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// Open `http://{host}:{port}/` in the default browser once the server is
/// listening. Fire-and-forget on a background task.
pub(crate) fn schedule_browser_open(host: String, port: u16) {
    tokio::spawn(async move {
        wait_for_port(port, std::time::Duration::from_secs(10)).await;
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

    // Put the launcher (and, on Linux, the browser it execs into when none is
    // already running) in its own process group. Otherwise it inherits the
    // CLI's foreground process group, and Ctrl+C's SIGINT — delivered by the
    // terminal to the whole foreground group, not just the CLI's PID — kills
    // the browser along with the server.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    command
        .spawn()
        .with_context(|| format!("open default browser for {url}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_for_port_returns_true_once_listening() {
        // Bind a listener, then confirm wait_for_port sees it quickly.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(wait_for_port(port, std::time::Duration::from_secs(2)).await);
    }

    #[tokio::test]
    async fn wait_for_port_times_out_when_nothing_listens() {
        // Bind to grab a free port, then drop it so the port is closed.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!wait_for_port(port, std::time::Duration::from_millis(150)).await);
    }
}
