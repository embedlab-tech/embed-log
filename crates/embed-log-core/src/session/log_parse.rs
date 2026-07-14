//! Log-file parsing and timestamp enrichment for the static HTML exporter.
//!
//! Splits the "turn raw `.log` text into timestamped entries" concern out of
//! `exporter.rs`, which keeps only HTML generation. Matches the behaviour of
//! the original Python `merge_logs.py` parser.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use regex::Regex;

/// A parsed log entry with timestamp variants.
pub(super) struct LogEntry {
    pub(super) ts: String,
    pub(super) text: String,
    pub(super) is_tx: bool,
    pub(super) abs_ts: Option<String>,
    pub(super) abs_num: Option<i64>,
    pub(super) rel_ts: Option<String>,
    pub(super) rel_num: Option<i64>,
}

pub(super) fn parse_log_file(
    content: &str,
    pane_id: Option<&str>,
    pane_label: Option<&str>,
) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    let mut pending: Option<LogEntry> = None;
    let prefix_variants = prefix_variants(pane_id, pane_label);

    for raw_line in content.lines() {
        if let Some((ts, text)) = parse_line(raw_line) {
            // Flush previous entry.
            if let Some(entry) = pending.take() {
                entries.push(entry);
            }
            let is_tx = text.contains("[TX::");
            let clean_text = strip_embedlog_prefixes(&text, &prefix_variants);

            let (abs_ts, abs_num, rel_ts, rel_num) = if let Some(ms) = relative_ts_to_ms(&ts) {
                (None, None, Some(ts.clone()), Some(ms))
            } else {
                let abs_num = parse_absolute_to_ms(raw_line);
                (Some(ts.clone()), abs_num, None, None)
            };

            pending = Some(LogEntry {
                ts,
                text: clean_text,
                is_tx,
                abs_ts,
                abs_num,
                rel_ts,
                rel_num,
            });
        } else if raw_line.trim().is_empty() {
            continue;
        } else if let Some(ref mut entry) = pending {
            // Continuation line — append to previous entry.
            entry.text.push(' ');
            entry.text.push_str(raw_line.trim());
        }
    }
    if let Some(entry) = pending {
        entries.push(entry);
    }
    entries
}

fn parse_line(raw: &str) -> Option<(String, String)> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }

    // Strip ANSI prefix.
    let re = ansi_prefix_re();
    let (line, ansi_prefix) = if let Some(m) = re.find(line) {
        (&line[m.end()..], &line[..m.end()])
    } else {
        (line, "")
    };

    // [MM-DD HH:MM:SS.mmm] message
    let re = short_space_bracket_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[1],
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            ms3(caps.get(6).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[7])));
    }

    // [T+HH:MM:SS.mmm] message
    let re = relative_bracket_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "T+{}:{}:{}.{}",
            &caps[1],
            &caps[2],
            &caps[3],
            ms3(caps.get(4).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[5])));
    }

    // [YYYY-MM-DDTHH:MM:SS.mmm] message (full ISO bracket)
    let re = full_iso_bracket_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            &caps[6],
            ms3(caps.get(7).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[8])));
    }

    // Bare ISO: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DD HH:MM:SS
    let re = bare_iso_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            &caps[6],
            ms3(caps.get(7).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[8])));
    }

    let re = space_iso_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            &caps[6],
            ms3(caps.get(7).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[8])));
    }

    // Bare relative: T+HH:MM:SS.mmm
    let re = bare_relative_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "T+{}:{}:{}.{}",
            &caps[1],
            &caps[2],
            &caps[3],
            ms3(caps.get(4).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[5])));
    }

    None
}

