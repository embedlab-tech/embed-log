//! `embed-log sessions` — inspect and export recorded sessions, plus the
//! `sessions marker` sub-subcommands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use clap::{Subcommand, ValueEnum};
use regex::Regex;

use embed_log_core::session::SessionExporter;

/// `embed-log sessions <command>`.
#[derive(Subcommand)]
pub(crate) enum SessionsCommand {
    /// List sessions under a log directory.
    List {
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long = "with-markers")]
        with_markers: bool,
    },
    /// Show one session manifest.
    Info {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Export a session as HTML or raw merged text.
    Export {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ExportFormat::Html)]
        format: ExportFormat,
    },
    /// Print or follow the session-wide combined JSONL stream.
    #[command(visible_alias = "tail-combined")]
    Combined {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        follow: bool,
        #[arg(long, alias = "last")]
        lines: Option<usize>,
    },
    /// Print recorded event-detection hits from events.jsonl.
    Events {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        /// Print JSONL instead of compact human-readable lines.
        #[arg(long)]
        json: bool,
        /// Restrict to event severity (info, warn, error, fatal).
        #[arg(long)]
        severity: Option<String>,
        /// Restrict to source_id.
        #[arg(long)]
        source: Option<String>,
        /// Substring that must appear in the event message.
        #[arg(long)]
        contains: Option<String>,
        /// Regex that must match the event message.
        #[arg(long)]
        regex: Option<String>,
        /// Stop after printing this many events.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Search combined JSONL across sessions with structured filters.
    #[command(
        long_about = "Search all session combined.jsonl files under a log directory.\n\nExamples:\n  embed-log sessions search --dir logs --source DUT\n  embed-log sessions search --dir logs --source DUT --from 2026-07-03T09:00:00 --to 2026-07-03T15:00:00\n  embed-log sessions search --dir logs --job nightly-42 --kind network_capture --dst-port 5683\n  embed-log sessions search --dir logs --contains panic --regex 'ERROR|WARN'\n\nTime filters accept RFC3339 (with timezone) or local wall-clock forms like 2026-07-03T09:00:00 or 2026-07-03 09:00:00."
    )]
    Search {
        /// Root logs directory containing session subdirectories.
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        /// Restrict to session ids or unique prefixes. Repeatable.
        #[arg(long = "session")]
        sessions: Vec<String>,
        /// Restrict to sessions whose manifest has this job_id.
        #[arg(long)]
        job: Option<String>,
        /// Restrict to one or more source_id values. Repeatable.
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Restrict to source_kind (uart, udp, file, network_capture).
        #[arg(long)]
        kind: Option<String>,
        /// Earliest timestamp_iso to include.
        #[arg(long)]
        from: Option<String>,
        /// Latest timestamp_iso to include.
        #[arg(long)]
        to: Option<String>,
        /// Substring that must appear in the message field.
        #[arg(long)]
        contains: Option<String>,
        /// Regex that must match the message field.
        #[arg(long)]
        regex: Option<String>,
        /// Restrict to packet entries with this UDP source port.
        #[arg(long = "src-port")]
        src_port: Option<u16>,
        /// Restrict to packet entries with this UDP destination port.
        #[arg(long = "dst-port")]
        dst_port: Option<u16>,
        /// Restrict to packet entries whose src_ip or dst_ip matches this address.
        #[arg(long)]
        ip: Option<String>,
        /// Stop after printing this many matching entries.
        #[arg(long)]
        limit: Option<usize>,
        /// Print only the number of matches.
        #[arg(long)]
        count: bool,
    },
    /// List markers in a session.
    Marker {
        #[command(subcommand)]
        command: MarkerCommand,
    },
}

