use clap::{Parser, Subcommand};

/// Terminal UI for embed-log.
///
/// Connects to a running embed-log server's `/ws` endpoint and renders the
/// live viewer in the terminal. Use `embed-log run --tui` / `embed-log demo
/// --tui` to start both server and TUI in one process; this binary is for
/// connecting to an already-running server.
#[derive(Parser)]
#[command(
    name = "embed-log-tui",
    version,
    about = "Terminal UI for embed-log (ratatui + crossterm)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// WebSocket URL of a running embed-log server (ws://host:port/ws).
    ///
    /// Shorthand for `embed-log-tui connect <url>`.
    #[arg(long, value_name = "URL")]
    url: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Connect to a running embed-log server by WebSocket URL.
    Connect {
        /// WebSocket URL, e.g. ws://127.0.0.1:8080/ws
        url: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Connect { url }) => embed_log_tui::run_client(&url),
        None => {
            if let Some(url) = cli.url {
                embed_log_tui::run_client(&url)
            } else {
                anyhow::bail!(
                    "no connection target. Use `embed-log-tui connect <ws-url>` or `--url <ws-url>`, \
                     or launch via `embed-log run --tui` / `embed-log demo --tui`."
                )
            }
        }
    }
}