/// Enrich timestamp variants and return effective first_log_at.
pub(super) fn enrich_timestamps(
    log_data: &mut HashMap<String, Vec<LogEntry>>,
    timestamp_mode: &str,
    first_log_at: &Option<String>,
) -> Option<String> {
    // Try to parse origin from first_log_at. Keep its fixed-offset wall clock
    // when deriving absolute display timestamps; Python merge_logs.py strips
    // timezone suffixes and preserves the supplied clock time.
    let mut origin_fixed: Option<DateTime<FixedOffset>> = None;
    if let Some(fla) = first_log_at {
        let token = if fla.ends_with('Z') {
            format!("{}+00:00", &fla[..fla.len() - 1])
        } else {
            fla.clone()
        };
        origin_fixed = DateTime::parse_from_rfc3339(&token).ok();
    }

    let mut origin_ms = origin_fixed.map(|dt| dt.timestamp_millis());

    // If no origin from first_log_at, try min absNum.
    if origin_ms.is_none() {
        origin_ms = log_data
            .values()
            .flat_map(|entries| entries.iter().filter_map(|e| e.abs_num))
            .min();
    }

    for entries in log_data.values_mut() {
        for entry in entries.iter_mut() {
            // Compute rel from abs.
            if let (None, Some(abs_num), Some(origin_ms)) =
                (entry.rel_num, entry.abs_num, origin_ms)
            {
                let rel = (abs_num - origin_ms).max(0);
                entry.rel_num = Some(rel);
                entry.rel_ts = Some(format_relative_ms(rel));
            }
            // Compute abs from rel. If the user supplied a fixed-offset origin,
            // preserve that origin's displayed clock rather than converting to
            // the machine's local timezone.
            if let (None, Some(rel_num)) = (entry.abs_num, entry.rel_num) {
                if let Some(origin) = origin_fixed {
                    let abs_dt = origin + chrono::Duration::milliseconds(rel_num);
                    entry.abs_num = Some(abs_dt.timestamp_millis());
                    entry.abs_ts = Some(abs_dt.format("%m-%d %H:%M:%S%.3f").to_string());
                } else if let Some(ms) = origin_ms {
                    if let Some(abs_utc) = Utc.timestamp_millis_opt(ms + rel_num).single() {
                        entry.abs_num = Some(abs_utc.timestamp_millis());
                        let local = abs_utc.with_timezone(&Local);
                        entry.abs_ts = Some(format_absolute_display(&local));
                    }
                }
            }
            // Set display ts based on mode.
            if timestamp_mode == "relative" {
                if let Some(rel_ts) = &entry.rel_ts {
                    entry.ts = rel_ts.clone();
                } else if let Some(abs_ts) = &entry.abs_ts {
                    entry.ts = abs_ts.clone();
                }
            } else if let Some(abs_ts) = &entry.abs_ts {
                entry.ts = abs_ts.clone();
            } else if let Some(rel_ts) = &entry.rel_ts {
                entry.ts = rel_ts.clone();
            }
        }
    }

    origin_fixed
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, false))
        .or_else(|| first_log_at.clone())
}

// ── Regex patterns (compiled once via OnceLock) ──

fn ansi_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:\x1b\[[0-9;]*m)+").unwrap())
}

fn short_space_bracket_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\[(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})\.(\d+)\]\s?(.*)").unwrap()
    })
}

fn relative_bracket_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[T\+(\d{1,2}):(\d{2}):(\d{2})\.(\d+)\]\s?(.*)").unwrap())
}

fn full_iso_bracket_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\[(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})\.(\d+)(?:[Zz]|[+-]\d{2}:?\d{2})?\]\s?(.*)").unwrap()
    })
}

fn bare_iso_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})\.(\d+)(?:[Zz]|[+-]\d{2}:?\d{2})?\s?(.*)").unwrap()
    })
}

fn space_iso_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})\.(\d+)(?:[Zz]|[+-]\d{2}:?\d{2})?\s?(.*)").unwrap()
    })
}

fn bare_relative_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^T\+(\d{1,2}):(\d{2}):(\d{2})\.(\d+)\s?(.*)").unwrap())
}

// ── Helpers ──

fn ms3(frac: Option<&str>) -> String {
    match frac {
        Some(f) => {
            let f = f.trim_end_matches('Z').trim_end_matches('z');
            if f.len() >= 3 {
                f[..3].to_string()
            } else {
                format!("{f:0<3}")
            }
        }
        None => "000".to_string(),
    }
}

fn relative_ts_to_ms(ts: &str) -> Option<i64> {
    let caps = relative_ts_re().captures(ts)?;
    let h: i64 = caps[1].parse().ok()?;
    let m: i64 = caps[2].parse().ok()?;
    let s: i64 = caps[3].parse().ok()?;
    let ms_str = &caps[4];
    let ms: i64 = if ms_str.len() >= 3 {
        ms_str[..3].parse().ok()?
    } else {
        format!("{ms_str:0<3}")[..3].parse().ok()?
    };
    Some(h * 3_600_000 + m * 60_000 + s * 1000 + ms)
}