/// `embed-log sessions marker <command>`.
#[derive(Clone, Debug, Subcommand)]
pub(crate) enum MarkerCommand {
    /// List markers for a session.
    List {
        session_id: String,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Show one marker by index (1-based).
    Show {
        session_id: String,
        marker_index: usize,
        #[arg(long, alias = "log-dir", default_value = "logs")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExportFormat {
    Html,
    Raw,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionRecord {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: serde_json::Value,
}

/// Dispatch `embed-log sessions`.
pub(crate) fn cmd_sessions(command: SessionsCommand) -> Result<()> {
    match command {
        SessionsCommand::List {
            dir,
            json,
            limit,
            with_markers,
        } => list_sessions(&dir, json, limit, with_markers),
        SessionsCommand::Marker { command } => cmd_session_marker(command),
        SessionsCommand::Info {
            session_id,
            dir,
            json,
        } => show_session_info(&dir, &session_id, json),
        SessionsCommand::Combined {
            session_id,
            dir,
            follow,
            lines,
        } => show_session_combined(&dir, &session_id, follow, lines),
        SessionsCommand::Events {
            session_id,
            dir,
            json,
            severity,
            source,
            contains,
            regex,
            limit,
        } => show_session_events(
            &dir,
            &session_id,
            EventsFilters::compile(severity, source, contains, regex, limit)?,
            json,
        ),
        SessionsCommand::Search {
            dir,
            sessions,
            job,
            sources,
            kind,
            from,
            to,
            contains,
            regex,
            src_port,
            dst_port,
            ip,
            limit,
            count,
        } => search_sessions(
            &dir,
            SearchFilters::compile(
                sessions, job, sources, kind, from, to, contains, regex, src_port, dst_port, ip,
                limit, count,
            )?,
        ),
        SessionsCommand::Export {
            session_id,
            dir,
            output,
            format,
        } => {
            let session = resolve_session(&dir, &session_id)?;
            match format {
                ExportFormat::Html => {
                    let output = output.unwrap_or_else(|| session.dir.join("session.html"));
                    export_session_html(&session, output)?;
                }
                ExportFormat::Raw => {
                    let output = output.unwrap_or_else(|| session.dir.join("session.raw.log"));
                    export_session_raw(&session, output)?;
                }
            }
            Ok(())
        }
    }
}

fn list_sessions(dir: &Path, json: bool, limit: Option<usize>, with_markers: bool) -> Result<()> {
    let mut sessions = load_sessions(dir)?;
    if let Some(limit) = limit {
        sessions.truncate(limit);
    }
    // Apply --with-markers filter before any output (JSON or human).
    if with_markers {
        sessions.retain(|s| count_markers_in_session(&s.dir) > 0);
    }

    if json {
        let rows: Vec<_> = sessions
            .iter()
            .map(|session| {
                let marker_count = count_markers_in_session(&session.dir);
                let mut entry = serde_json::json!({
                    "id": session.id,
                    "dir": session.dir,
                    "manifest": session.manifest,
                });
                entry["marker_count"] = serde_json::json!(marker_count);
                entry
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "sessions": rows }))?
        );
    } else {
        for session in sessions {
            let marker_count = count_markers_in_session(&session.dir);
            let started_at = session
                .manifest
                .get("started_at")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!(
                "{}\t{}\t{}\t{} marker(s)",
                session.id,
                started_at,
                session.dir.display(),
                marker_count
            );
        }
    }
    Ok(())
}

fn show_session_info(dir: &Path, session_id: &str, json: bool) -> Result<()> {
    let session = resolve_session(dir, session_id)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&session.manifest)?);
    } else {
        println!("session: {}", session.id);
        println!("dir:     {}", session.dir.display());
        if let Some(started_at) = session.manifest.get("started_at").and_then(|v| v.as_str()) {
            println!("started: {started_at}");
        }
        if let Some(status) = session.manifest.get("html_status").and_then(|v| v.as_str()) {
            println!("html:    {status}");
        }
        if let Some(combined_file) = session
            .manifest
            .get("combined_file")
            .and_then(|v| v.as_str())
        {
            println!("combined: {combined_file}");
        }
        if let Some(source_files) = session
            .manifest
            .get("source_files")
            .and_then(|v| v.as_object())
        {
            println!("sources: {}", source_files.len());
            for (name, path) in source_files {
                println!("  {name}: {}", path.as_str().unwrap_or(""));
            }
        }
    }
    Ok(())
}

