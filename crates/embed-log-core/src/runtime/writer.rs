//! The session writer: the single task that commits every log record.
//!
//! All sources feed one [`Writer`]. It owns each source's session log file,
//! the session's `first_log_at`, and the per-source line counters, and it is
//! the only place that appends to `combined.jsonl`, the replay buffer and the
//! live broadcast. Records are therefore committed in one global order without
//! a commit lock.
//!
//! Anything that must change writer state between records, such as session
//! rotation, runs inside the task through [`WriterHandle::exec`].

use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};

use chrono::{DateTime, Local};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::error;

use crate::clock::{format_relative_millis, SessionClock};
use crate::levels::LevelDetector;
use crate::models::{Ansi, LogEntry, TimestampMode};
use crate::net::live::{LiveEvent, LiveMessage};
use crate::net::watch::TemporaryWatch;
use crate::net::ws_server::SourceRuntimeStats;
use crate::session::SessionManager;

pub(crate) const REPLAY_BUFFER_SIZE: usize = 5000;

/// Flush the buffered source log periodically rather than once per line.
/// This keeps the file reasonably durable while preserving BufWriter batching
/// under high-volume capture.
pub(crate) const LOG_FLUSH_EVERY_LINES: usize = 256;

/// Source-constant metadata stored with every `combined.jsonl` record.
#[derive(Clone, Default)]
pub(crate) struct SourceRuntimeMeta {
    pub source_id: String,
    pub source_label: String,
    pub source_kind: String,
    pub tab_labels: Vec<String>,
    pub app_name: String,
    pub job_id: Option<String>,
}

/// State shared between the writer and the HTTP/WebSocket server.
pub(crate) struct WriterShared {
    pub broadcast_tx: broadcast::Sender<LiveMessage>,
    pub replay: Arc<Mutex<VecDeque<LiveMessage>>>,
    pub session_manager: Arc<Mutex<SessionManager>>,
    pub watches: Arc<RwLock<HashMap<String, TemporaryWatch>>>,
    pub ts_mode: TimestampMode,
}

/// One source feeding the writer.
pub(crate) struct SourceSpec {
    pub name: String,
    pub log_path: PathBuf,
    pub entries: mpsc::Receiver<LogEntry>,
    pub stats: Option<Arc<SourceRuntimeStats>>,
    /// Shared with the control API, which validates `line_idx` against it.
    pub line_counter: Arc<AtomicU64>,
    /// Level classifier for received lines; `None` when `levels: off`.
    pub level_detector: Option<LevelDetector>,
    pub meta: SourceRuntimeMeta,
}

struct SourceWriter {
    name: String,
    /// `None` once every sender for this source is gone.
    entries: Option<mpsc::Receiver<LogEntry>>,
    log_path: PathBuf,
    /// `None` while the session log cannot be opened; retried per record.
    file: Option<BufWriter<std::fs::File>>,
    lines_since_flush: usize,
    stats: Option<Arc<SourceRuntimeStats>>,
    line_counter: Arc<AtomicU64>,
    level_detector: Option<LevelDetector>,
    meta: SourceRuntimeMeta,
    /// [`SourceRuntimeMeta`] fields pre-serialized for `combined.jsonl`.
    combined_tail: String,
}

impl SourceWriter {
    fn flush(&mut self) {
        if let Some(file) = &mut self.file {
            let _ = file.flush();
        }
        self.lines_since_flush = 0;
    }
}

type WriterJob = Box<dyn FnOnce(&mut Writer) + Send>;

/// Sends work to run inside the writer task.
#[derive(Clone)]
pub(crate) struct WriterHandle {
    jobs: mpsc::Sender<WriterJob>,
}

impl WriterHandle {
    /// Run `job` inside the writer task, between two records, and return its
    /// result. Fails only when the writer has stopped.
    pub async fn exec<R: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Writer) -> R + Send + 'static,
    ) -> Result<R, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let job: WriterJob = Box::new(move |writer| {
            let _ = reply_tx.send(job(writer));
        });
        self.jobs
            .send(job)
            .await
            .map_err(|_| "session writer stopped".to_string())?;
        reply_rx
            .await
            .map_err(|_| "session writer stopped".to_string())
    }
}

pub(crate) struct Writer {
    sources: Vec<SourceWriter>,
    shared: WriterShared,
    clock: SessionClock,
    first_log_at: Option<DateTime<Local>>,
    /// Round-robin start position, so a busy source cannot starve others.
    next_source: usize,
}

