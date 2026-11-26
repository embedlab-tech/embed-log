//! Per-UART dictionary changes, optionally armed by a raw console-line regex.
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::config::models::ParserConfig;
use crate::parsers::{try_create_dictionary_parser, validate_dictionary, StreamParser};
use crate::session::manager::atomic_write_file;
use crate::session::SessionManager;

pub const MAX_DICTIONARY_BYTES: usize = 8 * 1024 * 1024;
const MAX_STAGED_BYTES: usize = 64 * 1024 * 1024;
const APPLY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ANCHOR_LINE: usize = 16 * 1024;
const ANCHOR_LINE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize)]
pub struct ArmedStatus {
    pub dictionary_id: String,
    pub activate_on_regex: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DictionaryInfo {
    pub dictionary_id: String,
    pub build_id: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfiguredDictionary {
    pub path: String,
    pub dictionary: Option<DictionaryInfo>,
    pub error: Option<String>,
    /// The file differs from the snapshot taken at startup or at the last reload
    /// (for example after a firmware rebuild). Computed on every status read.
    pub changed_on_disk: bool,
    #[serde(skip)]
    stamp: Option<FileStamp>,
}

impl DictionaryStatus {
    /// The recorded choice wins while it still holds the active dictionary
    /// (the active one can also change out of band, for example by a raw-line
    /// anchor); otherwise the first entry with the active id.
    fn resolved_active_index(&self) -> Option<usize> {
        let id = &self.dictionary.as_ref()?.dictionary_id;
        let holds = |i: usize| {
            self.configured[i]
                .dictionary
                .as_ref()
                .map(|d| &d.dictionary_id)
                == Some(id)
        };
        self.active_index
            .filter(|&i| i < self.configured.len() && holds(i))
            .or_else(|| (0..self.configured.len()).find(|&i| holds(i)))
    }
}

/// Cheap change detector for a dictionary file: modification time and size.
type FileStamp = (std::time::SystemTime, u64);

fn file_stamp(path: &str) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

#[derive(Clone, Debug, Serialize)]
pub struct DictionaryStatus {
    pub source_id: String,
    pub generation: u64,
    pub parser_type: String,
    pub dictionary: Option<DictionaryInfo>,
    pub raw_file: Option<String>,
    pub raw_offset: u64,
    pub raw_error: Option<String>,
    pub configured: Vec<ConfiguredDictionary>,
    /// The configured entry the active dictionary comes from. Two files with the
    /// same bytes share a `dictionary_id`, so the entry is tracked on its own:
    /// Reload and `parser.watch` follow this file, not "the first with this id".
    pub active_index: Option<usize>,
    pub armed: Option<ArmedStatus>,
    /// `parser.watch`: the active file is reloaded automatically when it changes.
    pub watch: bool,
    /// Why the last automatic reload failed; cleared by the next success.
    pub watch_error: Option<String>,
}

/// Result of re-reading the active configured dictionary file.
#[derive(Clone, Debug, Serialize)]
pub struct ReloadOutcome {
    /// `false` when the file is byte-identical to the active dictionary.
    pub reloaded: bool,
    pub path: String,
    pub dictionary: DictionaryInfo,
    pub status: DictionaryStatus,
}

fn read_dictionary_file(path: &str) -> Result<String, String> {
    let mut json = String::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_DICTIONARY_BYTES + 1) as u64)
        .read_to_string(&mut json)
        .map_err(|e| e.to_string())?;
    Ok(json)
}

type ActivationResult = (
    Result<DictionaryStatus, String>,
    oneshot::Sender<Result<DictionaryStatus, String>>,
    Vec<String>,
);

struct Staged {
    info: DictionaryInfo,
    json: Arc<str>,
}

struct Update {
    config: ParserConfig,
    parser: Box<dyn StreamParser>,
    info: DictionaryInfo,
    expected_generation: u64,
    arm: Option<Regex>,
    deadline: Instant,
    ack: oneshot::Sender<Result<DictionaryStatus, String>>,
}

