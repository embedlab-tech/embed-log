//! `embed-log sessions` — inspect and export recorded sessions.

use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use clap::{Subcommand, ValueEnum};
use regex::Regex;

use embed_log_core::config::{load_config, resolve_logs_root};
use embed_log_core::naming::slugify;
use embed_log_core::postprocess::{dedupe_entry, denoise_message, elapsed_time};
use embed_log_core::session::SessionExporter;

use crate::commands::daemon::{http_post_json, resolve_mutating_endpoint};
use crate::util::open_url_in_default_browser;

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
    /// Rotate a running server to a new titled experiment session.
    New {
        /// Human-readable experiment title, preserved verbatim in the manifest.
        #[arg(long)]
        title: String,
        /// Registered daemon name. Defaults to EMBED_LOG_INSTANCE or the only instance.
        #[arg(long, conflicts_with = "url")]
        instance: Option<String>,
        /// Explicit unregistered HTTP endpoint.
        #[arg(long)]
        url: Option<String>,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Import an offline, absolutely timestamped log file into a saved session.
    Import {
        session_id: String,
        /// External log file to copy into the session.
        #[arg(long)]
        file: PathBuf,
        /// New source id. Defaults to a slug derived from the file name.
        #[arg(long)]
        source: Option<String>,
        /// Label of the new tab.
        #[arg(long)]
        tab: String,
        /// Display label for the imported pane. Defaults to --source.
        #[arg(long)]
        label: Option<String>,
        #[command(flatten)]
        log_dir: LogDirArgs,
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
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
    /// Open a session's self-contained HTML report in the default browser.
    Open {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
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
        /// Include redundant materialized merge records from legacy sessions.
        #[arg(long)]
        include_materialized_merges: bool,
    },
    /// Read a bounded page from the session-global combined sequence.
    Read {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
        /// Return only records from this source while keeping a global cursor.
        #[arg(long)]
        source: Option<String>,
        /// Return records with a global sequence greater than this cursor.
        #[arg(long, conflicts_with = "last")]
        after: Option<u64>,
        /// Return the final N matching records instead of forward pagination.
        #[arg(long, conflicts_with_all = ["after", "limit"])]
        last: Option<usize>,
        /// Maximum records returned by forward pagination.
        #[arg(long, default_value_t = DEFAULT_READ_LIMIT)]
        limit: usize,
        /// Maximum UTF-8 bytes printed as evidence (including metadata).
        #[arg(long, default_value_t = DEFAULT_EVIDENCE_BYTES)]
        max_bytes: usize,
        /// Maximum UTF-8 bytes in one rendered message.
        #[arg(long, default_value_t = DEFAULT_MESSAGE_BYTES)]
        max_message_bytes: usize,
        /// Timestamp shown in compact records.
        #[arg(long, value_enum, default_value_t = TimeDisplay::Relative)]
        time: TimeDisplay,
        /// Emit the compact structured envelope instead of concise text.
        #[arg(long)]
        json: bool,
        /// Include redundant materialized merge records from legacy sessions.
        #[arg(long)]
        include_materialized_merges: bool,
    },
    /// Read bounded cross-source context around a sequence.
    Around {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
        /// Target session-global sequence.
        #[arg(long)]
        sequence: u64,
        /// Number of combined records before the target.
        #[arg(long, default_value_t = 10)]
        before: usize,
        /// Number of combined records after the target.
        #[arg(long, default_value_t = 20)]
        after: usize,
        /// Maximum UTF-8 bytes printed as evidence (including metadata).
        #[arg(long, default_value_t = DEFAULT_EVIDENCE_BYTES)]
        max_bytes: usize,
        /// Maximum UTF-8 bytes in one rendered message.
        #[arg(long, default_value_t = DEFAULT_MESSAGE_BYTES)]
        max_message_bytes: usize,
        /// Timestamp shown in compact records.
        #[arg(long, value_enum, default_value_t = TimeDisplay::Relative)]
        time: TimeDisplay,
        /// Emit the compact structured envelope instead of concise text.
        #[arg(long)]
        json: bool,
        /// Include redundant materialized merge records from legacy sessions.
        #[arg(long)]
        include_materialized_merges: bool,
    },
    /// Show a token-efficient overview of one session (recommended first call for agents).
    Summary {
        session_id: String,
        #[command(flatten)]
        log_dir: LogDirArgs,
        #[arg(long)]
        json: bool,
        /// Include redundant materialized merge records from legacy sessions.
        #[arg(long)]
        include_materialized_merges: bool,
    },
    /// Search combined JSONL across sessions with structured filters.
    #[command(
        long_about = "Search all session combined.jsonl files under a log directory.\n\nExamples:\n  embed-log sessions search --dir logs --source DUT\n  embed-log sessions search --dir logs --source DUT --from 2026-07-03T09:00:00 --to 2026-07-03T15:00:00\n  embed-log sessions search --dir logs --job nightly-42 --kind udp --contains timeout\n  embed-log sessions search --dir logs --contains panic --regex 'ERROR|WARN'\n\nTime filters accept RFC3339 (with timezone) or local wall-clock forms like 2026-07-03T09:00:00 or 2026-07-03 09:00:00."
    )]
    Search {
        #[command(flatten)]
        log_dir: Box<LogDirArgs>,
        /// Restrict to session ids or unique prefixes. Repeatable.
        #[arg(long = "session")]
        sessions: Vec<String>,
        /// Restrict to sessions whose manifest has this job_id.
        #[arg(long)]
        job: Option<String>,
        /// Restrict to one or more source_id values. Repeatable.
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Restrict to source_kind (uart, udp, or file).
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
        /// Stop after printing this many matching entries (the first N). Conflicts with --last.
        #[arg(long)]
        limit: Option<usize>,
        /// Keep only the last N matching entries. Conflicts with --limit.
        #[arg(long)]
        last: Option<usize>,
        /// Print only the number of matches.
        #[arg(long)]
        count: bool,
        /// Emit the compact structured envelope instead of concise text.
        #[arg(long)]
        json: bool,
        /// Print N lines of context (before and after) around each match. Conflicts with --count and --last.
        #[arg(short = 'C', long)]
        context: Option<usize>,
        /// Print N lines of context before each match. Conflicts with --count and --last.
        #[arg(short = 'B', long = "before-context")]
        before_context: Option<usize>,
        /// Print N lines of context after each match. Conflicts with --count and --last.
        #[arg(short = 'A', long = "after-context")]
        after_context: Option<usize>,
        /// Include redundant materialized merge records from legacy sessions.
        #[arg(long)]
        include_materialized_merges: bool,
    },
}

