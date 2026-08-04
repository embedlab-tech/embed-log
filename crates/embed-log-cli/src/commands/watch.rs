//! Durable temporary watches retained by a running Embed-log process.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout_at, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::daemon::{resolve_mutating_endpoint, InstanceRecord};
use super::tx::parse_duration;
use crate::output::report_json_failure;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWrite = futures::stream::SplitSink<WsStream, Message>;
type WsRead = futures::stream::SplitStream<WsStream>;

#[derive(Debug, Subcommand)]
pub(crate) enum WatchCommand {
    /// Add a temporary one-shot condition.
    Add {
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE; never inferred.
        #[arg(long, conflicts_with = "url")]
        instance: Option<String>,
        /// Explicit unregistered HTTP endpoint.
        #[arg(long)]
        url: Option<String>,
        /// Source whose RX/log messages are checked.
        #[arg(long)]
        source: String,
        /// Match a literal substring.
        #[arg(long, conflicts_with = "regex", required_unless_present = "regex")]
        contains: Option<String>,
        /// Match a regular expression.
        #[arg(
            long,
            conflicts_with = "contains",
            required_unless_present = "contains"
        )]
        regex: Option<String>,
        /// Active lifetime; matched state remains available until removal.
        #[arg(long, default_value = "30s", value_parser = parse_duration)]
        ttl: Duration,
        /// Compatibility spelling; watches are one-shot by default.
        #[arg(long, hide = true)]
        once: bool,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Wait for a retained match without streaming ordinary logs.
    Wait {
        /// Watch identifier returned by `watch add`.
        watch_id: String,
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE; never inferred.
        #[arg(long, conflicts_with = "url")]
        instance: Option<String>,
        /// Explicit unregistered HTTP endpoint.
        #[arg(long)]
        url: Option<String>,
        /// Maximum time this CLI invocation waits; the server-side TTL is unchanged.
        #[arg(long, default_value = "30s", value_parser = parse_duration)]
        timeout: Duration,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Remove a watch and deactivate its event rule.
    Remove {
        /// Watch identifier returned by `watch add`.
        watch_id: String,
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
}

impl WatchCommand {
    pub(crate) fn machine_output(&self) -> bool {
        match self {
            Self::Add { json, .. } | Self::Wait { json, .. } | Self::Remove { json, .. } => *json,
        }
    }
}

pub(crate) async fn cmd_watch(command: WatchCommand) -> Result<()> {
    match command {
        WatchCommand::Add {
            instance,
            url,
            source,
            contains,
            regex,
            ttl,
            once: _,
            json,
        } => add(instance, url, source, contains, regex, ttl, json).await,
        WatchCommand::Wait {
            watch_id,
            instance,
            url,
            timeout,
            json,
        } => wait(watch_id, instance, url, timeout, json).await,
        WatchCommand::Remove {
            watch_id,
            instance,
            url,
            json,
        } => remove(watch_id, instance, url, json).await,
    }
}

async fn add(
    instance: Option<String>,
    url: Option<String>,
    source: String,
    contains: Option<String>,
    regex: Option<String>,
    ttl: Duration,
    output_json: bool,
) -> Result<()> {
    anyhow::ensure!(
        ttl <= Duration::from_secs(24 * 60 * 60),
        "--ttl must not exceed 24h"
    );
    if let Some(pattern) = contains.as_ref() {
        anyhow::ensure!(!pattern.is_empty(), "--contains must not be empty");
    }
    if let Some(pattern) = regex.as_ref() {
        anyhow::ensure!(!pattern.is_empty(), "--regex must not be empty");
        regex::Regex::new(pattern).with_context(|| format!("invalid --regex {pattern:?}"))?;
    }
    let (record, endpoint) = resolve_mutating_endpoint(instance.as_deref(), url.as_deref())?;
    let mut client = ControlClient::connect(&endpoint).await?;
    let response = client
        .request(json!({
            "type":"watch.create",
            "source_id":source,
            "contains":contains,
            "regex":regex,
            "ttl_ms":ttl.as_millis() as u64,
        }))
        .await?;
    ensure_ok(&response, "create watch")?;
    let watch = response
        .get("watch")
        .cloned()
        .context("watch response omitted watch")?;
    let output = json!({
        "ok":true,
        "instance":instance_name(record.as_ref()),
        "endpoint":endpoint,
        "watch":watch,
    });
    if output_json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!(
            "added {} on {} (expires {})",
            output["watch"]["id"].as_str().unwrap_or("watch"),
            output["watch"]["source_id"].as_str().unwrap_or("source"),
            output["watch"]["expires_at"].as_str().unwrap_or("unknown")
        );
    }
    client.close().await;
    Ok(())
}

