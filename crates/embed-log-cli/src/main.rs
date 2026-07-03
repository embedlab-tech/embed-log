//! `embed-log` CLI entry point.
//!
//! This file holds only the `clap` definitions (`Cli`, `Command`) and the
//! `main()` dispatch. Each subcommand's implementation lives in
//! [`commands`].

mod commands;
mod config;
mod demo_config;
mod util;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::misc;
use commands::run::{cmd_demo, cmd_onboard, cmd_run, RunOverrides};
use commands::sessions::{cmd_sessions, SessionsCommand};
use commands::ui::cmd_ui;

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
    #[command(hide = true)]
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

    /// Validate a config file and print the resolved runtime summary.
    Validate {
        /// YAML config file. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
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
            no_open_browser,
            tui,
            log_dir,
            host,
            ws_port,
        }) => {
            let open_browser = !no_open_browser;
            let overrides = RunOverrides {
                log_dir,
                host,
                ws_port,
            };
            cmd_run(
                config.as_ref(),
                &frontend_dir,
                open_browser,
                tui,
                &overrides,
            )
            .await
        }
        Some(Command::Version { config, json }) => misc::cmd_version(config.as_deref(), json),
        Some(Command::Doctor { config, json }) => misc::cmd_doctor(config.as_deref(), json),
        Some(Command::Ports { json }) => misc::cmd_ports(json),
        Some(Command::Hello) => misc::cmd_hello(),
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

        Some(Command::Validate { config, json }) => {
            let path = crate::config::resolve_config_path(config.as_ref());
            misc::cmd_validate(&path, json)
        }
        Some(Command::Init { output }) => misc::cmd_init(&output),
        Some(Command::Merge {
            tabs,
            output,
            timestamp_mode,
            first_log_at,
        }) => misc::cmd_merge(&tabs, &output, &timestamp_mode, first_log_at),
        Some(Command::Parse { html, output }) => misc::cmd_parse(&html, &output),
        None => {
            let open_browser = !cli.no_open_browser;
            cmd_run(
                cli.config.as_ref(),
                &cli.frontend_dir,
                open_browser,
                cli.tui,
                &RunOverrides::default(),
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ui_flag_carries_config() {
        let cli = Cli::parse_from(["embed-log", "--ui", "--config", "desktop.yml"]);
        assert!(cli.ui);
        assert_eq!(cli.config, Some(PathBuf::from("desktop.yml")));
    }

    #[test]
    fn run_alias_accepts_no_open_browser_flag() {
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

    #[test]
    fn sessions_command_surface_parses_core_subcommands() {
        for args in [
            ["embed-log", "sessions", "list"].as_slice(),
            ["embed-log", "sessions", "info", "abc"].as_slice(),
            ["embed-log", "sessions", "export", "abc", "--format", "raw"].as_slice(),
            ["embed-log", "sessions", "combined", "abc", "--lines", "10"].as_slice(),
            [
                "embed-log",
                "sessions",
                "events",
                "abc",
                "--severity",
                "fatal",
            ]
            .as_slice(),
            [
                "embed-log",
                "sessions",
                "search",
                "--source",
                "DUT",
                "--from",
                "2026-07-03T09:00:00",
            ]
            .as_slice(),
        ] {
            Cli::parse_from(args);
        }
    }

    #[test]
    fn new_commands_parse() {
        Cli::parse_from(["embed-log", "validate"]);
        Cli::parse_from([
            "embed-log",
            "validate",
            "--json",
            "--config",
            "embed-log.yml",
        ]);
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

    #[test]
    fn version_and_doctor_accept_json_flag() {
        Cli::parse_from(["embed-log", "version", "--json"]);
        Cli::parse_from(["embed-log", "doctor", "--json", "--config", "x.yml"]);
        Cli::parse_from(["embed-log", "ports", "--json"]);
        Cli::parse_from(["embed-log", "hello"]);
    }
}
