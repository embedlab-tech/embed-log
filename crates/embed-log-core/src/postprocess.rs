//! Log-line postprocessing: denoising for human/agent-facing display, and
//! structural deduplication for compact whole-session exports.
//!
//! Raw session artifacts (`combined.jsonl`, per-source `.log` files) are
//! never modified by anything in this module — everything here is a
//! read-time transform applied by callers (the CLI's `--format` handling,
//! `sessions export --format jsonl-deduped`), not a change to what gets
//! recorded.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// Compaction levels this module implements. `Raw` isn't a variant — untouched
/// data simply skips this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionLevel {
    /// [`dedupe_entry`] — lossless structural dedup for whole-session exports.
    Deduped,
    /// [`denoise_message`] — denoised plaintext for human/agent-facing display.
    Compact,
    /// [`elapsed_time`] plus source-name shortcodes (the latter assigned by
    /// the caller, not this module — see `ShortcodeTable` in the CLI) — an
    /// extra compaction pass on top of `Compact`. Collapsing consecutive
    /// duplicate messages was considered for this level and deliberately
    /// rejected: it makes line-by-line analysis harder (you lose the ability
    /// to point at "the 3rd occurrence" of a repeated line).
    Ultra,
}

fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Same pattern already used by the frontend's static export
    // (frontend/export.js) to strip ANSI/terminal-control sequences from
    // exported text — ported here so the CLI produces equally clean output.
    RE.get_or_init(|| Regex::new(r"\x1b(?:\[[0-9;]*[A-Za-z]|\][^\x07]*\x07|[^\[\]])").unwrap())
}

fn leading_timestamp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}:\d{2}:\d{2}\.\d{3}\s*").unwrap())
}

fn bracket_padding_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\s+([A-Za-z]+)\]").unwrap())
}

fn uptime_counter_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[\d{8}\]\s+").unwrap())
}

/// Strip ANSI/terminal-control sequences (SGR color codes, cursor movement,
/// erase-line, OSC sequences). Real example from a live session: a shell
/// prompt echo like `gwl outside> \x1b[13D\x1b[J[00000000] <inf> ...`.
pub fn strip_ansi(text: &str) -> String {
    ansi_re().replace_all(text, "").into_owned()
}

/// If `message` starts with an `HH:MM:SS.mmm` timestamp equal to `clock_time`
/// (the record's own rendered time), strip it. Some sources stamp their own
/// lines with a timestamp — e.g. pytest output like
/// `15:41:23.644 [   ERROR] Timeout waiting for event='dcf_edhoc'` — which
/// duplicates the record's own `timestamp`/`timestamp_iso` field.
pub fn strip_duplicate_leading_timestamp(message: &str, clock_time: &str) -> String {
    if let Some(m) = leading_timestamp_re().find(message) {
        if m.as_str().trim() == clock_time {
            return message[m.end()..].to_string();
        }
    }
    message.to_string()
}

/// Collapse column-padded bracketed log-level tags: `[   ERROR]` -> `[ERROR]`.
pub fn unpad_bracket_level(text: &str) -> String {
    bracket_padding_re().replace_all(text, "[$1]").into_owned()
}

/// Strip a Zephyr-style device uptime counter (`[00000002] <inf> ...`) while
/// keeping the level tag (`<inf>`/`<err>`/`<wrn>` — real signal). The counter
/// duplicates the record's own relative timestamp, so it's dropped; the tag
/// is not, so this only strips the prefix when a `<...>` tag immediately
/// follows (checked directly rather than via regex lookahead, which the
/// `regex` crate doesn't support).
pub fn strip_uptime_counter(text: &str) -> String {
    if let Some(m) = uptime_counter_re().find(text) {
        let rest = &text[m.end()..];
        if rest.starts_with('<') {
            return rest.to_string();
        }
    }
    text.to_string()
}

/// Apply all [`CompactionLevel::Compact`] denoising steps, in order: strip
/// ANSI, then drop a duplicate leading timestamp, then un-pad bracketed
/// level tags, then drop redundant device uptime counters.
pub fn denoise_message(message: &str, clock_time: &str) -> String {
    let text = strip_ansi(message);
    let text = strip_duplicate_leading_timestamp(&text, clock_time);
    let text = unpad_bracket_level(&text);
    strip_uptime_counter(&text)
}

/// [`CompactionLevel::Ultra`]: session-relative elapsed time (the record's
/// `relNum` field — milliseconds since session start), formatted compactly:
/// `H:MM:SS.mmm` once the session has run an hour, `M:SS.mmm` once it's run a
/// minute, else just `S.mmm`. Shorter than an absolute timestamp for typical
/// session lengths since it never has to encode hour-of-day, and directly
/// answers "how far into the run is this" — usually the more useful question
/// when debugging a test. The absolute anchor isn't lost: it's already
/// recorded once in the session manifest's `started_at` (surfaced by
/// `sessions summary`), so nothing new needs to be stored for it. Falls back
/// to `fallback_clock` (the entry's own absolute wall-clock time) if
/// `relNum` is missing.
pub fn elapsed_time(entry: &Value, fallback_clock: &str) -> String {
    let Some(rel_ms) = entry.get("relNum").and_then(|v| v.as_f64()) else {
        return fallback_clock.to_string();
    };
    let total_ms = rel_ms.max(0.0) as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{ms:03}")
    } else if m > 0 {
        format!("{m}:{s:02}.{ms:03}")
    } else {
        format!("{s}.{ms:03}")
    }
}

