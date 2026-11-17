//! `GET /api/sessions/{id}/history`: older records of selected sources,
//! streamed from the session journal.
//!
//! The browser loads history when Highlight is used on a live pane. Reading
//! the whole `combined.jsonl` in the browser needed the full file as one
//! string (browsers cap strings at ~512 MiB) and parsed the records of every
//! source. Here the journal is read line by line on a blocking thread and only
//! the requested sources' records are sent, without fields the viewer does not
//! use, as newline-delimited JSON.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::ws_server::ServerState;

/// Records are sent in chunks of about this many bytes.
const CHUNK_BYTES: usize = 256 * 1024;

#[derive(Debug, Default, Deserialize)]
pub struct HistoryQuery {
    /// Comma-separated source ids.
    #[serde(default)]
    sources: String,
    /// Only records with a smaller sequence are returned.
    before: Option<u64>,
}

/// The journal fields the viewer uses; everything else is dropped.
#[derive(Debug, Deserialize, Serialize)]
struct HistoryRecord {
    source_id: String,
    sequence: u64,
    message: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    is_tx: Option<bool>,
    #[serde(default, skip_serializing)]
    source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    #[serde(rename = "absTs", default, skip_serializing_if = "Option::is_none")]
    abs_ts: Option<String>,
    #[serde(rename = "absNum", default, skip_serializing_if = "Option::is_none")]
    abs_num: Option<f64>,
    #[serde(rename = "relTs", default, skip_serializing_if = "Option::is_none")]
    rel_ts: Option<String>,
    #[serde(rename = "relNum", default, skip_serializing_if = "Option::is_none")]
    rel_num: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timestamp_iso: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timestamp_num: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line_idx: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    level: Option<String>,
}

/// Which journal records a history request wants.
struct HistoryFilter {
    sources: HashSet<String>,
    /// `"source_id":"<id>"` as written in the journal, for a cheap pre-check.
    needles: Vec<String>,
    before: Option<u64>,
}

impl HistoryFilter {
    fn new(sources: HashSet<String>, before: Option<u64>) -> Self {
        let needles = sources
            .iter()
            .filter_map(|source| serde_json::to_string(source).ok())
            .map(|quoted| format!("\"source_id\":{quoted}"))
            .collect();
        Self {
            sources,
            needles,
            before,
        }
    }

    /// The compact JSON line for one journal line, or `None` when the record
    /// is not wanted (other source, materialized merge record, not before
    /// `before`) or the line is not a complete record.
    fn line(&self, line: &str) -> Option<Vec<u8>> {
        // Records of other sources are skipped without deserializing them.
        if !self
            .needles
            .iter()
            .any(|needle| line.contains(needle.as_str()))
        {
            return None;
        }
        let record: HistoryRecord = serde_json::from_str(line).ok()?;
        if !self.sources.contains(&record.source_id)
            || record.source_kind.as_deref() == Some("merge")
            || self.before.is_some_and(|before| record.sequence >= before)
        {
            return None;
        }
        let mut out = serde_json::to_vec(&record).ok()?;
        out.push(b'\n');
        Some(out)
    }
}

/// The journal of a session under `logs_root`, refusing paths that escape it.
pub(crate) fn journal_path(logs_root: &Path, session_id: &str) -> Result<PathBuf, StatusCode> {
    if session_id.is_empty() || session_id.contains("..") || session_id.contains(['/', '\\']) {
        return Err(StatusCode::FORBIDDEN);
    }
    let root = std::fs::canonicalize(logs_root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = std::fs::canonicalize(logs_root.join(session_id).join("combined.jsonl"))
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !path.starts_with(&root) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !path.is_file() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(path)
}

/// Read the journal and send matching records in chunks. Stops when the
/// client goes away (the receiver is dropped).
fn send_history(
    path: PathBuf,
    filter: HistoryFilter,
    tx: tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            let _ = tx.blocking_send(Err(error));
            return;
        }
    };
    let mut reader = BufReader::with_capacity(CHUNK_BYTES, file);
    let mut line = String::new();
    let mut chunk = Vec::with_capacity(CHUNK_BYTES + 4096);
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            // A line without its newline is a record still being written.
            Ok(_) if !line.ends_with('\n') => break,
            Ok(_) => {
                if let Some(out) = filter.line(&line) {
                    chunk.extend_from_slice(&out);
                }
            }
            // Invalid UTF-8 cannot be a journal record; skip it.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => continue,
            Err(error) => {
                let _ = tx.blocking_send(Err(error));
                return;
            }
        }
        if chunk.len() >= CHUNK_BYTES {
            let full = std::mem::replace(&mut chunk, Vec::with_capacity(CHUNK_BYTES + 4096));
            if tx.blocking_send(Ok(Bytes::from(full))).is_err() {
                return;
            }
        }
    }
    if !chunk.is_empty() {
        let _ = tx.blocking_send(Ok(Bytes::from(chunk)));
    }
}