async fn wait(
    watch_id: String,
    instance: Option<String>,
    url: Option<String>,
    timeout: Duration,
    output_json: bool,
) -> Result<()> {
    let (record, endpoint) = resolve_mutating_endpoint(instance.as_deref(), url.as_deref())?;
    let mut client = ControlClient::connect(&endpoint).await?;
    let deadline = Instant::now() + timeout;
    loop {
        let response = client
            .request(json!({"type":"watch.get","watch_id":watch_id}))
            .await?;
        if response.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown server error");
            let code = if error == "watch not found" {
                "WATCH_NOT_FOUND"
            } else {
                "WATCH_ERROR"
            };
            return fail_wait(
                output_json,
                code,
                &watch_id,
                record.as_ref(),
                &endpoint,
                None,
                format!("get watch failed: {error}"),
            );
        }
        let watch = response
            .get("watch")
            .cloned()
            .context("watch response omitted watch")?;
        match watch.get("status").and_then(Value::as_str) {
            Some("matched") => {
                let output = json!({
                    "ok":true,
                    "instance":instance_name(record.as_ref()),
                    "endpoint":endpoint,
                    "watch_id":watch_id,
                    "status":"matched",
                    "match":watch.get("match").cloned().unwrap_or(Value::Null),
                    "next_cursor":watch.pointer("/match/sequence").cloned(),
                });
                if output_json {
                    println!("{}", serde_json::to_string(&output)?);
                } else {
                    println!(
                        "{} matched at line {}: {}",
                        watch_id,
                        output["match"]["line_idx"].as_u64().unwrap_or(0),
                        output["match"]["message"].as_str().unwrap_or("")
                    );
                }
                client.close().await;
                return Ok(());
            }
            Some("expired") => {
                return fail_wait(
                    output_json,
                    "WATCH_EXPIRED",
                    &watch_id,
                    record.as_ref(),
                    &endpoint,
                    Some(watch),
                    format!("watch {watch_id:?} expired before matching"),
                );
            }
            Some("active") => {}
            Some(status) => anyhow::bail!("watch {watch_id:?} returned unknown status {status:?}"),
            None => anyhow::bail!("watch {watch_id:?} response omitted status"),
        }
        if Instant::now() >= deadline {
            return fail_wait(
                output_json,
                "WATCH_WAIT_TIMEOUT",
                &watch_id,
                record.as_ref(),
                &endpoint,
                Some(watch),
                format!("timed out after {timeout:?} waiting for watch {watch_id:?}"),
            );
        }
        sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now()))).await;
    }
}

async fn remove(
    watch_id: String,
    instance: Option<String>,
    url: Option<String>,
    output_json: bool,
) -> Result<()> {
    let (record, endpoint) = resolve_mutating_endpoint(instance.as_deref(), url.as_deref())?;
    let mut client = ControlClient::connect(&endpoint).await?;
    let response = client
        .request(json!({"type":"watch.delete","watch_id":watch_id}))
        .await?;
    ensure_ok(&response, "remove watch")?;
    let output = json!({
        "ok":true,
        "instance":instance_name(record.as_ref()),
        "endpoint":endpoint,
        "watch_id":watch_id,
        "source_id":response.get("source_id").cloned().unwrap_or(Value::Null),
    });
    if output_json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("removed watch {watch_id}");
    }
    client.close().await;
    Ok(())
}