struct Armed {
    config: ParserConfig,
    // UartSource is shared during reconnect; the RX owner alone takes this parser.
    parser: Mutex<Box<dyn StreamParser>>,
    info: DictionaryInfo,
    regex: Regex,
    pending: Vec<u8>,
    line_start: bool,
    since: Option<Instant>,
}

/// Cloneable control-plane handle; the RX owner alone applies updates.
#[derive(Clone)]
pub struct DictionaryControl {
    updates: mpsc::Sender<Update>,
    staged: Arc<Mutex<HashMap<String, Staged>>>,
    status: Arc<Mutex<DictionaryStatus>>,
    session: Arc<Mutex<SessionManager>>,
    artifact_name: String,
    capture_raw: Arc<AtomicBool>,
}

pub struct DictionaryRuntime {
    control: DictionaryControl,
    updates: mpsc::Receiver<Update>,
    raw: Option<(PathBuf, File)>,
    armed: Option<Armed>,
}

impl DictionaryControl {
    pub fn new(
        source: &str,
        config: &ParserConfig,
        session: Arc<Mutex<SessionManager>>,
    ) -> Result<(Self, DictionaryRuntime), String> {
        let (tx, rx) = mpsc::channel(8);
        let control = Self {
            updates: tx,
            staged: Arc::new(Mutex::new(HashMap::new())),
            status: Arc::new(Mutex::new(DictionaryStatus {
                source_id: source.into(),
                generation: 0,
                parser_type: config.parser_type.clone(),
                dictionary: None,
                raw_file: None,
                raw_offset: 0,
                raw_error: None,
                configured: Vec::new(),
                active_index: None,
                armed: None,
                watch: config.watch,
                watch_error: None,
            })),
            session,
            artifact_name: format!("{:x}", Sha256::digest(source.as_bytes())),
            capture_raw: Arc::new(AtomicBool::new(false)),
        };
        if matches!(
            config.parser_type.as_str(),
            "zephyr-dict" | "zephyr-dict-hex"
        ) {
            let paths = config
                .database
                .as_ref()
                .ok_or("dictionary path is missing")?
                .paths();
            if paths.is_empty() {
                return Err("dictionary path list is empty".into());
            }
            for (index, path) in paths.iter().enumerate() {
                // Stamp first: a write that lands after the read must show as changed.
                let stamp = file_stamp(path);
                let staged = read_dictionary_file(path).and_then(|json| control.stage_bytes(&json));
                if index == 0 {
                    let mut status = control.status.lock().unwrap();
                    status.dictionary = Some(staged.clone()?);
                    status.active_index = Some(0);
                }
                control
                    .status
                    .lock()
                    .unwrap()
                    .configured
                    .push(ConfiguredDictionary {
                        path: path.clone(),
                        dictionary: staged.as_ref().ok().cloned(),
                        error: staged.err(),
                        changed_on_disk: false,
                        stamp,
                    });
            }
        }
        let runtime = DictionaryRuntime {
            control: control.clone(),
            updates: rx,
            raw: None,
            armed: None,
        };
        Ok((control, runtime))
    }

    fn directory(&self) -> PathBuf {
        self.session
            .lock()
            .unwrap()
            .session_dir()
            .join("dictionaries")
    }

    /// The RX owner is gone. Sources that cannot switch dictionaries at runtime
    /// (anything but UART) drop their runtime straight away.
    pub fn is_closed(&self) -> bool {
        self.updates.is_closed()
    }

    pub fn status(&self) -> DictionaryStatus {
        let mut status = self.status.lock().unwrap().clone();
        status.active_index = status.resolved_active_index();
        for item in &mut status.configured {
            item.changed_on_disk =
                item.dictionary.is_some() && file_stamp(&item.path) != item.stamp;
        }
        status
    }

