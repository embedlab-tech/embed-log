//! `embed-log` CLI entry point.
//!
//! This file holds only the `clap` definitions (`Cli`, `Command`) and the
//! `main()` dispatch. Each subcommand's implementation lives in
//! [`commands`].

mod commands;
mod config;
mod output;
mod util;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};

use commands::daemon::{cmd_start_daemon, cmd_status, cmd_stop};
use commands::export::cmd_export;
use commands::misc;
use commands::run::{cmd_run, cmd_run_quick, RunOverrides};
use commands::schema::cmd_schema;
use commands::sessions::{cmd_sessions, SessionsCommand};
use commands::skill::{cmd_skill, SkillKind};
use commands::tx::{cmd_tx, parse_duration, TxInput, TxOptions};
use commands::watch::{cmd_watch, WatchCommand};

#[derive(Parser)]
#[command(
    name = "embed-log",
    about = "Collect UART/UDP logs and view them in a browser UI",
    version
)]
struct Cli {
    /// YAML config file for browser or TUI mode. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Path to the frontend directory for browser mode.
    #[arg(long, default_value = "frontend")]
    frontend_dir: PathBuf,

    /// Launch the terminal UI (ratatui) instead of the default browser UI.
    #[arg(long)]
    tui: bool,

    /// Do not open the default browser after starting the web server.
    #[arg(long)]
    no_open_browser: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the log server from a config file or explicit UART/file sources
    Run {
        /// UART devices for a no-config quick run (for example: /dev/ttyUSB0).
        #[arg(value_name = "UART", conflicts_with = "config")]
        serial_paths: Vec<PathBuf>,

        /// YAML config file. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
        #[arg(short, long, conflicts_with_all = ["serial_paths", "serial", "file"])]
        config: Option<PathBuf>,

        /// Add a UART device for a no-config quick run. Repeat for multiple devices.
        #[arg(short = 's', long, value_name = "PATH", conflicts_with = "config")]
        serial: Vec<PathBuf>,

        /// Watch a file for appended logs in a no-config quick run. Repeat for multiple files.
        #[arg(short = 'f', long, value_name = "PATH", conflicts_with = "config")]
        file: Vec<PathBuf>,

        /// UART baud rate for every quick-run serial source.
        #[arg(long, default_value_t = 115_200)]
        baud: u32,

        /// Write the generated quick-run configuration to this YAML file.
        #[arg(long, value_name = "PATH", conflicts_with = "config")]
        save_config: Option<PathBuf>,

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
        #[arg(long = "port", alias = "ws-port")]
        ws_port: Option<u16>,

        /// Start as a background daemon.
        #[arg(long, conflicts_with = "tui", requires_all = ["instance", "config"])]
        daemon: bool,

        /// Name used to discover and control this daemon.
        #[arg(long, requires = "daemon")]
        instance: Option<String>,

        /// Machine-readable daemon startup result.
        #[arg(long, requires = "daemon")]
        json: bool,

        /// Internal foreground mode used by the daemon launcher.
        #[arg(long, hide = true)]
        daemon_child: bool,
    },

    /// Discover commands, options, limits, outputs, and stable errors as JSON.
    Schema {
        /// Command or topic to describe, for example `sessions.read`, `sessions read`, `errors`, or `config`.
        #[arg(value_name = "SELECTOR", num_args = 0..=2)]
        selector: Vec<String>,
        /// Explicitly request JSON output; schema is JSON by default.
        #[arg(long)]
        json: bool,
        /// Indent the JSON document for human inspection.
        #[arg(long)]
        pretty: bool,
    },

    /// Print the version-matched agent skill embedded in this binary.
    Skill {
        /// Investigation mode whose guidance should be printed.
        #[arg(value_enum)]
        kind: SkillKind,
        /// Wrap the Markdown skill and version metadata in one JSON document.
        #[arg(long)]
        json: bool,
    },

    /// Show readiness and source status for a running daemon or URL.
    Status {
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE or the only running instance.
        #[arg(long, conflicts_with = "url")]
        instance: Option<String>,
        /// Explicit unregistered HTTP endpoint.
        #[arg(long)]
        url: Option<String>,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },

    /// Generate the active daemon session's canonical HTML report.
    Export {
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE; never inferred.
        #[arg(long, conflicts_with = "url")]
        instance: Option<String>,
        /// Explicit unregistered HTTP endpoint.
        #[arg(long)]
        url: Option<String>,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },

