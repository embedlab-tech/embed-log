//! UART transmission through a running Embed-log daemon.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use regex::Regex;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::time::{timeout_at, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::daemon::{resolve_mutating_endpoint, InstanceRecord};
use crate::output::report_json_failure;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONTEXT: usize = 1_000;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsRead = futures::stream::SplitStream<WsStream>;

#[derive(Debug)]
pub(crate) enum TxInput {
    Line(String),
    Raw(String),
    File(PathBuf),
    Stdin,
}

#[derive(Debug)]
pub(crate) struct TxOptions {
    pub instance: Option<String>,
    pub url: Option<String>,
    pub source: String,
    pub input: TxInput,
    pub expect: Option<String>,
    pub expect_regex: Option<String>,
    pub timeout: Duration,
    pub context: usize,
    pub json: bool,
}

#[derive(Debug)]
enum Matcher {
    Contains(String),
    Regex(Regex),
}

impl Matcher {
    fn matches(&self, message: &str) -> bool {
        match self {
            Self::Contains(needle) => message.contains(needle),
            Self::Regex(regex) => regex.is_match(message),
        }
    }

    fn description(&self) -> (&'static str, &str) {
        match self {
            Self::Contains(pattern) => ("contains", pattern),
            Self::Regex(regex) => ("regex", regex.as_str()),
        }
    }
}

struct ReceiveState {
    source: String,
    matcher: Option<Matcher>,
    armed: bool,
    matched: Option<Value>,
    context: VecDeque<Value>,
    context_limit: usize,
    entries_seen: usize,
}

impl ReceiveState {
    fn new(source: String, matcher: Option<Matcher>, context_limit: usize) -> Self {
        Self {
            source,
            matcher,
            armed: false,
            matched: None,
            context: VecDeque::new(),
            context_limit,
            entries_seen: 0,
        }
    }

    fn accept(&mut self, entry: Value) {
        if entry.get("source_id").and_then(Value::as_str) != Some(self.source.as_str()) {
            return;
        }
        self.entries_seen += 1;
        if self.context_limit > 0 {
            if self.context.len() == self.context_limit {
                self.context.pop_front();
            }
            self.context.push_back(entry.clone());
        }
        if self.armed
            && self.matched.is_none()
            && !entry.get("is_tx").and_then(Value::as_bool).unwrap_or(false)
            && self.matcher.as_ref().is_some_and(|matcher| {
                matcher.matches(entry.get("message").and_then(Value::as_str).unwrap_or(""))
            })
        {
            self.matched = Some(entry);
        }
    }

    fn context_json(&self) -> Vec<Value> {
        self.context.iter().cloned().collect()
    }

    fn truncated(&self) -> bool {
        self.entries_seen > self.context.len()
    }
}

