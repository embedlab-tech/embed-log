//! Explicit, per-UART dictionary changes. No firmware-boundary guessing.
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
}

type ActivationResult = (
    Result<DictionaryStatus, String>,
    oneshot::Sender<Result<DictionaryStatus, String>>,
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
    deadline: Instant,
    ack: oneshot::Sender<Result<DictionaryStatus, String>>,
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
                let staged = (|| {
                    let mut json = String::new();
                    File::open(path)
                        .map_err(|e| e.to_string())?
                        .take((MAX_DICTIONARY_BYTES + 1) as u64)
                        .read_to_string(&mut json)
                        .map_err(|e| e.to_string())?;
                    control.stage_bytes(&json)
                })();
                if index == 0 {
                    control.status.lock().unwrap().dictionary = Some(staged.clone()?);
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
                    });
            }
        }
        let runtime = DictionaryRuntime {
            control: control.clone(),
            updates: rx,
            raw: None,
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

    pub fn status(&self) -> DictionaryStatus {
        self.status.lock().unwrap().clone()
    }

    /// Upload exact UTF-8 JSON bytes, not an arbitrary server-side file path.
    /// SHA256 binds the snapshot to the actual artifact, not a branch name.
    pub fn stage(&self, json: &str) -> Result<DictionaryInfo, String> {
        let info = self.stage_bytes(json)?;
        self.capture_raw.store(true, Ordering::Release);
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

    /// Called between complete RX feeds. Dropping the old parser intentionally
    /// abandons its partial record; no bytes are ever joined across generations.
    pub fn apply_next(
        &mut self,
        parser: &mut Box<dyn StreamParser>,
        config: &mut ParserConfig,
    ) -> Option<ActivationResult> {
        let update = self.updates.try_recv().ok()?;
        let ack = update.ack;
        let result = (|| {
            if ack.is_closed() || Instant::now() >= update.deadline {
                return Err("activation expired before apply".into());
            }
            self.record_raw(&[])?;
            let mut status = self.control.status.lock().unwrap();
            if status.generation != update.expected_generation {
                return Err(format!(
                    "generation conflict: active generation is {}",
                    status.generation
                ));
            }
            let next = DictionaryStatus {
                source_id: status.source_id.clone(),
                generation: status.generation + 1,
                parser_type: update.config.parser_type.clone(),
                dictionary: Some(update.info),
                raw_file: status.raw_file.clone(),
                raw_offset: status.raw_offset,
                raw_error: status.raw_error.clone(),
                configured: status.configured.clone(),
            };
            let dir = self.control.directory();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let mut history = OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(format!("{}-changes.jsonl", self.control.artifact_name)))
                .map_err(|e| e.to_string())?;
            let event = serde_json::json!({"timestamp": chrono::Local::now().to_rfc3339(), "previous": *status, "active": next, "partial_record_discarded": true});
            writeln!(history, "{event}").map_err(|e| e.to_string())?;
            *parser = update.parser;
            *config = update.config;
            *status = next.clone();
            Ok(next)
        })();
        Some((result, ack))
    }
}

#[cfg(all(test, unix))]
#[path = "dictionary_tests.rs"]
mod tests;
