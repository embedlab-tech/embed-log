//! Log line rendering & virtualization.
//!
//! Mirrors `frontend/lines.js`:
//! - Windowed rendering: only format the visible window of lines (+ overscan)
//!   so 100k-line panes stay responsive.
//! - Color: structured `color` field → [`ratatui::style::Color`] (mirrors
//!   `embed_log_core::models::Ansi::code`). TX lines are always yellow.
//! - Line tags: `<wrn>`, `[ERR]`, `[error]`, etc. → severity color (mirrors
//!   `lines.js::_lineTagClass`).
//! - Filter: per-pane regex skips non-matching lines from the visible window
//!   while keeping raw indices stable.
//! - Marker gutter: a colored left marker for lines with markers (user or
//!   event, severity-colored for event markers).
//!
//! The browser parses ANSI escapes in the `data` field; the TUI instead uses
//! the structured `message` + `color` fields and applies its own styling —
//! same visual result, no ANSI parser needed.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    protocol::Marker,
    state::{StoredLine, TimestampMode},
};

/// Overscan rows above/below the visible window (mirrors `lines.js` OVERSCAN).
pub const OVERSCAN: usize = 60;

/// Map a structured color name to a ratatui color (mirrors `Ansi::code`).
pub fn color_for(name: Option<&str>) -> Option<Color> {
    match name? {
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        _ => None,
    }
}

/// Detect a line tag (`<wrn>`, `[ERR]`, `[error]`, …) and return its severity
/// color, mirroring `lines.js::_lineTagClass`. Returns `None` when no tag.
pub fn line_tag_color(message: &str) -> Option<Color> {
    // Scan for the first tag match without allocating a regex per call.
    // Tolerates `<tag>` and `[tag]` forms, case-insensitive for bracket form.
    let bytes = message.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let close: u8 = if bytes[i] == b'<' {
            b'>'
        } else if bytes[i] == b'[' {
            b']'
        } else {
            i += 1;
            continue;
        };
        // Find the closer from i+1.
        if let Some(rel) = message[i + 1..].find(close as char) {
            let tag = &message[i + 1..i + 1 + rel];
            if let Some(c) = classify_tag(tag) {
                return Some(c);
            }
            i += rel + 2;
        } else {
            break;
        }
    }
    None
}

/// Classify a tag string to a color, matching the browser's set.
fn classify_tag(tag: &str) -> Option<Color> {
    match tag.to_ascii_lowercase().as_str() {
        "wrn" | "warn" | "warning" => Some(Color::Yellow),
        "dbg" | "debug" => Some(Color::DarkGray),
        "err" | "error" => Some(Color::Red),
        _ => None,
    }
}

/// The marker gutter symbol for a line (colored left border).
pub fn marker_for_line(markers: &[Marker], line_idx: u64) -> Option<(char, Color)> {
    // First matching marker wins (markers are sorted by line_idx upstream).
    let m = markers
        .iter()
        .find(|m| m.line_idx <= line_idx && line_idx <= m.end_idx.max(m.line_idx))?;
    if m.is_event() {
        // Severity color for event markers; default red if missing.
        let c = match m.severity.as_str() {
            "fatal" => Color::LightRed,
            "error" => Color::Red,
            "warn" => Color::Yellow,
            "info" => Color::Blue,
            _ => Color::Red,
        };
        Some(('▎', c))
    } else {
        Some(('▎', Color::Cyan))
    }
}

/// A single formatted log line ready to render.
pub fn format_line(
    line: &StoredLine,
    markers: &[Marker],
    mode: TimestampMode,
    filter_rx: Option<&regex::Regex>,
) -> Option<Line<'static>> {
    // Filter: skip non-matching lines (raw index stays stable upstream).
    if let Some(rx) = filter_rx {
        if !rx.is_match(&line.message) {
            return None;
        }
    }

    // Timestamp span (abs or rel).
    let ts = match mode {
        TimestampMode::Absolute => line.abs_ts.clone(),
        TimestampMode::Relative => line.rel_ts.clone(),
    };
    let ts_span = Span::styled(ts, Style::default().fg(Color::DarkGray));

    // Marker gutter.
    let gutter: Span = match marker_for_line(markers, line.line_idx) {
        Some((ch, color)) => Span::styled(ch.to_string(), Style::default().fg(color)),
        None => Span::raw(" "),
    };

    // Message color precedence: TX → yellow; else structured color; else tag color.
    let msg_color = if line.is_tx {
        Some(Color::Yellow)
    } else {
        color_for(line.color.as_deref()).or_else(|| line_tag_color(&line.message))
    };
    let msg_style = match msg_color {
        Some(c) => Style::default().fg(c),
        None => Style::default(),
    };
    // TX lines also get bold to stand out.
    let msg_style = if line.is_tx {
        msg_style.add_modifier(Modifier::BOLD)
    } else {
        msg_style
    };

    let msg_span = Span::styled(line.message.clone(), msg_style);

    Some(Line::from(vec![
        gutter,
        Span::raw(" "),
        ts_span,
        Span::raw("  "),
        msg_span,
    ]))
}

