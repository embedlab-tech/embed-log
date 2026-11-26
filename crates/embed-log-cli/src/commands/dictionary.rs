//! Explicit runtime dictionary control through the existing control WebSocket.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;

use super::daemon::resolve_mutating_endpoint;
use super::watch::ControlClient;
use crate::output::report_json_failure;

#[derive(Debug, Args)]
pub(crate) struct DictionaryArgs {
    #[arg(long, global = true, conflicts_with = "url")]
    pub instance: Option<String>,
    #[arg(long, global = true)]
    pub url: Option<String>,
    /// UART source to control. Required by every command except `decode`.
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: DictionaryCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DictionaryCommand {
    /// Upload a dictionary, optionally arming activation on a raw UART line.
    Stage {
        database: PathBuf,
        #[arg(long)]
        activate_on_regex: Option<String>,
    },
    /// Apply a staged dictionary at an explicit RX boundary (not firmware detection).
    Activate {
        dictionary_id: String,
        #[arg(long)]
        expected_generation: u64,
        #[arg(long, default_value = "zephyr-dict", value_parser = ["zephyr-dict", "zephyr-dict-hex"])]
        parser_type: String,
    },
    /// Show active DB, generation and raw capture offset.
    Status,
    /// Turn a `.log` that holds undecoded `[dict-raw:v1]` records (captured while
    /// no dictionary was available) into readable text. Works offline.
    Decode {
        /// The `.log` file to decode, or `-` for stdin.
        file: PathBuf,
        /// The dictionary of the firmware that produced the log.
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value = "zephyr-dict", value_parser = ["zephyr-dict", "zephyr-dict-hex"])]
        parser_type: String,
        /// Write the result here instead of standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

impl DictionaryArgs {
    fn source(&self) -> Result<&str> {
        self.source
            .as_deref()
            .context("--source is required for this command")
    }
}

fn run_decode(
    file: &Path,
    database: &Path,
    parser_type: &str,
    output: Option<&Path>,
) -> Result<()> {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    if file.as_os_str() == "-" {
        std::io::stdin().read_to_end(&mut bytes)?;
    } else {
        bytes = std::fs::read(file).with_context(|| format!("read {}", file.display()))?;
    }
    let input = String::from_utf8_lossy(&bytes);
    let (decoded, report) = embed_log_core::parsers::raw_decode::decode_log_with_database(
        &input,
        parser_type,
        &database.to_string_lossy(),
    )
    .map_err(anyhow::Error::msg)?;
    match output {
        Some(path) => {
            std::fs::write(path, &decoded).with_context(|| format!("write {}", path.display()))?
        }
        None => std::io::stdout().write_all(decoded.as_bytes())?,
    }
    eprintln!(
        "decoded {} messages from {} raw lines ({} bytes){}{}",
        report.decoded_lines,
        report.raw_lines,
        report.raw_bytes,
        if report.notices > 0 {
            format!("; {} parser notices in the output", report.notices)
        } else {
            String::new()
        },
        if report.invalid_lines > 0 {
            format!(
                "; {} unreadable raw lines kept as they were",
                report.invalid_lines
            )
        } else {
            String::new()
        },
    );
    Ok(())
}

pub(crate) async fn cmd_dictionary(args: DictionaryArgs) -> Result<()> {
    let result = run(&args).await;
    match result {
        Err(error) if args.json => Err(report_json_failure(
            "DICTIONARY_FAILED",
            error.to_string(),
            json!({"source_id": args.source}),
        )),
        result => result,
    }
}

async fn run(args: &DictionaryArgs) -> Result<()> {
    if let DictionaryCommand::Decode {
        file,
        database,
        parser_type,
        output,
    } = &args.command
    {
        return run_decode(file, database, parser_type, output.as_deref());
    }
    let source = args.source()?.to_string();
    let (_, endpoint) = resolve_mutating_endpoint(args.instance.as_deref(), args.url.as_deref())?;
    let mut request = match &args.command {
        DictionaryCommand::Stage {
            database,
            activate_on_regex,
        } => {
            let bytes =
                std::fs::read(database).with_context(|| format!("read {}", database.display()))?;
            anyhow::ensure!(
                bytes.len() <= embed_log_core::sources::dictionary::MAX_DICTIONARY_BYTES,
                "dictionary exceeds 8 MiB"
            );
            let mut request = json!({"type": "dictionary.stage", "dictionary_json": String::from_utf8(bytes).context("dictionary must be UTF-8")?});
            if let Some(pattern) = activate_on_regex {
                request["activate_on_regex"] = json!(pattern);
            }
            request
        }
        DictionaryCommand::Activate {
            dictionary_id,
            expected_generation,
            parser_type,
        } => {
            json!({"type": "dictionary.activate", "dictionary_id": dictionary_id, "expected_generation": expected_generation, "parser_type": parser_type})
        }
        DictionaryCommand::Status => json!({"type": "dictionary.status"}),
        DictionaryCommand::Decode { .. } => unreachable!("handled above"),
    };
    request["source_id"] = json!(source);
    let mut client = ControlClient::connect(&endpoint).await?;
    let response = client.request(request).await?;
    client.close().await;
    anyhow::ensure!(
        response.get("ok").and_then(|v| v.as_bool()) == Some(true),
        "{}",
        response
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("dictionary command failed")
    );
    if args.json {
        println!("{}", serde_json::to_string(&response)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: DictionaryArgs,
    }

    #[test]
    fn stage_accepts_raw_line_regex() {
        let args = Cli::try_parse_from([
            "dictionary",
            "--source",
            "UART_A",
            "stage",
            "target.json",
            "--activate-on-regex",
            "^READY:target$",
        ])
        .unwrap()
        .args;
        assert!(
            matches!(args.command, DictionaryCommand::Stage { activate_on_regex: Some(pattern), .. }
                         if pattern == "^READY:target$")
        );
    }

    #[test]
    fn decode_needs_no_source_but_a_file_and_a_database() {
        let args = Cli::try_parse_from([
            "dictionary",
            "decode",
            "session.log",
            "--database",
            "db.json",
            "-o",
            "out.log",
        ])
        .unwrap()
        .args;
        assert!(args.source.is_none());
        assert!(matches!(
            args.command,
            DictionaryCommand::Decode { ref parser_type, output: Some(_), .. }
                if parser_type == "zephyr-dict"
        ));
        assert!(Cli::try_parse_from(["dictionary", "decode", "session.log"]).is_err());
    }

    #[test]
    fn dictionary_cli_requires_source_and_activation_generation() {
        assert!(
            Cli::try_parse_from(["dictionary", "--source", "UART_A", "--json", "status"])
                .unwrap()
                .args
                .json
        );
        // Runtime commands need a source; `decode` does not (checked when run).
        let no_source = Cli::try_parse_from(["dictionary", "status"]).unwrap().args;
        assert!(no_source.source().is_err());
        assert!(
            Cli::try_parse_from(["dictionary", "--source", "UART_A", "activate", "sha256"])
                .is_err()
        );
        assert!(Cli::try_parse_from([
            "dictionary",
            "--source",
            "UART_A",
            "activate",
            "sha256",
            "--expected-generation",
            "0"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "dictionary",
            "--source",
            "UART_A",
            "activate",
            "sha256",
            "--expected-generation",
            "0",
            "--parser-type",
            "text"
        ])
        .is_err());
    }
}