    /// Write to a daemon-owned UART, optionally waiting for an RX match.
    Tx {
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE; never inferred.
        #[arg(long, conflicts_with = "url")]
        instance: Option<String>,
        /// Explicit unregistered HTTP endpoint.
        #[arg(long)]
        url: Option<String>,
        /// Writable source name.
        #[arg(long)]
        source: String,
        /// Send a line, normalizing its ending to one carriage return.
        #[arg(long, conflicts_with_all = ["raw", "file", "stdin"], required_unless_present_any = ["raw", "file", "stdin"])]
        line: Option<String>,
        /// Send this UTF-8 text exactly, without line-ending normalization.
        #[arg(long, allow_hyphen_values = true, conflicts_with_all = ["line", "file", "stdin"], required_unless_present_any = ["line", "file", "stdin"])]
        raw: Option<String>,
        /// Send the file's bytes exactly.
        #[arg(long, value_name = "PATH", conflicts_with_all = ["line", "raw", "stdin"], required_unless_present_any = ["line", "raw", "stdin"])]
        file: Option<PathBuf>,
        /// Read exact bytes from stdin.
        #[arg(long, conflicts_with_all = ["line", "raw", "file"], required_unless_present_any = ["line", "raw", "file"])]
        stdin: bool,
        /// Wait for an RX message containing this text.
        #[arg(long, conflicts_with = "expect_regex")]
        expect: Option<String>,
        /// Wait for an RX message matching this regular expression.
        #[arg(long)]
        expect_regex: Option<String>,
        /// Maximum wait for the expectation.
        #[arg(long, default_value = "30s", value_parser = parse_duration)]
        timeout: std::time::Duration,
        /// Maximum live entries returned as bounded context.
        #[arg(long, default_value_t = 20)]
        context: usize,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },

    /// Manage temporary server-side log watches.
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },

    /// Gracefully stop a registered daemon.
    Stop {
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE or the only running instance.
        #[arg(long)]
        instance: Option<String>,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
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
        /// Inspect a UART path directly (repeatable).
        #[arg(short = 's', long, value_name = "PATH")]
        serial: Vec<PathBuf>,
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
        command: Box<SessionsCommand>,
    },

    /// Print a greeting (smoke-test target)
    #[command(hide = true)]
    Hello,

    /// Validate a config file and print the resolved runtime summary.
    Validate {
        /// YAML config file. Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = std::env::args_os().collect::<Vec<_>>();
    let json_hint =
        args.iter().any(|arg| arg == "--json") || args.get(1).is_some_and(|arg| arg == "schema");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            if json_hint {
                let _ = crate::output::report_json_failure(
                    "CLI_USAGE",
                    error.to_string(),
                    serde_json::json!({"exit_code": error.exit_code()}),
                );
            } else {
                let _ = error.print();
            }
            return ExitCode::from(2);
        }
    };
    let machine_output = cli.machine_output();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if !crate::output::is_json_failure_reported(&error) {
                if machine_output {
                    let message = format!("{error:#}");
                    let code = crate::output::generic_error_code(&message);
                    let _ =
                        crate::output::report_json_failure(code, message, serde_json::Value::Null);
                } else {
                    eprintln!("Error: {error:#}");
                }
            }
            ExitCode::FAILURE
        }
    }
}

