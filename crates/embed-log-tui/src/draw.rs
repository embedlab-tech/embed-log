//! Frame rendering — tab bar, pane content, status bar.
//!
//! Phase 2 layout:
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ [ Device ][ UART ][ CoAP ][ Sensors ][ … ]    │  ← tab bar
//! ├─────────────────────────┬────────────────────┤
//! │ DUT Device              │ Host Controller     │  ← pane titles
//! │ <line rendering: P3>    │ <line rendering:P3> │
//! │                         │                     │
//! ├─────────────────────────┴────────────────────┤
//! ● connected │ session s1 │ abs │ DUT 42 lines   │  ← status bar
//! └──────────────────────────────────────────────┘
//! ```
//!
//! Phase 3 replaces the pane placeholder with virtualized log lines; Phase 4
//! adds the unwrap-mode layout. Events tab (Phase 6) appends an extra tab.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::state::{ConnState, State};

/// Render one full frame from `state`.
pub fn draw(f: &mut Frame, state: &State) {
    let area = f.area();

    // Vertical: tab bar (3) | pane content (fill) | status bar (1).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    draw_tab_bar(f, state, chunks[0]);
    draw_pane_content(f, state, chunks[1]);
    draw_status_bar(f, state, chunks[2]);
}

/// Mouse hit in the pane area.
#[derive(Debug, Clone)]
pub struct PaneHit {
    pub pane_id: String,
    pub pane_index: usize,
    pub raw_idx: Option<usize>,
}

