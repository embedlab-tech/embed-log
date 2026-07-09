//! `embed-log sessions` — inspect and export recorded sessions, plus the
//! `sessions marker` sub-subcommands.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use clap::{Subcommand, ValueEnum};
use regex::Regex;

use embed_log_core::config::{load_config, resolve_logs_root};
use embed_log_core::postprocess::{dedupe_entry, denoise_message, elapsed_time};
use embed_log_core::session::SessionExporter;

/// Shared `--dir`/`--config` args for resolving which logs directory a
/// `sessions` command operates on. Flattened into every subcommand so the
/// flags and resolution order are identical everywhere — see
/// [`resolve_sessions_dir`].
#[derive(Clone, Debug, clap::Args)]
pub(crate) struct LogDirArgs {
    /// Logs directory to inspect. Wins over --config/any resolved config. If
    /// omitted, resolved from --config (or the same env-var/default lookup
    /// `run` uses), reading that config's `logs.dir`; falls back to ./logs
    /// if no config file is found.
    #[arg(long, alias = "log-dir")]
    dir: Option<PathBuf>,
    /// Config file to read the logs directory from when --dir is not given.
    /// Defaults to EMBED_LOG_CONFIG_YML_PATH, then embed-log.yml (same as `run`).
    #[arg(short, long)]
    config: Option<PathBuf>,
}

/// Resolve which logs directory a `sessions` command should use. Precedence:
/// 1. `--dir`: used verbatim, no config involved, nothing printed.
/// 2. `--config` (or the same env-var/default lookup `run` uses): if that
///    config file exists, its `logs.dir` resolved via `resolve_logs_root`
///    (the same function `run` uses, so behavior can't drift between them).
/// 3. `./logs`, unchanged from earlier versions, when no config file exists.
///
/// Prints one note to stderr whenever the directory wasn't given explicitly
/// via --dir, so the choice is never silent.
pub(crate) fn resolve_sessions_dir(args: &LogDirArgs) -> Result<PathBuf> {
    if let Some(dir) = &args.dir {
        return Ok(dir.clone());
    }
    let config_path = crate::config::resolve_config_path(args.config.as_ref());
    if config_path.exists() {
        let cfg = load_config(&config_path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", config_path.display()))?;
        let dir = resolve_logs_root(&config_path, &cfg.logs.dir);
        eprintln!(
            "sessions: using logs dir from {}: {}",
            config_path.display(),
            dir.display()
        );
        Ok(dir)
    } else {
        eprintln!(
            "sessions: no --dir given and no config found at {} (pass --dir or --config); defaulting to ./logs",
            config_path.display()
        );
        Ok(PathBuf::from("logs"))
    }
}

/// `embed-log sessions <command>`.
#[derive(Subcommand)]
pub(crate) enum SessionsCommand {
    /// List sessions under a log directory.
    List {
        #[command(flatten)]
        log_dir: LogDirArgs,
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
        #[command(flatten)]
        log_dir: LogDirArgs,
        #[arg(long)]
        json: bool,
    },
    /// Export a session as HTML or raw merged text.
    Export {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ExportFormat::Html)]
        format: ExportFormat,
    },
    /// Print or follow the session-wide combined JSONL stream.
    #[command(visible_alias = "tail-combined")]
    Combined {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
        #[arg(long)]
        follow: bool,
        #[arg(long, alias = "last")]
        lines: Option<usize>,
        /// Output format: jsonl (default), compact, or mini-jsonl.
        #[arg(long, value_enum, default_value_t = OutputFormat::Jsonl)]
        format: OutputFormat,
    },
    /// Print recorded event-detection hits from events.jsonl.
    Events {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
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
        /// Output format: jsonl (default), compact, or mini-jsonl. Ignored when --json is set.
        #[arg(long, value_enum, default_value_t = OutputFormat::Jsonl)]
        format: OutputFormat,
    },
    /// Show a token-efficient overview of one session (recommended first call for agents).
    Summary {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
        #[arg(long)]
        json: bool,
    },
    /// Search combined JSONL across sessions with structured filters.
    #[command(
        long_about = "Search all session combined.jsonl files under a log directory.\n\nExamples:\n  embed-log sessions search --dir logs --source DUT\n  embed-log sessions search --dir logs --source DUT --from 2026-07-03T09:00:00 --to 2026-07-03T15:00:00\n  embed-log sessions search --dir logs --job nightly-42 --kind network_capture --dst-port 5683\n  embed-log sessions search --dir logs --contains panic --regex 'ERROR|WARN'\n\nTime filters accept RFC3339 (with timezone) or local wall-clock forms like 2026-07-03T09:00:00 or 2026-07-03 09:00:00."
    )]
    Search {
        #[command(flatten)]
        log_dir: LogDirArgs,
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
        /// Earliest timestamp expressed as a relative duration (e.g. 10m, 1h, 2d) before now. Conflicts with --from.
        #[arg(long)]
        since: Option<String>,
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
        /// Stop after printing this many matching entries (the first N). Conflicts with --last.
        #[arg(long)]
        limit: Option<usize>,
        /// Keep only the last N matching entries. Conflicts with --limit.
        #[arg(long)]
        last: Option<usize>,
        /// Print only the number of matches.
        #[arg(long)]
        count: bool,
        /// Output format: jsonl (default), compact, or mini-jsonl.
        #[arg(long, value_enum, default_value_t = OutputFormat::Jsonl)]
        format: OutputFormat,
        /// Print N lines of context (before and after) around each match. Conflicts with --count and --last.
        #[arg(short = 'C', long)]
        context: Option<usize>,
        /// Print N lines of context before each match. Conflicts with --count and --last.
        #[arg(short = 'B', long = "before-context")]
        before_context: Option<usize>,
        /// Print N lines of context after each match. Conflicts with --count and --last.
        #[arg(short = 'A', long = "after-context")]
        after_context: Option<usize>,
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
        #[command(flatten)]
        log_dir: LogDirArgs,
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
        #[command(flatten)]
        log_dir: LogDirArgs,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExportFormat {
    Html,
    Raw,
    /// Lossless, structurally deduplicated `combined.jsonl` — same information,
    /// pure duplicate fields removed and session/source-constant fields hoisted
    /// to a one-time header instead of repeated per line. Not to be confused
    /// with `--format mini-jsonl` on search/combined/events, which is a
    /// smaller, lossy, per-line rendering — this is a whole-session, lossless
    /// export meant for handing off to another tool for offline analysis.
    JsonlDeduped,
}

/// Output format shared by `sessions search`, `sessions combined`, and `sessions events`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Default)]
pub(crate) enum OutputFormat {
    #[default]
    Jsonl,
    Compact,
    MiniJsonl,
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
            log_dir,
            json,
            limit,
            with_markers,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            list_sessions(&dir, json, limit, with_markers)
        }
        SessionsCommand::Marker { command } => cmd_session_marker(command),
        SessionsCommand::Info {
            session_id,
            log_dir,
            json,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_session_info(&dir, &session_id, json)
        }
        SessionsCommand::Combined {
            session_id,
            log_dir,
            follow,
            lines,
            format,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_session_combined(&dir, &session_id, follow, lines, format)
        }
        SessionsCommand::Events {
            session_id,
            log_dir,
            json,
            severity,
            source,
            contains,
            regex,
            limit,
            format,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_session_events(
                &dir,
                &session_id,
                EventsFilters::compile(severity, source, contains, regex, limit)?,
                json,
                format,
            )
        }
        SessionsCommand::Summary {
            session_id,
            log_dir,
            json,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_session_summary(&dir, &session_id, json)
        }
        SessionsCommand::Search {
            log_dir,
            sessions,
            job,
            sources,
            kind,
            from,
            to,
            since,
            contains,
            regex,
            src_port,
            dst_port,
            ip,
            limit,
            last,
            count,
            format,
            context,
            before_context,
            after_context,
        } => {
            if from.is_some() && since.is_some() {
                anyhow::bail!("cannot combine --from with --since; pick one");
            }
            if limit.is_some() && last.is_some() {
                anyhow::bail!("cannot combine --limit with --last; pick one");
            }
            let has_context = context.is_some() || before_context.is_some() || after_context.is_some();
            if has_context && count {
                anyhow::bail!("cannot combine context flags (-C/-B/-A) with --count");
            }
            if has_context && last.is_some() {
                anyhow::bail!("cannot combine context flags (-C/-B/-A) with --last; not supported together yet");
            }
            let dir = resolve_sessions_dir(&log_dir)?;
            let from = match since {
                Some(raw) => Some(
                    (Local::now() - parse_duration_shorthand(&raw)?)
                        .fixed_offset()
                        .to_rfc3339(),
                ),
                None => from,
            };
            let filters = SearchFilters::compile(
                sessions, job, sources, kind, from, to, contains, regex, src_port, dst_port, ip,
                limit, count,
            )?;
            if has_context {
                let before = before_context.or(context).unwrap_or(0);
                let after = after_context.or(context).unwrap_or(0);
                search_sessions_with_context(&dir, filters, format, before, after)
            } else if let Some(last) = last {
                search_sessions_last_n(&dir, filters, format, last)
            } else {
                search_sessions(&dir, filters, format)
            }
        }
        SessionsCommand::Export {
            session_id,
            log_dir,
            output,
            format,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
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
                ExportFormat::JsonlDeduped => {
                    let output = output.unwrap_or_else(|| session.dir.join("session.jsonl"));
                    export_session_jsonl_deduped(&session, output)?;
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

/// Resolve a session by exact id, unique id prefix, or the literal `latest`
/// (newest session under `log_dir`).
pub(crate) fn resolve_session(log_dir: &Path, session_id: &str) -> Result<SessionRecord> {
    let sessions = load_sessions(log_dir)?;

    if session_id == "latest" {
        return sessions
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no sessions found under {}", log_dir.display()));
    }

    let matches: Vec<_> = sessions
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
            log_dir,
            json,
            search,
            pane,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            list_markers(&dir, &session_id, json, search, pane)
        }
        MarkerCommand::Show {
            session_id,
            marker_index,
            log_dir,
            json,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_marker(&dir, &session_id, marker_index, json)
        }
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
    format: OutputFormat,
) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let session = resolve_session(dir, session_id)?;
    let path = events_file_path(&session);
    if !path.exists() {
        return Ok(());
    }
    let file = std::fs::File::open(&path)
        .with_context(|| format!("open events file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut printed = 0usize;
    let mut codes = ShortcodeTable::default();
    if !json {
        note_elapsed_time_format(format);
    }

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
            println!("{}", render_event(&event, format, &mut codes));
        }
        if filters.limit.is_some_and(|limit| printed >= limit) {
            break;
        }
    }
    Ok(())
}

fn events_file_path(session: &SessionRecord) -> PathBuf {
    session
        .manifest
        .get("events_file")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| session.dir.join("events.jsonl"))
}

