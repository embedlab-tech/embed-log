//! `embed-log sessions events` and `embed-log sessions decode`: device
//! restarts/crashes recorded in a session, and crash dumps symbolized with
//! the firmware ELF.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use embed_log_core::symbols;

use crate::commands::sessions::{resolve_session, SessionRecord};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionEvent {
    pub sequence: u64,
    pub source_id: String,
    pub event: String,
    pub timestamp: String,
    pub message: String,
}

fn journal(session: &SessionRecord) -> PathBuf {
    session
        .manifest
        .get("combined_file")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| session.dir.join("combined.jsonl"))
}

/// Restart/crash records of a session journal, oldest first.
pub(crate) fn session_events(journal: &Path) -> Result<Vec<SessionEvent>> {
    let file = std::fs::File::open(journal)
        .with_context(|| format!("cannot open {}", journal.display()))?;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if !line.contains("\"event\":") {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let (Some(event), Some(sequence), Some(source)) = (
            record.get("event").and_then(|v| v.as_str()),
            record.get("sequence").and_then(|v| v.as_u64()),
            record.get("source_id").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        events.push(SessionEvent {
            sequence,
            source_id: source.to_string(),
            event: event.to_string(),
            timestamp: record
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            message: embed_log_core::postprocess::strip_ansi(
                record
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            ),
        });
    }
    Ok(events)
}

fn kind_matches(event: &SessionEvent, kind: Option<&str>) -> bool {
    match kind {
        None => true,
        Some("restart") => event.event == embed_log_core::events::BOOT,
        Some(kind) => event.event == kind,
    }
}

/// `embed-log sessions events`
pub(crate) fn show_events(
    log_dir: &Path,
    session_id: &str,
    kind: Option<&str>,
    sources: &[String],
    json: bool,
) -> Result<()> {
    let session = resolve_session(log_dir, session_id)?;
    let events: Vec<SessionEvent> = session_events(&journal(&session))?
        .into_iter()
        .filter(|e| kind_matches(e, kind))
        .filter(|e| sources.is_empty() || sources.contains(&e.source_id))
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&events)?);
        return Ok(());
    }
    if events.is_empty() {
        println!("No restarts or crashes in session {}.", session.id);
        return Ok(());
    }
    let mut totals: std::collections::BTreeMap<&str, (u64, u64)> = Default::default();
    for e in &events {
        let entry = totals.entry(&e.source_id).or_default();
        if e.event == embed_log_core::events::BOOT {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    for e in &events {
        let label = if e.event == embed_log_core::events::BOOT {
            "restart"
        } else {
            "crash"
        };
        println!(
            "#{:<8} {:<20} {:<12} {:<8} {}",
            e.sequence, e.timestamp, e.source_id, label, e.message
        );
    }
    println!();
    for (source, (boots, crashes)) in totals {
        println!("{source}: {boots} restart(s), {crashes} crash(es)");
    }
    Ok(())
}

/// `embed-log sessions decode`
pub(crate) fn decode_crashes(
    log_dir: &Path,
    session_id: &str,
    sequence: Option<u64>,
    sources: &[String],
    elf: Option<&Path>,
    json: bool,
) -> Result<()> {
    let session = resolve_session(log_dir, session_id)?;
    let journal = journal(&session);
    let crashes: Vec<SessionEvent> = session_events(&journal)?
        .into_iter()
        .filter(|e| e.event == embed_log_core::events::CRASH)
        .filter(|e| sources.is_empty() || sources.contains(&e.source_id))
        .filter(|e| sequence.map_or(true, |seq| e.sequence == seq))
        .collect();
    if crashes.is_empty() {
        match sequence {
            Some(seq) => bail!(
                "record {seq} is not a detected crash in session {}",
                session.id
            ),
            None => bail!("no crashes detected in session {}", session.id),
        }
    }
    // Decoding everything skips sources without an ELF; a crash asked for
    // explicitly (--sequence / --source) must be decodable.
    let explicit = sequence.is_some() || !sources.is_empty();
    let mut reports = Vec::new();
    for crash in &crashes {
        let elf_path = match elf {
            Some(path) => path.to_path_buf(),
            None => {
                match symbols::manifest_elf(&session.dir, &session.manifest, &crash.source_id) {
                    Ok(path) => path,
                    Err(error) if !explicit => {
                        eprintln!(
                            "skipping crash #{} on {}: {error}",
                            crash.sequence, crash.source_id
                        );
                        continue;
                    }
                    Err(error) => bail!("{error}; pass --elf <firmware.elf>"),
                }
            }
        };
        let mut report =
            symbols::decode_crash(&journal, &crash.source_id, crash.sequence, &elf_path)
                .map_err(anyhow::Error::msg)?;
        if elf.is_none() {
            report.elf = elf_path
                .strip_prefix(&session.dir)
                .unwrap_or(&elf_path)
                .to_string_lossy()
                .replace('\\', "/");
        }
        reports.push(report);
    }
    if reports.is_empty() {
        bail!("no firmware ELF is configured for the crashed sources; pass --elf <firmware.elf>");
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        for (i, report) in reports.iter().enumerate() {
            if i > 0 {
                println!();
            }
            print!("{}", symbols::report_text(report));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_events_from_a_journal() {
        let dir = std::env::temp_dir().join(format!("embed-log-cli-events-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("combined.jsonl");
        std::fs::write(
            &journal,
            [
                r#"{"sequence":1,"source_id":"DUT","event":"boot","message":"*** Booting Zephyr OS ***","timestamp":"t1"}"#,
                r#"{"sequence":2,"source_id":"DUT","message":"<inf> ready"}"#,
                r#"{"sequence":3,"source_id":"NET","event":"crash","message":"\u001b[31mASSERTION FAIL\u001b[0m","timestamp":"t3"}"#,
                r#"{"sequence":4,"source_id":"DUT","message":"partial"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let events = session_events(&journal).unwrap();
        let summary: Vec<_> = events
            .iter()
            .map(|e| {
                (
                    e.sequence,
                    e.source_id.as_str(),
                    e.event.as_str(),
                    e.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (1, "DUT", "boot", "*** Booting Zephyr OS ***"),
                (3, "NET", "crash", "ASSERTION FAIL")
            ]
        );
        assert!(kind_matches(&events[0], Some("restart")));
        assert!(!kind_matches(&events[0], Some("crash")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
