//! Selection text formatting + clipboard copy.
//!
//! Mirrors `frontend/selection.js`:
//! - Exact scope: selected lines from the active pane.
//! - Context scope: lines from all panes in the active tab within the
//!   selection's time range, sorted by timestamp.
//! - Copy to clipboard via `arboard` (best-effort; logs on failure).

use crate::state::{SelectionScope, State};

/// Build the copyable text from the current selection.
///
/// Exact scope: `ts  [pane]  message` per selected line in the active pane.
/// Context scope: `[ts] [pane] message` per synced line across tab panes,
/// grouped by pane, sorted by timestamp (mirrors `_formatRangeRaw`).
pub fn selection_text(state: &State) -> String {
    match state.selection_scope {
        SelectionScope::Exact => selection_exact(state),
        SelectionScope::Context => selection_context(state),
    }
}

/// Exact-scope text: selected lines from the active pane.
fn selection_exact(state: &State) -> String {
    let Some(pane) = state.active_pane_id() else {
        return String::new();
    };
    let Some(lines) = state.raw_lines.get(&pane) else {
        return String::new();
    };
    let indices = state.selected_for(&pane);
    if indices.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for idx in &indices {
        if let Some(line) = lines.get(*idx as usize) {
            let ts = match state.timestamp_mode {
                crate::state::TimestampMode::Absolute => &line.abs_ts,
                crate::state::TimestampMode::Relative => &line.rel_ts,
            };
            out.push(format!("{ts}  [{pane}]  {}", line.message));
        }
    }
    out.join("\n")
}

/// Context-scope text: synced lines across tab panes within the time range.
fn selection_context(state: &State) -> String {
    let entries = state.collect_context_entries();
    if entries.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    let mut current_pane: Option<String> = None;
    for (pane_id, idx, _ts) in &entries {
        if Some(pane_id) != current_pane.as_ref() {
            if current_pane.is_some() {
                out.push(String::new()); // blank line between panes
            }
            current_pane = Some(pane_id.clone());
        }
        if let Some(line) = state
            .raw_lines
            .get(pane_id)
            .and_then(|l| l.get(*idx as usize))
        {
            let ts = match state.timestamp_mode {
                crate::state::TimestampMode::Absolute => &line.abs_ts,
                crate::state::TimestampMode::Relative => &line.rel_ts,
            };
            out.push(format!("[{ts}] [{pane_id}] {}", line.message));
        }
    }
    out.join("\n")
}

/// Copy text to the system clipboard (best-effort).
///
/// Uses `arboard` if available. On headless/SSH where the clipboard is
/// unreachable, this silently logs and returns — the caller can fall back
/// to writing a file.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.set_text(text).map_err(|e| e.to_string()),
        Err(e) => Err(format!("clipboard unavailable: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::TabDef;
    use crate::state::StoredLine;

    fn state_with_panes(panes: &[(&str, Vec<f64>)]) -> State {
        // panes: [(pane_id, [abs_num per line])]
        let mut cfg = crate::protocol::ConfigMessage {
            tabs: vec![TabDef {
                label: "T".into(),
                panes: panes.iter().map(|(p, _)| p.to_string()).collect(),
                ..Default::default()
            }],
            ..Default::default()
        };
        for (p, _) in panes {
            cfg.pane_labels.insert(p.to_string(), p.to_string());
        }
        let _ = &mut cfg;
        let mut s = State::default();
        s.apply_config(&cfg);
        for (pane_id, nums) in panes {
            for (i, &num) in nums.iter().enumerate() {
                s.append_line(
                    pane_id,
                    StoredLine {
                        line_idx: i as u64,
                        abs_num: num,
                        abs_ts: format!("ts{i}"),
                        rel_ts: format!("rel{i}"),
                        message: format!("msg-{pane_id}-{i}"),
                        ..Default::default()
                    },
                );
            }
        }
        s
    }

    #[test]
    fn exact_scope_returns_selected_lines() {
        let mut s = state_with_panes(&[("DUT", vec![100.0, 200.0, 300.0])]);
        s.active_pane = 0;
        s.toggle_select_active(0); // select line 0
        s.toggle_select_active(2); // select line 2
        let text = selection_exact(&s);
        assert!(text.contains("msg-DUT-0"));
        assert!(text.contains("msg-DUT-2"));
        assert!(!text.contains("msg-DUT-1"));
    }

    #[test]
    fn context_scope_collects_across_panes() {
        let mut s = state_with_panes(&[
            ("DUT", vec![100.0, 200.0, 300.0]),
            ("HOST", vec![150.0, 250.0]),
        ]);
        s.active_pane = 0;
        // Select DUT line 1 (abs_num=200.0) — context range = [200, 200].
        s.toggle_select_active(1);
        s.selection_scope = SelectionScope::Context;
        let entries = s.collect_context_entries();
        // DUT line 1 (200.0) and HOST line 1 (250.0)? No — 250 > 200.
        // Only DUT[1]=200.0 is in [200,200]. HOST[0]=150 < 200, HOST[1]=250 > 200.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "DUT");
    }

    #[test]
    fn context_scope_wider_range() {
        let mut s = state_with_panes(&[
            ("DUT", vec![100.0, 200.0, 300.0]),
            ("HOST", vec![150.0, 250.0]),
        ]);
        s.active_pane = 0;
        // Select DUT lines 0 and 2 → range [100, 300].
        s.toggle_select_active(0);
        s.toggle_select_active(2);
        s.selection_scope = SelectionScope::Context;
        let entries = s.collect_context_entries();
        // All lines within [100, 300]: DUT[0,1,2] + HOST[0,1].
        assert_eq!(entries.len(), 5);
        // Sorted by ts: 100, 150, 200, 250, 300.
        assert!((entries[0].2 - 100.0).abs() < 1.0);
        assert!((entries[1].2 - 150.0).abs() < 1.0);
        assert!((entries[4].2 - 300.0).abs() < 1.0);
    }

    #[test]
    fn empty_selection_returns_empty() {
        let s = state_with_panes(&[("DUT", vec![100.0])]);
        assert!(selection_exact(&s).is_empty());
    }
}