/// `HH:MM:SS.mmm` clock time, preferring `timestamp_iso`, falling back to the
/// raw `timestamp` string field.
fn clock_time(entry: &serde_json::Value) -> String {
    if let Some(iso) = entry.get("timestamp_iso").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(iso) {
            return dt.format("%H:%M:%S%.3f").to_string();
        }
    }
    entry
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// [`CompactionLevel::Ultra`] source-name shortcodes for `--format
/// compact`/`mini-jsonl`: derived from the source's own name — initials of
/// its `_`/`-`-separated words (`COUNTER` -> `C`, `MCU_LINK_RX` -> `MLR`,
/// `NODE-RED-COAP` -> `NRC`) — rather than an arbitrary scan-order letter, so
/// codes are mnemonic and mostly stable across runs (the same source tends
/// to get the same code regardless of when it's first seen). On a collision
/// between two differently-named sources whose initials coincide, falls back
/// to the shortest unique prefix of the full name — source names are already
/// guaranteed unique by config validation (`sources[i].name duplicate` is a
/// load error), so this always terminates. Announces each new mapping to
/// stderr the moment it's assigned, rather than requiring a lookahead pass
/// over data that may be streamed (`combined --follow`) — the legend builds
/// up alongside the output instead of needing to be known upfront.
#[derive(Default)]
struct ShortcodeTable {
    codes: std::collections::HashMap<String, String>,
    used: std::collections::HashSet<String>,
}

impl ShortcodeTable {
    fn code_for(&mut self, source_id: &str) -> String {
        if let Some(code) = self.codes.get(source_id) {
            return code.clone();
        }
        let code = self.assign(source_id);
        eprintln!("sessions: source code {code} = {source_id}");
        self.used.insert(code.clone());
        self.codes.insert(source_id.to_string(), code.clone());
        code
    }

    fn assign(&self, source_id: &str) -> String {
        let initials = Self::initials(source_id);
        if !self.used.contains(&initials) {
            return initials;
        }
        // Collision: widen to progressively longer prefixes of the full name.
        // Char-based (not byte slicing) so this can't panic on a non-ASCII
        // source name.
        let chars: Vec<char> = source_id.chars().collect();
        (2..=chars.len())
            .map(|len| chars[..len].iter().collect::<String>().to_ascii_uppercase())
            .find(|candidate| !self.used.contains(candidate))
            .unwrap_or_else(|| source_id.to_ascii_uppercase())
    }