/// Load every session under `log_dir` that has a `manifest.json`, newest id first.
pub(crate) fn load_sessions(log_dir: &Path) -> Result<Vec<SessionRecord>> {
    let mut sessions = Vec::new();
    if !log_dir.exists() {
        return Ok(sessions);
    }

    for entry in
        std::fs::read_dir(log_dir).with_context(|| format!("read {}", log_dir.display()))?
    {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&manifest_path)
                .with_context(|| format!("read {}", manifest_path.display()))?,
        )
        .with_context(|| format!("parse {}", manifest_path.display()))?;
        let id = manifest
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or_else(|| {
                dir.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        sessions.push(SessionRecord { id, dir, manifest });
    }

    sessions.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(sessions)
}

/// Resolve a session by exact id or unique id prefix.
pub(crate) fn resolve_session(log_dir: &Path, session_id: &str) -> Result<SessionRecord> {
    let matches: Vec<_> = load_sessions(log_dir)?
        .into_iter()
        .filter(|session| session.id == session_id || session.id.starts_with(session_id))
        .collect();

    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => anyhow::bail!("session not found: {session_id}"),
        _ => anyhow::bail!("ambiguous session id prefix: {session_id}"),
    }
}

/// Extract markers from parsed JSON, supporting both wrapper and bare-array formats.
pub(crate) fn extract_markers(parsed: &serde_json::Value) -> Vec<serde_json::Value> {
    // 1) Top-level array  [ {...}, ... ]
    if let Some(arr) = parsed.as_array() {
        return arr.clone();
    }
    // 2) Wrapper object  { "session_id": "...", "markers": [...] }
    if let Some(arr) = parsed.get("markers").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    Vec::new()
}

/// Load markers from a session directory's `markers.json`. Missing file → empty.
pub(crate) fn load_markers_file(session_dir: &Path) -> Result<Vec<serde_json::Value>> {
    let path = session_dir.join("markers.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(extract_markers(&parsed))
}

/// Count markers in a session without surfacing parse errors (returns 0).
pub(crate) fn count_markers_in_session(session_dir: &Path) -> usize {
    let path = session_dir.join("markers.json");
    if !path.exists() {
        return 0;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return 0,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    extract_markers(&parsed).len()
}

/// Handle `sessions marker list/show`.
fn cmd_session_marker(command: MarkerCommand) -> Result<()> {
    match command {
        MarkerCommand::List {
            session_id,
            dir,
            json,
            search,
            pane,
        } => list_markers(&dir, &session_id, json, search, pane),
        MarkerCommand::Show {
            session_id,
            marker_index,
            dir,
            json,
        } => show_marker(&dir, &session_id, marker_index, json),
    }
}

fn list_markers(
    dir: &Path,
    session_id: &str,
    json: bool,
    search: Option<String>,
    pane: Option<String>,
) -> Result<()> {
    let session = resolve_session(dir, session_id)?;
    let all_markers = load_markers_file(&session.dir)?;

    if json && search.is_none() && pane.is_none() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": session.id,
                "markers": all_markers,
            }))?
        );
        return Ok(());
    }

    // Apply filters while preserving original 1-based indexes.
    // Missing fields do NOT match (no false positives).
    // --search is case-insensitive.
    let search_lower = search.as_ref().map(|s| s.to_lowercase());
    let filtered: Vec<(usize, &serde_json::Value)> = all_markers
        .iter()
        .enumerate()
        .filter(|(_, m)| marker_matches(m, &search_lower, &pane))
        .collect();

    if json {
        let json_markers: Vec<serde_json::Value> = filtered
            .iter()
            .map(|(idx, m)| {
                let mut entry = serde_json::json!({
                    "index": idx + 1,
                });
                if let Some(obj) = m.as_object() {
                    for (k, v) in obj {
                        entry[k] = v.clone();
                    }
                }
                entry
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": session.id,
                "markers": json_markers,
            }))?
        );
    } else {
        println!("Session: {}", session.id);
        println!("Markers: {}", filtered.len());
        println!();
        for (orig_idx, m) in &filtered {
            let pane_id = m.get("paneId").and_then(|v| v.as_str()).unwrap_or("?");
            let line = m.get("lineIdx").and_then(|v| v.as_u64()).unwrap_or(0);
            let end_idx = m.get("endIdx").and_then(|v| v.as_u64());
            let desc = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let num_ts = m.get("numTs").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let lines_str = format_line_range(line, end_idx);
            println!("  {}. [{}] {}", orig_idx + 1, pane_id, lines_str);
            println!("     {}", desc);
            println!("     numTs={}", num_ts);
            println!();
        }
    }
    Ok(())
}