pub(crate) async fn cmd_tx(options: TxOptions) -> Result<()> {
    anyhow::ensure!(
        options.context <= MAX_CONTEXT,
        "--context must not exceed {MAX_CONTEXT}"
    );
    let (record, endpoint) =
        resolve_mutating_endpoint(options.instance.as_deref(), options.url.as_deref())?;
    let ws_url = control_ws_url(&endpoint)?;
    let (data, line_ending) = read_input(&options.input)?;
    anyhow::ensure!(!data.is_empty(), "TX input must not be empty");

    let matcher = match (options.expect.as_ref(), options.expect_regex.as_ref()) {
        (Some(pattern), None) => {
            anyhow::ensure!(!pattern.is_empty(), "--expect must not be empty");
            Some(Matcher::Contains(pattern.clone()))
        }
        (None, Some(pattern)) => {
            anyhow::ensure!(!pattern.is_empty(), "--expect-regex must not be empty");
            Some(Matcher::Regex(Regex::new(pattern).with_context(|| {
                format!("invalid --expect-regex {pattern:?}")
            })?))
        }
        (None, None) => None,
        (Some(_), Some(_)) => anyhow::bail!("--expect conflicts with --expect-regex"),
    };
    let expects_reply = matcher.is_some();
    let mut state = ReceiveState::new(options.source.clone(), matcher, options.context);

    let (stream, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("connect to control WebSocket {ws_url}"))?;
    let (mut write, mut read) = stream.split();

    send_json(&mut write, json!({"id":"tx-hello","type":"hello"})).await?;
    let hello = wait_for_response(
        &mut read,
        "tx-hello",
        Instant::now() + COMMAND_TIMEOUT,
        &mut state,
    )
    .await?;
    let sources = hello
        .get("sources")
        .and_then(Value::as_object)
        .context("hello response omitted sources")?;
    let source = sources.get(&options.source).with_context(|| {
        let choices = sources.keys().cloned().collect::<Vec<_>>().join(", ");
        format!(
            "unknown source {:?}; writable source choices: {choices}",
            options.source
        )
    })?;
    anyhow::ensure!(
        source.get("writable").and_then(Value::as_bool) == Some(true),
        "source {:?} is not writable",
        options.source
    );
    let session_id = hello
        .pointer("/session/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if expects_reply {
        send_json(
            &mut write,
            json!({"id":"tx-subscribe","type":"subscribe","sources":[options.source]}),
        )
        .await?;
        wait_for_response(
            &mut read,
            "tx-subscribe",
            Instant::now() + COMMAND_TIMEOUT,
            &mut state,
        )
        .await?;
        // The expectation is armed only after subscription acknowledgement and
        // before tx.write is sent. Pre-existing subscription traffic cannot match.
        state.context.clear();
        state.entries_seen = 0;
        state.armed = true;
    }

    let deadline = Instant::now()
        + if expects_reply {
            options.timeout
        } else {
            COMMAND_TIMEOUT
        };
    send_json(
        &mut write,
        json!({
            "id": "tx-write",
            "type": "tx.write",
            "source_id": options.source,
            "origin": "cli",
            "data_bytes": data,
            "line_ending": line_ending,
        }),
    )
    .await?;

    let tx_result = wait_for_response(
        &mut read,
        "tx-write",
        Instant::now() + COMMAND_TIMEOUT,
        &mut state,
    )
    .await?;
    anyhow::ensure!(
        tx_result.get("ok").and_then(Value::as_bool) == Some(true),
        "UART write failed: {}",
        tx_result
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
    );
    let bytes_written = tx_result
        .get("bytes")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    while expects_reply && state.matched.is_none() {
        match receive_one(&mut read, deadline, &mut state).await {
            Ok(()) => {}
            Err(_error) if Instant::now() >= deadline => {
                return expectation_timeout(
                    &options,
                    record.as_ref(),
                    &endpoint,
                    &session_id,
                    bytes_written,
                    &state,
                );
            }
            Err(error) => return Err(error),
        }
    }

    let output = success_output(
        &options,
        record.as_ref(),
        &endpoint,
        &session_id,
        bytes_written,
        &state,
    );
    if options.json {
        println!("{}", serde_json::to_string(&output)?);
    } else if let Some(matched) = &state.matched {
        println!(
            "wrote {bytes_written} bytes to {}; matched at line {}: {}",
            options.source,
            matched.get("line_idx").and_then(Value::as_u64).unwrap_or(0),
            matched.get("message").and_then(Value::as_str).unwrap_or("")
        );
    } else {
        println!("wrote {bytes_written} bytes to {}", options.source);
    }
    let _ = write.send(Message::Close(None)).await;
    Ok(())
}

fn success_output(
    options: &TxOptions,
    record: Option<&InstanceRecord>,
    endpoint: &str,
    session_id: &str,
    bytes_written: u64,
    state: &ReceiveState,
) -> Value {
    let expectation = state.matcher.as_ref().map(|matcher| {
        let (kind, pattern) = matcher.description();
        json!({"kind":kind,"pattern":pattern,"matched":true,"entry":state.matched})
    });
    json!({
        "ok": true,
        "instance": record.map(|record| record.instance.as_str()),
        "endpoint": endpoint,
        "session_id": session_id,
        "source_id": options.source,
        "bytes_written": bytes_written,
        "expectation": expectation,
        "next_cursor": state.matched.as_ref().and_then(|entry| entry.get("sequence")).cloned(),
        "context": state.context_json(),
        "truncated": state.truncated(),
    })
}

fn expectation_timeout(
    options: &TxOptions,
    record: Option<&InstanceRecord>,
    endpoint: &str,
    session_id: &str,
    bytes_written: u64,
    state: &ReceiveState,
) -> Result<()> {
    let matcher = state.matcher.as_ref().expect("timeout requires matcher");
    let (kind, pattern) = matcher.description();
    let message = format!(
        "timed out after {:?} waiting for {} {:?} on source {:?}",
        options.timeout, kind, pattern, options.source
    );
    if options.json {
        return Err(report_json_failure(
            "EXPECT_TIMEOUT",
            message,
            json!({
                "instance": record.map(|record| record.instance.as_str()),
                "endpoint": endpoint,
                "session_id": session_id,
                "source_id": options.source,
                "bytes_written": bytes_written,
                "expectation": {"kind":kind,"pattern":pattern,"matched":false},
                "next_cursor": state.context.back().and_then(|entry| entry.get("sequence")).cloned(),
                "context": state.context_json(),
                "truncated": state.truncated(),
            }),
        ));
    }
    anyhow::bail!(message)
}