    /// First letter of each `_`/`-`-separated word, uppercased. A name with
    /// no separators reduces to just its own first letter.
    fn initials(source_id: &str) -> String {
        source_id
            .split(['_', '-'])
            .filter(|segment| !segment.is_empty())
            .filter_map(|segment| segment.chars().next())
            .map(|c| c.to_ascii_uppercase())
            .collect()
    }
}

/// One-time reminder that `--format compact`/`mini-jsonl` show elapsed time,
/// not wall-clock time — call once per invocation before any `Ultra`-level
/// output is printed. `CompactionLevel::Compact` isn't affected (still
/// absolute time), only `Ultra`.
fn note_elapsed_time_format(format: OutputFormat) {
    if matches!(format, OutputFormat::Compact | OutputFormat::MiniJsonl) {
        eprintln!(
            "sessions: times below are elapsed since each entry's own session start \
             (see `sessions summary <id>` for the absolute start time)"
        );
    }
}

/// `1:23.644 A#1234 panic: watchdog reset` — the `--format compact` line for a
/// combined/search entry: `message` is denoised (ANSI/duplicate-timestamp/
/// padding/uptime-counter noise stripped, see `embed_log_core::postprocess`),
/// the timestamp is session-relative elapsed time, and the source is a
/// shortcode (see `ShortcodeTable`) — `--format jsonl` remains the byte-exact
/// escape hatch for anyone who needs the original wall-clock time or names.
fn format_compact_entry(entry: &serde_json::Value, codes: &mut ShortcodeTable) -> String {
    let clock = clock_time(entry);
    let source = entry.get("source_id").and_then(|v| v.as_str()).unwrap_or("?");
    let code = codes.code_for(source);
    let ts = elapsed_time(entry, &clock);
    let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let message = denoise_message(message, &clock);
    match entry.get("line_idx").and_then(|v| v.as_u64()) {
        Some(idx) => format!("{ts} {code}#{idx} {message}"),
        None => format!("{ts} {code} {message}"),
    }
}

/// Absolute-time, full-source-name compact line for `sessions summary`'s tiny
/// "recent" preview (5 lines, no `--format` flag of its own). Deliberately
/// skips shortcodes/elapsed-time (`format_compact_entry`'s `Ultra`-level
/// behavior): a 5-line preview doesn't carry a legend well, and
/// `compute_session_summary` is documented as side-effect-free/pure —
/// `ShortcodeTable::code_for` prints to stderr as a side effect, which would
/// break that contract.
fn format_summary_preview_line(entry: &serde_json::Value) -> String {
    let clock = clock_time(entry);
    let source = entry.get("source_id").and_then(|v| v.as_str()).unwrap_or("?");
    let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let message = denoise_message(message, &clock);
    match entry.get("line_idx").and_then(|v| v.as_u64()) {
        Some(idx) => format!("{clock} {source}#{idx} {message}"),
        None => format!("{clock} {source} {message}"),
    }
}

/// `{"t","s","i","m"}` (plus `src`/`dst`/`len` for packet entries) — the
/// `--format mini-jsonl` object for a combined/search entry. `t`/`s`/`m` are
/// elapsed-time/shortcoded/denoised, same as `format_compact_entry`.
fn format_mini_entry(entry: &serde_json::Value, codes: &mut ShortcodeTable) -> serde_json::Value {
    let clock = clock_time(entry);
    let source = entry.get("source_id").and_then(|v| v.as_str()).unwrap_or("?");
    let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let mut mini = serde_json::json!({
        "t": elapsed_time(entry, &clock),
        "s": codes.code_for(source),
        "m": denoise_message(message, &clock),
    });
    if let Some(idx) = entry.get("line_idx").and_then(|v| v.as_u64()) {
        mini["i"] = serde_json::json!(idx);
    }
    if let (Some(src_ip), Some(dst_ip)) = (
        entry.get("src_ip").and_then(|v| v.as_str()),
        entry.get("dst_ip").and_then(|v| v.as_str()),
    ) {
        let with_port = |ip: &str, port: Option<u64>| match port {
            Some(p) => format!("{ip}:{p}"),
            None => ip.to_string(),
        };
        mini["src"] = serde_json::json!(with_port(
            src_ip,
            entry.get("src_port").and_then(|v| v.as_u64())
        ));
        mini["dst"] = serde_json::json!(with_port(
            dst_ip,
            entry.get("dst_port").and_then(|v| v.as_u64())
        ));
        if let Some(len) = entry.get("payload_len").and_then(|v| v.as_u64()) {
            mini["len"] = serde_json::json!(len);
        }
    }
    mini
}

/// Render one combined/search entry per `--format`. `raw_line` is the original
/// JSONL text, reused verbatim for `Jsonl` so byte-for-byte content is preserved.
fn render_entry(
    entry: &serde_json::Value,
    raw_line: &str,
    format: OutputFormat,
    codes: &mut ShortcodeTable,
) -> String {
    match format {
        OutputFormat::Jsonl => raw_line.to_string(),
        OutputFormat::Compact => format_compact_entry(entry, codes),
        OutputFormat::MiniJsonl => {
            serde_json::to_string(&format_mini_entry(entry, codes)).unwrap_or_default()
        }
    }
}

/// `{ts}  {severity:<5}  {source:<16}  {name}: {message}` — the human-readable
/// line for an event (used for both the default and `--format compact`).
/// `message` is denoised, same as `format_compact_entry`.
fn format_compact_event(event: &serde_json::Value, codes: &mut ShortcodeTable) -> String {
    let clock = clock_time(event);
    let ts = elapsed_time(event, &clock);
    let severity = event
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("info");
    let source = event
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let code = codes.code_for(source);
    let name = event
        .get("event_id")
        .and_then(|v| v.as_str())
        .unwrap_or("event");
    let message = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let message = denoise_message(message, &clock);
    // `code` is a 1-2 char shortcode now, not a full source name — no point
    // padding it to a 16-wide column like the old full-name alignment did.
    format!("{ts}  {severity:<5}  {code}  {name}: {message}")
}

