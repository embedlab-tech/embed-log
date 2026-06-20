//! Events tab — terminal-friendly event timeline.
//!
//! Mirrors `frontend/events.js` data model but renders as a vertical list
//! (not an SVG swimlane): events grouped by lane (`event_id`), each entry
//! severity-colored with timestamp + source + message. This is the pragmatic
//! TUI translation of the browser's 2D swimlane — the same data, the same
//! filter/sync/click-to-navigate semantics, rendered for a terminal.
//!
//! Key bindings (active in the Events tab):
//! - `j`/`k`/`↑`/`↓` — move cursor through events
//! - `Enter` — sync log panes to the event's timestamp + switch to the
//!   tab containing the event's source
//! - `+`/`-` — zoom (narrow/widen the visible time range)
//! - `0` — reset zoom to full data range
//! - `f` — toggle source/severity filters (cycles)
//! - `K` — show event detail popup (message + captures)

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Points},
        Block, Borders, List, ListItem, Paragraph,
    },
    Frame,
};

use crate::protocol::EventPayload;
use crate::state::State;

/// Severity → color (mirrors `events.js::SEVERITY_COLORS`).
pub fn severity_color(severity: &str) -> Color {
    match severity {
        "fatal" => Color::LightRed,
        "error" => Color::Red,
        "warn" => Color::Yellow,
        "info" => Color::Blue,
        _ => Color::DarkGray,
    }
}

/// The events view state: cursor position, time-range zoom, and filters.
#[derive(Debug, Clone, Default)]
pub struct EventsView {
    /// Cursor index into the filtered event list.
    pub cursor: usize,
    /// View range `[start, end]` in epoch-ms; `None` = auto (full data range).
    pub view_range: Option<(f64, f64)>,
    /// Hidden source ids (filtered out).
    pub hidden_sources: std::collections::HashSet<String>,
    /// Hidden severities (filtered out).
    pub hidden_severities: std::collections::HashSet<String>,
    /// Whether the detail popup is shown.
    pub show_detail: bool,
}

impl EventsView {
    /// Compute lane assignment: `event_id → lane index`, preserving first-seen
    /// order (mirrors `events.js::_computeLanes`).
    pub fn lanes(state: &State, view: &EventsView) -> Vec<String> {
        let mut lanes: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for ev in &state.events {
            if view.hidden_sources.contains(&ev.source_id)
                || view.hidden_severities.contains(&ev.severity)
            {
                continue;
            }
            if seen.insert(ev.event_id.clone()) {
                lanes.push(ev.event_id.clone());
            }
        }
        // Fallback: lanes from configured rules when no events yet.
        if lanes.is_empty() {
            for (src, rules) in &state.event_rules {
                if view.hidden_sources.contains(src) {
                    continue;
                }
                for r in rules {
                    if view.hidden_severities.contains(&r.severity) {
                        continue;
                    }
                    if seen.insert(r.name.clone()) {
                        lanes.push(r.name.clone());
                    }
                }
            }
        }
        lanes
    }

    /// Compute the data range `[min - 5%, max + 5%]` of filtered events
    /// (mirrors `events.js::_dataRange`).
    pub fn data_range(state: &State, view: &EventsView) -> (f64, f64) {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for ev in &state.events {
            if view.hidden_sources.contains(&ev.source_id)
                || view.hidden_severities.contains(&ev.severity)
            {
                continue;
            }
            if ev.timestamp_num.is_finite() {
                min = min.min(ev.timestamp_num);
                max = max.max(ev.timestamp_num);
            }
        }
        if !min.is_finite() {
            return (0.0, 1.0);
        }
        let span = (max - min).max(1000.0);
        (min - span * 0.05, max + span * 0.05)
    }

    /// Effective range: view_range or data_range.
    pub fn effective_range(state: &State, view: &EventsView) -> (f64, f64) {
        view.view_range
            .unwrap_or_else(|| Self::data_range(state, view))
    }