/// Windowed view: given a pane's lines, a scroll offset (top raw index), a
/// visible row count, and overscan, return the lines to render plus the raw
/// index range they cover.
///
/// `scroll` is the raw index of the line pinned to the top of the viewport.
/// `visible` is the number of rows that fit. The returned range extends
/// `OVERSCAN` rows above and below where possible.
pub fn visible_window(
    lines: &[StoredLine],
    scroll: usize,
    visible: usize,
) -> (usize, Vec<&StoredLine>) {
    if lines.is_empty() {
        return (0, Vec::new());
    }
    let len = lines.len();
    // Clamp scroll to valid range (last `visible` lines at most).
    let max_scroll = len.saturating_sub(visible);
    let scroll = scroll.min(max_scroll);

    let start = scroll.saturating_sub(OVERSCAN);
    let end = (scroll + visible + OVERSCAN).min(len);
    let out: Vec<&StoredLine> = lines[start..end].iter().collect();
    (start, out)
}

/// Filtered view: returns the raw indices of lines matching the filter (or
/// all indices if no filter). Used to compute the filtered visible window.
///
/// In the browser, the filter doesn't shift raw indices — it just hides
/// non-matches. The TUI does the same: we render only matching lines within
/// the visible window, but `line_idx` and marker lookups still use raw
/// indices.
pub fn filtered_indices(lines: &[StoredLine], filter_rx: Option<&regex::Regex>) -> Vec<usize> {
    match filter_rx {
        None => (0..lines.len()).collect(),
        Some(rx) => lines
            .iter()
            .enumerate()
            .filter(|(_, l)| rx.is_match(&l.message))
            .map(|(i, _)| i)
            .collect(),
    }
}