impl SessionsCommand {
    pub(crate) fn machine_output(&self) -> bool {
        match self {
            Self::New { json, .. }
            | Self::Import { json, .. }
            | Self::List { json, .. }
            | Self::Info { json, .. }
            | Self::Summary { json, .. } => *json,
            Self::Read { json, .. } | Self::Around { json, .. } | Self::Search { json, .. } => {
                *json
            }
            Self::Open { .. } | Self::Export { .. } | Self::Combined { .. } => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExportFormat {
    Html,
    Raw,
    /// Lossless, structurally deduplicated `combined.jsonl` — same information,
    /// pure duplicate fields removed and session/source-constant fields hoisted
    /// to a one-time header instead of repeated per line. Not to be confused
    /// with `--format mini-jsonl` on search/combined, which is a
    /// smaller, lossy, per-line rendering — this is a whole-session, lossless
    /// export meant for handing off to another tool for offline analysis.
    JsonlDeduped,
}

/// Output format shared by `sessions search` and `sessions combined`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Default)]
pub(crate) enum OutputFormat {
    #[default]
    Jsonl,
    Compact,
    MiniJsonl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TimeDisplay {
    Relative,
    Absolute,
}

// Reader output intentionally has only two modes: concise text and --json.
// Raw stored objects remain available through session export/diagnostic paths.
#[derive(Debug, Clone)]
pub(crate) struct SessionRecord {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: serde_json::Value,
}

/// Dispatch `embed-log sessions`.
pub(crate) fn cmd_sessions(command: SessionsCommand) -> Result<()> {
    match command {
        SessionsCommand::New {
            title,
            instance,
            url,
            json,
        } => create_titled_session(instance.as_deref(), url.as_deref(), &title, json),
        SessionsCommand::Import {
            session_id,
            file,
            source,
            tab,
            label,
            log_dir,
            json,
        } => crate::commands::session_import::import_session(
            &session_id,
            &log_dir,
            &file,
            source.as_deref(),
            &tab,
            label.as_deref(),
            json,
        ),
        SessionsCommand::List {
            log_dir,
            json,
            limit,
            with_markers,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            list_sessions(&dir, json, limit, with_markers)
        }
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
            include_materialized_merges,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_session_combined(
                &dir,
                &session_id,
                follow,
                lines,
                format,
                include_materialized_merges,
            )
        }
        SessionsCommand::Read {
            session_id,
            log_dir,
            source,
            after,
            last,
            limit,
            max_bytes,
            max_message_bytes,
            time,
            json,
            include_materialized_merges,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            read_session(
                &dir,
                &session_id,
                ReadOptions {
                    source,
                    after,
                    last,
                    limit,
                    max_bytes,
                    max_message_bytes,
                    time,
                    json,
                    include_materialized_merges,
                },
            )
        }
        SessionsCommand::Around {
            session_id,
            log_dir,
            sequence,
            before,
            after,
            max_bytes,
            max_message_bytes,
            time,
            json,
            include_materialized_merges,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            around_session(
                &dir,
                &session_id,
                AroundOptions {
                    sequence,
                    before,
                    after,
                    max_bytes,
                    max_message_bytes,
                    time,
                    json,
                    include_materialized_merges,
                },
            )
        }
        SessionsCommand::Summary {
            session_id,
            log_dir,
            json,
            include_materialized_merges,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            show_session_summary(&dir, &session_id, json, include_materialized_merges)
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
            limit,
            last,
            count,
            json,
            context,
            before_context,
            after_context,
            include_materialized_merges,
        } => {
            if from.is_some() && since.is_some() {
                anyhow::bail!("cannot combine --from with --since; pick one");
            }
            if limit.is_some() && last.is_some() {
                anyhow::bail!("cannot combine --limit with --last; pick one");
            }
            let has_context =
                context.is_some() || before_context.is_some() || after_context.is_some();
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
            // Search is an evidence reader: avoid an unbounded default while
            // retaining --last and --count's distinct whole-stream behavior.
            let limit = if last.is_none() && !count {
                limit.or(Some(DEFAULT_SEARCH_LIMIT))
            } else {
                limit
            };
            let filters = SearchFilters::compile(
                sessions,
                job,
                sources,
                kind,
                from,
                to,
                contains,
                regex,
                limit,
                count,
                include_materialized_merges,
            )?;
            if has_context {
                let before = before_context.or(context).unwrap_or(0);
                let after = after_context.or(context).unwrap_or(0);
                search_sessions_with_context(&dir, filters, json, before, after)
            } else if let Some(last) = last {
                search_sessions_last_n(&dir, filters, json, last)
            } else {
                search_sessions(&dir, filters, json)
            }
        }
        SessionsCommand::Open {
            session_id,
            log_dir,
        } => {
            let dir = resolve_sessions_dir(&log_dir)?;
            let session = resolve_session(&dir, &session_id)?;
            let html = session.dir.join("session.html");
            // Explicit open always refreshes through the canonical exporter so
            // stale or pre-atomic reports are repaired before the browser opens.
            export_session_html(&session, html.clone())?;
            let path = html.canonicalize().unwrap_or(html);
            open_url_in_default_browser(&path.display().to_string())
                .context("open session report in default browser")?;
            println!("opened {}", path.display());
            Ok(())
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

fn create_titled_session(
    instance: Option<&str>,
    url: Option<&str>,
    title: &str,
    json: bool,
) -> Result<()> {
    validate_session_title(title)?;
    let (_, endpoint) = resolve_mutating_endpoint(instance, url)?;
    let response = http_post_json(
        &endpoint,
        "/api/session/rotate",
        &serde_json::json!({ "title": title }),
    )?;
    if response.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        anyhow::bail!(
            "session rotation failed: {}",
            response
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown backend error")
        );
    }
    let session = response
        .get("session")
        .context("rotation response omitted session")?;
    let session_id = session
        .get("id")
        .and_then(|value| value.as_str())
        .context("rotation response omitted session id")?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": true,
                "session_id": session_id,
                "title": title,
                "session": session,
            }))?
        );
    } else {
        println!("new session: {session_id}");
        println!("  title: {title}");
        println!("  endpoint: {endpoint}");
    }
    Ok(())
}