impl Cli {
    fn machine_output(&self) -> bool {
        match self.command.as_ref() {
            Some(Command::Schema { .. }) => true,
            Some(Command::Skill { json, .. }) => *json,
            Some(Command::Run { json, .. })
            | Some(Command::Status { json, .. })
            | Some(Command::Export { json, .. })
            | Some(Command::Tx { json, .. })
            | Some(Command::Stop { json, .. })
            | Some(Command::Version { json, .. })
            | Some(Command::Doctor { json, .. })
            | Some(Command::Ports { json })
            | Some(Command::Validate { json, .. }) => *json,
            Some(Command::Watch { command }) => command.machine_output(),
            Some(Command::Sessions { command }) => command.machine_output(),
            Some(Command::Hello) | None => false,
        }
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Run {
            serial_paths,
            config,
            serial,
            file,
            baud,
            save_config,
            frontend_dir,
            no_open_browser,
            tui,
            log_dir,
            host,
            ws_port,
            daemon,
            instance,
            json,
            daemon_child,
        }) => {
            let open_browser = !no_open_browser;
            let overrides = RunOverrides {
                log_dir,
                host,
                ws_port,
            };
            if daemon && !daemon_child {
                if !serial_paths.is_empty() || !serial.is_empty() || !file.is_empty() {
                    anyhow::bail!("--daemon currently requires --config; CLI-only daemon sources are a later milestone");
                }
                cmd_start_daemon(
                    instance.as_deref().expect("clap requires --instance"),
                    config.as_ref(),
                    &frontend_dir,
                    &overrides,
                    json,
                )
            } else if serial_paths.is_empty() && serial.is_empty() && file.is_empty() {
                cmd_run(
                    config.as_ref(),
                    &frontend_dir,
                    open_browser && !daemon_child,
                    tui,
                    daemon_child,
                    &overrides,
                )
                .await
            } else {
                let serial = serial_paths.into_iter().chain(serial).collect();
                cmd_run_quick(
                    serial,
                    file,
                    baud,
                    save_config.as_deref(),
                    &frontend_dir,
                    open_browser,
                    tui,
                    &overrides,
                )
                .await
            }
        }
        Some(Command::Schema {
            selector,
            json: _,
            pretty,
        }) => cmd_schema(Cli::command(), &selector, pretty),
        Some(Command::Skill { kind, json }) => cmd_skill(kind, json),
        Some(Command::Status {
            instance,
            url,
            json,
        }) => cmd_status(instance.as_deref(), url.as_deref(), json),
        Some(Command::Export {
            instance,
            url,
            json,
        }) => cmd_export(instance.as_deref(), url.as_deref(), json),
        Some(Command::Tx {
            instance,
            url,
            source,
            line,
            raw,
            file,
            stdin,
            expect,
            expect_regex,
            timeout,
            context,
            json,
        }) => {
            let input = match (line, raw, file, stdin) {
                (Some(value), None, None, false) => TxInput::Line(value),
                (None, Some(value), None, false) => TxInput::Raw(value),
                (None, None, Some(path), false) => TxInput::File(path),
                (None, None, None, true) => TxInput::Stdin,
                _ => unreachable!("clap enforces exactly one TX input"),
            };
            cmd_tx(TxOptions {
                instance,
                url,
                source,
                input,
                expect,
                expect_regex,
                timeout,
                context,
                json,
            })
            .await
        }
        Some(Command::Watch { command }) => cmd_watch(command).await,
        Some(Command::Stop { instance, json }) => cmd_stop(instance.as_deref(), json),
        Some(Command::Version { config, json }) => misc::cmd_version(config.as_deref(), json),
        Some(Command::Doctor {
            config,
            serial,
            json,
        }) => misc::cmd_doctor(config.as_deref(), &serial, json),
        Some(Command::Ports { json }) => misc::cmd_ports(json),
        Some(Command::Hello) => misc::cmd_hello(),
        Some(Command::Sessions { command }) => cmd_sessions(*command),
        Some(Command::Validate { config, json }) => {
            let path = crate::config::resolve_config_path(config.as_ref());
            misc::cmd_validate(&path, json)
        }
        None => {
            let open_browser = !cli.no_open_browser;
            cmd_run(
                cli.config.as_ref(),
                &cli.frontend_dir,
                open_browser,
                cli.tui,
                false,
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
    fn run_accepts_quick_serial_and_file_sources() {
        let cli = Cli::parse_from([
            "embed-log",
            "run",
            "/dev/ttyUSB0",
            "-s",
            "/dev/ttyUSB1",
            "-f",
            "device.log",
            "--baud",
            "9600",
        ]);
        match cli.command {
            Some(Command::Run {
                serial_paths,
                serial,
                file,
                baud,
                config,
                ..
            }) => {
                assert_eq!(serial_paths, vec![PathBuf::from("/dev/ttyUSB0")]);
                assert_eq!(serial, vec![PathBuf::from("/dev/ttyUSB1")]);
                assert_eq!(file, vec![PathBuf::from("device.log")]);
                assert_eq!(baud, 9_600);
                assert!(config.is_none());
            }
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
        assert_eq!(cli.config, Some(PathBuf::from("embed-log.yml")));
        assert!(cli.no_open_browser);
    }

    #[test]
    fn removed_product_surfaces_are_rejected() {
        for args in [
            ["embed-log", "--ui"].as_slice(),
            ["embed-log", "demo"].as_slice(),
            ["embed-log", "init"].as_slice(),
            ["embed-log", "onboard"].as_slice(),
            ["embed-log", "update"].as_slice(),
            ["embed-log", "merge"].as_slice(),
            ["embed-log", "parse"].as_slice(),
            ["embed-log", "sessions", "import"].as_slice(),
            ["embed-log", "sessions", "bundle"].as_slice(),
            ["embed-log", "sessions", "prune"].as_slice(),
            ["embed-log", "sessions", "marker"].as_slice(),
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
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
            [
                "embed-log",
                "sessions",
                "new",
                "--instance",
                "bench-a",
                "--title",
                "test run",
                "--json",
            ]
            .as_slice(),
            ["embed-log", "sessions", "list"].as_slice(),
            ["embed-log", "sessions", "info", "abc"].as_slice(),
            [
                "embed-log",
                "sessions",
                "read",
                "abc",
                "--after",
                "10",
                "--limit",
                "20",
                "--time",
                "none",
                "--json",
            ]
            .as_slice(),
            [
                "embed-log",
                "sessions",
                "around",
                "abc",
                "--sequence",
                "12",
                "--before",
                "2",
                "--after",
                "3",
                "--time",
                "relative",
            ]
            .as_slice(),
            ["embed-log", "sessions", "open", "latest"].as_slice(),
            ["embed-log", "sessions", "export", "abc", "--format", "raw"].as_slice(),
            ["embed-log", "sessions", "combined", "abc", "--lines", "10"].as_slice(),
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
    fn active_export_accepts_explicit_daemon_targets() {
        Cli::parse_from(["embed-log", "export", "--instance", "bench-a", "--json"]);
        Cli::parse_from(["embed-log", "export", "--url", "http://127.0.0.1:18080"]);
        assert!(Cli::try_parse_from([
            "embed-log",
            "export",
            "--instance",
            "bench-a",
            "--url",
            "http://127.0.0.1:18080",
        ])
        .is_err());
    }

    #[test]
    fn tx_requires_exactly_one_input_and_parses_expectation_options() {
        let cli = Cli::parse_from([
            "embed-log",
            "tx",
            "--instance",
            "bench-a",
            "--source",
            "DUT_UART",
            "--line",
            "reset",
            "--expect",
            "boot complete",
            "--timeout",
            "250ms",
            "--context",
            "4",
            "--json",
        ]);
        match cli.command {
            Some(Command::Tx {
                line,
                timeout,
                context,
                json,
                ..
            }) => {
                assert_eq!(line.as_deref(), Some("reset"));
                assert_eq!(timeout, std::time::Duration::from_millis(250));
                assert_eq!(context, 4);
                assert!(json);
            }
            _ => panic!("expected tx command"),
        }
        assert!(Cli::try_parse_from(["embed-log", "tx", "--source", "DUT"]).is_err());
        assert!(Cli::try_parse_from([
            "embed-log",
            "tx",
            "--source",
            "DUT",
            "--line",
            "one",
            "--raw",
            "two",
        ])
        .is_err());
    }

    #[test]
    fn skill_command_accepts_modes_and_raw_or_json_output() {
        Cli::parse_from(["embed-log", "skill", "live"]);
        Cli::parse_from(["embed-log", "skill", "recorded", "--json"]);
        assert!(Cli::try_parse_from(["embed-log", "skill"]).is_err());
    }

    #[test]
    fn schema_command_accepts_index_topics_and_command_paths() {
        for args in [
            ["embed-log", "schema"].as_slice(),
            ["embed-log", "schema", "tx", "--json"].as_slice(),
            ["embed-log", "schema", "sessions.read", "--pretty"].as_slice(),
            ["embed-log", "schema", "sessions", "around"].as_slice(),
            ["embed-log", "schema", "errors"].as_slice(),
            ["embed-log", "schema", "config"].as_slice(),
        ] {
            Cli::parse_from(args);
        }
    }

    #[test]
    fn validate_command_parses() {
        Cli::parse_from(["embed-log", "validate"]);
        Cli::parse_from([
            "embed-log",
            "validate",
            "--json",
            "--config",
            "embed-log.yml",
        ]);
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
            "--port",
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
