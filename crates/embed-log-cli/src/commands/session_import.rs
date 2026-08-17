//! Offline import of an external, timestamped log file into a saved session.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use regex::Regex;
use serde_json::{json, Value};

use embed_log_core::naming::slugify;

use super::sessions::{resolve_session, LogDirArgs};

#[derive(Debug, Clone)]
struct ImportedEntry {
    timestamp: DateTime<chrono::FixedOffset>,
    message: String,
}

/// Import an external absolute-timestamped file as a new source and tab.
pub(crate) fn import_session(
    session_id: &str,
    log_dir: &LogDirArgs,
    input: &Path,
    source: Option<&str>,
    tab: &str,
    label: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let session = resolve_session(&super::sessions::resolve_sessions_dir(log_dir)?, session_id)?;
    let source_id = source
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let stem = input
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("imported");
            let slug = slugify(stem);
            if slug.is_empty() {
                "imported-log".to_string()
            } else {
                slug
            }
        });
    let pane_label = label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&source_id);

    let result = import_into_session(
        &session.dir,
        &session.manifest,
        input,
        &source_id,
        tab,
        pane_label,
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "session_id": session.id,
                "source_id": source_id,
                "tab": tab,
                "label": pane_label,
                "records": result.records,
                "source_file": result.source_file,
                "combined_file": result.combined_file,
            }))?
        );
    } else {
        println!(
            "imported {} records into session {} as source {} (tab {:?})",
            result.records, session.id, source_id, tab
        );
    }
    Ok(())
}

struct ImportResult {
    records: usize,
    source_file: PathBuf,
    combined_file: PathBuf,
}

fn import_into_session(
    session_dir: &Path,
    manifest: &Value,
    input: &Path,
    source_id: &str,
    tab_label: &str,
    pane_label: &str,
) -> Result<ImportResult> {
    anyhow::ensure!(!source_id.contains('/'), "source id must not contain '/'");
    anyhow::ensure!(!tab_label.trim().is_empty(), "tab label must not be empty");
    anyhow::ensure!(
        input.is_file(),
        "input log does not exist: {}",
        input.display()
    );

    let existing_sources = manifest
        .get("source_files")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    anyhow::ensure!(
        !existing_sources.contains_key(source_id),
        "source already exists in session: {source_id}"
    );

    let entries = parse_import_file(input)?;
    anyhow::ensure!(
        !entries.is_empty(),
        "input contains no timestamped log entries"
    );

    let source_filename = format!("imported__{}.log", slugify(source_id));
    let source_file = unique_path(session_dir, &source_filename);
    fs::copy(input, &source_file)
        .with_context(|| format!("copy imported log to {}", source_file.display()))?;

    let combined_file = manifest
        .get("combined_file")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| session_dir.join("combined.jsonl"));
    let mut next_sequence = max_sequence(&combined_file)?.saturating_add(1);
    let mut combined = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&combined_file)
        .with_context(|| format!("open combined file {}", combined_file.display()))?;

    for (line_idx, entry) in entries.iter().enumerate() {
        let abs_num = entry.timestamp.timestamp_millis();
        let abs_ts = entry.timestamp.format("%m-%d %H:%M:%S%.3f").to_string();
        let timestamp_iso = entry.timestamp.to_rfc3339();
        let record = json!({
            "type": "rx",
            "data": entry.message,
            "message": entry.message,
            "timestamp": abs_ts,
            "timestamp_iso": timestamp_iso,
            "timestamp_num": abs_num,
            "absTs": abs_ts,
            "absNum": abs_num,
            "source_id": source_id,
            "source_kind": "file-import",
            "source_label": pane_label,
            "origin": "IMPORT",
            "line_idx": line_idx,
            "sequence": next_sequence,
            "session_id": manifest.get("session_id").and_then(Value::as_str).unwrap_or_default(),
        });
        writeln!(combined, "{}", serde_json::to_string(&record)?)
            .with_context(|| format!("append imported record to {}", combined_file.display()))?;
        next_sequence = next_sequence.saturating_add(1);
    }
    combined.flush()?;

    let mut updated = manifest.clone();
    let root = updated
        .as_object_mut()
        .context("session manifest must be an object")?;
    root.entry("source_files")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("manifest source_files must be an object")?
        .insert(source_id.to_string(), json!(source_file));
    root.entry("pane_labels")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("manifest pane_labels must be an object")?
        .insert(source_id.to_string(), json!(pane_label));
    root.entry("pane_kinds")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("manifest pane_kinds must be an object")?
        .insert(source_id.to_string(), json!("file-import"));
    root.entry("tabs")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("manifest tabs must be an array")?
        .push(json!({ "label": tab_label, "panes": [source_id] }));
    root.insert("html_status".to_string(), json!("pending"));
    root.insert("html_error".to_string(), Value::Null);
    atomic_write_json(&session_dir.join("manifest.json"), &updated)?;

    Ok(ImportResult {
        records: entries.len(),
        source_file,
        combined_file,
    })
}