fn show_marker(dir: &Path, session_id: &str, marker_index: usize, json: bool) -> Result<()> {
    let session = resolve_session(dir, session_id)?;
    let all_markers = load_markers_file(&session.dir)?;

    if marker_index == 0 || marker_index > all_markers.len() {
        anyhow::bail!(
            "marker index {marker_index} out of range (session has {} markers)",
            all_markers.len()
        );
    }

    let m = &all_markers[marker_index - 1];

    if json {
        println!("{}", serde_json::to_string_pretty(m)?);
    } else {
        let pane_id = m.get("paneId").and_then(|v| v.as_str()).unwrap_or("?");
        let line = m.get("lineIdx").and_then(|v| v.as_u64()).unwrap_or(0);
        let end_idx = m.get("endIdx").and_then(|v| v.as_u64());
        let desc = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let num_ts = m.get("numTs").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let created = m.get("createdAt").and_then(|v| v.as_str()).unwrap_or("");
        let lines_str = match end_idx {
            Some(end) if end != line => format!("{}-{}", line, end),
            _ => format!("{}", line),
        };
        println!("Marker {}", marker_index);
        println!("  Pane:        {}", pane_id);
        println!("  Lines:       {}", lines_str);
        println!("  Description: {}", desc);
        println!("  Timestamp:   {}", num_ts);
        println!("  Created:     {}", created);
    }
    Ok(())
}

/// `lines {l}-{end}` for a range, `line {l}` for a single line (list view).
fn format_line_range(line: u64, end_idx: Option<u64>) -> String {
    match end_idx {
        Some(end) if end != line => format!("lines {}-{}", line, end),
        _ => format!("line {}", line),
    }
}

/// Does a marker match the optional (lowercased) search text and pane filter?
/// Missing fields never match (no false positives).
fn marker_matches(
    m: &serde_json::Value,
    search_lower: &Option<String>,
    pane: &Option<String>,
) -> bool {
    if let Some(pat) = search_lower {
        match m.get("description").and_then(|v| v.as_str()) {
            Some(desc) => {
                if !desc.to_lowercase().contains(pat) {
                    return false;
                }
            }
            None => return false, // missing field doesn't match
        }
    }
    if let Some(pane_filter) = pane {
        match m.get("paneId").and_then(|v| v.as_str()) {
            Some(pid) => {
                if pid != pane_filter.as_str() {
                    return false;
                }
            }
            None => return false, // missing field doesn't match
        }
    }
    true
}

#[derive(Debug)]
struct EventsFilters {
    severity: Option<String>,
    source: Option<String>,
    contains: Option<String>,
    regex: Option<Regex>,
    limit: Option<usize>,
}

impl EventsFilters {
    fn compile(
        severity: Option<String>,
        source: Option<String>,
        contains: Option<String>,
        regex: Option<String>,
        limit: Option<usize>,
    ) -> Result<Self> {
        Ok(Self {
            severity,
            source,
            contains,
            regex: regex.map(|pat| Regex::new(&pat)).transpose()?,
            limit,
        })
    }

    fn matches(&self, event: &serde_json::Value) -> bool {
        if let Some(severity) = &self.severity {
            if event.get("severity").and_then(|v| v.as_str()) != Some(severity.as_str()) {
                return false;
            }
        }
        if let Some(source) = &self.source {
            if event.get("source_id").and_then(|v| v.as_str()) != Some(source.as_str()) {
                return false;
            }
        }
        let message = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(contains) = &self.contains {
            if !message.contains(contains) {
                return false;
            }
        }
        if let Some(regex) = &self.regex {
            if !regex.is_match(message) {
                return false;
            }
        }
        true
    }
}