    /// Upload exact UTF-8 JSON bytes, not an arbitrary server-side file path.
    /// SHA256 binds the snapshot to the actual artifact, not a branch name.
    pub fn stage(&self, json: &str) -> Result<DictionaryInfo, String> {
        let info = self.stage_bytes(json)?;
        self.capture_raw.store(true, Ordering::Release);
        Ok(info)
    }

    /// Arm only after the RX owner acknowledges the raw-line matcher.
    pub async fn stage_with_regex(
        &self,
        json: &str,
        activate_on_regex: Option<&str>,
    ) -> Result<DictionaryInfo, String> {
        let Some(pattern) = activate_on_regex else {
            return self.stage(json);
        };
        if pattern.chars().count() > 1024 {
            return Err("activate_on_regex exceeds 1024 characters".into());
        }
        let regex = Regex::new(pattern).map_err(|e| format!("invalid activate_on_regex: {e}"))?;
        if regex.is_match("") {
            return Err("activate_on_regex must not match an empty line".into());
        }
        let status = self.status();
        if status.parser_type != "zephyr-dict" {
            return Err("activate_on_regex requires zephyr-dict parser".into());
        }
        let info = self.stage(json)?;
        if status.dictionary.as_ref() == Some(&info) {
            return Err("dictionary is already active".into());
        }
        self.queue_update(
            &info.dictionary_id,
            "zephyr-dict",
            status.generation,
            Some(regex),
        )
        .await?;
        Ok(info)
    }

    fn stage_bytes(&self, json: &str) -> Result<DictionaryInfo, String> {
        if json.len() > MAX_DICTIONARY_BYTES {
            return Err("dictionary exceeds 8 MiB".into());
        }
        validate_dictionary(json)?;
        let info = DictionaryInfo {
            dictionary_id: format!("{:x}", Sha256::digest(json.as_bytes())),
            build_id: serde_json::from_str::<serde_json::Value>(json)
                .map_err(|e| e.to_string())?
                .get("build_id")
                .cloned(),
        };
        let mut staged = self.staged.lock().unwrap();
        if !staged.contains_key(&info.dictionary_id) {
            if staged.values().map(|s| s.json.len()).sum::<usize>() + json.len() > MAX_STAGED_BYTES
            {
                return Err("staged dictionaries exceed 64 MiB per source".into());
            }
            let dir = self.directory();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            atomic_write_file(
                &dir.join(format!("{}.json", info.dictionary_id)),
                json.as_bytes(),
            )
            .map_err(|e| e.to_string())?;
            staged.insert(
                info.dictionary_id.clone(),
                Staged {
                    info: info.clone(),
                    json: json.into(),
                },
            );
        }
        Ok(info)
    }

    pub async fn activate(
        &self,
        dictionary_id: &str,
        parser_type: &str,
        expected_generation: u64,
    ) -> Result<DictionaryStatus, String> {
        self.activate_configured(dictionary_id, parser_type, expected_generation, None)
            .await
    }

    /// Like `activate`, naming the configured entry it comes from. Needed when
    /// several files hold identical bytes: they share one `dictionary_id`, so the
    /// id alone cannot say which file Reload and `parser.watch` should follow.
    pub async fn activate_configured(
        &self,
        dictionary_id: &str,
        parser_type: &str,
        expected_generation: u64,
        configured_index: Option<usize>,
    ) -> Result<DictionaryStatus, String> {
        if let Some(index) = configured_index {
            let status = self.status.lock().unwrap();
            let entry = status
                .configured
                .get(index)
                .ok_or("unknown configured dictionary index")?;
            if entry.dictionary.as_ref().map(|d| d.dictionary_id.as_str()) != Some(dictionary_id) {
                return Err("configured dictionary index does not match dictionary_id".into());
            }
        }
        let current = self.status();
        if configured_index.is_some()
            && current
                .dictionary
                .as_ref()
                .map(|d| d.dictionary_id.as_str())
                == Some(dictionary_id)
            && current.parser_type == parser_type
        {
            // Identical bytes under another file: decoding does not change, only the
            // file that Reload follows. Idempotent like a repeated activation.
            self.status.lock().unwrap().active_index = configured_index;
            return Ok(self.status());
        }
        self.queue_update(dictionary_id, parser_type, expected_generation, None)
            .await?;
        if configured_index.is_some() {
            self.status.lock().unwrap().active_index = configured_index;
        }
        Ok(self.status())
    }