fn parse_import_file(path: &Path) -> Result<Vec<ImportedEntry>> {
    let file = fs::File::open(path)?;
    let timestamp_re = Regex::new(r"^\s*\[([^\]]+)\]\s?(.*)$")?;
    // CI device dumps use Python-dict syntax rather than strict JSONL. We only
    // need the timestamp for ordering/synchronization; preserving the complete
    // original record as the message avoids lossy or unsafe pseudo-JSON parsing.
    let structured_timestamp_re = Regex::new(r#"['\"]timestamp['\"]\s*:\s*['\"]([^'\"]+)['\"]"#)?;
    // Fallback for records where the timestamp is embedded in another textual
    // representation and is not stored under a `timestamp` field.
    let embedded_timestamp_re = Regex::new(
        r"(?P<timestamp>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2}))",
    )?;
    let mut entries = Vec::new();
    let mut pending: Option<ImportedEntry> = None;

    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Some(caps) = timestamp_re.captures(&line) {
            if let Some(entry) = pending.take() {
                entries.push(entry);
            }
            let raw_ts = &caps[1];
            let timestamp = parse_absolute_timestamp(raw_ts)
                .with_context(|| format!("unsupported or invalid timestamp [{raw_ts}]"))?;
            pending = Some(ImportedEntry {
                timestamp,
                message: caps[2].to_string(),
            });
        } else if let Some(caps) = structured_timestamp_re.captures(&line) {
            if let Some(entry) = pending.take() {
                entries.push(entry);
            }
            let raw_ts = &caps[1];
            let timestamp = parse_absolute_timestamp(raw_ts).with_context(|| {
                format!("unsupported or invalid structured timestamp {raw_ts:?}")
            })?;
            pending = Some(ImportedEntry {
                timestamp,
                message: line.trim().to_string(),
            });
        } else if let Some(caps) = embedded_timestamp_re.captures(&line) {
            if let Some(entry) = pending.take() {
                entries.push(entry);
            }
            let raw_ts = caps.name("timestamp").unwrap().as_str();
            let timestamp = parse_absolute_timestamp(raw_ts)
                .with_context(|| format!("unsupported or invalid embedded timestamp {raw_ts:?}"))?;
            pending = Some(ImportedEntry {
                timestamp,
                message: line.trim().to_string(),
            });
        } else if !line.trim().is_empty() {
            if let Some(entry) = pending.as_mut() {
                if !entry.message.is_empty() {
                    entry.message.push(' ');
                }
                entry.message.push_str(line.trim());
            } else {
                bail!("untimestamped line before the first timestamp: {line:?}");
            }
        }
    }
    if let Some(entry) = pending {
        entries.push(entry);
    }

    // Stable ordering is required by the frontend's binary-search synchronizer.
    entries.sort_by_key(|entry| entry.timestamp.timestamp_millis());
    Ok(entries)
}

fn parse_absolute_timestamp(raw: &str) -> Result<DateTime<chrono::FixedOffset>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(raw) {
        return Ok(value);
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(raw, format) {
            let local = Local
                .from_local_datetime(&value)
                .single()
                .context("ambiguous local timestamp")?;
            return Ok(local.fixed_offset());
        }
    }
    bail!("invalid absolute timestamp: {raw}")
}

fn max_sequence(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let file = fs::File::open(path)?;
    let mut max = 0;
    for line in BufReader::new(file).lines() {
        let value: Value = serde_json::from_str(&line?)?;
        max = max.max(value.get("sequence").and_then(Value::as_u64).unwrap_or(0));
    }
    Ok(max)
}

fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("imported");
    let ext = Path::new(filename)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("log");
    for index in 2.. {
        let candidate = dir.join(format!("{stem}__{index}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn atomic_write_json(path: &Path, value: &Value) -> Result<()> {
    let temp = path.with_extension("json.importing");
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_utc_and_sorts_entries_for_sync() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("device.log");
        fs::write(
            &input,
            "[2026-08-16T23:37:47.000Z] later\n[2026-08-16T23:37:46.443Z] first\n",
        )
        .unwrap();
        let entries = parse_import_file(&input).unwrap();
        assert_eq!(entries[0].message, "first");
        assert_eq!(entries[0].timestamp.timestamp_millis(), 1786923466443);
        assert_eq!(entries[1].message, "later");
    }

    #[test]
    fn parses_python_dict_device_dump_and_preserves_record() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("device.log");
        fs::write(
            &input,
            "{'seq': 2, 'timestamp': '2026-08-16T23:44:47.207000000Z', 'code': 'TAMPER'}\n",
        )
        .unwrap();
        let entries = parse_import_file(&input).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].message,
            fs::read_to_string(&input).unwrap().trim()
        );
        assert_eq!(entries[0].timestamp.timestamp_millis(), 1786923887207);
    }

    #[test]
    fn parses_embedded_timestamp_without_timestamp_field() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("device.log");
        fs::write(&input, "event happened at 2026-08-16T23:44:47.207Z\n").unwrap();
        let entries = parse_import_file(&input).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].message,
            "event happened at 2026-08-16T23:44:47.207Z"
        );
    }

    #[test]
    fn imports_manifest_and_combined_records() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("device.log");
        fs::write(&input, "[2026-08-16T23:37:46.443Z] boot\n").unwrap();
        let combined = dir.path().join("combined.jsonl");
        fs::write(&combined, "{\"sequence\":7}\n").unwrap();
        let manifest = json!({
            "session_id": "session-1",
            "source_files": {},
            "tabs": [],
            "combined_file": combined,
        });
        let result = import_into_session(
            dir.path(),
            &manifest,
            &input,
            "DEVICE",
            "Device log",
            "Device",
        )
        .unwrap();
        assert_eq!(result.records, 1);
        let updated: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(updated["tabs"][0]["panes"][0], "DEVICE");
        let line = fs::read_to_string(combined).unwrap();
        assert!(line.contains("\"sequence\":8"));
        assert!(line.contains("\"source_kind\":\"file-import\""));
    }
}