impl Writer {
    pub fn new(shared: WriterShared, sources: Vec<SourceSpec>) -> Self {
        let sources = sources
            .into_iter()
            .filter_map(|spec| {
                let Some(file) = open_log_file(&spec.name, &spec.log_path) else {
                    // Dropping the receiver stops the source, as before.
                    return None;
                };
                Some(SourceWriter {
                    combined_tail: combined_record_tail(&spec.meta),
                    name: spec.name,
                    entries: Some(spec.entries),
                    log_path: spec.log_path,
                    file: Some(file),
                    lines_since_flush: 0,
                    stats: spec.stats,
                    line_counter: spec.line_counter,
                    level_detector: spec.level_detector,
                    meta: spec.meta,
                })
            })
            .collect();
        Self {
            sources,
            clock: SessionClock::new(shared.ts_mode),
            shared,
            first_log_at: None,
            next_source: 0,
        }
    }

    /// Spawn the writer task.
    pub fn spawn(self) -> (WriterHandle, tokio::task::JoinHandle<()>) {
        let (jobs_tx, jobs_rx) = mpsc::channel(8);
        let handle = tokio::spawn(self.run(jobs_rx));
        (WriterHandle { jobs: jobs_tx }, handle)
    }

    async fn run(mut self, mut jobs: mpsc::Receiver<WriterJob>) {
        enum Event {
            Job(Option<WriterJob>),
            Entry(Option<(usize, LogEntry)>),
        }
        let mut jobs_open = true;
        let mut entries_open = !self.sources.is_empty();
        loop {
            let event = tokio::select! {
                // Jobs first, so rotation does not wait behind a busy source.
                biased;
                job = jobs.recv(), if jobs_open => Event::Job(job),
                entry = poll_fn(|cx| self.poll_entry(cx)), if entries_open => Event::Entry(entry),
                else => break,
            };
            match event {
                Event::Job(Some(job)) => job(&mut self),
                Event::Job(None) => jobs_open = false,
                Event::Entry(Some((index, entry))) => self.commit(index, entry),
                Event::Entry(None) => entries_open = false,
            }
        }
        for source in &mut self.sources {
            source.flush();
        }
    }

    /// Next queued entry from any source, round-robin. `Ready(None)` once every
    /// source's channel is closed.
    fn poll_entry(&mut self, cx: &mut Context<'_>) -> Poll<Option<(usize, LogEntry)>> {
        let count = self.sources.len();
        let mut any_open = false;
        for offset in 0..count {
            let index = (self.next_source + offset) % count;
            let source = &mut self.sources[index];
            let Some(entries) = source.entries.as_mut() else {
                continue;
            };
            match entries.poll_recv(cx) {
                Poll::Ready(Some(entry)) => {
                    self.next_source = (index + 1) % count;
                    return Poll::Ready(Some((index, entry)));
                }
                Poll::Ready(None) => {
                    source.entries = None;
                    source.flush();
                }
                Poll::Pending => any_open = true,
            }
        }
        if any_open {
            Poll::Pending
        } else {
            Poll::Ready(None)
        }
    }

    /// Names and session log paths of the sources this writer serves.
    pub fn source_names(&self) -> impl Iterator<Item = &str> {
        self.sources.iter().map(|source| source.name.as_str())
    }

    pub fn first_log_at(&self) -> Option<DateTime<Local>> {
        self.first_log_at
    }

    /// Switch every source to a new session: write to `log_paths` from the
    /// next record on, and restart `first_log_at` and the line counters.
    pub fn start_session(&mut self, log_paths: &HashMap<String, PathBuf>) {
        for source in &mut self.sources {
            source.flush();
            if let Some(path) = log_paths.get(&source.name) {
                source.log_path = path.clone();
                source.file = open_log_file(&source.name, path);
            }
            source.line_counter.store(0, Ordering::Relaxed);
        }
        self.first_log_at = None;
    }