/// `{"t","s","sev","ev","m"}` — the `--format mini-jsonl` object for an event.
/// `t`/`s`/`m` are elapsed-time/shortcoded/denoised, same as
/// `format_compact_entry`.
fn format_mini_event(event: &serde_json::Value, codes: &mut ShortcodeTable) -> serde_json::Value {
    let clock = clock_time(event);
    let source = event.get("source_id").and_then(|v| v.as_str()).unwrap_or("?");
    let message = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
    serde_json::json!({
        "t": elapsed_time(event, &clock),
        "s": codes.code_for(source),
        "sev": event.get("severity").and_then(|v| v.as_str()).unwrap_or("info"),
        "ev": event.get("event_id").and_then(|v| v.as_str()).unwrap_or("event"),
        "m": denoise_message(message, &clock),
    })
}

/// Render one event per `--format`. `Jsonl` (the default) keeps today's
/// human-readable line — `--json` is the separate flag for raw JSONL.
fn render_event(event: &serde_json::Value, format: OutputFormat, codes: &mut ShortcodeTable) -> String {
    match format {
        OutputFormat::MiniJsonl => {
            serde_json::to_string(&format_mini_event(event, codes)).unwrap_or_default()
        }
        OutputFormat::Jsonl | OutputFormat::Compact => format_compact_event(event, codes),
    }
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

/// Parse a relative duration shorthand like `10m`, `1h`, `30s`, `2d` (used by `--since`).
fn parse_duration_shorthand(raw: &str) -> Result<chrono::Duration> {
    if raw.len() < 2 {
        anyhow::bail!("invalid duration {raw:?} (use a number followed by s/m/h/d, e.g. 10m)");
    }
    let (num, unit) = raw.split_at(raw.len() - 1);
    let n: i64 = num
        .parse()
        .with_context(|| format!("invalid duration {raw:?} (use a number followed by s/m/h/d)"))?;
    Ok(match unit {
        "s" => chrono::Duration::seconds(n),
        "m" => chrono::Duration::minutes(n),
        "h" => chrono::Duration::hours(n),
        "d" => chrono::Duration::days(n),
        other => anyhow::bail!("invalid duration unit {other:?} in {raw:?} (use s/m/h/d)"),
    })
}

/// Push `item` onto `buffer`, evicting the oldest entry if it would exceed `cap`.
/// Used to keep only the last N of something (search matches, recent lines)
/// while scanning a stream in a single bounded-memory pass.
fn push_bounded(buffer: &mut VecDeque<String>, item: String, cap: usize) {
    if cap == 0 {
        return;
    }
    if buffer.len() >= cap {
        buffer.pop_front();
    }
    buffer.push_back(item);
}

/// Inclusive `[start, end]` window of `before`/`after` lines around `idx`,
/// clamped to the valid range `[0, len-1]`.
fn context_window(idx: usize, before: usize, after: usize, len: usize) -> (usize, usize) {
    let start = idx.saturating_sub(before);
    let end = (idx + after).min(len.saturating_sub(1));
    (start, end)
}

/// Resolve a literal `"latest"` in `filters.sessions` to the id of the newest
/// session in `sessions` (which is assumed already sorted newest-first).
fn resolve_latest_session_filter(filters: &mut SearchFilters, sessions: &[SessionRecord]) {
    if let Some(pos) = filters.sessions.iter().position(|s| s == "latest") {
        if let Some(newest) = sessions.first() {
            filters.sessions[pos] = newest.id.clone();
        }
    }
}

fn search_sessions(dir: &Path, mut filters: SearchFilters, format: OutputFormat) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let sessions = load_sessions(dir)?;
    resolve_latest_session_filter(&mut filters, &sessions);
    let mut matches = 0usize;
    let mut codes = ShortcodeTable::default();
    if !filters.count {
        note_elapsed_time_format(format);
    }

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
                println!("{}", render_entry(&entry, &line, format, &mut codes));
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

/// `sessions search -C/-B/-A`: like `search_sessions`, but prints `before`/`after`
/// lines of surrounding combined.jsonl context around each match. Reads each
/// session's combined.jsonl fully into memory (same precedent as
/// `show_session_combined`) since context windows need random access to
/// neighboring lines.
fn search_sessions_with_context(
    dir: &Path,
    mut filters: SearchFilters,
    format: OutputFormat,
    before: usize,
    after: usize,
) -> Result<()> {
    let sessions = load_sessions(dir)?;
    resolve_latest_session_filter(&mut filters, &sessions);
    let mut match_num = 0usize;
    let mut codes = ShortcodeTable::default();
    note_elapsed_time_format(format);

    for session in sessions
        .iter()
        .filter(|session| filters.matches_session(session))
    {
        let path = match manifest_combined_file(session) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let lines: Vec<&str> = text.lines().collect();
        let parsed: Vec<Option<serde_json::Value>> = lines
            .iter()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect();

        for (idx, entry_opt) in parsed.iter().enumerate() {
            let Some(entry) = entry_opt else { continue };
            if !filters.matches_entry(entry) {
                continue;
            }
            match_num += 1;
            let source_id = entry.get("source_id").and_then(|v| v.as_str()).unwrap_or("?");
            println!(
                "# match {match_num} session={} source={source_id} line={}",
                session.id,
                idx + 1
            );
            let (start, end) = context_window(idx, before, after, lines.len());
            for (i, line) in lines.iter().enumerate().take(end + 1).skip(start) {
                let Some(ctx_entry) = &parsed[i] else { continue };
                let rendered = render_entry(ctx_entry, line, format, &mut codes);
                if i == idx {
                    println!("{rendered}   << MATCH");
                } else {
                    println!("{rendered}");
                }
            }
            println!();
            if filters.limit.is_some_and(|limit| match_num >= limit) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// `sessions search --last N`: like `search_sessions`, but keeps only the
/// chronologically-last `N` matches in a bounded ring buffer instead of
/// printing (or stopping at) the first `N`. Sessions are walked oldest-first
/// so the buffer's insertion order matches wall-clock order.
fn search_sessions_last_n(
    dir: &Path,
    mut filters: SearchFilters,
    format: OutputFormat,
    last: usize,
) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let sessions = load_sessions(dir)?;
    resolve_latest_session_filter(&mut filters, &sessions);
    let mut buffer: VecDeque<String> = VecDeque::with_capacity(last.min(4096));
    let mut codes = ShortcodeTable::default();
    if !filters.count {
        note_elapsed_time_format(format);
    }

    for session in sessions
        .iter()
        .rev() // oldest-first, so the ring buffer ends up holding the newest matches
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
            push_bounded(&mut buffer, render_entry(&entry, &line, format, &mut codes), last);
        }
    }

    if filters.count {
        println!("{}", buffer.len());
    } else {
        for line in &buffer {
            println!("{line}");
        }
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

/// Render one line of `combined.jsonl` per `--format`. Falls back to the raw
/// line if it isn't valid JSON (defensive; combined.jsonl is append-only and
/// machine-written, but a line caught mid-write during `--follow` could be
/// truncated).
fn render_combined_line(line: &str, format: OutputFormat, codes: &mut ShortcodeTable) -> String {
    match format {
        OutputFormat::Jsonl => line.to_string(),
        _ => match serde_json::from_str::<serde_json::Value>(line) {
            Ok(entry) => render_entry(&entry, line, format, codes),
            Err(_) => line.to_string(),
        },
    }
}

fn show_session_combined(
    dir: &Path,
    session_id: &str,
    follow: bool,
    lines: Option<usize>,
    format: OutputFormat,
) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Duration;

    let session = resolve_session(dir, session_id)?;
    let path = manifest_combined_file(&session)?;
    let mut codes = ShortcodeTable::default();
    note_elapsed_time_format(format);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    let selected = match lines {
        Some(count) => &all[all.len().saturating_sub(count)..],
        None => &all[..],
    };
    for line in selected {
        println!("{}", render_combined_line(line, format, &mut codes));
    }
    if !follow {
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut pos = file.metadata()?.len();
    let mut pending = String::new();
    loop {
        let len = file.metadata()?.len();
        if len < pos {
            pos = 0;
        }
        if len > pos {
            file.seek(SeekFrom::Start(pos))?;
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            if format == OutputFormat::Jsonl {
                print!("{buf}");
            } else {
                pending.push_str(&buf);
                while let Some(newline_at) = pending.find('\n') {
                    let raw_line: String = pending.drain(..=newline_at).collect();
                    let raw_line = raw_line.trim_end_matches('\n');
                    if !raw_line.is_empty() {
                        println!("{}", render_combined_line(raw_line, format, &mut codes));
                    }
                }
            }
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
    let markers = load_markers_file(&session.dir)?;
    let events = load_events_file(&events_file_path(session));
    let event_rules = session
        .manifest
        .get("event_rules")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let exporter = SessionExporter::new(
        output.clone(),
        source_files,
        tabs,
        pane_labels,
        frontend_dir,
        timestamp_mode,
        first_log_at,
    )
    .with_markers(markers)
    .with_events(events, event_rules);
    exporter.export()?;
    println!("{}", output.display());
    Ok(())
}

/// Load events from a session's events.jsonl file. Missing/unreadable file -> empty.
/// Mirrors `SessionManager::load_events` for callers that only have a `SessionRecord`.
fn load_events_file(path: &Path) -> Vec<serde_json::Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
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

/// `sessions export --format jsonl-deduped` — a lossless, structurally
/// deduplicated single-file export of `combined.jsonl`: per-line duplicate
/// fields removed (via `postprocess::dedupe_entry`) and session/source-constant
/// fields (`app_name`, `job_id`, `session_id`, `source_kind`, `source_label`,
/// `tab_labels`) hoisted into a one-time header line instead of repeated on
/// every record. ~48% smaller than the original on a measured real session,
/// with zero information loss — for handing a whole session to another
/// tool/agent for offline analysis.
pub(crate) fn export_session_jsonl_deduped(session: &SessionRecord, output: PathBuf) -> Result<()> {
    use std::io::{BufRead, BufReader, BufWriter, Write};

    let combined_path = manifest_combined_file(session)?;

    // manifest.json's own `app_name` field isn't reliably populated (seen
    // `null` there even when every combined.jsonl record carries a real
    // value) — peek the first record instead.
    let app_name = {
        let file = std::fs::File::open(&combined_path)
            .with_context(|| format!("open {}", combined_path.display()))?;
        BufReader::new(file)
            .lines()
            .next()
            .transpose()?
            .and_then(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
            .and_then(|entry| entry.get("app_name").cloned())
            .unwrap_or(serde_json::Value::Null)
    };
    let job_id = session
        .manifest
        .get("job_id")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let pane_kinds = session
        .manifest
        .get("pane_kinds")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let pane_labels = session
        .manifest
        .get("pane_labels")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let tabs = session
        .manifest
        .get("tabs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut source_ids: Vec<String> = pane_kinds.keys().chain(pane_labels.keys()).cloned().collect();
    source_ids.sort();
    source_ids.dedup();

    let mut sources = serde_json::Map::new();
    for source_id in &source_ids {
        let source_tabs: Vec<&str> = tabs
            .iter()
            .filter(|tab| {
                tab.get("panes")
                    .and_then(|p| p.as_array())
                    .is_some_and(|panes| panes.iter().any(|p| p.as_str() == Some(source_id)))
            })
            .filter_map(|tab| tab.get("label").and_then(|v| v.as_str()))
            .collect();
        sources.insert(
            source_id.clone(),
            serde_json::json!({
                "kind": pane_kinds.get(source_id).cloned().unwrap_or(serde_json::Value::Null),
                "label": pane_labels.get(source_id).cloned().unwrap_or(serde_json::Value::Null),
                "tabs": source_tabs,
            }),
        );
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(
        std::fs::File::create(&output).with_context(|| format!("create {}", output.display()))?,
    );
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&serde_json::json!({
            "kind": "header",
            "session_id": session.id,
            "app_name": app_name,
            "job_id": job_id,
            "sources": sources,
        }))?
    )?;

    const HEADER_COVERED_FIELDS: [&str; 6] = [
        "app_name",
        "job_id",
        "session_id",
        "source_kind",
        "source_label",
        "tab_labels",
    ];
    let file = std::fs::File::open(&combined_path)
        .with_context(|| format!("open {}", combined_path.display()))?;
    for line_result in BufReader::new(file).lines() {
        let line = line_result?;
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let mut deduped = dedupe_entry(&entry);
        if let Some(obj) = deduped.as_object_mut() {
            for field in HEADER_COVERED_FIELDS {
                obj.remove(field);
            }
        }
        writeln!(writer, "{}", serde_json::to_string(&deduped)?)?;
    }

    println!("{}", output.display());
    Ok(())
}

struct SourceSummary {
    count: u64,
    first: Option<String>,
    last: Option<String>,
}

struct SessionSummary {
    job_id: Option<String>,
    started_at: Option<String>,
    duration: String,
    sources: std::collections::BTreeMap<String, SourceSummary>,
    events: std::collections::BTreeMap<String, u64>,
    recent: VecDeque<String>,
}

/// Single pass over `combined.jsonl` (+ `events.jsonl` if present) computing
/// everything `sessions summary` reports. Kept separate from printing so the
/// aggregation logic is unit-testable without capturing stdout.
fn compute_session_summary(session: &SessionRecord) -> SessionSummary {
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader};

    let job_id = session
        .manifest
        .get("job_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let started_at = session
        .manifest
        .get("started_at")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let mut per_source: BTreeMap<String, SourceSummary> = BTreeMap::new();
    let mut recent: VecDeque<String> = VecDeque::with_capacity(5);
    let mut overall_first: Option<DateTime<FixedOffset>> = None;
    let mut overall_last: Option<DateTime<FixedOffset>> = None;

    if let Ok(path) = manifest_combined_file(session) {
        if let Ok(file) = std::fs::File::open(&path) {
            for line_result in BufReader::new(file).lines() {
                let Ok(line) = line_result else { continue };
                let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let source_id = entry
                    .get("source_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let ts_iso = entry.get("timestamp_iso").and_then(|v| v.as_str());

                let stats = per_source.entry(source_id).or_insert_with(|| SourceSummary {
                    count: 0,
                    first: None,
                    last: None,
                });
                stats.count += 1;
                if stats.first.is_none() {
                    stats.first = ts_iso.map(str::to_owned);
                }
                if let Some(ts) = ts_iso {
                    stats.last = Some(ts.to_owned());
                    if let Ok(parsed) = parse_search_time(ts) {
                        if overall_first.map_or(true, |first| parsed < first) {
                            overall_first = Some(parsed);
                        }
                        if overall_last.map_or(true, |last| parsed > last) {
                            overall_last = Some(parsed);
                        }
                    }
                }

                push_bounded_recent(&mut recent, format_summary_preview_line(&entry));
            }
        }
    }

    let mut severity_counts: BTreeMap<String, u64> = BTreeMap::new();
    let events_path = events_file_path(session);
    if events_path.exists() {
        if let Ok(file) = std::fs::File::open(&events_path) {
            for line_result in BufReader::new(file).lines() {
                let Ok(line) = line_result else { continue };
                let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let severity = event
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info")
                    .to_string();
                *severity_counts.entry(severity).or_insert(0) += 1;
            }
        }
    }

    let duration = match (overall_first, overall_last) {
        (Some(first), Some(last)) => human_duration(first, last),
        _ => "00:00:00".to_string(),
    };

    SessionSummary {
        job_id,
        started_at,
        duration,
        sources: per_source,
        events: severity_counts,
        recent,
    }
}

/// `sessions summary <SESSION_ID>` — a single token-efficient overview: per-source
/// line counts/first/last timestamps, event severity counts, and the last 5
/// combined.jsonl lines. Recommended first call for agents inspecting a session.
fn show_session_summary(dir: &Path, session_id: &str, json: bool) -> Result<()> {
    let session = resolve_session(dir, session_id)?;
    let summary = compute_session_summary(&session);

    if json {
        let sources_json: Vec<_> = summary
            .sources
            .iter()
            .map(|(id, s)| {
                serde_json::json!({
                    "source_id": id,
                    "count": s.count,
                    "first": s.first,
                    "last": s.last,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": session.id,
                "job_id": summary.job_id,
                "started_at": summary.started_at,
                "duration": summary.duration,
                "sources": sources_json,
                "events": summary.events,
                "recent": summary.recent,
            }))?
        );
    } else {
        match &summary.job_id {
            Some(job_id) => println!("session: {} job={job_id}", session.id),
            None => println!("session: {}", session.id),
        }
        println!("duration: {}", summary.duration);
        println!("sources:");
        for (id, s) in &summary.sources {
            println!(
                "  {id} count={} first={} last={}",
                s.count,
                s.first.as_deref().unwrap_or("?"),
                s.last.as_deref().unwrap_or("?")
            );
        }
        if !summary.events.is_empty() {
            let parts: Vec<String> = summary
                .events
                .iter()
                .map(|(severity, count)| format!("{severity}={count}"))
                .collect();
            println!("events:");
            println!("  {}", parts.join(" "));
        }
        if !summary.recent.is_empty() {
            println!("recent:");
            for line in &summary.recent {
                println!("  {line}");
            }
        }
    }
    Ok(())
}

/// Push onto a fixed-size (5) "recent lines" ring buffer, evicting the oldest entry.
fn push_bounded_recent(recent: &mut VecDeque<String>, line: String) {
    push_bounded(recent, line, 5);
}

/// `HH:MM:SS` between two timestamps (negative/reversed inputs clamp to zero).
fn human_duration(start: DateTime<FixedOffset>, end: DateTime<FixedOffset>) -> String {
    let total_seconds = (end - start).num_seconds().max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
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

    // ------------------  Phase 1 agent-facing improvements  ------------------

    #[test]
    fn resolve_session_latest_returns_newest() {
        let root = temp_log_dir();
        write_test_session(&root, "2026-07-06_10-00-00");
        write_test_session(&root, "2026-07-06_14-00-00");
        let session = resolve_session(&root, "latest").unwrap();
        assert_eq!(session.id, "2026-07-06_14-00-00");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_session_latest_no_sessions_is_error() {
        let root = temp_log_dir();
        let err = resolve_session(&root, "latest").unwrap_err();
        assert!(err.to_string().contains("no sessions found"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_latest_session_filter_replaces_latest_token() {
        let root = temp_log_dir();
        write_test_session(&root, "2026-07-06_10-00-00");
        write_test_session(&root, "2026-07-06_14-00-00");
        let sessions = load_sessions(&root).unwrap();
        let mut filters = SearchFilters::compile(
            vec!["latest".to_string()],
            None,
            vec![],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap();
        resolve_latest_session_filter(&mut filters, &sessions);
        assert_eq!(filters.sessions, vec!["2026-07-06_14-00-00".to_string()]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_duration_shorthand_accepts_s_m_h_d() {
        assert_eq!(
            parse_duration_shorthand("30s").unwrap(),
            chrono::Duration::seconds(30)
        );
        assert_eq!(
            parse_duration_shorthand("10m").unwrap(),
            chrono::Duration::minutes(10)
        );
        assert_eq!(
            parse_duration_shorthand("1h").unwrap(),
            chrono::Duration::hours(1)
        );
        assert_eq!(
            parse_duration_shorthand("2d").unwrap(),
            chrono::Duration::days(2)
        );
    }

    #[test]
    fn parse_duration_shorthand_rejects_bad_unit() {
        assert!(parse_duration_shorthand("10x").is_err());
        assert!(parse_duration_shorthand("m").is_err());
        assert!(parse_duration_shorthand("").is_err());
    }

    #[test]
    fn format_compact_entry_with_and_without_line_idx() {
        let with_idx = serde_json::json!({
            "source_id": "DUT",
            "message": "panic: watchdog reset",
            "timestamp_iso": "2026-07-03T12:00:01.123+00:00",
            "line_idx": 1234,
        });
        let mut codes = ShortcodeTable::default();
        assert_eq!(
            format_compact_entry(&with_idx, &mut codes),
            "12:00:01.123 D#1234 panic: watchdog reset"
        );

        let without_idx = serde_json::json!({
            "source_id": "DUT",
            "message": "hello",
            "timestamp_iso": "2026-07-03T12:00:01.123+00:00",
        });
        // Same source as above ("DUT") — same table reused, so it keeps code "D".
        assert_eq!(
            format_compact_entry(&without_idx, &mut codes),
            "12:00:01.123 D hello"
        );
    }

    #[test]
    fn format_mini_entry_includes_packet_fields() {
        let packet = serde_json::json!({
            "source_id": "COAP",
            "message": "udp ...",
            "timestamp_iso": "2026-07-03T12:00:01.123+00:00",
            "line_idx": 42,
            "src_ip": "192.168.1.2",
            "src_port": 49152,
            "dst_ip": "224.0.1.187",
            "dst_port": 5683,
            "payload_len": 32,
        });
        let mut codes = ShortcodeTable::default();
        let mini = format_mini_entry(&packet, &mut codes);
        assert_eq!(mini["t"], "12:00:01.123");
        assert_eq!(mini["s"], "C");
        assert_eq!(mini["i"], 42);
        assert_eq!(mini["src"], "192.168.1.2:49152");
        assert_eq!(mini["dst"], "224.0.1.187:5683");
        assert_eq!(mini["len"], 32);
    }

    #[test]
    fn format_compact_entry_denoises_ansi_and_duplicate_timestamp() {
        let entry = serde_json::json!({
            "source_id": "PYTEST",
            "message": "15:41:23.644 [   ERROR] \u{1b}[91mTimeout waiting for event='dcf_edhoc'\u{1b}[0m",
            "timestamp_iso": "2026-07-06T15:41:23.644+02:00",
            "line_idx": 3603,
        });
        let mut codes = ShortcodeTable::default();
        assert_eq!(
            format_compact_entry(&entry, &mut codes),
            "15:41:23.644 P#3603 [ERROR] Timeout waiting for event='dcf_edhoc'"
        );
    }

    #[test]
    fn format_mini_entry_denoises_message() {
        let entry = serde_json::json!({
            "source_id": "RELAY",
            "message": "node outside> \u{1b}[13D\u{1b}[J[00000000] <inf> rv8263: interrupt configured",
            "timestamp_iso": "2026-07-06T14:31:31.877+02:00",
        });
        let mut codes = ShortcodeTable::default();
        assert_eq!(
            format_mini_entry(&entry, &mut codes)["m"],
            "node outside> [00000000] <inf> rv8263: interrupt configured"
        );
    }

    #[test]
    fn format_compact_entry_uses_elapsed_time_and_assigns_distinct_codes() {
        let mut codes = ShortcodeTable::default();
        let a = serde_json::json!({
            "source_id": "PYTEST",
            "message": "hi",
            "timestamp_iso": "2026-07-06T14:31:31.877+02:00",
            "relNum": 83_644.0,
        });
        let b = serde_json::json!({
            "source_id": "COUNTER",
            "message": "hi",
            "timestamp_iso": "2026-07-06T14:31:31.877+02:00",
            "relNum": 1_000.0,
        });
        assert_eq!(format_compact_entry(&a, &mut codes), "1:23.644 P hi");
        assert_eq!(format_compact_entry(&b, &mut codes), "1.000 C hi");
        // Same source seen again later keeps its already-assigned code.
        assert_eq!(format_compact_entry(&a, &mut codes), "1:23.644 P hi");
    }

    #[test]
    fn shortcode_table_collision_falls_back_to_longer_prefix() {
        let mut codes = ShortcodeTable::default();
        // Both reduce to "C" as bare initials — second one must not overwrite the first.
        assert_eq!(codes.code_for("COUNTER"), "C");
        assert_eq!(codes.code_for("CLIENT"), "CL");
        // Repeat calls are stable.
        assert_eq!(codes.code_for("COUNTER"), "C");
        assert_eq!(codes.code_for("CLIENT"), "CL");
    }

    #[test]
    fn shortcode_table_uses_meaningful_initials() {
        let mut codes = ShortcodeTable::default();
        assert_eq!(codes.code_for("COUNTER"), "C");
        assert_eq!(codes.code_for("RELAY"), "R");
        assert_eq!(codes.code_for("MCU_LINK"), "ML");
        assert_eq!(codes.code_for("MCU_LINK_RX"), "MLR");
        assert_eq!(codes.code_for("MCU_LINK_TX"), "MLT");
        assert_eq!(codes.code_for("NODE-RED"), "NR");
        assert_eq!(codes.code_for("NODE-RED-COAP"), "NRC");
    }

    #[test]
    fn context_window_clamps_to_bounds() {
        assert_eq!(context_window(5, 2, 2, 10), (3, 7));
        assert_eq!(context_window(0, 5, 0, 10), (0, 0));
        assert_eq!(context_window(9, 0, 5, 10), (9, 9));
    }

    #[test]
    fn push_bounded_keeps_only_last_n() {
        let mut buffer = VecDeque::new();
        for i in 0..5 {
            push_bounded(&mut buffer, i.to_string(), 3);
        }
        assert_eq!(
            buffer,
            VecDeque::from(["2".to_string(), "3".to_string(), "4".to_string()])
        );
    }

    #[test]
    fn push_bounded_zero_cap_keeps_nothing() {
        let mut buffer = VecDeque::new();
        push_bounded(&mut buffer, "x".to_string(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn human_duration_formats_hh_mm_ss() {
        let start = DateTime::parse_from_rfc3339("2026-07-03T12:00:00+00:00").unwrap();
        let end = DateTime::parse_from_rfc3339("2026-07-03T12:14:22+00:00").unwrap();
        assert_eq!(human_duration(start, end), "00:14:22");
    }

    #[test]
    fn human_duration_clamps_negative_to_zero() {
        let start = DateTime::parse_from_rfc3339("2026-07-03T12:00:00+00:00").unwrap();
        let end = DateTime::parse_from_rfc3339("2026-07-03T11:00:00+00:00").unwrap();
        assert_eq!(human_duration(start, end), "00:00:00");
    }

    #[test]
    fn summary_counts_sources_and_events() {
        let root = temp_log_dir();
        let dir = write_test_session(&root, "2026-07-06_14-31-18");
        std::fs::write(
            dir.join("combined.jsonl"),
            concat!(
                "{\"source_id\":\"COUNTER\",\"message\":\"boot\",\"timestamp_iso\":\"2026-07-06T14:31:18+02:00\"}\n",
                "{\"source_id\":\"PYTEST\",\"message\":\"Timeout waiting for event='dcf_edhoc'\",\"timestamp_iso\":\"2026-07-06T14:41:23+02:00\"}\n",
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("events.jsonl"),
            "{\"severity\":\"error\",\"source_id\":\"PYTEST\",\"message\":\"timeout\"}\n",
        )
        .unwrap();

        let session = resolve_session(&root, "2026-07-06_14-31-18").unwrap();
        let summary = compute_session_summary(&session);
        assert_eq!(summary.sources.len(), 2);
        assert_eq!(summary.sources["PYTEST"].count, 1);
        assert_eq!(summary.duration, "00:10:05");
        assert_eq!(summary.events["error"], 1);
        assert_eq!(summary.recent.len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_sessions_command_surface_parses_new_flags() {
        use clap::Parser as _;

        for args in [
            [
                "embed-log",
                "sessions",
                "search",
                "--session",
                "latest",
                "--format",
                "compact",
            ]
            .as_slice(),
            ["embed-log", "sessions", "search", "--since", "10m"].as_slice(),
            ["embed-log", "sessions", "search", "--last", "50"].as_slice(),
            ["embed-log", "sessions", "search", "-C", "5"].as_slice(),
            ["embed-log", "sessions", "search", "-B", "2", "-A", "3"].as_slice(),
            ["embed-log", "sessions", "summary", "latest"].as_slice(),
            [
                "embed-log",
                "sessions",
                "combined",
                "latest",
                "--format",
                "mini-jsonl",
            ]
            .as_slice(),
        ] {
            crate::Cli::try_parse_from(args).unwrap();
        }
    }

    // ------------------  --dir/--config logs-dir resolution  ------------------

    #[test]
    fn resolve_sessions_dir_explicit_dir_wins_over_config() {
        let root = temp_log_dir();
        // Deliberately not a loadable config — proves --dir short-circuits
        // before any config is even read.
        let config_path = root.join("embed-log.yml");
        std::fs::write(&config_path, "not valid yaml {{").unwrap();
        let args = LogDirArgs {
            dir: Some(PathBuf::from("explicit-dir")),
            config: Some(config_path),
        };
        assert_eq!(
            resolve_sessions_dir(&args).unwrap(),
            PathBuf::from("explicit-dir")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_sessions_dir_reads_logs_dir_from_config() {
        let root = temp_log_dir();
        let config_path = root.join("embed-log.yml");
        std::fs::write(&config_path, "logs:\n  dir: some/relative/path\n").unwrap();
        let args = LogDirArgs {
            dir: None,
            config: Some(config_path.clone()),
        };
        let expected = resolve_logs_root(&config_path, "some/relative/path");
        assert_eq!(resolve_sessions_dir(&args).unwrap(), expected);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_sessions_dir_falls_back_to_bare_logs_when_no_config() {
        let root = temp_log_dir();
        let args = LogDirArgs {
            dir: None,
            config: Some(root.join("nonexistent.yml")),
        };
        assert_eq!(resolve_sessions_dir(&args).unwrap(), PathBuf::from("logs"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_session_jsonl_deduped_hoists_constants_to_header() {
        let root = temp_log_dir();
        let dir = root.join("s1");
        std::fs::create_dir_all(&dir).unwrap();
        let combined_path = dir.join("combined.jsonl");
        std::fs::write(
            &combined_path,
            concat!(
                "{\"data\":\"boot\",\"message\":\"boot\",\"absNum\":1.0,\"timestamp_num\":1.0,\"absTs\":\"07-06 00:00:00.000\",\"timestamp\":\"07-06 00:00:00.000\",\"timestamp_iso\":\"2026-07-06T00:00:00+00:00\",\"app_name\":\"app\",\"job_id\":null,\"session_id\":\"s1\",\"source_id\":\"DUT\",\"source_kind\":\"uart\",\"source_label\":\"DUT\",\"tab_labels\":[\"Main\"],\"line_idx\":0}\n",
                "{\"data\":\"next\",\"message\":\"next\",\"absNum\":2.0,\"timestamp_num\":2.0,\"absTs\":\"07-06 00:00:01.000\",\"timestamp\":\"07-06 00:00:01.000\",\"timestamp_iso\":\"2026-07-06T00:00:01+00:00\",\"app_name\":\"app\",\"job_id\":null,\"session_id\":\"s1\",\"source_id\":\"DUT\",\"source_kind\":\"uart\",\"source_label\":\"DUT\",\"tab_labels\":[\"Main\"],\"line_idx\":1}\n",
            ),
        )
        .unwrap();
        let manifest = serde_json::json!({
            "session_id": "s1",
            "job_id": null,
            "combined_file": combined_path.display().to_string(),
            "pane_kinds": {"DUT": "uart"},
            "pane_labels": {"DUT": "DUT"},
            "tabs": [{"label": "Main", "panes": ["DUT"]}],
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let session = resolve_session(&root, "s1").unwrap();
        let output = dir.join("session.jsonl");
        export_session_jsonl_deduped(&session, output.clone()).unwrap();

        let text = std::fs::read_to_string(&output).unwrap();
        let mut lines = text.lines();
        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header["session_id"], "s1");
        assert_eq!(header["app_name"], "app");
        assert_eq!(header["sources"]["DUT"]["kind"], "uart");
        assert_eq!(header["sources"]["DUT"]["label"], "DUT");
        assert_eq!(header["sources"]["DUT"]["tabs"][0], "Main");

        let body: Vec<serde_json::Value> =
            lines.map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(body.len(), 2);
        for entry in &body {
            for field in [
                "data",
                "timestamp_num",
                "timestamp",
                "app_name",
                "job_id",
                "session_id",
                "source_kind",
                "source_label",
                "tab_labels",
            ] {
                assert!(entry.get(field).is_none(), "unexpected field {field}");
            }
        }
        assert_eq!(body[0]["message"], "boot");
        assert_eq!(body[0]["absNum"], 1.0);
        assert_eq!(body[0]["timestamp_iso"], "2026-07-06T00:00:00+00:00");

        std::fs::remove_dir_all(root).unwrap();
    }
}