    /// Re-read the configured file behind the active dictionary and activate it.
    ///
    /// For the rebuild loop: the firmware build overwrites the dictionary
    /// file in place, while the startup snapshot keeps decoding with the old
    /// one. A missing or invalid file leaves the active dictionary untouched.
    pub async fn reload(&self, expected_generation: u64) -> Result<ReloadOutcome, String> {
        let status = self.status();
        // Checked up front: an unchanged file must not hide that another client
        // changed the dictionary since the caller last looked.
        if status.generation != expected_generation {
            return Err(format!(
                "generation conflict: active generation is {}",
                status.generation
            ));
        }
        let active = status
            .dictionary
            .as_ref()
            .ok_or("no active dictionary to reload")?;
        let index = status
            .active_index
            .ok_or("the active dictionary was uploaded, not read from a configured file; select a configured dictionary first")?;
        let path = status.configured[index].path.clone();
        // Stamp before reading: a write that lands after the read stays "changed".
        let stamp = file_stamp(&path);
        let info = read_dictionary_file(&path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .and_then(|json| self.stage_bytes(&json).map_err(|e| format!("{path}: {e}")))?;
        if info.dictionary_id == active.dictionary_id {
            // Same bytes (for example a touched file): the hint is stale, not the dictionary.
            self.status.lock().unwrap().configured[index].stamp = stamp;
            return Ok(ReloadOutcome {
                reloaded: false,
                path,
                dictionary: info,
                status: self.status(),
            });
        }
        self.queue_update(
            &info.dictionary_id,
            &status.parser_type,
            expected_generation,
            None,
        )
        .await?;
        {
            let mut current = self.status.lock().unwrap();
            // Keep the selector bound to the file: it now holds the new content.
            if let Some(item) = current.configured.get_mut(index) {
                item.dictionary = Some(info.clone());
                item.error = None;
                item.stamp = stamp;
            }
            current.active_index = Some(index);
            // Frequent rebuilds must not exhaust the per-source staging budget.
            let still_used = current.configured.iter().any(|item| {
                item.dictionary.as_ref().map(|d| &d.dictionary_id) == Some(&active.dictionary_id)
            });
            if !still_used {
                self.staged.lock().unwrap().remove(&active.dictionary_id);
            }
        }
        Ok(ReloadOutcome {
            reloaded: true,
            path,
            dictionary: info,
            status: self.status(),
        })
    }

    /// `parser.watch`: reload the active file whenever a rebuild overwrites it.
    ///
    /// Polls file stamps (no platform watcher), waits until the file stops
    /// changing so a half-written dictionary is not read, and tries each version
    /// once: a bad file is reported in `watch_error` until it changes again.
    /// Ends when the UART source stops.
    pub async fn watch_active_file(self) {
        const POLL: Duration = Duration::from_secs(1);
        const SETTLE: Duration = Duration::from_millis(500);
        let mut failed: Option<Option<FileStamp>> = None;
        loop {
            tokio::time::sleep(POLL).await;
            if self.updates.is_closed() {
                return;
            }
            let status = self.status();
            let Some(item) = status.active_index.and_then(|i| status.configured.get(i)) else {
                continue;
            };
            if !item.changed_on_disk {
                failed = None;
                continue;
            }
            let seen = file_stamp(&item.path);
            if failed == Some(seen) {
                continue;
            }
            tokio::time::sleep(SETTLE).await;
            if file_stamp(&item.path) != seen {
                continue; // still being written; look again next round
            }
            match self.reload(self.status().generation).await {
                Ok(outcome) => {
                    failed = None;
                    self.status.lock().unwrap().watch_error = None;
                    if outcome.reloaded {
                        tracing::info!(
                            "[{}] dictionary reloaded from {}",
                            status.source_id,
                            outcome.path
                        );
                    }
                }
                // Another client changed the dictionary first; evaluate again.
                Err(e) if e.contains("generation conflict") => {}
                Err(e) => {
                    tracing::warn!(
                        "[{}] automatic dictionary reload failed: {e}",
                        status.source_id
                    );
                    self.status.lock().unwrap().watch_error = Some(e);
                    failed = Some(seen);
                }
            }
        }
    }

    async fn queue_update(
        &self,
        dictionary_id: &str,
        parser_type: &str,
        expected_generation: u64,
        arm: Option<Regex>,
    ) -> Result<DictionaryStatus, String> {
        let config = {
            let staged = self.staged.lock().unwrap();
            let db = staged
                .get(dictionary_id)
                .ok_or("dictionary is not staged for this source")?;
            let dir = self.directory();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(format!("{dictionary_id}.json"));
            // Re-create from owned bytes: file edits cannot silently change the binding.
            atomic_write_file(&path, db.json.as_bytes()).map_err(|e| e.to_string())?;
            ParserConfig {
                parser_type: parser_type.into(),
                database: Some(path.to_string_lossy().into_owned().into()),
                ..Default::default()
            }
        };
        let parser = try_create_dictionary_parser(&config)?;
        let info = self.staged.lock().unwrap()[dictionary_id].info.clone();
        self.capture_raw.store(true, Ordering::Release);
        let (ack, receive) = oneshot::channel();
        let deadline = Instant::now() + APPLY_TIMEOUT;
        let update = Update {
            config,
            parser,
            info,
            expected_generation,
            arm,
            deadline,
            ack,
        };
        tokio::time::timeout_at(deadline, async {
            self.updates
                .send(update)
                .await
                .map_err(|_| "UART source stopped".to_string())?;
            receive
                .await
                .map_err(|_| "UART source stopped before acknowledgement".to_string())?
        })
        .await
        .map_err(|_| "activation timed out; query dictionary.status before retrying".to_string())?
    }
}

impl DictionaryRuntime {
    /// Snapshot the initial parser too; never reread a mutable user DB on reconnect.
    pub fn current_config(&self, mut config: ParserConfig) -> Result<ParserConfig, String> {
        if let Some(db) = self.control.status().dictionary {
            let dir = self.control.directory();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(format!("{}.json", db.dictionary_id));
            if !path.exists() {
                let staged = self.control.staged.lock().unwrap();
                atomic_write_file(&path, staged[&db.dictionary_id].json.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            config.database = Some(path.to_string_lossy().into_owned().into());
        }
        Ok(config)
    }

    /// Preserve received bytes before decoding, including ambiguous FWU intervals.
    /// Reuse the open file; rotation starts a self-contained raw stream + DB snapshots.
    pub fn record_raw(&mut self, bytes: &[u8]) -> Result<(), String> {
        if !self.control.capture_raw.load(Ordering::Acquire) {
            return Ok(());
        }
        let dir = self.control.directory();
        let path = dir.join(format!("{}-uart.bin", self.control.artifact_name));
        if self.raw.as_ref().map(|(p, _)| p) != Some(&path) {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for db in self.control.staged.lock().unwrap().values() {
                atomic_write_file(
                    &dir.join(format!("{}.json", db.info.dictionary_id)),
                    db.json.as_bytes(),
                )
                .map_err(|e| e.to_string())?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            let offset = file.metadata().map_err(|e| e.to_string())?.len();
            let mut status = self.control.status.lock().unwrap();
            status.raw_file = Some(
                std::fs::canonicalize(&path)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned(),
            );
            status.raw_offset = offset;
            atomic_write_file(
                &dir.join(format!("{}-initial.json", self.control.artifact_name)),
                &serde_json::to_vec_pretty(&*status).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            self.raw = Some((path, file));
        }
        self.raw
            .as_mut()
            .unwrap()
            .1
            .write_all(bytes)
            .map_err(|e| e.to_string())?;
        let mut status = self.control.status.lock().unwrap();
        status.raw_offset += bytes.len() as u64;
        status.raw_error = None;
        Ok(())
    }

    /// A failed optional raw sink must not stop normal UART decoding.
    pub fn disable_raw(&mut self, error: String) {
        self.control.capture_raw.store(false, Ordering::Release);
        self.raw = None;
        self.control.status.lock().unwrap().raw_error = Some(error);
    }

    /// Called between RX feeds; only this owner may arm or replace the parser.
    pub fn apply_next(
        &mut self,
        parser: &mut Box<dyn StreamParser>,
        config: &mut ParserConfig,
    ) -> Option<ActivationResult> {
        let update = self.updates.try_recv().ok()?;
        let ack = update.ack;
        let mut lines = Vec::new();
        let result = (|| {
            if ack.is_closed() || Instant::now() >= update.deadline {
                return Err("activation expired before apply".into());
            }
            let status = self.control.status();
            let same = status.dictionary.as_ref() == Some(&update.info)
                && status.parser_type == update.config.parser_type;
            if update.arm.is_none() && same {
                lines.extend(self.reset_anchor(parser));
                self.armed = None;
                self.control.status.lock().unwrap().armed = None;
                return Ok(self.control.status());
            }
            if status.generation != update.expected_generation {
                return Err(format!(
                    "generation conflict: active generation is {}",
                    status.generation
                ));
            }
            if let Some(regex) = update.arm {
                if status.parser_type != "zephyr-dict" {
                    return Err("activate_on_regex requires zephyr-dict parser".into());
                }
                if same {
                    return Err("dictionary is already active".into());
                }
                lines.extend(self.reset_anchor(parser));
                self.control.status.lock().unwrap().armed = Some(ArmedStatus {
                    dictionary_id: update.info.dictionary_id.clone(),
                    activate_on_regex: regex.as_str().into(),
                });
                self.armed = Some(Armed {
                    config: update.config,
                    parser: Mutex::new(update.parser),
                    info: update.info,
                    regex,
                    pending: Vec::new(),
                    line_start: true,
                    since: None,
                });
                return Ok(self.control.status());
            }
            if let Err(e) = self.record_raw(&[]) {
                self.disable_raw(e);
            }
            // Flush a possible marker prefix through the OLD parser before switching.
            lines.extend(self.reset_anchor(parser));
            let next = self.switch(
                parser,
                config,
                update.parser,
                update.config,
                update.info,
                serde_json::json!({"reason": "explicit"}),
            )?;
            self.armed = None;
            lines.push(format!(
                "[dictionary: generation {}; DB {}; partial record discarded; explicit boundary]",
                next.generation,
                next.dictionary.as_ref().unwrap().dictionary_id
            ));
            Ok(next)
        })();
        Some((result, ack, lines))
    }

    fn switch(
        &mut self,
        parser: &mut Box<dyn StreamParser>,
        config: &mut ParserConfig,
        next_parser: Box<dyn StreamParser>,
        next_config: ParserConfig,
        info: DictionaryInfo,
        mut event: serde_json::Value,
    ) -> Result<DictionaryStatus, String> {
        let mut status = self.control.status.lock().unwrap();
        let mut next = status.clone();
        next.generation += 1;
        next.parser_type = next_config.parser_type.clone();
        next.dictionary = Some(info);
        next.armed = None;
        let dir = self.control.directory();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut history = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{}-changes.jsonl", self.control.artifact_name)))
            .map_err(|e| e.to_string())?;
        event["timestamp"] = serde_json::json!(chrono::Local::now().to_rfc3339());
        event["previous"] = serde_json::json!(*status);
        event["active"] = serde_json::json!(next);
        event["partial_record_discarded"] = serde_json::json!(true);
        writeln!(history, "{event}").map_err(|e| e.to_string())?;
        *parser = next_parser;
        *config = next_config;
        *status = next.clone();
        Ok(next)
    }

    /// Match actual console bytes, never strings manufactured by a candidate DB.
    /// A complete marker line is consumed; bytes after its LF use the new parser.
    pub fn feed(
        &mut self,
        parser: &mut Box<dyn StreamParser>,
        config: &mut ParserConfig,
        bytes: &[u8],
    ) -> Vec<String> {
        let mut lines = self.expire_anchor(parser);
        let Some(mut armed) = self.armed.take() else {
            lines.extend(parser.feed(bytes));
            return lines;
        };
        let mut passthrough = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            if !armed.line_start {
                passthrough.push(byte);
                if byte == b'\n' {
                    armed.line_start = true;
                }
                continue;
            }
            armed.since.get_or_insert_with(Instant::now);
            armed.pending.push(byte);
            if byte == b'\n' {
                let text = std::str::from_utf8(&armed.pending)
                    .ok()
                    .map(|s| s.trim_end_matches(['\r', '\n']));
                if let Some(anchor) = text.filter(|s| armed.regex.is_match(s)) {
                    lines.extend(parser.feed(&passthrough));
                    let status = self.control.status();
                    let boundary = if status.raw_error.is_none() {
                        status
                            .raw_offset
                            .checked_sub((bytes.len() - index - 1) as u64)
                    } else {
                        None
                    };
                    let event = serde_json::json!({"reason": "anchor", "anchor_line": anchor,
                        "anchor_raw_offset": boundary});
                    match self.switch(parser, config, armed.parser.into_inner().unwrap(), armed.config, armed.info, event) {
                        Ok(next) => lines.push(format!(
                            "[dictionary: generation {}; DB {}; auto-activated by raw UART anchor {:?}]",
                            next.generation, next.dictionary.as_ref().unwrap().dictionary_id, anchor)),
                        Err(error) => {
                            self.control.status.lock().unwrap().armed = None;
                            lines.push(format!("[dictionary: anchor activation failed; disarmed: {error}]"));
                            lines.extend(parser.feed(&armed.pending));
                        }
                    }
                    lines.extend(parser.feed(&bytes[index + 1..]));
                    return lines;
                }
                passthrough.append(&mut armed.pending);
                armed.since = None;
            } else if (byte < 0x20 && !matches!(byte, b'\r' | b'\t'))
                || armed.pending.len() > MAX_ANCHOR_LINE
            {
                passthrough.append(&mut armed.pending);
                armed.since = None;
                armed.line_start = false;
            }
        }
        lines.extend(parser.feed(&passthrough));
        self.armed = Some(armed);
        lines
    }

    /// Flush partial console bytes before reconnect/rearming/explicit activation.
    pub fn reset_anchor(&mut self, parser: &mut Box<dyn StreamParser>) -> Vec<String> {
        let Some(armed) = &mut self.armed else {
            return Vec::new();
        };
        let lines = parser.feed(&armed.pending);
        armed.pending.clear();
        armed.since = None;
        armed.line_start = true;
        lines
    }

    fn expire_anchor(&mut self, parser: &mut Box<dyn StreamParser>) -> Vec<String> {
        if self
            .armed
            .as_ref()
            .and_then(|a| a.since)
            .is_some_and(|start| start.elapsed() >= ANCHOR_LINE_TIMEOUT)
        {
            let mut lines = self.reset_anchor(parser);
            self.armed.as_mut().unwrap().line_start = false;
            lines.push("[dictionary: incomplete anchor line timed out; keeping active DB]".into());
            lines
        } else {
            Vec::new()
        }
    }

    pub fn on_idle(&mut self, parser: &mut Box<dyn StreamParser>, idle: Duration) -> Vec<String> {
        let mut lines = self.expire_anchor(parser);
        lines.extend(parser.on_idle(idle));
        lines
    }
}

#[cfg(all(test, unix))]
#[path = "dictionary_tests.rs"]
mod tests;