    /// Persist and publish one record.
    fn commit(&mut self, index: usize, entry: LogEntry) {
        let source = &mut self.sources[index];
        let source_name = source.name.as_str();
        if source.file.is_none() {
            source.file = open_log_file(source_name, &source.log_path);
            if source.file.is_none() {
                return;
            }
        }

        // Set origin on first entry for the current session and broadcast session_info update.
        if self.first_log_at.is_none() {
            self.first_log_at = Some(entry.timestamp);
            let session = if let Ok(mut manager) = self.shared.session_manager.lock() {
                let _ = manager.mark_first_log_at(entry.timestamp);
                manager.build_session_info()
            } else {
                json!({
                    "first_log_at": entry.timestamp.to_rfc3339(),
                    "timestamp_mode": self.shared.ts_mode.to_string(),
                })
            };
            let session_info = json!({
                "type": "session_info",
                "session": session,
            });
            let _ = self.shared.broadcast_tx.send(LiveEvent::new(session_info));
        }

        // Format for log file: [timestamp] message
        let file_ts = self.clock.file_timestamp(entry.timestamp);
        let file_line = format!("[{file_ts}] {}\n", entry.message);
        if let Some(stats) = &source.stats {
            stats.record_dequeued(file_line.len());
        }
        let file = source.file.as_mut().expect("log file opened above");
        if let Err(e) = file.write_all(file_line.as_bytes()) {
            error!("[{source_name}] log write error: {e}");
        } else {
            source.lines_since_flush += 1;
            // Flush a partial batch when the queue goes idle so low-volume
            // sources remain observable without sacrificing batching during
            // sustained capture.
            let queue_idle = source.entries.as_ref().map_or(true, |rx| rx.is_empty());
            if source.lines_since_flush >= LOG_FLUSH_EVERY_LINES || queue_idle {
                let _ = file.flush();
                source.lines_since_flush = 0;
            }
        }

        // Build WS payload with BOTH absolute and relative timestamps.
        // The frontend needs both so the timestamp toggle button works.
        let abs_ts = entry.timestamp.format("%m-%d %H:%M:%S%.3f").to_string();
        let abs_num = entry.timestamp.timestamp_millis() as f64;

        // Calculate relative timestamp from origin (first log).
        let (rel_ts, rel_num) = match self.first_log_at {
            Some(origin_ts) => {
                let delta = entry.timestamp - origin_ts;
                let total_ms = delta.num_milliseconds().max(0) as u64;
                (format_relative_millis(total_ms), total_ms as f64)
            }
            None => (format_relative_millis(0), 0.0),
        };

        // Always send absolute as the display timestamp.
        // The frontend recalculates using metadata when the mode changes.
        let display_ts = abs_ts.clone();
        let ts_iso = entry.timestamp.to_rfc3339();

        let data = if let Some(ref color) = entry.color {
            if let Some(ansi) = Ansi::code(color) {
                format!("{ansi}{}{}", entry.message, Ansi::RESET)
            } else {
                entry.message.clone()
            }
        } else {
            entry.message.clone()
        };

        let is_tx = entry.source.starts_with("TX::");

        // Derive origin:
        // - TX::<origin> → the part after TX:: (pytest, ui, etc.)
        // - Normal RX where entry.source == source_name → "SERIAL"
        // - Injected where entry.source != source_name → the caller-provided origin
        let origin = if is_tx {
            entry
                .source
                .strip_prefix("TX::")
                .unwrap_or("ui")
                .to_string()
        } else if entry.source == source_name {
            "SERIAL".to_string()
        } else {
            entry.source.clone()
        };

        // Raw message without ANSI wrapping (for SDK / structured API).
        let message = entry.message.clone();

        // Increment per-source line counter.
        let line_idx = source.line_counter.fetch_add(1, Ordering::Relaxed);

        let mut payload = json!({
            "type": if is_tx { "tx" } else { "rx" },
            "data": data,
            "timestamp": display_ts,
            "timestamp_iso": ts_iso,
            "timestamp_num": abs_num,
            "source_id": source_name,
            "line_idx": line_idx,
            "origin": origin,
            "color": entry.color,
            "message": message,
            // Send both absolute and relative so the frontend can toggle.
            "absTs": abs_ts,
            "absNum": abs_num,
            "relTs": rel_ts,
            "relNum": rel_num,
        });
        if let (Some(payload_obj), Some(meta_obj)) = (
            payload.as_object_mut(),
            entry.meta.as_ref().and_then(|m| m.as_object()),
        ) {
            for (key, value) in meta_obj {
                payload_obj.insert(key.clone(), value.clone());
            }
        }

        // Classify received lines once, here, so live clients, replay,
        // combined.jsonl and reports share one result. TX echoes have no
        // level; an explicit `level` in injected metadata is kept as-is.
        if !is_tx && payload.get("level").is_none() {
            if let Some(level) = source
                .level_detector
                .as_mut()
                .and_then(|d| d.detect(&message))
            {
                payload["level"] = json!(level);
            }
        }

        // Serialize the payload once: the same text is broadcast, and the
        // combined.jsonl record is that text plus the source's constant fields.
        let payload_text = match self.shared.session_manager.lock() {
            Ok(mut manager) => {
                if let Some(object) = payload.as_object_mut() {
                    object.insert("sequence".to_string(), json!(manager.next_sequence()));
                    object.insert("session_id".to_string(), json!(manager.session_id()));
                }
                let payload_text = payload.to_string();
                let appended = combined_record_line(
                    &payload,
                    &payload_text,
                    &source.meta,
                    &source.combined_tail,
                )
                .map_err(anyhow::Error::from)
                .and_then(|line| manager.append_combined_line(line));
                if let Err(error) = appended {
                    error!("[{source_name}] failed to append combined entry: {error}");
                    return;
                }
                payload_text
            }
            Err(_) => {
                error!("[{source_name}] session manager lock poisoned");
                return;
            }
        };

        crate::net::watch::retain_matching_watches(
            &self.shared.watches,
            source_name,
            is_tx,
            &message,
            &payload,
        );

        // Store in the replay buffer and broadcast to WS clients atomically.
        crate::net::ws_server::publish_replayable(
            &self.shared.replay,
            &self.shared.broadcast_tx,
            REPLAY_BUFFER_SIZE,
            LiveEvent::with_text(payload, payload_text),
        );
    }
}