fn show_session_events(
    dir: &Path,
    session_id: &str,
    filters: EventsFilters,
    json: bool,
) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let session = resolve_session(dir, session_id)?;
    let path = session
        .manifest
        .get("events_file")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| session.dir.join("events.jsonl"));
    if !path.exists() {
        return Ok(());
    }
    let file = std::fs::File::open(&path)
        .with_context(|| format!("open events file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut printed = 0usize;

    for line_result in reader.lines() {
        let line = line_result.with_context(|| format!("read {}", path.display()))?;
        let event: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !filters.matches(&event) {
            continue;
        }
        printed += 1;
        if json {
            println!("{line}");
        } else {
            let ts = event
                .get("timestamp_iso")
                .or_else(|| event.get("timestamp"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let severity = event
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("info");
            let source = event
                .get("source_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let name = event
                .get("event_id")
                .and_then(|v| v.as_str())
                .unwrap_or("event");
            let message = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
            println!("{ts}  {severity:<5}  {source:<16}  {name}: {message}");
        }
        if filters.limit.is_some_and(|limit| printed >= limit) {
            break;
        }
    }
    Ok(())
}

struct SearchFilters {
    sessions: Vec<String>,
    job: Option<String>,
    sources: Vec<String>,
    kind: Option<String>,
    from: Option<DateTime<FixedOffset>>,
    to: Option<DateTime<FixedOffset>>,
    contains: Option<String>,
    regex: Option<Regex>,
    src_port: Option<u16>,
    dst_port: Option<u16>,
    ip: Option<String>,
    limit: Option<usize>,
    count: bool,
}

impl SearchFilters {
    #[allow(clippy::too_many_arguments)]
    fn compile(
        sessions: Vec<String>,
        job: Option<String>,
        sources: Vec<String>,
        kind: Option<String>,
        from: Option<String>,
        to: Option<String>,
        contains: Option<String>,
        regex: Option<String>,
        src_port: Option<u16>,
        dst_port: Option<u16>,
        ip: Option<String>,
        limit: Option<usize>,
        count: bool,
    ) -> Result<Self> {
        Ok(Self {
            sessions,
            job,
            sources,
            kind,
            from: from.as_deref().map(parse_search_time).transpose()?,
            to: to.as_deref().map(parse_search_time).transpose()?,
            contains,
            regex: regex.map(|pat| Regex::new(&pat)).transpose()?,
            src_port,
            dst_port,
            ip,
            limit,
            count,
        })
    }

    fn matches_session(&self, session: &SessionRecord) -> bool {
        if !self.sessions.is_empty()
            && !self
                .sessions
                .iter()
                .any(|prefix| session.id == *prefix || session.id.starts_with(prefix))
        {
            return false;
        }
        if let Some(job) = &self.job {
            let session_job = session.manifest.get("job_id").and_then(|v| v.as_str());
            if session_job != Some(job.as_str()) {
                return false;
            }
        }
        true
    }

    fn matches_entry(&self, entry: &serde_json::Value) -> bool {
        if !self.sources.is_empty() {
            let source_id = entry.get("source_id").and_then(|v| v.as_str());
            if !self
                .sources
                .iter()
                .any(|source| Some(source.as_str()) == source_id)
            {
                return false;
            }
        }
        if let Some(kind) = &self.kind {
            let source_kind = entry.get("source_kind").and_then(|v| v.as_str());
            if source_kind != Some(kind.as_str()) {
                return false;
            }
        }
        if let Some(contains) = &self.contains {
            let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
            if !message.contains(contains) {
                return false;
            }
        }
        if let Some(regex) = &self.regex {
            let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
            if !regex.is_match(message) {
                return false;
            }
        }
        if self.from.is_some() || self.to.is_some() {
            let timestamp = match entry.get("timestamp_iso").and_then(|v| v.as_str()) {
                Some(value) => match parse_search_time(value) {
                    Ok(ts) => ts,
                    Err(_) => return false,
                },
                None => return false,
            };
            if let Some(from) = self.from {
                if timestamp < from {
                    return false;
                }
            }
            if let Some(to) = self.to {
                if timestamp > to {
                    return false;
                }
            }
        }
        if let Some(src_port) = self.src_port {
            if entry.get("src_port").and_then(|v| v.as_u64()) != Some(src_port as u64) {
                return false;
            }
        }
        if let Some(dst_port) = self.dst_port {
            if entry.get("dst_port").and_then(|v| v.as_u64()) != Some(dst_port as u64) {
                return false;
            }
        }
        if let Some(ip) = &self.ip {
            let src_ip = entry.get("src_ip").and_then(|v| v.as_str());
            let dst_ip = entry.get("dst_ip").and_then(|v| v.as_str());
            if src_ip != Some(ip.as_str()) && dst_ip != Some(ip.as_str()) {
                return false;
            }
        }
        true
    }
}

fn parse_search_time(raw: &str) -> Result<DateTime<FixedOffset>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt);
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, fmt) {
            if let Some(local_dt) = Local.from_local_datetime(&naive).single() {
                return Ok(local_dt.fixed_offset());
            }
        }
    }
    anyhow::bail!("invalid time {raw:?} (use RFC3339 or local wall-clock like 2026-07-03T09:00:00)")
}