fn format_relative_ms(total_ms: i64) -> String {
    let neg = total_ms < 0;
    let total = total_ms.unsigned_abs();
    let hours = total / 3_600_000;
    let minutes = (total % 3_600_000) / 60_000;
    let seconds = (total % 60_000) / 1000;
    let millis = total % 1000;
    if neg {
        format!("T+-{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("T+{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    }
}

fn format_absolute_display(dt: &DateTime<Local>) -> String {
    dt.format("%m-%d %H:%M:%S%.3f").to_string()
}

fn parse_absolute_to_ms(raw: &str) -> Option<i64> {
    // Try full ISO: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DD HH:MM:SS.
    // Keep this allocation-free on the hot path; shutdown/export may parse
    // hundreds of thousands of lines.
    let line = raw.trim();
    let line = ansi_prefix_re()
        .find(line)
        .filter(|m| m.start() == 0)
        .map(|m| &line[m.end()..])
        .unwrap_or(line)
        .trim_start();

    // Try bracket format first.
    let inner = if line.starts_with('[') {
        line.find(']').map(|end| &line[1..end])
    } else {
        Some(line)
    }?;

    // Parse YYYY-MM-DD[THH:MM:SS.mmm].
    let caps = absolute_ts_re().captures(inner)?;
    let year: i32 = caps[1].parse().ok()?;
    let month: u32 = caps[2].parse().ok()?;
    let day: u32 = caps[3].parse().ok()?;
    let hour: u32 = caps[4].parse().ok()?;
    let min: u32 = caps[5].parse().ok()?;
    let sec: u32 = caps[6].parse().ok()?;
    let nano = caps
        .get(7)
        .map(|m| {
            let frac = m.as_str();
            let padded = format!("{frac:0<9}");
            padded[..9].parse::<u32>().unwrap_or(0)
        })
        .unwrap_or(0);

    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_nano_opt(hour, min, sec, nano)?;
    let ndt = NaiveDateTime::new(date, time);
    let local = Local.from_local_datetime(&ndt).single()?;
    Some(local.timestamp_millis())
}

fn relative_ts_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^T\+(\d{1,2}):(\d{2}):(\d{2})\.(\d+)$").unwrap())
}

fn absolute_ts_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?").unwrap()
    })
}

fn prefix_variants(pane_id: Option<&str>, pane_label: Option<&str>) -> Vec<String> {
    let mut variants = std::collections::HashSet::new();
    for value in [pane_id, pane_label].into_iter().flatten() {
        variants.insert(value.to_string());
        variants.insert(value.replace('-', "_"));
        variants.insert(value.replace('_', "-"));
    }
    variants.into_iter().collect()
}

fn strip_embedlog_prefixes(text: &str, variants: &[String]) -> String {
    let mut rest = text;
    let mut changed = false;

    while let Some(after_prefix) = strip_bracket_prefix(rest, "SERIAL").or_else(|| {
        variants
            .iter()
            .find_map(|variant| strip_bracket_prefix(rest, variant))
    }) {
        rest = after_prefix.trim_start();
        changed = true;
    }

    if changed {
        rest.to_string()
    } else {
        text.to_string()
    }
}

fn strip_bracket_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let text = text.trim_start();
    let after_open = text.strip_prefix('[')?;
    let end = after_open.find(']')?;
    let name = &after_open[..end];
    if name.eq_ignore_ascii_case(prefix) {
        Some(&after_open[end + 1..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_short_space_bracket() {
        let (ts, msg) = parse_line("[06-15 14:30:05.123] hello world").unwrap();
        assert_eq!(ts, "06-15 14:30:05.123");
        assert_eq!(msg, "hello world");
    }

    #[test]
    fn parse_relative_bracket() {
        let (ts, msg) = parse_line("[T+00:00:05.250] boot ok").unwrap();
        assert_eq!(ts, "T+00:00:05.250");
        assert_eq!(msg, "boot ok");
    }

    #[test]
    fn parse_full_iso_bracket() {
        let (ts, msg) = parse_line("[2024-06-15T14:30:05.123] test").unwrap();
        assert_eq!(ts, "06-15 14:30:05.123");
        assert_eq!(msg, "test");
    }

    #[test]
    fn parse_line_no_timestamp_returns_none() {
        assert!(parse_line("raw message without timestamp").is_none());
    }

    #[test]
    fn parse_line_empty_returns_none() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
    }

    #[test]
    fn continuation_lines_join_previous() {
        let content = "[T+00:00:00.000] boot ok\nstack trace line 2\n[T+00:00:01.000] next";
        let entries = parse_log_file(content, None, None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "boot ok stack trace line 2");
        assert_eq!(entries[1].text, "next");
    }

    #[test]
    fn tx_detection() {
        let content = "[T+00:00:00.000] [TX::UI] ping";
        let entries = parse_log_file(content, None, None);
        assert!(entries[0].is_tx);
    }

    #[test]
    fn strips_common_runtime_prefixes_without_regex_per_line() {
        let content = "[T+00:00:00.000] [SERIAL] [DUT_UART] boot";
        let entries = parse_log_file(content, Some("DUT_UART"), Some("DUT UART"));
        assert_eq!(entries[0].text, "boot");
    }

    #[test]
    fn relative_ts_to_ms_correct() {
        assert_eq!(relative_ts_to_ms("T+00:00:00.000"), Some(0));
        assert_eq!(relative_ts_to_ms("T+00:00:01.250"), Some(1250));
        assert_eq!(relative_ts_to_ms("T+01:02:03.456"), Some(3_723_456));
    }
}