    /// Filtered + range-bounded events, sorted by timestamp.
    pub fn visible_events<'a>(state: &'a State, view: &EventsView) -> Vec<&'a EventPayload> {
        let (start, end) = Self::effective_range(state, view);
        let mut events: Vec<&EventPayload> = state
            .events
            .iter()
            .filter(|ev| {
                !view.hidden_sources.contains(&ev.source_id)
                    && !view.hidden_severities.contains(&ev.severity)
                    && ev.timestamp_num >= start
                    && ev.timestamp_num <= end
            })
            .collect();
        events.sort_by(|a, b| {
            a.timestamp_num
                .partial_cmp(&b.timestamp_num)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        events
    }

    /// Zoom in/out around the center of the current range.
    /// `factor > 1.0` = zoom in (narrow), `< 1.0` = zoom out (widen).
    /// Mirrors `events.js::_zoom`.
    pub fn zoom_ranges(&mut self, data: (f64, f64), current: (f64, f64), factor: f64) {
        let center = (current.0 + current.1) / 2.0;
        let span = (current.1 - current.0).max(1.0);
        let new_span = span / factor;
        let mut start = center - new_span / 2.0;
        let mut end = center + new_span / 2.0;
        // Clamp to data range when zooming out.
        if factor < 1.0 {
            if start < data.0 {
                start = data.0;
                end = start + new_span;
            }
            if end > data.1 {
                end = data.1;
                start = end - new_span;
            }
        }
        self.view_range = Some((start, end));
    }

    /// Reset zoom to full data range.
    pub fn reset_zoom(&mut self) {
        self.view_range = None;
    }

    /// Move cursor by `delta`, clamping to the visible event list.
    pub fn move_cursor(&mut self, delta: i32, len: usize) {
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let new = self.cursor as i32 + delta;
        self.cursor = new.clamp(0, len as i32 - 1) as usize;
    }
}

/// Render the events view: header + swimlane canvas + detail panel.
pub fn draw_events(f: &mut Frame, state: &State, view: &EventsView, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(6),
        ])
        .split(area);

    draw_events_header(f, state, view, chunks[0]);

    let events = EventsView::visible_events(state, view);
    let lanes = EventsView::lanes(state, view);
    let selected = events.get(view.cursor).copied();

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(20)])
        .split(chunks[1]);

    draw_lane_labels(f, &lanes, selected, mid[0]);
    draw_timeline_canvas(f, &events, &lanes, selected, mid[1]);
    draw_event_detail_panel(f, state, selected, chunks[2]);

    if view.show_detail {
        if let Some(ev) = selected {
            draw_event_detail(f, state, ev, area);
        }
    }
}