/// `GET /api/sessions/{session_id}/history?sources=A,B&before=1234`
pub async fn session_history_handler(
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<HistoryQuery>,
    State(state): State<ServerState>,
) -> Response {
    let sources: HashSet<String> = query
        .sources
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_string)
        .collect();
    if sources.is_empty() {
        return (StatusCode::BAD_REQUEST, "sources is required").into_response();
    }
    let path = match journal_path(&state.logs_root, &session_id) {
        Ok(path) => path,
        Err(status) => {
            return (status, status.canonical_reason().unwrap_or("error")).into_response()
        }
    };
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let filter = HistoryFilter::new(sources, query.before);
    tokio::task::spawn_blocking(move || send_history(path, filter, tx));
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn history_line_keeps_requested_sources_before_the_cutoff() {
        let wanted = sources(&["DUT", "NET"]);
        let record = r#"{"absNum":1.0,"absTs":"11-16 12:00:00.000","app_name":"embed-log","data":"<inf> hi","level":"inf","line_idx":3,"message":"<inf> hi","origin":"SERIAL","relNum":0.0,"relTs":"T+00:00:00.000","sequence":7,"session_id":"s","source_id":"DUT","source_kind":"uart","tab_labels":["DUT"],"timestamp":"11-16 12:00:00.000","timestamp_iso":"2026-11-16T12:00:00+01:00","timestamp_num":1.0,"type":"rx"}"#;
        let out = HistoryFilter::new(wanted.clone(), Some(8))
            .line(record)
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["source_id"], "DUT");
        assert_eq!(value["sequence"], 7);
        assert_eq!(value["message"], "<inf> hi");
        assert_eq!(value["level"], "inf");
        assert_eq!(value["absTs"], "11-16 12:00:00.000");
        assert_eq!(value["type"], "rx");
        for dropped in [
            "data",
            "app_name",
            "tab_labels",
            "origin",
            "source_kind",
            "session_id",
        ] {
            assert!(value.get(dropped).is_none(), "{dropped} should be dropped");
        }
        assert!(out.len() < record.len());

        let all = HistoryFilter::new(wanted.clone(), None);
        assert!(
            HistoryFilter::new(wanted.clone(), Some(7))
                .line(record)
                .is_none(),
            "not before cutoff"
        );
        assert!(
            HistoryFilter::new(sources(&["HOST"]), None)
                .line(record)
                .is_none(),
            "other source"
        );
        let merged = record.replace(r#""source_kind":"uart""#, r#""source_kind":"merge""#);
        assert!(all.line(&merged).is_none(), "materialized merge");
        assert!(all.line(&record[..record.len() - 5]).is_none(), "partial");
        // A source id that is only a prefix of another must not match.
        assert!(HistoryFilter::new(sources(&["DU"]), None)
            .line(record)
            .is_none());
    }

    #[tokio::test]
    async fn handler_streams_only_requested_records() {
        use super::super::ws_server::tests::test_state;
        let root = std::env::temp_dir().join(format!("embed-log-history-h-{}", std::process::id()));
        std::fs::create_dir_all(root.join("s1")).unwrap();
        let mut journal = String::new();
        for sequence in 1..=5000u64 {
            let source = if sequence % 2 == 0 { "DUT" } else { "HOST" };
            journal.push_str(&format!(
                "{{\"message\":\"line {sequence} {}\",\"sequence\":{sequence},\"source_id\":\"{source}\",\"type\":\"rx\"}}\n",
                "x".repeat(200)
            ));
        }
        journal.push_str("{\"message\":\"partial"); // record still being written
        std::fs::write(root.join("s1").join("combined.jsonl"), journal).unwrap();
        let (state, _) = test_state(root.clone());

        let response = session_history_handler(
            AxumPath("s1".into()),
            Query(HistoryQuery {
                sources: "DUT".into(),
                before: Some(4001),
            }),
            State(state.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4 << 20)
            .await
            .unwrap();
        let rows: Vec<serde_json::Value> = body
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2000);
        assert_eq!(rows[0]["sequence"], 2);
        assert_eq!(rows[1999]["sequence"], 4000);
        assert!(rows.iter().all(|row| row["source_id"] == "DUT"));

        let response = session_history_handler(
            AxumPath("s1".into()),
            Query(HistoryQuery {
                sources: String::new(),
                before: None,
            }),
            State(state.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = session_history_handler(
            AxumPath("..".into()),
            Query(HistoryQuery {
                sources: "DUT".into(),
                before: None,
            }),
            State(state),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_path_refuses_escapes() {
        let root = std::env::temp_dir().join(format!("embed-log-history-{}", std::process::id()));
        std::fs::create_dir_all(root.join("s1")).unwrap();
        std::fs::write(root.join("s1").join("combined.jsonl"), "").unwrap();
        assert!(journal_path(&root, "s1").is_ok());
        assert_eq!(
            journal_path(&root, "..").unwrap_err(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            journal_path(&root, "a/b").unwrap_err(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            journal_path(&root, "missing").unwrap_err(),
            StatusCode::NOT_FOUND
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