fn fail_wait(
    output_json: bool,
    code: &str,
    watch_id: &str,
    record: Option<&InstanceRecord>,
    endpoint: &str,
    watch: Option<Value>,
    message: String,
) -> Result<()> {
    if output_json {
        return Err(report_json_failure(
            code,
            message,
            json!({
                "instance":instance_name(record),
                "endpoint":endpoint,
                "watch_id":watch_id,
                "watch":watch,
            }),
        ));
    }
    anyhow::bail!(message)
}

fn ensure_ok(response: &Value, action: &str) -> Result<()> {
    anyhow::ensure!(
        response.get("ok").and_then(Value::as_bool) == Some(true),
        "{action} failed: {}",
        response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown server error")
    );
    Ok(())
}

fn instance_name(record: Option<&InstanceRecord>) -> Option<&str> {
    record.map(|record| record.instance.as_str())
}

struct ControlClient {
    write: WsWrite,
    read: WsRead,
    next_id: u64,
}

impl ControlClient {
    async fn connect(endpoint: &str) -> Result<Self> {
        let ws_url = control_ws_url(endpoint)?;
        let (stream, _) = connect_async(&ws_url)
            .await
            .with_context(|| format!("connect to control WebSocket {ws_url}"))?;
        let (write, read) = stream.split();
        Ok(Self {
            write,
            read,
            next_id: 1,
        })
    }

    async fn request(&mut self, mut command: Value) -> Result<Value> {
        let id = format!("watch-cli-{}", self.next_id);
        self.next_id += 1;
        command["id"] = Value::String(id.clone());
        self.write
            .send(Message::Text(command.to_string()))
            .await
            .context("send watch control command")?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let message = timeout_at(deadline, self.read.next())
                .await
                .context("watch control response timed out")?
                .context("watch control WebSocket closed")?
                .context("read watch control response")?;
            match message {
                Message::Text(text) => {
                    let response: Value =
                        serde_json::from_str(&text).context("parse watch control response")?;
                    if response.get("type").and_then(Value::as_str) == Some("error")
                        && response
                            .get("id")
                            .and_then(Value::as_str)
                            .map_or(true, |response_id| response_id == id)
                    {
                        anyhow::bail!(
                            "watch control command failed: {}",
                            response
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown server error")
                        );
                    }
                    if response.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                        return Ok(response);
                    }
                }
                Message::Close(_) => anyhow::bail!("watch control WebSocket closed"),
                Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
            }
        }
    }

    async fn close(&mut self) {
        let _ = self.write.send(Message::Close(None)).await;
    }
}

fn control_ws_url(endpoint: &str) -> Result<String> {
    let base = endpoint.trim_end_matches('/');
    let base = if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if base.starts_with("ws://") || base.starts_with("wss://") {
        base.to_string()
    } else {
        anyhow::bail!("unsupported endpoint {endpoint:?}; use an explicit URL scheme")
    };
    Ok(format!("{base}/api/v1/control"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: WatchCommand,
    }

    #[test]
    fn watch_cli_requires_matcher_and_parses_durations() {
        let parsed = TestCli::parse_from([
            "test",
            "add",
            "--instance",
            "bench",
            "--source",
            "DUT",
            "--contains",
            "ready",
            "--ttl",
            "250ms",
        ]);
        match parsed.command {
            WatchCommand::Add { ttl, .. } => assert_eq!(ttl, Duration::from_millis(250)),
            _ => panic!("expected add"),
        }
        assert!(TestCli::try_parse_from(["test", "add", "--source", "DUT"]).is_err());
        assert!(TestCli::try_parse_from([
            "test",
            "add",
            "--source",
            "DUT",
            "--contains",
            "one",
            "--regex",
            "two",
        ])
        .is_err());
    }
}