async fn send_json(
    write: &mut futures::stream::SplitSink<WsStream, Message>,
    value: Value,
) -> Result<()> {
    write
        .send(Message::Text(value.to_string()))
        .await
        .context("send control WebSocket command")
}

async fn wait_for_response(
    read: &mut WsRead,
    expected_id: &str,
    deadline: Instant,
    state: &mut ReceiveState,
) -> Result<Value> {
    loop {
        let message = next_json(read, deadline).await?;
        if message.get("type").and_then(Value::as_str) == Some("log.entry") {
            state.accept(message);
            continue;
        }
        if message.get("type").and_then(Value::as_str) == Some("stream_gap") {
            anyhow::bail!(
                "control stream gap skipped {} messages; expectation result is unsafe",
                message.get("skipped").and_then(Value::as_u64).unwrap_or(0)
            );
        }
        if message.get("type").and_then(Value::as_str) == Some("error")
            && message
                .get("id")
                .and_then(Value::as_str)
                .map_or(true, |id| id == expected_id)
        {
            anyhow::bail!(
                "control command failed: {}",
                message
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            );
        }
        if message.get("id").and_then(Value::as_str) == Some(expected_id) {
            return Ok(message);
        }
    }
}

async fn receive_one(read: &mut WsRead, deadline: Instant, state: &mut ReceiveState) -> Result<()> {
    let message = next_json(read, deadline).await?;
    match message.get("type").and_then(Value::as_str) {
        Some("log.entry") => state.accept(message),
        Some("stream_gap") => anyhow::bail!(
            "control stream gap skipped {} messages; expectation result is unsafe",
            message.get("skipped").and_then(Value::as_u64).unwrap_or(0)
        ),
        Some("error") => anyhow::bail!(
            "control stream error: {}",
            message
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        ),
        _ => {}
    }
    Ok(())
}

async fn next_json(read: &mut WsRead, deadline: Instant) -> Result<Value> {
    loop {
        let message = timeout_at(deadline, read.next())
            .await
            .context("control WebSocket response timed out")?
            .context("control WebSocket closed")?
            .context("read control WebSocket message")?;
        match message {
            Message::Text(text) => {
                return serde_json::from_str(&text).context("parse control WebSocket response")
            }
            Message::Close(_) => anyhow::bail!("control WebSocket closed"),
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
    }
}

fn read_input(input: &TxInput) -> Result<(Vec<u8>, bool)> {
    match input {
        TxInput::Line(line) => Ok((line.as_bytes().to_vec(), true)),
        TxInput::Raw(raw) => Ok((raw.as_bytes().to_vec(), false)),
        TxInput::File(path) => Ok((read_file(path)?, false)),
        TxInput::Stdin => {
            let mut data = Vec::new();
            std::io::stdin()
                .read_to_end(&mut data)
                .context("read TX data from stdin")?;
            Ok((data, false))
        }
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read TX file {}", path.display()))
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
        anyhow::bail!("unsupported endpoint {endpoint:?}; use http://, https://, ws://, or wss://")
    };
    Ok(format!("{base}/api/v1/control"))
}

pub(crate) fn parse_duration(value: &str) -> Result<Duration, String> {
    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 0.001)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1.0)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60.0)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3600.0)
    } else {
        return Err("duration must end in ms, s, m, or h (for example 30s)".to_string());
    };
    let amount: f64 = number
        .parse()
        .map_err(|_| format!("invalid duration {value:?}"))?;
    if !amount.is_finite() || amount <= 0.0 {
        return Err("duration must be greater than zero".to_string());
    }
    Ok(Duration::from_secs_f64(amount * multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parser_accepts_supported_units() {
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("2s").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_duration("1.5m").unwrap(), Duration::from_secs(90));
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("10").is_err());
    }

    #[test]
    fn control_url_maps_http_and_rejects_bare_host() {
        assert_eq!(
            control_ws_url("http://127.0.0.1:18080").unwrap(),
            "ws://127.0.0.1:18080/api/v1/control"
        );
        assert!(control_ws_url("127.0.0.1:18080").is_err());
    }

    #[test]
    fn receive_state_matches_only_rx_and_bounds_context() {
        let mut state = ReceiveState::new(
            "DUT".to_string(),
            Some(Matcher::Contains("ready".to_string())),
            2,
        );
        state.armed = true;
        state.accept(json!({"source_id":"DUT","message":"ready","is_tx":true}));
        state.accept(json!({"source_id":"DUT","message":"first","is_tx":false}));
        state.accept(json!({"source_id":"DUT","message":"ready now","is_tx":false}));
        assert_eq!(state.matched.as_ref().unwrap()["message"], "ready now");
        assert_eq!(state.context.len(), 2);
        assert!(state.truncated());
    }
}