/// Hit-test the pane content for mouse interactions.
/// Returns the pane under `(column,row)` and, when the click lands on a visible
/// log line, the raw line index of that row.
pub fn hit_test_pane(state: &State, area: Rect, column: u16, row: u16) -> Option<PaneHit> {
    if state.events_tab_active() {
        return None;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let content = chunks[1];
    let panes = state.active_tab_panes();
    if panes.is_empty() {
        return None;
    }
    let ratio = (state.splitter * 100.0).round() as u16;
    let ratio = ratio.clamp(10, 90);
    let constraints: Vec<Constraint> = match panes.len() {
        1 => vec![Constraint::Percentage(100)],
        _ => vec![
            Constraint::Percentage(ratio),
            Constraint::Percentage(100 - ratio),
        ],
    };
    let pane_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(content);

    for (i, pane_id) in panes.iter().enumerate() {
        let outer = pane_areas[i];
        if column < outer.x
            || column >= outer.x + outer.width
            || row < outer.y
            || row >= outer.y + outer.height
        {
            continue;
        }
        let inner = Block::default().borders(Borders::ALL).inner(outer);
        let mut raw_idx = None;
        if column >= inner.x
            && column < inner.x + inner.width
            && row >= inner.y
            && row < inner.y + inner.height
        {
            if let Some(lines) = state.raw_lines.get(pane_id) {
                let markers = state.markers_for(pane_id);
                let filter = state.filters.get(pane_id).and_then(|f| f.as_ref());
                let scroll = state.scroll.get(pane_id).copied().unwrap_or(0);
                let visible = inner.height.max(1) as usize;
                let (_, rows) = crate::lines::pane_rows(
                    lines,
                    &markers,
                    state.timestamp_mode,
                    filter,
                    scroll,
                    visible,
                );
                let rel_y = (row - inner.y) as usize;
                raw_idx = rows.get(rel_y).map(|(idx, _)| *idx);
            }
        }
        return Some(PaneHit {
            pane_id: pane_id.clone(),
            pane_index: i,
            raw_idx,
        });
    }
    None
}

/// Render the tab bar (or pane list in unwrap mode).
fn draw_tab_bar(f: &mut Frame, state: &State, area: Rect) {
    let mut titles: Vec<Line> = if state.unwrap {
        state
            .panes
            .iter()
            .map(|p| Line::from(state.pane_label(p)))
            .collect()
    } else {
        state
            .tabs
            .iter()
            .map(|t| Line::from(t.label.clone()))
            .collect()
    };
    // Append Events tab when enabled (non-unwrap mode).
    if state.events_enabled && !state.unwrap {
        titles.push(Line::from("⚡ Events"));
    }

    let tab_count = state.tab_count();
    let active = state.active_tab.min(tab_count.saturating_sub(1));

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("embed-log"))
        .select(active)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

/// Render the active tab's panes (1 or 2 side by side). Phase 2 shows a
/// placeholder; Phase 3 draws real log lines.
fn draw_pane_content(f: &mut Frame, state: &State, area: Rect) {
    // Events tab: render the event timeline instead of panes.
    if state.events_tab_active() {
        crate::events::draw_events(f, state, &state.events_view, area);
        return;
    }

    let panes = state.active_tab_panes();
    if panes.is_empty() {
        let msg = if state.conn == ConnState::Connected {
            "connected — waiting for config…"
        } else {
            "connecting…"
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    // Split horizontally across the active tab's panes, using the splitter ratio.
    let ratio = (state.splitter * 100.0).round() as u16;
    let ratio = ratio.clamp(10, 90);
    let constraints: Vec<Constraint> = match panes.len() {
        1 => vec![Constraint::Percentage(100)],
        _ => vec![
            Constraint::Percentage(ratio),
            Constraint::Percentage(100 - ratio),
        ],
    };
    let pane_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, pane_id) in panes.iter().enumerate() {
        let label = state.pane_label(pane_id);
        let count = state.raw_lines.get(pane_id).map(|v| v.len()).unwrap_or(0);
        let kind = state
            .pane_kinds
            .get(pane_id)
            .cloned()
            .unwrap_or_else(|| "?".to_string());
        let focused = i == state.active_pane;
        let title = format!(" {label} [{kind}] · {count} lines ");
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        let inner = block.inner(pane_areas[i]);
        f.render_widget(block, pane_areas[i]);

        let stored = state.raw_lines.get(pane_id);
        let count = stored.map(|v| v.len()).unwrap_or(0);
        if count == 0 {
            f.render_widget(
                Paragraph::new("(no lines yet)").style(Style::default().fg(Color::DarkGray)),
                inner,
            );
            continue;
        }

        let stored = stored.unwrap();
        let markers = state.markers_for(pane_id);
        let filter = state.filters.get(pane_id).and_then(|f| f.as_ref());
        let scroll = state.scroll.get(pane_id).copied().unwrap_or(0);
        // Visible rows = inner height; clamp to ≥1.
        let visible = inner.height.max(1) as usize;
        let (_, lines) = crate::lines::pane_lines(
            stored,
            &markers,
            state.timestamp_mode,
            filter,
            scroll,
            visible,
        );
        f.render_widget(Paragraph::new(lines), inner);
    }
}

/// Render the single-line status bar.
fn draw_status_bar(f: &mut Frame, state: &State, area: Rect) {
    let conn_span = match state.conn {
        ConnState::Connected => Span::styled("● connected", Style::default().fg(Color::Green)),
        ConnState::Connecting => Span::styled("◌ connecting", Style::default().fg(Color::Yellow)),
        ConnState::Reconnecting => {
            Span::styled("↻ reconnecting", Style::default().fg(Color::Yellow))
        }
        ConnState::Disconnected => Span::styled("○ disconnected", Style::default().fg(Color::Red)),
    };
    let ts_mode = match state.timestamp_mode {
        crate::state::TimestampMode::Absolute => "abs",
        crate::state::TimestampMode::Relative => "rel",
    };
    let session = if state.session.id.is_empty() {
        "—".to_string()
    } else {
        state.session.id.clone()
    };
    let app = if state.app_name.is_empty() {
        "embed-log"
    } else {
        &state.app_name
    };

    let active_pane = state.active_pane_id().unwrap_or_default();
    let active_label = state.pane_label(&active_pane);

    let scope = match state.selection_scope {
        crate::state::SelectionScope::Exact => "exact",
        crate::state::SelectionScope::Context => "context",
    };
    let unwrap = if state.unwrap { " │ UNWRAP" } else { "" };

    let line = Line::from(vec![
        conn_span,
        Span::raw(" │ "),
        Span::raw(app.to_string()),
        Span::raw(" │ session "),
        Span::styled(session, Style::default().fg(Color::Cyan)),
        Span::raw(format!(
            " │ {ts_mode} │ {active_label} │ {scope}{unwrap} │ ?:help q=quit "
        )),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Black)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ConfigMessage, Marker, TabDef};
    use crate::state::{ConnState, State, StoredLine};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::collections::HashMap;
    fn populated_state() -> State {
        let mut cfg = ConfigMessage {
            app_name: "embed-log demo".into(),
            tabs: vec![
                TabDef {
                    label: "Device".into(),
                    panes: vec!["DUT".into(), "HOST".into()],
                    ..Default::default()
                },
                TabDef {
                    label: "UART".into(),
                    panes: vec!["UART_DUT".into()],
                    ..Default::default()
                },
            ],
            pane_labels: {
                let mut m = HashMap::new();
                m.insert("DUT".into(), "DUT Device".into());
                m.insert("HOST".into(), "Host Controller".into());
                m.insert("UART_DUT".into(), "UART Main".into());
                m
            },
            pane_kinds: {
                let mut m = HashMap::new();
                m.insert("DUT".into(), "udp".into());
                m.insert("HOST".into(), "udp".into());
                m.insert("UART_DUT".into(), "uart".into());
                m
            },
            ..Default::default()
        };
        // Markers + a session id via the session blob.
        cfg.markers = vec![Marker {
            pane_id: "DUT".into(),
            line_idx: 0,
            description: "note".into(),
            ..Default::default()
        }];
        cfg.session = serde_json::json!({
            "id": "2026-06-17_test",
            "timestamp_mode": "absolute",
            "first_log_at": "2026-06-17T00:00:00Z",
        });

        let mut s = State {
            app_name: "embed-log demo".into(),
            ..Default::default()
        };
        s.apply_config(&cfg);
        s.conn = ConnState::Connected;
        s.append_line(
            "DUT",
            StoredLine {
                line_idx: 0,
                message: "boot complete".into(),
                ..Default::default()
            },
        );
        s
    }

    /// Draw `state` to a fresh TestBackend of the given size and return the
    /// buffer content as a single string (rows joined with '\n').
    fn render(state: &State, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, state)).unwrap();
        let buf = terminal.backend().buffer();
        let area = buf.area();
        let mut rows = Vec::with_capacity(area.height as usize);
        for y in 0..area.height {
            let mut row = String::new();
            for x in 0..area.width {
                row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            rows.push(row.trim_end().to_string());
        }
        rows.join("\n")
    }

    #[test]
    fn renders_tab_labels_and_active_pane() {
        let s = populated_state();
        let out = render(&s, 80, 24);
        // Tab bar shows both tab labels.
        assert!(out.contains("Device"), "missing Device tab: {out}");
        assert!(out.contains("UART"), "missing UART tab: {out}");
        // Active tab 0 → DUT pane title shown with its label + kind.
        assert!(out.contains("DUT Device"), "missing DUT pane title: {out}");
        assert!(out.contains("udp"), "missing pane kind: {out}");
    }

    #[test]
    fn renders_status_bar_connected_and_session() {
        let s = populated_state();
        let out = render(&s, 120, 24);
        assert!(out.contains("connected"), "missing conn indicator: {out}");
        assert!(out.contains("2026-06-17_test"), "missing session id: {out}");
        assert!(out.contains("abs"), "missing timestamp mode: {out}");
        assert!(out.contains("q=quit"), "missing key hint: {out}");
    }

    #[test]
    fn renders_line_count_in_pane_title() {
        let s = populated_state();
        let out = render(&s, 140, 24);
        // DUT has 1 line → title shows "1 lines".
        assert!(out.contains("1 lines"), "missing line count: {out}");
    }

    #[test]
    fn renders_last_message_preview() {
        let s = populated_state();
        let out = render(&s, 140, 24);
        assert!(
            out.contains("boot complete"),
            "missing last msg preview: {out}"
        );
    }

    #[test]
    fn renders_connecting_state_when_no_tabs() {
        let s = State::default();
        let out = render(&s, 80, 24);
        assert!(
            out.contains("connecting"),
            "expected connecting placeholder: {out}"
        );
    }

    #[test]
    fn switching_active_tab_changes_pane_title() {
        let mut s = populated_state();
        s.active_tab = 1; // UART tab → UART_DUT pane
        let out = render(&s, 80, 24);
        assert!(out.contains("UART Main"), "missing UART pane title: {out}");
        assert!(out.contains("uart"), "missing uart kind: {out}");
    }

    #[test]
    fn events_tab_appears_when_enabled() {
        let mut s = populated_state();
        s.events_enabled = true;
        let out = render(&s, 120, 24);
        assert!(
            out.contains("⚡ Events") || out.contains("Events"),
            "missing Events tab: {out}"
        );
    }

    #[test]
    fn events_tab_renders_header_and_list() {
        let mut s = populated_state();
        s.events_enabled = true;
        s.events.push(crate::protocol::EventPayload {
            event_id: "fatal_error".into(),
            source_id: "DUT".into(),
            severity: "error".into(),
            timestamp: "06-14 09:30:45.123".into(),
            timestamp_num: 1718347845123.0,
            message: "ZEPHYR FATAL ERROR".into(),
            ..Default::default()
        });
        // Events tab sits at index tabs.len().
        s.active_tab = s.tabs.len();
        let out = render(&s, 120, 24);
        assert!(
            out.contains("Event Timeline") || out.contains("Events (1)"),
            "missing events header: {out}"
        );
        assert!(
            out.contains("fatal_error") || out.contains("ZEPHYR FATAL ERROR"),
            "missing event item: {out}"
        );
    }
}
