//! Explicit runtime dictionary control through the existing control WebSocket.
use std::path::PathBuf;

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
    #[arg(long)]
    pub source: String,
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
    };
    request["source_id"] = json!(args.source);
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
    fn dictionary_cli_requires_source_and_activation_generation() {
        assert!(
            Cli::try_parse_from(["dictionary", "--source", "UART_A", "--json", "status"])
                .unwrap()
                .args
                .json
        );
        assert!(Cli::try_parse_from(["dictionary", "status"]).is_err());
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