fn draw_lane_labels(f: &mut Frame, lanes: &[String], selected: Option<&EventPayload>, area: Rect) {
    let items: Vec<ListItem> = lanes
        .iter()
        .enumerate()
        .map(|(i, lane)| {
            let active = selected.is_some_and(|ev| ev.event_id == *lane);
            let line = Line::from(vec![
                Span::styled(
                    format!("L{:<2}", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
                Span::styled(
                    lane.clone(),
                    if active {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(" Lanes "));
    f.render_widget(list, area);
}

fn draw_timeline_canvas(
    f: &mut Frame,
    events: &[&EventPayload],
    lanes: &[String],
    selected: Option<&EventPayload>,
    area: Rect,
) {
    let (start, end) = if events.is_empty() {
        (0.0, 1.0)
    } else {
        let min = events
            .iter()
            .map(|e| e.timestamp_num)
            .fold(f64::INFINITY, f64::min);
        let max = events
            .iter()
            .map(|e| e.timestamp_num)
            .fold(f64::NEG_INFINITY, f64::max);
        let span = (max - min).max(1000.0);
        (min - span * 0.05, max + span * 0.05)
    };
    let lane_max = lanes.len().max(1) as f64;

    let canvas = Canvas::default()
        .block(Block::default().borders(Borders::ALL).title(" Timeline "))
        .marker(symbols::Marker::Braille)
        .x_bounds([start, end])
        .y_bounds([0.0, lane_max])
        .paint(|ctx| {
            // Horizontal lane guides + labels
            for idx in 0..lanes.len() {
                let y = lane_max - 1.0 - idx as f64 + 0.5;
                // guide line
                ctx.draw(&ratatui::widgets::canvas::Line {
                    x1: start,
                    y1: y,
                    x2: end,
                    y2: y,
                    color: Color::DarkGray,
                });
                // lane marker at left edge
                ctx.print(start, y, format!("L{}", idx + 1));
            }

            // Plot per-severity batches for color grouping
            for sev in ["fatal", "error", "warn", "info"] {
                let coords: Vec<(f64, f64)> = events
                    .iter()
                    .filter(|ev| ev.severity == sev)
                    .filter_map(|ev| {
                        let idx = lanes.iter().position(|l| l == &ev.event_id)? as f64;
                        let y = lane_max - 1.0 - idx + 0.5;
                        Some((ev.timestamp_num, y))
                    })
                    .collect();
                if !coords.is_empty() {
                    ctx.draw(&Points {
                        coords: &coords,
                        color: severity_color(sev),
                    });
                }
            }

            // Highlight selected event with a white point over the colored dot.
            if let Some(ev) = selected {
                if let Some(idx) = lanes.iter().position(|l| l == &ev.event_id) {
                    let y = lane_max - 1.0 - idx as f64 + 0.5;
                    let coords = vec![(ev.timestamp_num, y)];
                    ctx.draw(&Points {
                        coords: &coords,
                        color: Color::White,
                    });
                }
            }
        });
    f.render_widget(canvas, area);
}

fn draw_event_detail_panel(
    f: &mut Frame,
    state: &State,
    selected: Option<&EventPayload>,
    area: Rect,
) {
    let text = if let Some(ev) = selected {
        let source = state.pane_label(&ev.source_id);
        let captures = if ev.captures.is_empty() {
            String::new()
        } else {
            format!(" | captures: {}", ev.captures.join(", "))
        };
        format!(
            "[{}] {} | {} | {} | line {}{}\n{}",
            ev.severity, ev.event_id, source, ev.timestamp, ev.line_idx, captures, ev.message
        )
    } else {
        "No events yet.".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Selected event ");
    f.render_widget(Paragraph::new(text).block(block), area);
}

/// Render the events header: title, count, zoom range, filter status.
fn draw_events_header(f: &mut Frame, state: &State, view: &EventsView, area: Rect) {
    let count = state.events.len();
    let (start, end) = EventsView::effective_range(state, view);
    let zoom = if view.view_range.is_some() {
        "zoomed"
    } else {
        "auto"
    };
    let filters = if view.hidden_sources.is_empty() && view.hidden_severities.is_empty() {
        ""
    } else {
        " [filtered]"
    };
    let text = format!(
        "⚡ Event Timeline │ {count} events │ {zoom} │ range [{start:.0}..{end:.0}]{filters} │ Enter=sync +/= zoom 0=reset K=detail"
    );
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Yellow)),
        area,
    );
}

/// Render a centered popup with the selected event's details.
fn draw_event_detail(f: &mut Frame, state: &State, ev: &EventPayload, area: Rect) {
    let popup_area = centered_rect(60, 50, area);

    let captures = if ev.captures.is_empty() {
        String::new()
    } else {
        format!("\nCaptures: {}", ev.captures.join(" | "))
    };
    let source_label = state.pane_label(&ev.source_id);
    let text = format!(
        "[{}] {}\n\nSource: {}\nTime:   {}\nLine:   {}\nSeverity: {}{}",
        ev.severity, ev.event_id, source_label, ev.timestamp, ev.line_idx, ev.severity, captures,
    );

    let color = severity_color(&ev.severity);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", ev.event_id))
        .border_style(Style::default().fg(color));
    f.render_widget(Paragraph::new(text).block(block), popup_area);
}

/// Centered rect helper for popups.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{EventPayload, EventRuleSummary};
    use std::collections::HashMap;

    fn ev(event_id: &str, source: &str, severity: &str, ts: f64) -> EventPayload {
        EventPayload {
            event_id: event_id.into(),
            source_id: source.into(),
            severity: severity.into(),
            timestamp_num: ts,
            timestamp: format!("ts{ts}"),
            message: format!("msg-{event_id}"),
            captures: vec![],
            ..Default::default()
        }
    }

    fn state_with_events(events: Vec<EventPayload>) -> State {
        State {
            events,
            event_rules: {
                let mut m = HashMap::new();
                m.insert(
                    "DUT".into(),
                    vec![EventRuleSummary {
                        name: "fatal_error".into(),
                        severity: "error".into(),
                    }],
                );
                m
            },
            events_enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn lanes_preserve_first_seen_order() {
        let state = state_with_events(vec![
            ev("b", "DUT", "error", 200.0),
            ev("a", "DUT", "info", 100.0),
            ev("b", "DUT", "error", 300.0),
        ]);
        let view = EventsView::default();
        let lanes = EventsView::lanes(&state, &view);
        assert_eq!(lanes, vec!["b", "a"]); // first-seen order, not sorted
    }

    #[test]
    fn visible_events_sorted_by_timestamp() {
        let state = state_with_events(vec![
            ev("a", "DUT", "info", 300.0),
            ev("b", "DUT", "error", 100.0),
            ev("c", "DUT", "warn", 200.0),
        ]);
        let view = EventsView::default();
        let events = EventsView::visible_events(&state, &view);
        assert_eq!(events[0].timestamp_num, 100.0);
        assert_eq!(events[1].timestamp_num, 200.0);
        assert_eq!(events[2].timestamp_num, 300.0);
    }

    #[test]
    fn data_range_adds_5_percent_padding() {
        let state = state_with_events(vec![
            ev("a", "DUT", "info", 1000.0),
            ev("b", "DUT", "error", 2000.0),
        ]);
        let view = EventsView::default();
        let (start, end) = EventsView::data_range(&state, &view);
        // span = 1000, padding = 50 each side.
        assert!((start - 950.0).abs() < 1.0);
        assert!((end - 2050.0).abs() < 1.0);
    }

    #[test]
    fn zoom_narrows_range() {
        let state = state_with_events(vec![
            ev("a", "DUT", "info", 1000.0),
            ev("b", "DUT", "error", 3000.0),
        ]);
        let mut view = EventsView::default();
        let (s0, e0) = EventsView::effective_range(&state, &view);
        let data = EventsView::data_range(&state, &view);
        let current = EventsView::effective_range(&state, &view);
        view.zoom_ranges(data, current, 2.0); // zoom in
        let (s1, e1) = view.view_range.unwrap();
        assert!(e1 - s1 < e0 - s0);
    }

    #[test]
    fn reset_zoom_clears_view_range() {
        let mut view = EventsView {
            view_range: Some((100.0, 200.0)),
            ..Default::default()
        };
        view.reset_zoom();
        assert!(view.view_range.is_none());
    }

    #[test]
    fn filter_by_severity() {
        let state = state_with_events(vec![
            ev("a", "DUT", "info", 100.0),
            ev("b", "DUT", "error", 200.0),
        ]);
        let view = EventsView {
            hidden_severities: ["info".into()].into(),
            ..Default::default()
        };
        let events = EventsView::visible_events(&state, &view);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "b");
    }

    #[test]
    fn move_cursor_clamps() {
        let mut view = EventsView::default();
        view.move_cursor(10, 3);
        assert_eq!(view.cursor, 2);
        view.move_cursor(-10, 3);
        assert_eq!(view.cursor, 0);
    }

    #[test]
    fn severity_color_mapping() {
        assert_eq!(severity_color("fatal"), Color::LightRed);
        assert_eq!(severity_color("error"), Color::Red);
        assert_eq!(severity_color("warn"), Color::Yellow);
        assert_eq!(severity_color("info"), Color::Blue);
    }

    #[test]
    fn lanes_fallback_to_rules_when_no_events() {
        let state = State {
            event_rules: {
                let mut m = HashMap::new();
                m.insert(
                    "DUT".into(),
                    vec![
                        EventRuleSummary {
                            name: "fatal_error".into(),
                            severity: "error".into(),
                        },
                        EventRuleSummary {
                            name: "reboot".into(),
                            severity: "info".into(),
                        },
                    ],
                );
                m
            },
            ..Default::default()
        };
        let view = EventsView::default();
        let lanes = EventsView::lanes(&state, &view);
        assert_eq!(lanes, vec!["fatal_error", "reboot"]);
    }
}