/// Fields that `combined.jsonl` records carry in addition to the live payload.
/// They are constant for a source, so they are serialized once per source.
const COMBINED_ONLY_KEYS: [&str; 5] = [
    "app_name",
    "job_id",
    "source_label",
    "source_kind",
    "tab_labels",
];

/// The [`COMBINED_ONLY_KEYS`] fields serialized as the end of a JSON object:
/// `,"app_name":…,"tab_labels":[…]}`.
fn combined_record_tail(source_meta: &SourceRuntimeMeta) -> String {
    let fields = json!({
        "app_name": source_meta.app_name,
        "job_id": source_meta.job_id,
        "source_label": source_meta.source_label,
        "source_kind": source_meta.source_kind,
        "tab_labels": source_meta.tab_labels,
    });
    format!(",{}", &fields.to_string()[1..])
}

/// Serialize the `combined.jsonl` record for a live payload, reusing the
/// payload's already-serialized text. Payloads whose injected metadata
/// overlaps the combined-only fields take the general (slower) path, so
/// the stored record never contains duplicate keys.
fn combined_record_line(
    payload: &serde_json::Value,
    payload_text: &str,
    source_meta: &SourceRuntimeMeta,
    tail: &str,
) -> serde_json::Result<Vec<u8>> {
    let can_append = payload.as_object().is_some_and(|obj| {
        obj.get("source_id").and_then(|v| v.as_str()) == Some(source_meta.source_id.as_str())
            && !COMBINED_ONLY_KEYS.iter().any(|key| obj.contains_key(*key))
    });
    if can_append {
        if let Some(body) = payload_text.strip_suffix('}') {
            let mut line = Vec::with_capacity(body.len() + tail.len());
            line.extend_from_slice(body.as_bytes());
            line.extend_from_slice(tail.as_bytes());
            return Ok(line);
        }
    }
    serde_json::to_vec(&build_combined_log_entry(payload, source_meta))
}

fn build_combined_log_entry(
    payload: &serde_json::Value,
    source_meta: &SourceRuntimeMeta,
) -> serde_json::Value {
    let mut combined = payload.clone();
    if let Some(obj) = combined.as_object_mut() {
        obj.insert("source_id".to_string(), json!(source_meta.source_id));
        obj.insert("app_name".to_string(), json!(source_meta.app_name));
        obj.insert("job_id".to_string(), json!(source_meta.job_id));
        obj.insert("source_label".to_string(), json!(source_meta.source_label));
        obj.insert("source_kind".to_string(), json!(source_meta.source_kind));
        obj.insert("tab_labels".to_string(), json!(source_meta.tab_labels));
    }
    combined
}