fn validate_session_title(title: &str) -> Result<()> {
    if title.trim().is_empty() {
        anyhow::bail!("--title must not be empty");
    }
    if title.chars().count() > 120 {
        anyhow::bail!("--title must not exceed 120 characters");
    }
    if slugify(title.trim()).is_empty() {
        anyhow::bail!("--title must contain a letter or number");
    }
    Ok(())
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

const MAX_BOUNDED_RECORDS: usize = 1_000;
const DEFAULT_READ_LIMIT: usize = 50;
const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_EVIDENCE_BYTES: usize = 16 * 1024;
const MAX_EVIDENCE_BYTES: usize = 64 * 1024;
const MIN_EVIDENCE_BYTES: usize = 256;
const DEFAULT_MESSAGE_BYTES: usize = 4 * 1024;

fn is_materialized_merge(record: &serde_json::Value) -> bool {
    record.get("source_kind").and_then(|value| value.as_str()) == Some("merge")
}

fn manifest_merge_members(session: &SessionRecord, merge_name: &str) -> Option<Vec<String>> {
    session
        .manifest
        .get("merges")
        .and_then(|value| value.as_array())
        .and_then(|merges| {
            merges.iter().find_map(|merge| {
                if merge.get("name").and_then(|value| value.as_str()) != Some(merge_name) {
                    return None;
                }
                Some(
                    merge
                        .get("of")
                        .and_then(|value| value.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect(),
                )
            })
        })
}

fn requested_source_ids(session: &SessionRecord, source: Option<&str>) -> Option<Vec<String>> {
    source.map(|source| {
        manifest_merge_members(session, source).unwrap_or_else(|| vec![source.to_string()])
    })
}

struct ReadOptions {
    source: Option<String>,
    after: Option<u64>,
    last: Option<usize>,
    limit: usize,
    max_bytes: usize,
    max_message_bytes: usize,
    time: TimeDisplay,
    json: bool,
    include_materialized_merges: bool,
}

struct AroundOptions {
    sequence: u64,
    before: usize,
    after: usize,
    max_bytes: usize,
    max_message_bytes: usize,
    time: TimeDisplay,
    json: bool,
    include_materialized_merges: bool,
}

fn read_session(dir: &Path, session_id: &str, options: ReadOptions) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let cap = options.last.unwrap_or(options.limit);
    validate_evidence_bytes(options.max_bytes, options.max_message_bytes)?;
    anyhow::ensure!(cap > 0, "--limit/--last must be greater than zero");
    anyhow::ensure!(
        cap <= MAX_BOUNDED_RECORDS,
        "--limit/--last must not exceed {MAX_BOUNDED_RECORDS}"
    );
    let session = resolve_session(dir, session_id)?;
    if let Some(source) = options.source.as_deref() {
        let is_legacy_merge = session
            .manifest
            .get("pane_kinds")
            .and_then(|value| value.get(source))
            .and_then(|value| value.as_str())
            == Some("merge")
            && manifest_merge_members(&session, source).is_none();
        anyhow::ensure!(
            !is_legacy_merge || options.include_materialized_merges,
            "legacy session does not store members for virtual source {source:?}; select its physical sources or pass --include-materialized-merges to read the old redundant records"
        );
    }
    let path = manifest_combined_file(&session)?;
    let file = std::fs::File::open(&path)
        .with_context(|| format!("open combined file {}", path.display()))?;
    let mut selected = VecDeque::new();
    let mut matching_count = 0usize;
    let mut invalid_records = 0usize;
    let mut previous_sequence = None;
    let mut max_sequence = 0u64;
    let mut available_sources = std::collections::BTreeSet::new();
    let mut forward_truncated = false;
    let requested_sources = requested_source_ids(&session, options.source.as_deref());
    if let Some(merges) = session
        .manifest
        .get("merges")
        .and_then(|value| value.as_array())
    {
        for merge in merges {
            if let Some(name) = merge.get("name").and_then(|value| value.as_str()) {
                available_sources.insert(name.to_string());
            }
        }
    }

    for line_result in BufReader::new(file).lines() {
        let line = line_result.with_context(|| format!("read {}", path.display()))?;
        let record: serde_json::Value = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(_) => {
                invalid_records += 1;
                continue;
            }
        };
        let sequence = validated_sequence(&record, previous_sequence, &session.id)?;
        previous_sequence = Some(sequence);
        max_sequence = sequence;
        if let Some(source) = record.get("source_id").and_then(|value| value.as_str()) {
            available_sources.insert(source.to_string());
        }
        if options.after.is_some_and(|after| sequence <= after) {
            continue;
        }
        if !options.include_materialized_merges && is_materialized_merge(&record) {
            continue;
        }
        if requested_sources.as_ref().is_some_and(|sources| {
            record
                .get("source_id")
                .and_then(|value| value.as_str())
                .map_or(true, |source| {
                    !sources.iter().any(|candidate| candidate == source)
                })
        }) {
            continue;
        }
        matching_count += 1;
        if options.last.is_some() {
            if selected.len() == cap {
                selected.pop_front();
            }
            selected.push_back(record);
        } else if selected.len() < cap {
            selected.push_back(record);
        } else {
            forward_truncated = true;
            break;
        }
    }
    if let Some(source) = options.source.as_deref() {
        anyhow::ensure!(
            available_sources.contains(source),
            "unknown source {source:?}; valid sources: {}",
            available_sources.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    if let Some(after) = options.after {
        anyhow::ensure!(
            after <= max_sequence,
            "cursor {after} is beyond the final sequence {max_sequence} in session {:?}",
            session.id
        );
    }
    let records = selected.into_iter().collect::<Vec<_>>();
    let truncated = options
        .last
        .map_or(forward_truncated, |_| matching_count > records.len());
    let next_cursor = if forward_truncated {
        records
            .last()
            .and_then(|record| record.get("sequence"))
            .and_then(|value| value.as_u64())
            .unwrap_or(options.after.unwrap_or(0))
    } else {
        max_sequence
    };
    render_bounded_records(
        &session,
        records,
        options.time,
        options.json,
        truncated,
        next_cursor,
        invalid_records,
        None,
        options.max_bytes,
        options.max_message_bytes,
    )
}

fn around_session(dir: &Path, session_id: &str, options: AroundOptions) -> Result<()> {
    use std::io::{BufRead, BufReader};

    validate_evidence_bytes(options.max_bytes, options.max_message_bytes)?;
    let context_size = options
        .before
        .checked_add(options.after)
        .and_then(|size| size.checked_add(1))
        .context("context size overflow")?;
    anyhow::ensure!(
        context_size <= MAX_BOUNDED_RECORDS,
        "--before + --after + target must not exceed {MAX_BOUNDED_RECORDS} records"
    );
    let session = resolve_session(dir, session_id)?;
    let target_sequence = options.sequence;
    let path = manifest_combined_file(&session)?;
    let file = std::fs::File::open(&path)
        .with_context(|| format!("open combined file {}", path.display()))?;
    let mut before = VecDeque::with_capacity(options.before);
    let mut records = Vec::with_capacity(options.before + options.after + 1);
    let mut found = false;
    let mut after_count = 0usize;
    let mut invalid_records = 0usize;
    let mut previous_sequence = None;

    for line_result in BufReader::new(file).lines() {
        let line = line_result.with_context(|| format!("read {}", path.display()))?;
        let record: serde_json::Value = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(_) => {
                invalid_records += 1;
                continue;
            }
        };
        let sequence = validated_sequence(&record, previous_sequence, &session.id)?;
        previous_sequence = Some(sequence);
        if !options.include_materialized_merges && is_materialized_merge(&record) {
            continue;
        }
        if !found {
            if sequence == target_sequence {
                records.extend(before.drain(..));
                records.push(record);
                found = true;
            } else {
                if before.len() == options.before && options.before > 0 {
                    before.pop_front();
                }
                if options.before > 0 {
                    before.push_back(record);
                }
            }
        } else if after_count < options.after {
            records.push(record);
            after_count += 1;
            if after_count == options.after {
                break;
            }
        }
    }
    anyhow::ensure!(
        found,
        "sequence {target_sequence} does not exist in session {:?}",
        session.id
    );
    let next_cursor = records
        .last()
        .and_then(|record| record.get("sequence"))
        .and_then(|value| value.as_u64())
        .unwrap_or(target_sequence);
    render_bounded_records(
        &session,
        records,
        options.time,
        options.json,
        false,
        next_cursor,
        invalid_records,
        Some(serde_json::json!({
            "sequence": target_sequence,
        })),
        options.max_bytes,
        options.max_message_bytes,
    )
}

fn validated_sequence(
    record: &serde_json::Value,
    previous: Option<u64>,
    session_id: &str,
) -> Result<u64> {
    let sequence = record
        .get("sequence")
        .and_then(|value| value.as_u64())
        .with_context(|| {
            format!(
                "session {session_id:?} contains records without global sequence; capture a new session with the current Embed-log version"
            )
        })?;
    if let Some(previous) = previous {
        let expected = previous
            .checked_add(1)
            .context("stored sequence exhausted")?;
        anyhow::ensure!(
            sequence == expected,
            "session {session_id:?} has a sequence gap or reorder: {previous} followed by {sequence}"
        );
    } else {
        anyhow::ensure!(
            sequence == 1,
            "session {session_id:?} must begin at sequence 1, found {sequence}"
        );
    }
    Ok(sequence)
}

#[allow(clippy::too_many_arguments)]
fn validate_evidence_bytes(max_bytes: usize, max_message_bytes: usize) -> Result<()> {
    anyhow::ensure!(
        (MIN_EVIDENCE_BYTES..=MAX_EVIDENCE_BYTES).contains(&max_bytes),
        "--max-bytes must be between {MIN_EVIDENCE_BYTES} and {MAX_EVIDENCE_BYTES}"
    );
    anyhow::ensure!(
        max_message_bytes > 0 && max_message_bytes <= max_bytes,
        "--max-message-bytes must be between 1 and --max-bytes"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_bounded_records(
    session: &SessionRecord,
    records: Vec<serde_json::Value>,
    time: TimeDisplay,
    json_output: bool,
    truncated: bool,
    next_cursor: u64,
    invalid_records: usize,
    target: Option<serde_json::Value>,
    max_bytes: usize,
    max_message_bytes: usize,
) -> Result<()> {
    let rendered: Vec<_> = records
        .iter()
        .map(|record| {
            let (message, clipped) = bounded_message(record, max_message_bytes);
            (record, message, clipped)
        })
        .collect();
    let mut visible = rendered.len();
    loop {
        let omitted = visible < rendered.len();
        let more = truncated || omitted;
        let cursor = if omitted {
            rendered[..visible]
                .last()
                .and_then(|(record, _, _)| record.get("sequence"))
                .and_then(|value| value.as_u64())
                .unwrap_or(next_cursor)
        } else {
            next_cursor
        };
        let clipped = rendered[..visible]
            .iter()
            .filter(|(_, _, clipped)| *clipped)
            .count();
        let text_header = format!("@session={} next={cursor} count={visible} more={} invalid={invalid_records} clipped={clipped}", session.id, u8::from(more));
        let output = if json_output {
            let mut output = serde_json::json!({
                "session_id": session.id,
                "fields": ["time", "sequence", "source", "index", "message"],
                "records": rendered[..visible].iter().map(|(record, message, _)| compact_tuple_with_message(record, time, message)).collect::<Vec<_>>(),
                "next_cursor": cursor,
                "truncated": more,
                "invalid_records": invalid_records,
                "count": visible,
                "clipped": clipped,
            });
            if let Some(target) = &target {
                output["target"] = target.clone();
            }
            serde_json::to_string(&output)?
        } else {
            let mut output = text_header;
            for (record, message, _) in &rendered[..visible] {
                output.push('\n');
                output.push_str(&compact_text_with_message(record, time, message));
            }
            output
        };
        if output.len() <= max_bytes || visible == 0 {
            println!("{output}");
            return Ok(());
        }
        visible -= 1;
    }
}

fn compact_tuple(record: &serde_json::Value, time: TimeDisplay) -> serde_json::Value {
    compact_tuple_with_message(record, time, &compact_message(record))
}

fn compact_tuple_with_message(
    record: &serde_json::Value,
    time: TimeDisplay,
    message: &str,
) -> serde_json::Value {
    serde_json::Value::Array(vec![
        serde_json::json!(compact_time(record, time).unwrap_or_default()),
        record
            .get("sequence")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        record
            .get("source_id")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        record
            .get("line_idx")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        serde_json::json!(message),
    ])
}

fn compact_text(record: &serde_json::Value, time: TimeDisplay) -> String {
    compact_text_with_message(record, time, &compact_message(record))
}

fn compact_text_with_message(
    record: &serde_json::Value,
    time: TimeDisplay,
    message: &str,
) -> String {
    let sequence = record
        .get("sequence")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let source = record
        .get("source_id")
        .and_then(|value| value.as_str())
        .map_or("?", |source| source);
    let source = record
        .get("line_idx")
        .and_then(|value| value.as_u64())
        .map_or_else(|| source.to_string(), |index| format!("{source}#{index}"));
    let time = compact_time(record, time).unwrap_or_default();
    format!("{time} seq={sequence} src={source} | {}", message)
}

fn bounded_message(record: &serde_json::Value, max_bytes: usize) -> (String, bool) {
    let message = compact_message(record);
    if message.len() <= max_bytes {
        return (message, false);
    }
    let suffix = format!("… [clipped original_bytes={}]", message.len());
    let keep = max_bytes.saturating_sub(suffix.len());
    let mut end = keep.min(message.len());
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{}", &message[..end], suffix), true)
}

fn compact_message(record: &serde_json::Value) -> String {
    let clock = clock_time(record);
    denoise_message(
        record
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        &clock,
    )
    .replace('\r', "\\r")
    .replace('\n', "\\n")
}

fn compact_time(record: &serde_json::Value, time: TimeDisplay) -> Option<String> {
    match time {
        TimeDisplay::Relative => Some(agent_relative_time(record)),
        TimeDisplay::Absolute => Some(
            record
                .get("timestamp_iso")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
        ),
    }
}

fn agent_relative_time(record: &serde_json::Value) -> String {
    let total_ms = record
        .get("relNum")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
        .max(0.0) as u64;
    let millis = total_ms % 1_000;
    let elapsed_seconds = total_ms / 1_000;
    format!("+{elapsed_seconds}.{millis:03}")
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
    let source = entry
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
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
    let source = entry
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let message = denoise_message(message, &clock);
    match entry.get("line_idx").and_then(|v| v.as_u64()) {
        Some(idx) => format!("{clock} {source}#{idx} {message}"),
        None => format!("{clock} {source} {message}"),
    }
}

/// `{"t","s","i","m"}` — the `--format mini-jsonl` object for a
/// combined/search entry. `t`/`s`/`m` are
/// elapsed-time/shortcoded/denoised, same as `format_compact_entry`.
fn format_mini_entry(entry: &serde_json::Value, codes: &mut ShortcodeTable) -> serde_json::Value {
    let clock = clock_time(entry);
    let source = entry
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let mut mini = serde_json::json!({
        "t": elapsed_time(entry, &clock),
        "s": codes.code_for(source),
        "m": denoise_message(message, &clock),
    });
    if let Some(idx) = entry.get("line_idx").and_then(|v| v.as_u64()) {
        mini["i"] = serde_json::json!(idx);
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

struct SearchFilters {
    sessions: Vec<String>,
    job: Option<String>,
    sources: Vec<String>,
    kind: Option<String>,
    from: Option<DateTime<FixedOffset>>,
    to: Option<DateTime<FixedOffset>>,
    contains: Option<String>,
    regex: Option<Regex>,
    limit: Option<usize>,
    count: bool,
    include_materialized_merges: bool,
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
        limit: Option<usize>,
        count: bool,
        include_materialized_merges: bool,
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
            limit,
            count,
            include_materialized_merges,
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

    fn matches_entry(&self, session: &SessionRecord, entry: &serde_json::Value) -> bool {
        if !self.include_materialized_merges && is_materialized_merge(entry) {
            return false;
        }
        if !self.sources.is_empty() {
            let source_id = entry.get("source_id").and_then(|v| v.as_str());
            let matches_source = self.sources.iter().any(|requested| {
                manifest_merge_members(session, requested).map_or_else(
                    || Some(requested.as_str()) == source_id,
                    |members| {
                        source_id
                            .is_some_and(|source| members.iter().any(|member| member == source))
                    },
                )
            });
            if !matches_source {
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

fn search_sessions(dir: &Path, mut filters: SearchFilters, json: bool) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let sessions = load_sessions(dir)?;
    resolve_latest_session_filter(&mut filters, &sessions);
    let mut matches = 0usize;
    let mut json_records = Vec::new();

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
            if !filters.matches_entry(session, &entry) {
                continue;
            }
            matches += 1;
            if !filters.count {
                if json {
                    json_records.push(compact_tuple(&entry, TimeDisplay::Relative));
                } else {
                    println!("{}", compact_text(&entry, TimeDisplay::Relative));
                }
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
    } else if json {
        println!(
            "{}",
            serde_json::json!({
                "session_id": serde_json::Value::Null,
                "fields": ["time", "sequence", "source", "index", "message"],
                "records": json_records,
                "next_cursor": 0,
                "truncated": false,
                "invalid_records": 0,
            })
        );
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
    _json: bool,
    before: usize,
    after: usize,
) -> Result<()> {
    let sessions = load_sessions(dir)?;
    resolve_latest_session_filter(&mut filters, &sessions);
    let mut match_num = 0usize;

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

        let visible_indices: Vec<usize> = parsed
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                entry.as_ref().and_then(|entry| {
                    (filters.include_materialized_merges || !is_materialized_merge(entry))
                        .then_some(idx)
                })
            })
            .collect();

        for (visible_idx, &idx) in visible_indices.iter().enumerate() {
            let entry = parsed[idx].as_ref().expect("visible entries are parsed");
            if !filters.matches_entry(session, entry) {
                continue;
            }
            match_num += 1;
            let source_id = entry
                .get("source_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            println!(
                "# match {match_num} session={} source={source_id} line={}",
                session.id,
                idx + 1
            );
            let (start, end) = context_window(visible_idx, before, after, visible_indices.len());
            for &raw_idx in &visible_indices[start..=end] {
                let ctx_entry = parsed[raw_idx]
                    .as_ref()
                    .expect("visible entries are parsed");
                let rendered = compact_text(ctx_entry, TimeDisplay::Relative);
                if raw_idx == idx {
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
    json: bool,
    last: usize,
) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let sessions = load_sessions(dir)?;
    resolve_latest_session_filter(&mut filters, &sessions);
    let mut buffer: VecDeque<String> = VecDeque::with_capacity(last.min(4096));

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
            if !filters.matches_entry(session, &entry) {
                continue;
            }
            let rendered = if json {
                serde_json::to_string(&compact_tuple(&entry, TimeDisplay::Relative))?
            } else {
                compact_text(&entry, TimeDisplay::Relative)
            };
            push_bounded(&mut buffer, rendered, last);
        }
    }

    if filters.count {
        println!("{}", buffer.len());
    } else if json {
        let records = buffer
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({"session_id": serde_json::Value::Null, "fields": ["time", "sequence", "source", "index", "message"], "records": records, "next_cursor": 0, "truncated": false, "invalid_records": 0})
        );
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
    include_materialized_merges: bool,
) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Duration;

    let session = resolve_session(dir, session_id)?;
    let path = manifest_combined_file(&session)?;
    let mut codes = ShortcodeTable::default();
    note_elapsed_time_format(format);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let all: Vec<&str> = text
        .lines()
        .filter(|line| {
            include_materialized_merges
                || serde_json::from_str::<serde_json::Value>(line)
                    .map(|entry| !is_materialized_merge(&entry))
                    .unwrap_or(true)
        })
        .collect();
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
            pending.push_str(&buf);
            while let Some(newline_at) = pending.find('\n') {
                let raw_line: String = pending.drain(..=newline_at).collect();
                let raw_line = raw_line.trim_end_matches('\n');
                if raw_line.is_empty() {
                    continue;
                }
                let materialized_merge = serde_json::from_str::<serde_json::Value>(raw_line)
                    .map(|entry| is_materialized_merge(&entry))
                    .unwrap_or(false);
                if include_materialized_merges || !materialized_merge {
                    println!("{}", render_combined_line(raw_line, format, &mut codes));
                }
            }
            std::io::stdout().flush()?;
            pos = len;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn manifest_virtual_merge_names(session: &SessionRecord) -> std::collections::HashSet<String> {
    session
        .manifest
        .get("merges")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|merge| {
            merge
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn manifest_merge_names(session: &SessionRecord) -> std::collections::HashSet<String> {
    session
        .manifest
        .get("pane_kinds")
        .and_then(|value| value.as_object())
        .into_iter()
        .flatten()
        .filter(|(_, kind)| kind.as_str() == Some("merge"))
        .map(|(name, _)| name.clone())
        .collect()
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
    // New manifests can reconstruct virtual panes from members. Older manifests
    // lack merge definitions, so retain their materialized source file in HTML.
    let merge_names = manifest_virtual_merge_names(session);
    let source_files = manifest_source_files(session)?
        .into_iter()
        .filter(|(name, _)| !merge_names.contains(name))
        .collect();
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
    let frontend_plugins = session
        .manifest
        .get("frontend_plugins")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let pane_plugins = session
        .manifest
        .get("pane_plugins")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let plugin_scripts = session
        .manifest
        .get("plugin_scripts")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let merges = session
        .manifest
        .get("merges")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));

    let exporter = SessionExporter::new(
        output.clone(),
        source_files,
        tabs,
        pane_labels,
        frontend_dir,
        timestamp_mode,
        first_log_at,
    )
    .with_combined_file(manifest_combined_file(session)?)
    .with_merges(merges)
    .with_plugins(frontend_plugins, pane_plugins, plugin_scripts)
    .with_markers(markers);
    exporter.export()?;
    if output == session.dir.join("session.html") {
        mark_recorded_html_ready(session, &output)?;
    }
    println!("{}", output.display());
    Ok(())
}

fn mark_recorded_html_ready(session: &SessionRecord, output: &Path) -> Result<()> {
    let manifest_path = session.dir.join("manifest.json");
    let mut manifest = session.manifest.clone();
    let object = manifest
        .as_object_mut()
        .context("session manifest root must be an object")?;
    object.insert(
        "session_html".to_string(),
        serde_json::json!(output.display().to_string()),
    );
    object.insert("html_status".to_string(), serde_json::json!("ready"));
    object.insert(
        "html_updated_at".to_string(),
        serde_json::json!(Local::now().to_rfc3339()),
    );
    object.insert("html_error".to_string(), serde_json::Value::Null);
    object.insert(
        "last_export_reason".to_string(),
        serde_json::json!("recorded_cli"),
    );

    let temp_path = session
        .dir
        .join(format!(".manifest.json.tmp-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut temp = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;
        temp.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        temp.sync_all()?;
        std::fs::rename(&temp_path, &manifest_path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result.with_context(|| format!("update manifest {}", manifest_path.display()))
}

pub(crate) fn export_session_raw(session: &SessionRecord, output: PathBuf) -> Result<()> {
    let merge_names = manifest_merge_names(session);
    let source_files = manifest_source_files(session)?;
    let mut merged = String::new();
    for (source, path) in source_files {
        if merge_names.contains(&source) {
            continue;
        }
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

    let mut source_ids: Vec<String> = pane_kinds
        .keys()
        .chain(pane_labels.keys())
        .cloned()
        .collect();
    source_ids.sort();
    source_ids.dedup();
    source_ids.retain(|source_id| {
        pane_kinds.get(source_id).and_then(|kind| kind.as_str()) != Some("merge")
    });

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
        if is_materialized_merge(&entry) {
            continue;
        }
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
    recent: VecDeque<String>,
}

/// Single pass over `combined.jsonl` computing
/// everything `sessions summary` reports. Kept separate from printing so the
/// aggregation logic is unit-testable without capturing stdout.
fn compute_session_summary(
    session: &SessionRecord,
    include_materialized_merges: bool,
) -> SessionSummary {
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
                if !include_materialized_merges && is_materialized_merge(&entry) {
                    continue;
                }
                let source_id = entry
                    .get("source_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let ts_iso = entry.get("timestamp_iso").and_then(|v| v.as_str());

                let stats = per_source
                    .entry(source_id)
                    .or_insert_with(|| SourceSummary {
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

    let duration = match (overall_first, overall_last) {
        (Some(first), Some(last)) => human_duration(first, last),
        _ => "00:00:00".to_string(),
    };

    SessionSummary {
        job_id,
        started_at,
        duration,
        sources: per_source,
        recent,
    }
}

/// `sessions summary <SESSION_ID>` — a single token-efficient overview: per-source
/// line counts/first/last timestamps and the last 5
/// combined.jsonl lines. Recommended first call for agents inspecting a session.
fn show_session_summary(
    dir: &Path,
    session_id: &str,
    json: bool,
    include_materialized_merges: bool,
) -> Result<()> {
    let session = resolve_session(dir, session_id)?;
    let summary = compute_session_summary(&session, include_materialized_merges);

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

    #[test]
    fn session_title_validation_matches_backend_contract() {
        assert!(validate_session_title("Reconnect #3").is_ok());
        assert!(validate_session_title("   ").is_err());
        assert!(validate_session_title("***").is_err());
        assert!(validate_session_title(&"x".repeat(121)).is_err());
    }

    // ------------------  Marker artifact tests  ------------------

    #[test]
    fn marker_file_loads_all_markers() {
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
    fn marker_file_missing_returns_empty() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        assert!(load_markers_file(&root.join("s1")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_file_empty_array_returns_empty() {
        let root = temp_log_dir();
        write_test_session(&root, "s1");
        write_markers(&root, "s1", &[]);
        assert!(load_markers_file(&root.join("s1")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_file_malformed_json_is_error() {
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
            Some("udp".to_string()),
            Some("2026-07-03T09:00:00+00:00".to_string()),
            Some("2026-07-03T15:00:00+00:00".to_string()),
            Some("panic".to_string()),
            Some("panic|fatal".to_string()),
            None,
            false,
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
            "source_kind": "udp",
            "timestamp_iso": "2026-07-03T10:00:00+00:00",
            "message": "panic in worker"
        });
        assert!(filters.matches_session(&session));
        assert!(filters.matches_entry(&session, &entry));
    }

    #[test]
    fn virtual_source_filter_expands_members_and_legacy_merge_records_are_hidden() {
        let session = SessionRecord {
            id: "s1".to_string(),
            dir: PathBuf::from("/tmp/s1"),
            manifest: serde_json::json!({
                "merges": [{"name":"MCU_LINK","of":["MCU_TX","MCU_RX"]}]
            }),
        };
        let filters = SearchFilters::compile(
            vec![],
            None,
            vec!["MCU_LINK".to_string()],
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
        assert!(filters.matches_entry(
            &session,
            &serde_json::json!({"source_id":"MCU_RX","source_kind":"uart"})
        ));
        assert!(!filters.matches_entry(
            &session,
            &serde_json::json!({"source_id":"MCU_LINK","source_kind":"merge"})
        ));
        assert_eq!(
            manifest_merge_members(&session, "MCU_LINK").unwrap(),
            ["MCU_TX", "MCU_RX"]
        );
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
    fn canonical_recorded_html_export_updates_manifest_status() {
        let root = temp_log_dir();
        let session_dir = write_test_session(&root, "recorded");
        let session = resolve_session(&root, "recorded").unwrap();
        let output = session_dir.join("session.html");
        export_session_html(&session, output.clone()).unwrap();

        let html = std::fs::read_to_string(&output).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(session_dir.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["html_status"], "ready");
        assert_eq!(manifest["last_export_reason"], "recorded_cli");
        assert_eq!(manifest["session_html"], output.display().to_string());
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
            false,
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
    fn bounded_compact_records_toggle_time_and_keep_global_and_local_positions() {
        let entry = serde_json::json!({
            "sequence": 719,
            "source_id": "DUT_UART",
            "line_idx": 428,
            "timestamp_iso": "2026-08-04T10:30:12.453+02:00",
            "relNum": 12_453.0,
            "message": "boot complete",
        });
        assert_eq!(
            compact_text(&entry, TimeDisplay::Relative),
            "+12.453 seq=719 src=DUT_UART#428 | boot complete"
        );
        assert_eq!(
            compact_text(&entry, TimeDisplay::Absolute),
            "2026-08-04T10:30:12.453+02:00 seq=719 src=DUT_UART#428 | boot complete"
        );
        assert_eq!(
            compact_tuple(&entry, TimeDisplay::Relative),
            serde_json::json!(["+12.453", 719, "DUT_UART", 428, "boot complete"])
        );
        assert_eq!(
            agent_relative_time(&serde_json::json!({"relNum":3_723_004.0})),
            "+3723.004"
        );
    }

    #[test]
    fn global_sequence_validation_rejects_legacy_and_reordered_records() {
        assert!(validated_sequence(&serde_json::json!({"message":"legacy"}), None, "s").is_err());
        assert_eq!(
            validated_sequence(&serde_json::json!({"sequence":1}), None, "s").unwrap(),
            1
        );
        assert!(validated_sequence(&serde_json::json!({"sequence":1}), Some(1), "s").is_err());
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
    fn format_mini_entry_includes_bounded_core_fields() {
        let entry = serde_json::json!({
            "source_id": "COAP",
            "message": "udp ...",
            "timestamp_iso": "2026-07-03T12:00:01.123+00:00",
            "line_idx": 42,
        });
        let mut codes = ShortcodeTable::default();
        let mini = format_mini_entry(&entry, &mut codes);
        assert_eq!(mini["t"], "12:00:01.123");
        assert_eq!(mini["s"], "C");
        assert_eq!(mini["i"], 42);
        assert_eq!(mini["m"], "udp ...");
        assert_eq!(mini.as_object().unwrap().len(), 4);
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
    fn summary_counts_sources_and_recent_lines() {
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
        let session = resolve_session(&root, "2026-07-06_14-31-18").unwrap();
        let summary = compute_session_summary(&session, false);
        assert_eq!(summary.sources.len(), 2);
        assert_eq!(summary.sources["PYTEST"].count, 1);
        assert_eq!(summary.duration, "00:10:05");
        assert_eq!(summary.recent.len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_sessions_command_surface_parses_new_flags() {
        use clap::Parser as _;

        for args in [
            ["embed-log", "sessions", "search", "--session", "latest"].as_slice(),
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