fn search_sessions(dir: &Path, filters: SearchFilters) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let sessions = load_sessions(dir)?;
    let mut matches = 0usize;

    for session in sessions
        .iter()
        .filter(|session| filters.matches_session(session))
    {
        let path = match manifest_combined_file(session) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);
        for line_result in reader.lines() {
            let line = match line_result {
                Ok(line) => line,
                Err(_) => continue,
            };
            let entry: serde_json::Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if !filters.matches_entry(&entry) {
                continue;
            }
            matches += 1;
            if !filters.count {
                println!("{line}");
            }
            if filters.limit.is_some_and(|limit| matches >= limit) {
                if filters.count {
                    println!("{matches}");
                }
                return Ok(());
            }
        }
    }

    if filters.count {
        println!("{matches}");
    }
    Ok(())
}

fn manifest_combined_file(session: &SessionRecord) -> Result<PathBuf> {
    session
        .manifest
        .get("combined_file")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("manifest missing combined_file"))
}

fn show_session_combined(
    dir: &Path,
    session_id: &str,
    follow: bool,
    lines: Option<usize>,
) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Duration;

    let session = resolve_session(dir, session_id)?;
    let path = manifest_combined_file(&session)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let selected = if let Some(count) = lines {
        let all: Vec<&str> = text.lines().collect();
        let start = all.len().saturating_sub(count);
        all[start..].join("\n")
    } else {
        text.trim_end_matches('\n').to_string()
    };
    if !selected.is_empty() {
        println!("{selected}");
    }
    if !follow {
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut pos = file.metadata()?.len();
    loop {
        let len = file.metadata()?.len();
        if len < pos {
            pos = 0;
        }
        if len > pos {
            file.seek(SeekFrom::Start(pos))?;
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            print!("{buf}");
            std::io::stdout().flush()?;
            pos = len;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn manifest_source_files(session: &SessionRecord) -> Result<HashMap<String, String>> {
    let source_files = session
        .manifest
        .get("source_files")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("manifest missing source_files"))?;

    Ok(source_files
        .iter()
        .filter_map(|(name, path)| path.as_str().map(|path| (name.clone(), path.to_string())))
        .collect())
}

pub(crate) fn export_session_html(session: &SessionRecord, output: PathBuf) -> Result<()> {
    let source_files = manifest_source_files(session)?;
    let tabs = session
        .manifest
        .get("tabs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let pane_labels = session
        .manifest
        .get("pane_labels")
        .and_then(|v| v.as_object())
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let timestamp_mode = session
        .manifest
        .get("timestamp_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("absolute")
        .to_string();
    let first_log_at = session
        .manifest
        .get("first_log_at")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let frontend_dir = std::env::current_dir()?.join("frontend");

    let exporter = SessionExporter::new(
        output.clone(),
        source_files,
        tabs,
        pane_labels,
        frontend_dir,
        timestamp_mode,
        first_log_at,
    );
    exporter.export()?;
    println!("{}", output.display());
    Ok(())
}

pub(crate) fn export_session_raw(session: &SessionRecord, output: PathBuf) -> Result<()> {
    let source_files = manifest_source_files(session)?;
    let mut merged = String::new();
    for (source, path) in source_files {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        for line in content.lines() {
            merged.push_str(&source);
            merged.push('\t');
            merged.push_str(line);
            merged.push('\n');
        }
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, merged)?;
    println!("{}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn temp_log_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "embed-log-cli-sessions-{}-{nanos}-{counter}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_markers(root: &Path, session_id: &str, markers: &[serde_json::Value]) {
        let dir = root.join(session_id);
        std::fs::create_dir_all(&dir).unwrap();
        let body = serde_json::json!({
            "session_id": session_id,
            "markers": markers,
        });
        std::fs::write(
            dir.join("markers.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
    }

    fn write_test_session(root: &Path, id: &str) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("main__dut__session.log");
        let combined_path = dir.join("combined.jsonl");
        std::fs::write(&log_path, "[2026-06-13 00:00:00.000] boot\n").unwrap();
        std::fs::write(
            &combined_path,
            "{\"source_id\":\"dut\",\"message\":\"boot\"}\n{\"source_id\":\"dut\",\"message\":\"next\"}\n",
        )
        .unwrap();
        let manifest = serde_json::json!({
            "session_id": id,
            "session_dir": dir.display().to_string(),
            "started_at": "2026-06-13T00:00:00+00:00",
            "timestamp_mode": "absolute",
            "tabs": [{ "label": "Main", "panes": ["dut"] }],
            "pane_labels": { "dut": "DUT" },
            "source_files": { "dut": log_path.display().to_string() },
            "combined_file": combined_path.display().to_string(),
            "html_status": "pending",
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    // ------------------  Marker loading tests  ------------------

    #[test]
    fn marker_list_prints_all_markers() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 10, "description": "boot started"}),
                serde_json::json!({"paneId": "DUT_UART", "lineIdx": 42, "description": "fatal error"}),
            ],
        );
        assert_eq!(load_markers_file(&root.join("s1")).unwrap().len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_no_file_returns_empty() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        assert!(load_markers_file(&root.join("s1")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_empty_array_returns_empty() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(&root, "s1", &[]);
        assert!(load_markers_file(&root.join("s1")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_unknown_session_is_error() {
        let root = temp_log_dir();
        let err = resolve_session(&root, "nonexistent").unwrap_err();
        assert!(err.to_string().contains("not found"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_list_malformed_json_is_error() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        std::fs::write(root.join("s1").join("markers.json"), "not valid json {{").unwrap();
        assert!(load_markers_file(&root.join("s1")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_load_bare_array_format() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        std::fs::write(
            root.join("s1").join("markers.json"),
            serde_json::to_string_pretty(&serde_json::json!([
                {"paneId": "DUT_UART", "lineIdx": 1, "description": "bare"}
            ]))
            .unwrap(),
        )
        .unwrap();
        let markers = load_markers_file(&root.join("s1")).unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0]["description"], "bare");
        std::fs::remove_dir_all(root).unwrap();
    }

    // ------------------  Marker filter tests  ------------------

    #[test]
    fn marker_filter_search_case_insensitive_and_missing_excluded() {
        let markers = [
            serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1, "description": "Boot Started"}),
            serde_json::json!({"paneId": "DUT_UART", "lineIdx": 2, "description": "fatal error: PANIC"}),
            serde_json::json!({"paneId": "DUT_UART", "lineIdx": 3}), // no description
        ];
        let pat = Some("fatal".to_string());
        let f: Vec<_> = markers
            .iter()
            .filter(|m| marker_matches(m, &pat, &None))
            .collect();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0]["lineIdx"], 2);

        let pat = Some("boot".to_string());
        let f: Vec<_> = markers
            .iter()
            .filter(|m| marker_matches(m, &pat, &None))
            .collect();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0]["lineIdx"], 1);
    }

    #[test]
    fn marker_filter_pane_missing_field_excluded() {
        let markers = [
            serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1, "description": "a"}),
            serde_json::json!({"lineIdx": 2, "description": "b"}), // no paneId
        ];
        let pane = Some("DUT_UART".to_string());
        let f: Vec<_> = markers
            .iter()
            .enumerate()
            .filter(|(_, m)| marker_matches(m, &None, &pane))
            .collect();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].0, 0);
    }

    #[test]
    fn marker_filter_no_filters_matches_all() {
        let markers = [
            serde_json::json!({"paneId": "A", "lineIdx": 1}),
            serde_json::json!({"paneId": "B", "lineIdx": 2}),
        ];
        let f: Vec<_> = markers
            .iter()
            .filter(|m| marker_matches(m, &None, &None))
            .collect();
        assert_eq!(f.len(), 2);
    }

    // ------------------  Line-range formatting  ------------------

    #[test]
    fn format_line_range_single_line() {
        assert_eq!(format_line_range(10, None), "line 10");
        assert_eq!(format_line_range(10, Some(10)), "line 10");
    }

    #[test]
    fn format_line_range_span() {
        assert_eq!(format_line_range(42, Some(45)), "lines 42-45");
    }

    // ------------------  Session listing / export  ------------------

    #[test]
    fn sessions_list_marker_count() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1})],
        );
        write_test_session(&root, "s2");
        assert_eq!(count_markers_in_session(&root.join("s1")), 1);
        assert_eq!(count_markers_in_session(&root.join("s2")), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sessions_list_with_markers_filter() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(
            &root,
            "s1",
            &[serde_json::json!({"paneId": "DUT_UART", "lineIdx": 1})],
        );
        write_test_session(&root, "s2");
        let sessions = load_sessions(&root).unwrap();
        let with_markers: Vec<_> = sessions
            .iter()
            .filter(|s| count_markers_in_session(&s.dir) > 0)
            .collect();
        assert_eq!(with_markers.len(), 1);
        assert_eq!(with_markers[0].id, "s1");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_combined_file_reads_manifest_path() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        let session = resolve_session(&root, "s1").unwrap();
        let path = manifest_combined_file(&session).unwrap();
        assert!(path.ends_with("combined.jsonl"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_filters_match_structured_entry() {
        let filters = SearchFilters::compile(
            vec!["s1".to_string()],
            Some("job-1".to_string()),
            vec!["dut".to_string()],
            Some("network_capture".to_string()),
            Some("2026-07-03T09:00:00+00:00".to_string()),
            Some("2026-07-03T15:00:00+00:00".to_string()),
            Some("panic".to_string()),
            Some("panic|fatal".to_string()),
            Some(49152),
            Some(5683),
            Some("127.0.0.1".to_string()),
            None,
            false,
        )
        .unwrap();
        let session = SessionRecord {
            id: "s1".to_string(),
            dir: PathBuf::from("/tmp/s1"),
            manifest: serde_json::json!({"job_id": "job-1"}),
        };
        let entry = serde_json::json!({
            "source_id": "dut",
            "source_kind": "network_capture",
            "timestamp_iso": "2026-07-03T10:00:00+00:00",
            "message": "panic in worker",
            "src_port": 49152,
            "dst_port": 5683,
            "src_ip": "127.0.0.1",
            "dst_ip": "127.0.0.1"
        });
        assert!(filters.matches_session(&session));
        assert!(filters.matches_entry(&entry));
    }

    #[test]
    fn parse_search_time_accepts_local_wall_clock() {
        let parsed = parse_search_time("2026-07-03T09:00:00").unwrap();
        assert_eq!(
            parsed.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-07-03 09:00:00"
        );
    }

    #[test]
    fn sessions_resolve_prefix_and_raw_export() {
        let root = temp_log_dir();
        let session_dir = write_test_session(&root, "2026-06-13_00-00-00");
        let session = resolve_session(&root, "2026-06-13").unwrap();
        assert_eq!(session.id, "2026-06-13_00-00-00");

        let output = session_dir.join("merged.raw.log");
        export_session_raw(&session, output.clone()).unwrap();
        let merged = std::fs::read_to_string(output).unwrap();
        assert!(merged.contains("dut\t[2026-06-13 00:00:00.000] boot"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_session_ambiguous_prefix_is_error() {
        let root = temp_log_dir();
        write_test_session(&root, "2026-06-13_00-00-00");
        write_test_session(&root, "2026-06-13_01-00-00");
        let err = resolve_session(&root, "2026-06-13").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