fn open_log_file(source_name: &str, log_path: &Path) -> Option<BufWriter<std::fs::File>> {
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(file) => Some(BufWriter::new(file)),
        Err(e) => {
            error!(
                "[{source_name}] cannot open log file {}: {e}",
                log_path.display()
            );
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::time::Duration;

    pub fn temp_dir(name: &str) -> PathBuf {
        let nanos = Local::now().timestamp_nanos_opt().unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "embed-log-runtime-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn test_manager(session_id: &str, dir: PathBuf, source_file: PathBuf) -> SessionManager {
        let mut source_files = HashMap::new();
        source_files.insert("dut".to_string(), source_file.display().to_string());
        let combined_file = dir.join("combined.jsonl");
        SessionManager::new(
            session_id,
            dir,
            &[json!({ "label": "Main", "panes": ["dut"] })],
            source_files,
            combined_file.display().to_string(),
            HashMap::from([("dut".to_string(), "DUT".to_string())]),
            HashMap::from([("dut".to_string(), "udp".to_string())]),
            json!({}),
            json!({}),
            json!({}),
            json!({}),
            Local::now().to_rfc3339(),
            "embed-log",
            None,
            None,
            "absolute",
            None,
            json!([]),
        )
    }

    pub fn test_source_meta() -> SourceRuntimeMeta {
        SourceRuntimeMeta {
            source_id: "dut".to_string(),
            source_label: "DUT".to_string(),
            source_kind: "udp".to_string(),
            tab_labels: vec!["Main".to_string()],
            app_name: "embed-log".to_string(),
            job_id: None,
        }
    }

    pub async fn wait_file_contains(path: &Path, needle: &str) {
        for _ in 0..200 {
            if std::fs::read_to_string(path)
                .map(|text| text.contains(needle))
                .unwrap_or(false)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{} did not contain {needle:?}", path.display());
    }

    pub fn read_jsonl(path: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// A running writer plus the handles a test needs to drive and observe it.
    pub struct TestWriter {
        pub writer: WriterHandle,
        pub task: tokio::task::JoinHandle<()>,
        pub senders: HashMap<String, mpsc::Sender<LogEntry>>,
        pub counters: HashMap<String, Arc<AtomicU64>>,
        pub manager: Arc<Mutex<SessionManager>>,
        pub replay: Arc<Mutex<VecDeque<LiveMessage>>>,
        pub broadcast_rx: broadcast::Receiver<LiveMessage>,
    }

    impl TestWriter {
        /// Spawn a writer for `sources` (`(name, session log path)`), each with
        /// a queue of `queue` entries. Source `dut` uses [`test_source_meta`]
        /// and `dut_levels`.
        pub fn spawn(
            manager: SessionManager,
            sources: &[(&str, PathBuf)],
            queue: usize,
            mut dut_levels: Option<LevelDetector>,
        ) -> Self {
            let manager = Arc::new(Mutex::new(manager));
            let replay = Arc::new(Mutex::new(VecDeque::new()));
            let (broadcast_tx, broadcast_rx) = broadcast::channel(4096);
            let mut senders = HashMap::new();
            let mut counters = HashMap::new();
            let mut specs = Vec::new();
            for (name, log_path) in sources {
                let (tx, rx) = mpsc::channel(queue);
                let counter = Arc::new(AtomicU64::new(0));
                senders.insert(name.to_string(), tx);
                counters.insert(name.to_string(), counter.clone());
                let meta = if *name == "dut" {
                    test_source_meta()
                } else {
                    SourceRuntimeMeta {
                        source_id: name.to_string(),
                        source_label: name.to_string(),
                        source_kind: "file".to_string(),
                        tab_labels: vec!["Main".to_string()],
                        app_name: "embed-log".to_string(),
                        job_id: None,
                    }
                };
                specs.push(SourceSpec {
                    name: name.to_string(),
                    log_path: log_path.clone(),
                    entries: rx,
                    stats: None,
                    line_counter: counter,
                    level_detector: if *name == "dut" {
                        dut_levels.take()
                    } else {
                        None
                    },
                    meta,
                });
            }
            let (writer, task) = Writer::new(
                WriterShared {
                    broadcast_tx,
                    replay: replay.clone(),
                    session_manager: manager.clone(),
                    watches: Arc::new(RwLock::new(HashMap::new())),
                    ts_mode: TimestampMode::Absolute,
                },
                specs,
            )
            .spawn();
            Self {
                writer,
                task,
                senders,
                counters,
                manager,
                replay,
                broadcast_rx,
            }
        }

        pub async fn send(&self, source: &str, message: &str) {
            self.send_as(source, source, message).await;
        }

        /// Send an entry on `source`'s queue whose `LogEntry::source` is `origin`.
        pub async fn send_as(&self, source: &str, origin: &str, message: &str) {
            self.senders[source]
                .send(LogEntry::new(Local::now(), origin, message))
                .await
                .unwrap();
        }

        /// Close every source queue and the job channel, and wait for the
        /// writer to finish.
        pub async fn finish(self) -> broadcast::Receiver<LiveMessage> {
            drop(self.senders);
            drop(self.writer);
            self.task.await.unwrap();
            self.broadcast_rx
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn combined_record_line_reuses_payload_text_and_matches_full_build() {
        let meta = test_source_meta();
        let tail = combined_record_tail(&meta);
        let payload = json!({
            "type": "rx",
            "source_id": "dut",
            "message": "boot \"quoted\" \u{1b}[0m",
            "sequence": 7,
            "session_id": "session-1",
            "custom": {"nested": [1, 2]},
        });
        let text = payload.to_string();
        let line = combined_record_line(&payload, &text, &meta, &tail).unwrap();
        assert!(
            line.starts_with(&text.as_bytes()[..text.len() - 1]),
            "reuses payload text"
        );
        let parsed: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(parsed, build_combined_log_entry(&payload, &meta));
    }

    #[test]
    fn combined_record_line_never_duplicates_injected_keys() {
        let meta = test_source_meta();
        let tail = combined_record_tail(&meta);
        for payload in [
            json!({"source_id": "dut", "source_kind": "merge", "message": "m"}),
            json!({"source_id": "other", "message": "m"}),
        ] {
            let text = payload.to_string();
            let line = combined_record_line(&payload, &text, &meta, &tail).unwrap();
            let line = String::from_utf8(line).unwrap();
            for key in ["\"source_id\"", "\"source_kind\""] {
                assert_eq!(line.matches(key).count(), 1, "{line}");
            }
            let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed, build_combined_log_entry(&payload, &meta));
            assert_eq!(parsed["source_kind"], "udp");
        }
    }

    #[tokio::test]
    async fn writer_appends_combined_jsonl_with_source_metadata() {
        let dir = temp_dir("combined-jsonl");
        let log = dir.join("main__dut__session-1.log");
        let levels = LevelDetector::new(&crate::levels::builtin_zephyr_profile(), "test").unwrap();
        let rig = TestWriter::spawn(
            test_manager("session-1", dir.clone(), log.clone()),
            &[("dut", log)],
            4,
            Some(levels),
        );
        rig.send("dut", "boot complete").await;
        rig.send("dut", "[00:00:01.000,000] <dbg> net: rx").await;
        rig.send_as("dut", "TX::ui", "<dbg> echoed command").await;
        rig.finish().await;

        let records = read_jsonl(&dir.join("combined.jsonl"));
        assert!(records[0].get("level").is_none(), "{}", records[0]);
        assert_eq!(records[1]["level"], "dbg");
        assert!(records[2].get("level").is_none(), "TX lines have no level");
        assert_eq!(records[2]["type"], "tx");
        assert_eq!(records[2]["origin"], "ui");
        let first = &records[0];
        assert_eq!(first["source_id"], "dut");
        assert_eq!(first["source_label"], "DUT");
        assert_eq!(first["source_kind"], "udp");
        assert_eq!(first["tab_labels"], json!(["Main"]));
        assert_eq!(first["session_id"], "session-1");
        assert_eq!(first["app_name"], "embed-log");
        assert_eq!(first["message"], "boot complete");
        assert_eq!(first["origin"], "SERIAL");
        assert_eq!(first["sequence"], 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn sources_commit_in_one_global_order() {
        let dir = temp_dir("global-order");
        let rig = TestWriter::spawn(
            test_manager("session-1", dir.clone(), dir.join("dut.log")),
            &[("DUT", dir.join("dut.log")), ("HOST", dir.join("host.log"))],
            64,
            None,
        );
        let dut = rig.senders["DUT"].clone();
        let host = rig.senders["HOST"].clone();
        let dut_sender = tokio::spawn(async move {
            for index in 0..50 {
                dut.send(LogEntry::new(Local::now(), "DUT", format!("dut-{index}")))
                    .await
                    .unwrap();
            }
        });
        let host_sender = tokio::spawn(async move {
            for index in 0..50 {
                host.send(LogEntry::new(Local::now(), "HOST", format!("host-{index}")))
                    .await
                    .unwrap();
            }
        });
        dut_sender.await.unwrap();
        host_sender.await.unwrap();
        let replay = rig.replay.clone();
        let mut broadcast_rx = rig.finish().await;

        let records = read_jsonl(&dir.join("combined.jsonl"));
        let sequences: Vec<u64> = records
            .iter()
            .map(|r| r["sequence"].as_u64().unwrap())
            .collect();
        assert_eq!(sequences, (1..=100).collect::<Vec<_>>());
        // Per-source line indexes count each source's own records.
        for source in ["DUT", "HOST"] {
            let line_idx: Vec<u64> = records
                .iter()
                .filter(|r| r["source_id"] == source)
                .map(|r| r["line_idx"].as_u64().unwrap())
                .collect();
            assert_eq!(line_idx, (0..50).collect::<Vec<_>>(), "{source}");
        }

        let mut broadcast_sequences = Vec::new();
        while let Ok(message) = broadcast_rx.try_recv() {
            if let Some(sequence) = message.value().get("sequence").and_then(|v| v.as_u64()) {
                broadcast_sequences.push(sequence);
            }
        }
        assert_eq!(broadcast_sequences, sequences);
        let replayed: Vec<u64> = replay
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.value()["sequence"].as_u64().unwrap())
            .collect();
        assert_eq!(replayed, sequences);
        assert!(std::fs::read_to_string(dir.join("host.log"))
            .unwrap()
            .contains("host-49"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn start_session_switches_log_files_and_restarts_counters() {
        let dir = temp_dir("start-session");
        let first_log = dir.join("session-1").join("dut.log");
        let second_log = dir.join("session-2").join("dut.log");
        let rig = TestWriter::spawn(
            test_manager("session-1", dir.clone(), first_log.clone()),
            &[("dut", first_log.clone())],
            4,
            None,
        );
        rig.send("dut", "first").await;
        rig.send("dut", "first again").await;
        wait_file_contains(&first_log, "first again").await;

        let paths = HashMap::from([("dut".to_string(), second_log.clone())]);
        let had_origin = rig
            .writer
            .exec(move |writer| {
                let had_origin = writer.first_log_at().is_some();
                writer.start_session(&paths);
                had_origin && writer.first_log_at().is_none()
            })
            .await
            .unwrap();
        assert!(had_origin, "first_log_at is restarted");
        assert_eq!(rig.counters["dut"].load(Ordering::Relaxed), 0);

        rig.send("dut", "second").await;
        wait_file_contains(&second_log, "second").await;
        rig.finish().await;
        assert!(!std::fs::read_to_string(&first_log)
            .unwrap()
            .contains("second"));
        let records = read_jsonl(&dir.join("combined.jsonl"));
        let last = records.last().unwrap();
        assert_eq!(last["message"], "second");
        assert_eq!(last["line_idx"], 0);
        assert_eq!(
            last["relNum"], 0.0,
            "relative time restarts with the session"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn jobs_run_before_queued_records() {
        const QUEUED: usize = 1000;
        let dir = temp_dir("job-priority");
        let log = dir.join("dut.log");
        let rig = TestWriter::spawn(
            test_manager("session-1", dir.clone(), log.clone()),
            &[("dut", log)],
            QUEUED,
            None,
        );
        for index in 0..QUEUED {
            rig.send("dut", &format!("queued-{index}")).await;
        }
        let manager = rig.manager.clone();
        let committed_before_job = rig
            .writer
            .exec(move |_| manager.lock().unwrap().record_count())
            .await
            .unwrap();
        assert!(
            (committed_before_job as usize) < QUEUED,
            "a job must not wait for the whole backlog ({committed_before_job} committed first)"
        );
        rig.finish().await;
        assert_eq!(read_jsonl(&dir.join("combined.jsonl")).len(), QUEUED);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn exec_fails_once_the_writer_stopped() {
        let dir = temp_dir("writer-stopped");
        let rig = TestWriter::spawn(
            test_manager("session-1", dir.clone(), dir.join("dut.log")),
            &[("dut", dir.join("dut.log"))],
            4,
            None,
        );
        let TestWriter { writer, task, .. } = rig;
        task.abort();
        let _ = task.await;
        assert!(writer.exec(|_| ()).await.is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
