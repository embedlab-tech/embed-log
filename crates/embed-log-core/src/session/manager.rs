use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde_json::json;
use tracing::{info, warn};

static ARTIFACT_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Manages a single session's artifacts: manifest, markers, and static HTML export.
pub struct SessionManager {
    session_id: String,
    session_dir: PathBuf,
    tabs: Vec<serde_json::Value>,
    source_files: HashMap<String, String>,
    combined_file: String,
    pane_labels: HashMap<String, String>,
    pane_kinds: HashMap<String, String>,
    pane_commands: serde_json::Value,
    frontend_plugins: serde_json::Value,
    pane_plugins: serde_json::Value,
    plugin_scripts: serde_json::Value,
    started_at: String,
    app_name: String,
    config_path: Option<String>,
    job_id: Option<String>,
    title: Option<String>,
    timestamp_mode: String,
    first_log_at: Option<String>,
    html_status: String,
    html_updated_at: Option<String>,
    html_error: Option<String>,
    next_sequence: u64,
    merges: serde_json::Value,
}

impl SessionManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: impl Into<String>,
        session_dir: PathBuf,
        tabs: &[serde_json::Value],
        source_files: HashMap<String, String>,
        combined_file: impl Into<String>,
        pane_labels: HashMap<String, String>,
        pane_kinds: HashMap<String, String>,
        pane_commands: serde_json::Value,
        frontend_plugins: serde_json::Value,
        pane_plugins: serde_json::Value,
        plugin_scripts: serde_json::Value,
        started_at: impl Into<String>,
        app_name: impl Into<String>,
        config_path: Option<String>,
        job_id: Option<String>,
        timestamp_mode: impl Into<String>,
        first_log_at: Option<String>,
        merges: serde_json::Value,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            session_dir,
            tabs: tabs.to_vec(),
            source_files,
            combined_file: combined_file.into(),
            pane_labels,
            pane_kinds,
            pane_commands,
            frontend_plugins,
            pane_plugins,
            plugin_scripts,
            started_at: started_at.into(),
            app_name: app_name.into(),
            config_path,
            job_id,
            title: None,
            timestamp_mode: timestamp_mode.into(),
            first_log_at,
            html_status: "pending".to_string(),
            html_updated_at: None,
            html_error: None,
            next_sequence: 1,
            merges,
        }
    }

    /// Attach the human experiment title persisted in the manifest and APIs.
    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title;
        self
    }

    /// Write the initial manifest.json.
    pub fn write_manifest(&self) -> Result<()> {
        let manifest = self.build_manifest();
        let path = self.manifest_path();
        atomic_write_file(&path, serde_json::to_string_pretty(&manifest)?.as_bytes())
            .with_context(|| format!("write manifest {}", path.display()))?;
        info!("manifest written: {}", path.display());
        Ok(())
    }

    /// Update the manifest with new fields.
    pub fn update_manifest(&self, updates: &serde_json::Value) -> Result<()> {
        let path = self.manifest_path();
        let mut manifest = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(e) => {
                    // Don't silently discard a corrupt manifest: back it up so
                    // the bad data is recoverable, then start fresh.
                    let backup = path.with_extension("json.corrupt");
                    let _ = std::fs::rename(&path, &backup);
                    warn!(
                        "manifest {} is corrupt ({e}); backed up to {} and recreating",
                        path.display(),
                        backup.display()
                    );
                    json!({})
                }
            }
        } else {
            json!({})
        };

        // Merge updates into manifest.
        if let (Some(obj), Some(updates_obj)) = (manifest.as_object_mut(), updates.as_object()) {
            for (key, val) in updates_obj {
                obj.insert(key.clone(), val.clone());
            }
        }

        atomic_write_file(&path, serde_json::to_string_pretty(&manifest)?.as_bytes())
            .with_context(|| format!("update manifest {}", path.display()))?;
        Ok(())
    }

    /// Persist the first log timestamp once.
    pub fn mark_first_log_at(&mut self, timestamp: DateTime<Local>) -> Result<()> {
        if self.first_log_at.is_some() {
            return Ok(());
        }
        let first_log_at = timestamp.to_rfc3339();
        self.first_log_at = Some(first_log_at.clone());
        self.update_manifest(&json!({ "first_log_at": first_log_at }))
    }

    /// Mark the session HTML as exported.
    pub fn mark_html_exported(&mut self, html_path: &Path) -> Result<()> {
        self.html_status = "ready".to_string();
        self.html_error = None;
        let now = Local::now().to_rfc3339();
        self.html_updated_at = Some(now.clone());
        self.update_manifest(&json!({
            "session_html": html_path.display().to_string(),
            "html_status": "ready",
            "html_updated_at": now,
            "html_error": serde_json::Value::Null,
            "last_export_reason": "manual",
        }))
    }

    /// Mark the HTML export as failed.
    pub fn mark_html_error(&mut self, error: &str) -> Result<()> {
        self.html_status = "error".to_string();
        self.html_error = Some(error.to_string());
        let now = Local::now().to_rfc3339();
        self.html_updated_at = Some(now.clone());
        self.update_manifest(&json!({
            "html_status": "error",
            "html_error": error,
            "html_updated_at": now,
        }))
    }

    /// Load existing markers from markers.json.
    pub fn load_markers(&self) -> Vec<serde_json::Value> {
        let path = self.session_dir.join("markers.json");
        if !path.exists() {
            return Vec::new();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Array(markers)) => markers,
                Ok(serde_json::Value::Object(mut obj)) => obj
                    .remove("markers")
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
            Err(_) => Vec::new(),
        }
    }

    /// Assign the next session-global sequence and append one combined record.
    /// Callers serialize this with live replay/broadcast publication.
    pub fn append_combined_entry(&mut self, entry: &mut serde_json::Value) -> Result<u64> {
        let sequence = self.next_sequence;
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .context("session sequence exhausted")?;
        if let Some(object) = entry.as_object_mut() {
            object.insert("sequence".to_string(), json!(sequence));
            object.insert("session_id".to_string(), json!(self.session_id));
        }
        let path = PathBuf::from(&self.combined_file);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open combined.jsonl {}", path.display()))?;
        let line = serde_json::to_string(entry)?;
        use std::io::Write;
        writeln!(file, "{line}")
            .with_context(|| format!("append combined entry to {}", path.display()))?;
        self.next_sequence = next_sequence;
        Ok(sequence)
    }

    /// Save markers to markers.json using the original-compatible wrapper shape.
    pub fn save_markers(&self, markers: &[serde_json::Value]) -> Result<()> {
        let path = self.session_dir.join("markers.json");
        let body = json!({
            "session_id": self.session_id,
            "markers": markers,
        });
        atomic_write_file(&path, serde_json::to_string_pretty(&body)?.as_bytes())
            .with_context(|| format!("save markers {}", path.display()))?;
        Ok(())
    }

    /// Insert `new_marker`, replacing any existing marker at the same
    /// `(paneId, lineIdx)`, and persist. Returns the full marker list so the
    /// caller can broadcast a `markers_update`.
    pub fn replace_marker(
        &self,
        pane_id: &str,
        line_idx: u64,
        new_marker: serde_json::Value,
    ) -> Result<Vec<serde_json::Value>> {
        let mut markers = self.load_markers();
        markers.retain(|m| {
            let same_pane = m.get("paneId").and_then(|v| v.as_str()) == Some(pane_id);
            let same_idx = m.get("lineIdx").and_then(|v| v.as_u64()) == Some(line_idx);
            !(same_pane && same_idx)
        });
        markers.push(new_marker);
        self.save_markers(&markers)?;
        Ok(markers)
    }

    /// Number of records appended to the current session.
    pub fn record_count(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    /// Build the session info payload sent to the frontend and HTTP clients.
    pub fn build_session_info(&self) -> serde_json::Value {
        let html_path = self.html_path();
        json!({
            "id": self.session_id,
            "job_id": self.job_id,
            "title": self.title,
            "app_name": self.app_name,
            "system_timezone": Local::now().offset().to_string(),
            "dir": self.session_dir.display().to_string(),
            "manifest": self.manifest_path().display().to_string(),
            "html": format!("/sessions/{}/session.html", self.session_id),
            "html_ready": self.html_status == "ready" && html_path.exists(),
            "html_status": self.html_status,
            "html_updated_at": self.html_updated_at,
            "html_error": self.html_error,
            "api": {
                "current": "/api/session/current",
                "export": "/api/session/export",
                "rotate": "/api/session/rotate",
                "sessions": "/api/sessions",
                "stats": "/api/stats",
                "health": "/api/health",
            },
            "started_at": self.started_at,
            "timestamp_mode": self.timestamp_mode,
            "first_log_at": self.first_log_at,
            "tabs": self.tabs,
            "pane_labels": self.pane_labels,
            "frontend_plugins": self.frontend_plugins,
            "pane_plugins": self.pane_plugins,
            "pane_kinds": self.pane_kinds,
            "pane_commands": self.pane_commands,
            "plugin_scripts": self.plugin_scripts,
            "sources": self.source_files,
            "source_files": self.source_files,
            "combined_file": self.combined_file,
            "merges": self.merges,
        })
    }

    fn build_manifest(&self) -> serde_json::Value {
        let html_path = self.html_path();
        json!({
            "session_id": self.session_id,
            "session_dir": self.session_dir.display().to_string(),
            "started_at": self.started_at,
            "system_timezone": Local::now().offset().to_string(),
            "job_id": self.job_id,
            "title": self.title,
            "config_path": self.config_path,
            "timestamp_mode": self.timestamp_mode,
            "first_log_at": self.first_log_at,
            "tabs": self.tabs,
            "pane_labels": self.pane_labels,
            "frontend_plugins": self.frontend_plugins,
            "pane_plugins": self.pane_plugins,
            "pane_kinds": self.pane_kinds,
            "pane_commands": self.pane_commands,
            "plugin_scripts": self.plugin_scripts,
            "source_files": self.source_files,
            "combined_file": self.combined_file,
            "merges": self.merges,
            "session_html": html_path.display().to_string(),
            "last_export_reason": serde_json::Value::Null,
            "html_status": self.html_status,
            "html_updated_at": self.html_updated_at,
            "html_error": self.html_error,
        })
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn combined_file(&self) -> PathBuf {
        PathBuf::from(&self.combined_file)
    }

    fn manifest_path(&self) -> PathBuf {
        self.session_dir.join("manifest.json")
    }

    fn html_path(&self) -> PathBuf {
        self.session_dir.join("session.html")
    }
}

pub(crate) fn atomic_write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    let nonce = ARTIFACT_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut temp = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        temp.write_all(bytes)?;
        temp.sync_all()?;
        std::fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_session_dir(name: &str) -> PathBuf {
        let nanos = Local::now().timestamp_nanos_opt().unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "embed-log-core-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manager(dir: PathBuf) -> SessionManager {
        let mut source_files = HashMap::new();
        source_files.insert("dut".to_string(), dir.join("dut.log").display().to_string());

        let mut pane_labels = HashMap::new();
        pane_labels.insert("dut".to_string(), "DUT".to_string());

        let mut pane_kinds = HashMap::new();
        pane_kinds.insert("dut".to_string(), "udp".to_string());

        SessionManager::new(
            "session-1",
            dir.clone(),
            &[json!({ "label": "Main", "panes": ["dut"] })],
            source_files,
            dir.join("combined.jsonl").display().to_string(),
            pane_labels,
            pane_kinds,
            json!({ "dut": ["help"] }),
            json!({ "hex": { "builtin": "hex" } }),
            json!({ "dut": [{ "name": "hex" }] }),
            json!({ "hex": "export default {};" }),
            "2026-06-13T00:00:00+00:00",
            "embed-log",
            Some("embed-log.yml".to_string()),
            Some("job-1".to_string()),
            "absolute",
            None,
            json!([]),
        )
    }

    #[test]
    fn replace_marker_overwrites_existing_marker_at_line() {
        let dir = temp_session_dir("replace-marker-any");
        let mgr = manager(dir);

        let existing =
            json!({ "paneId": "dut", "lineIdx": 5, "kind": "user", "description": "old" });
        mgr.save_markers(&[existing]).unwrap();

        let replacement =
            json!({ "paneId": "dut", "lineIdx": 5, "kind": "user", "description": "new" });
        let markers = mgr.replace_marker("dut", 5, replacement).unwrap();

        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0]["description"], "new");
    }

    #[test]
    fn update_manifest_backs_up_corrupt_file_instead_of_wiping_it() {
        let dir = temp_session_dir("corrupt-manifest");
        let mgr = manager(dir.clone());
        let path = dir.join("manifest.json");

        // Simulate a corrupt manifest on disk.
        std::fs::write(&path, "{not valid json").unwrap();

        mgr.update_manifest(&json!({ "html_status": "ready" }))
            .unwrap();

        // The bad content is preserved in a backup, not lost.
        let backup = dir.join("manifest.json.corrupt");
        assert!(backup.exists(), "corrupt manifest should be backed up");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{not valid json");

        // The new manifest is valid and contains the update.
        let rebuilt: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(rebuilt["html_status"], "ready");
    }

    #[test]
    fn manifest_and_session_info_include_original_compatibility_fields() {
        let dir = temp_session_dir("manifest");
        let mut mgr = manager(dir.clone());

        mgr.write_manifest().unwrap();
        mgr.mark_first_log_at(Local::now()).unwrap();

        let manifest_text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();

        for key in [
            "session_id",
            "session_dir",
            "started_at",
            "system_timezone",
            "job_id",
            "config_path",
            "timestamp_mode",
            "first_log_at",
            "tabs",
            "pane_labels",
            "frontend_plugins",
            "pane_plugins",
            "pane_kinds",
            "pane_commands",
            "plugin_scripts",
            "source_files",
            "combined_file",
            "merges",
            "session_html",
            "last_export_reason",
            "html_status",
            "html_updated_at",
            "html_error",
        ] {
            assert!(manifest.get(key).is_some(), "missing manifest key {key}");
        }

        let session = mgr.build_session_info();
        for key in [
            "id",
            "job_id",
            "app_name",
            "system_timezone",
            "dir",
            "manifest",
            "html",
            "html_ready",
            "html_status",
            "html_updated_at",
            "html_error",
            "api",
            "started_at",
            "timestamp_mode",
            "first_log_at",
            "tabs",
            "pane_labels",
            "frontend_plugins",
            "pane_plugins",
            "pane_kinds",
            "pane_commands",
            "sources",
            "combined_file",
            "merges",
        ] {
            assert!(session.get(key).is_some(), "missing session key {key}");
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_combined_entry_writes_jsonl() {
        let dir = temp_session_dir("combined");
        let mut mgr = manager(dir.clone());
        let mut payload = json!({ "source_id": "dut", "message": "boot", "source_kind": "udp" });
        assert_eq!(mgr.append_combined_entry(&mut payload).unwrap(), 1);
        let mut second = json!({ "source_id": "host", "message": "next" });
        assert_eq!(mgr.append_combined_entry(&mut second).unwrap(), 2);
        let path = dir.join("combined.jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["source_id"], "dut");
        assert_eq!(parsed["message"], "boot");
        assert_eq!(parsed["sequence"], 1);
        assert_eq!(parsed["session_id"], mgr.session_id());
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["sequence"], 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn markers_round_trip_in_original_wrapper_shape() {
        let dir = temp_session_dir("markers");
        let mgr = manager(dir.clone());

        let markers = vec![json!({ "pane": "dut", "line": 2, "label": "boot" })];
        mgr.save_markers(&markers).unwrap();

        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("markers.json")).unwrap())
                .unwrap();
        assert_eq!(raw["session_id"], "session-1");
        assert_eq!(raw["markers"], json!(markers));
        assert_eq!(mgr.load_markers(), markers);

        std::fs::remove_dir_all(dir).unwrap();
    }
}