/// Build the renderable rows for a pane: apply the filter, then window to the
/// visible range. Returns `(start_raw_index, Vec<(raw_idx, Line)>)`.
pub fn pane_rows(
    lines: &[StoredLine],
    markers: &[Marker],
    mode: TimestampMode,
    filter_rx: Option<&regex::Regex>,
    scroll: usize,
    visible: usize,
) -> (usize, Vec<(usize, Line<'static>)>) {
    let idxs = filtered_indices(lines, filter_rx);
    if idxs.is_empty() {
        return (0, Vec::new());
    }
    let max_scroll = idxs.len().saturating_sub(visible);
    let scroll = scroll.min(max_scroll);
    let start = scroll;
    let end = (scroll + visible).min(idxs.len());

    let mut out = Vec::with_capacity(end - start);
    for &raw_idx in &idxs[start..end] {
        if let Some(l) = format_line(&lines[raw_idx], markers, mode, filter_rx) {
            out.push((raw_idx, l));
        }
    }
    let start_raw = idxs.get(start).copied().unwrap_or(0);
    (start_raw, out)
}

/// Convenience wrapper returning lines only (used by draw code).
pub fn pane_lines(
    lines: &[StoredLine],
    markers: &[Marker],
    mode: TimestampMode,
    filter_rx: Option<&regex::Regex>,
    scroll: usize,
    visible: usize,
) -> (usize, Vec<Line<'static>>) {
    let (start, rows) = pane_rows(lines, markers, mode, filter_rx, scroll, visible);
    (start, rows.into_iter().map(|(_, l)| l).collect())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn line(message: &str, color: Option<&str>, is_tx: bool) -> StoredLine {
        StoredLine {
            message: message.into(),
            color: color.map(String::from),
            is_tx,
            line_idx: 0,
            abs_ts: "06-14 09:30:45.123".into(),
            rel_ts: "00:00:45.123".into(),
            ..Default::default()
        }
    }

    #[test]
    fn color_for_maps_known_names() {
        assert_eq!(color_for(Some("cyan")), Some(Color::Cyan));
        assert_eq!(color_for(Some("red")), Some(Color::Red));
        assert_eq!(color_for(Some("unknown")), None);
        assert_eq!(color_for(None), None);
    }

    #[test]
    fn line_tag_color_detects_brackets() {
        assert_eq!(line_tag_color("[ERR] boom"), Some(Color::Red));
        assert_eq!(line_tag_color("ok [WRN] careful"), Some(Color::Yellow));
        assert_eq!(line_tag_color("<dbg> trace"), Some(Color::DarkGray));
        assert_eq!(line_tag_color("<error> fail"), Some(Color::Red));
        assert_eq!(line_tag_color("[INFO] ok"), None);
        assert_eq!(line_tag_color("no tags here"), None);
    }

    #[test]
    fn format_line_tx_is_yellow_bold() {
        let l = line("version", None, true);
        let formatted = format_line(&l, &[], TimestampMode::Absolute, None).unwrap();
        // The message span is the last span; check its style.
        let msg_span = &formatted.spans[4];
        assert_eq!(msg_span.style.fg, Some(Color::Yellow));
        assert!(msg_span.style.add_modifier == Modifier::BOLD);
    }

    #[test]
    fn format_line_uses_structured_color() {
        let l = line("boot", Some("cyan"), false);
        let formatted = format_line(&l, &[], TimestampMode::Absolute, None).unwrap();
        let msg_span = &formatted.spans[4];
        assert_eq!(msg_span.style.fg, Some(Color::Cyan));
    }

    #[test]
    fn format_line_falls_back_to_tag_color() {
        let l = line("[ERR] something failed", None, false);
        let formatted = format_line(&l, &[], TimestampMode::Absolute, None).unwrap();
        let msg_span = &formatted.spans[4];
        assert_eq!(msg_span.style.fg, Some(Color::Red));
    }

    #[test]
    fn format_line_relative_timestamp() {
        let l = line("boot", None, false);
        let formatted = format_line(&l, &[], TimestampMode::Relative, None).unwrap();
        let ts_span = &formatted.spans[2];
        assert_eq!(ts_span.content, "00:00:45.123");
    }

    #[test]
    fn format_line_filter_skips_non_matching() {
        let l = line("boot complete", None, false);
        let rx = regex::Regex::new("FATAL").unwrap();
        assert!(format_line(&l, &[], TimestampMode::Absolute, Some(&rx)).is_none());
        let l2 = line("FATAL ERROR", None, false);
        assert!(format_line(&l2, &[], TimestampMode::Absolute, Some(&rx)).is_some());
    }

    #[test]
    fn marker_gutter_for_user_marker() {
        let markers = vec![Marker {
            pane_id: "DUT".into(),
            line_idx: 5,
            end_idx: 5,
            kind: "user".into(),
            ..Default::default()
        }];
        let (ch, color) = marker_for_line(&markers, 5).unwrap();
        assert_eq!(ch, '▎');
        assert_eq!(color, Color::Cyan);
        assert!(marker_for_line(&markers, 6).is_none());
    }

    #[test]
    fn marker_gutter_for_event_marker_severity_colored() {
        let markers = vec![Marker {
            pane_id: "DUT".into(),
            line_idx: 3,
            end_idx: 3,
            kind: "event".into(),
            severity: "error".into(),
            ..Default::default()
        }];
        let (ch, color) = marker_for_line(&markers, 3).unwrap();
        assert_eq!(ch, '▎');
        assert_eq!(color, Color::Red);
    }

    #[test]
    fn visible_window_respects_overscan_and_bounds() {
        let lines: Vec<StoredLine> = (0..1000)
            .map(|i| StoredLine {
                line_idx: i as u64,
                message: format!("m{i}"),
                ..Default::default()
            })
            .collect();
        // Scroll 500, visible 20 → start = 500-60=440, end = 500+20+60=580.
        let (start, window) = visible_window(&lines, 500, 20);
        assert_eq!(start, 440);
        assert_eq!(window.len(), 140);
        assert_eq!(window[0].line_idx, 440);
        assert_eq!(window[139].line_idx, 579);
    }

    #[test]
    fn visible_window_clamps_scroll_at_tail() {
        let lines: Vec<StoredLine> = (0..50)
            .map(|i| StoredLine {
                line_idx: i as u64,
                message: format!("m{i}"),
                ..Default::default()
            })
            .collect();
        // Scroll 999 → clamped to 50-20=30. start=30-60→0, end=30+20+60→50.
        let (start, window) = visible_window(&lines, 999, 20);
        assert_eq!(start, 0);
        assert_eq!(window.len(), 50);
    }

    #[test]
    fn visible_window_empty_lines() {
        let (start, window) = visible_window(&[], 0, 20);
        assert_eq!(start, 0);
        assert!(window.is_empty());
    }

    #[test]
    fn filtered_indices_no_filter_returns_all() {
        let lines = vec![line("a", None, false), line("b", None, false)];
        let idxs = filtered_indices(&lines, None);
        assert_eq!(idxs, vec![0, 1]);
    }

    #[test]
    fn filtered_indices_with_filter() {
        let lines = vec![
            line("boot", None, false),
            line("FATAL ERROR", None, false),
            line("ok", None, false),
            line("FATAL again", None, false),
        ];
        let rx = regex::Regex::new("FATAL").unwrap();
        let idxs = filtered_indices(&lines, Some(&rx));
        assert_eq!(idxs, vec![1, 3]);
    }

    #[test]
    fn pane_lines_applies_filter_then_window() {
        let lines: Vec<StoredLine> = (0..100)
            .map(|i| StoredLine {
                line_idx: i as u64,
                message: if i % 2 == 0 { "FATAL x" } else { "ok" }.to_string(),
                ..Default::default()
            })
            .collect();
        let rx = regex::Regex::new("FATAL").unwrap();
        // 50 matching lines (even indices). visible=10, scroll=0 → first 10
        // matching rows (mouse hit-testing needs a 1:1 visible-row mapping).
        let (start, out) = pane_lines(&lines, &[], TimestampMode::Absolute, Some(&rx), 0, 10);
        assert_eq!(start, 0); // first matching raw index
        assert_eq!(out.len(), 10);
    }
}