/// [`CompactionLevel::Deduped`]: clone `entry` with only fields *measured* as
/// exact duplicates of another field on the same record removed — `data`
/// (identical to `message`), `timestamp_num` (identical to `absNum`),
/// `timestamp` (identical to `absTs`) on every record checked across a real
/// session. Does not touch `color`: it was observed constant-`null` in that
/// session, but it's a per-line rendering attribute by design, not a
/// structural constant, so hoisting it out would be lossy for any session
/// where a line actually is colored. Session/source-constant fields
/// (`app_name`, `job_id`, `session_id`, `source_kind`, `source_label`,
/// `tab_labels`) are intentionally not handled here — hoisting those to a
/// one-time header requires manifest access this module deliberately
/// doesn't have; that's done at the export layer.
pub fn dedupe_entry(entry: &Value) -> Value {
    let mut out = entry.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.remove("data");
        obj.remove("timestamp_num");
        obj.remove("timestamp");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real strings observed in a live session (2026-07-06_14-31-18), not
    // synthetic examples.

    #[test]
    fn strip_ansi_removes_color_and_cursor_control() {
        let msg = "gwl outside> \u{1b}[13D\u{1b}[J[00000000] <inf> rv8263: interrupt configured";
        assert_eq!(
            strip_ansi(msg),
            "gwl outside> [00000000] <inf> rv8263: interrupt configured"
        );
    }

    #[test]
    fn strip_ansi_removes_sgr_wrapped_error() {
        let msg = "\u{1b}[91mTimeout waiting for event='dcf_edhoc'\u{1b}[0m";
        assert_eq!(strip_ansi(msg), "Timeout waiting for event='dcf_edhoc'");
    }

    #[test]
    fn strip_duplicate_leading_timestamp_removes_matching_prefix() {
        let msg = "15:41:23.644 [   ERROR] Timeout waiting for event='dcf_edhoc'";
        assert_eq!(
            strip_duplicate_leading_timestamp(msg, "15:41:23.644"),
            "[   ERROR] Timeout waiting for event='dcf_edhoc'"
        );
    }

    #[test]
    fn strip_duplicate_leading_timestamp_leaves_mismatched_prefix() {
        let msg = "15:41:23.644 something happened at a different logged time";
        assert_eq!(
            strip_duplicate_leading_timestamp(msg, "09:00:00.000"),
            msg
        );
    }

    #[test]
    fn unpad_bracket_level_collapses_padding() {
        assert_eq!(unpad_bracket_level("[   ERROR] boom"), "[ERROR] boom");
        assert_eq!(unpad_bracket_level("[INFO] fine"), "[INFO] fine");
    }

    #[test]
    fn strip_uptime_counter_removes_counter_keeps_level_tag() {
        let msg = "[00000002] <inf> flash_stm32_ospi: Read SFDP from octoFlash";
        assert_eq!(
            strip_uptime_counter(msg),
            "<inf> flash_stm32_ospi: Read SFDP from octoFlash"
        );
    }

    #[test]
    fn strip_uptime_counter_ignores_non_tag_bracket() {
        let msg = "[00000002] not a level tag";
        assert_eq!(strip_uptime_counter(msg), msg);
    }

    #[test]
    fn denoise_message_applies_all_steps() {
        let msg = "15:41:23.644 [   ERROR] \u{1b}[91mTimeout waiting for event='dcf_edhoc'\u{1b}[0m";
        assert_eq!(
            denoise_message(msg, "15:41:23.644"),
            "[ERROR] Timeout waiting for event='dcf_edhoc'"
        );
    }

    #[test]
    fn dedupe_entry_drops_only_measured_duplicates() {
        let entry = serde_json::json!({
            "data": "hello",
            "message": "hello",
            "absNum": 1.0,
            "timestamp_num": 1.0,
            "absTs": "07-06 14:31:31.877",
            "timestamp": "07-06 14:31:31.877",
            "timestamp_iso": "2026-07-06T14:31:31.877+02:00",
            "source_id": "READER",
            "color": "red",
            "line_idx": 0,
        });
        let deduped = dedupe_entry(&entry);
        assert!(deduped.get("data").is_none());
        assert!(deduped.get("timestamp_num").is_none());
        assert!(deduped.get("timestamp").is_none());
        assert_eq!(deduped["message"], "hello");
        assert_eq!(deduped["absNum"], 1.0);
        assert_eq!(deduped["absTs"], "07-06 14:31:31.877");
        assert_eq!(deduped["timestamp_iso"], "2026-07-06T14:31:31.877+02:00");
        assert_eq!(deduped["color"], "red");
        assert_eq!(deduped["line_idx"], 0);
    }

    #[test]
    fn elapsed_time_formats_by_magnitude() {
        assert_eq!(elapsed_time(&serde_json::json!({"relNum": 644.0}), "?"), "0.644");
        assert_eq!(
            elapsed_time(&serde_json::json!({"relNum": 83_644.0}), "?"),
            "1:23.644"
        );
        assert_eq!(
            elapsed_time(&serde_json::json!({"relNum": 3_723_644.0}), "?"),
            "1:02:03.644"
        );
    }

    #[test]
    fn elapsed_time_falls_back_when_rel_num_missing() {
        assert_eq!(
            elapsed_time(&serde_json::json!({"message": "hi"}), "15:41:23.644"),
            "15:41:23.644"
        );
    }
}
