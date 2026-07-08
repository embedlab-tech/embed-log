use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde_json::json;
use tracing::{info, warn};

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
    timestamp_mode: String,
    first_log_at: Option<String>,
    html_status: String,
    html_updated_at: Option<String>,
    html_error: Option<String>,
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
            timestamp_mode: timestamp_mode.into(),
            first_log_at,
            html_status: "pending".to_string(),
            html_updated_at: None,
            html_error: None,
        }
    }

    /// Write the initial manifest.json.
    pub fn write_manifest(&self) -> Result<()> {
        let manifest = self.build_manifest();
        let path = self.manifest_path();
        std::fs::write(&path, serde_json::to_string_pretty(&manifest)?)
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

        std::fs::write(&path, serde_json::to_string_pretty(&manifest)?)
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

    /// Append one combined log entry as a JSON line to combined.jsonl.
    pub fn append_combined_entry(&self, entry: &serde_json::Value) -> Result<()> {
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
        Ok(())
    }

    /// Append one event as a JSON line to events.jsonl.
    pub fn append_event(&self, event: &serde_json::Value) -> Result<()> {
        let path = self.session_dir.join("events.jsonl");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open events.jsonl {}", path.display()))?;
        let line = serde_json::to_string(event)?;
        use std::io::Write;
        writeln!(file, "{line}").with_context(|| format!("append event to {}", path.display()))?;
        Ok(())
    }

    /// Load all events from events.jsonl, preserving order.
    pub fn load_events(&self) -> Vec<serde_json::Value> {
        let path = self.session_dir.join("events.jsonl");
        if !path.exists() {
            return Vec::new();
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Save markers to markers.json using the original-compatible wrapper shape.
    pub fn save_markers(&self, markers: &[serde_json::Value]) -> Result<()> {
        let path = self.session_dir.join("markers.json");
        let body = json!({
            "session_id": self.session_id,
            "markers": markers,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&body)?)
            .with_context(|| format!("save markers {}", path.display()))?;
        Ok(())
    }

    /// Insert `new_marker`, replacing any existing marker at the same
    /// `(paneId, lineIdx)`, and persist. With `events_only`, only `kind:
    /// "event"` markers at that position are replaced (user markers are
    /// preserved); otherwise any marker there is replaced. Returns the full
    /// persisted list so the caller can broadcast a `markers_update`.
    ///
    /// Shared by the control-API marker.create handler and the event-marker
    /// writer so the load/replace/save logic lives in one place.
    pub fn replace_marker(
        &self,
        pane_id: &str,
        line_idx: u64,
        new_marker: serde_json::Value,
        events_only: bool,
    ) -> Result<Vec<serde_json::Value>> {
        let mut markers = self.load_markers();
        markers.retain(|m| {
            let same_pane = m.get("paneId").and_then(|v| v.as_str()) == Some(pane_id);
            let same_idx = m.get("lineIdx").and_then(|v| v.as_u64()) == Some(line_idx);
            let is_event = m.get("kind").and_then(|v| v.as_str()).unwrap_or("user") == "event";
            let drop = same_pane && same_idx && (!events_only || is_event);
            !drop
        });
        markers.push(new_marker);
        self.save_markers(&markers)?;
        Ok(markers)
    }

    /// Build the session info payload sent to the frontend and HTTP clients.
    pub fn build_session_info(&self) -> serde_json::Value {
        let html_path = self.html_path();
        json!({
            "id": self.session_id,
            "job_id": self.job_id,
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
        })
    }

    fn build_manifest(&self) -> serde_json::Value {
        let html_path = self.html_path();
        let events_path = self.session_dir.join("events.jsonl");
        json!({
            "session_id": self.session_id,
            "session_dir": self.session_dir.display().to_string(),
            "started_at": self.started_at,
            "system_timezone": Local::now().offset().to_string(),
            "job_id": self.job_id,
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
            "session_html": html_path.display().to_string(),
            "events_file": events_path.display().to_string(),
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

    fn manifest_path(&self) -> PathBuf {
        self.session_dir.join("manifest.json")
    }

    fn html_path(&self) -> PathBuf {
        self.session_dir.join("session.html")
    }
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
        )
    }

    #[test]
    fn replace_marker_events_only_preserves_user_markers() {
        let dir = temp_session_dir("replace-marker-event");
        let mgr = manager(dir);

        let user = json!({ "paneId": "dut", "lineIdx": 5, "kind": "user", "description": "mine" });
        let old_event =
            json!({ "paneId": "dut", "lineIdx": 5, "kind": "event", "description": "old" });
        mgr.save_markers(&[user.clone(), old_event]).unwrap();

        let new_event =
            json!({ "paneId": "dut", "lineIdx": 5, "kind": "event", "description": "new" });
        let markers = mgr.replace_marker("dut", 5, new_event, true).unwrap();

        // User marker survives; the old event at this line is replaced.
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().any(|m| m["kind"] == "user"));
        let events: Vec<_> = markers.iter().filter(|m| m["kind"] == "event").collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["description"], "new");
    }

    #[test]
    fn replace_marker_non_events_only_overwrites_any_marker_at_line() {
        let dir = temp_session_dir("replace-marker-any");
        let mgr = manager(dir);

        let existing =
            json!({ "paneId": "dut", "lineIdx": 5, "kind": "user", "description": "old" });
        mgr.save_markers(&[existing]).unwrap();

        let replacement =
            json!({ "paneId": "dut", "lineIdx": 5, "kind": "user", "description": "new" });
        let markers = mgr.replace_marker("dut", 5, replacement, false).unwrap();

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
        ] {
            assert!(session.get(key).is_some(), "missing session key {key}");
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_combined_entry_writes_jsonl() {
        let dir = temp_session_dir("combined");
        let mgr = manager(dir.clone());
        let payload = json!({ "source_id": "dut", "message": "boot", "source_kind": "udp" });
        mgr.append_combined_entry(&payload).unwrap();
        let path = dir.join("combined.jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["source_id"], "dut");
        assert_eq!(parsed["message"], "boot");
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

    #[test]
    fn append_event_writes_jsonl() {
        let dir = temp_session_dir("append-event");
        let mgr = manager(dir.clone());

        let event1 = json!({"type": "event", "event_id": "boot_complete", "severity": "info"});
        let event2 = json!({"type": "event", "event_id": "fatal_error", "severity": "error"});

        mgr.append_event(&event1).unwrap();
        mgr.append_event(&event2).unwrap();

        let path = dir.join("events.jsonl");
        assert!(path.exists());

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[0]).unwrap(),
            event1
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[1]).unwrap(),
            event2
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_events_reads_all_events_in_order() {
        let dir = temp_session_dir("load-events");
        let mgr = manager(dir.clone());

        let event1 = json!({"type": "event", "event_id": "a"});
        let event2 = json!({"type": "event", "event_id": "b"});
        let event3 = json!({"type": "event", "event_id": "c"});

        mgr.append_event(&event1).unwrap();
        mgr.append_event(&event2).unwrap();
        mgr.append_event(&event3).unwrap();

        let events = mgr.load_events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], event1);
        assert_eq!(events[1], event2);
        assert_eq!(events[2], event3);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_events_empty_when_no_file() {
        let dir = temp_session_dir("no-events");
        let mgr = manager(dir.clone());
        let events = mgr.load_events();
        assert!(events.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
