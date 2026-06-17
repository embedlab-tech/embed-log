//! Key dispatch — maps crossterm key events to State mutations + client commands.
//!
//! This is the interaction layer (Phases 4–5): scrolling, pane focus, sync,
//! selection, copy, markers, filter, unwrap, timestamp mode, theme.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    client::ClientHandle,
    input::InputKey,
    protocol::ClientCommand,
    state::{SelectionScope, State},
};

/// Result of handling a key.
pub enum KeyAction {
    /// Continue running.
    Continue,
    /// Quit the app.
    Quit,
}

/// Visible rows in the active pane, derived from terminal height.
/// Chrome: tab bar (3) + status bar (1) + pane borders (2) = 6 rows.
fn pane_visible_rows(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(6).max(1) as usize
}

/// Handle one key event. Returns whether to quit.
pub fn handle_key(
    state: &mut State,
    client: &mut ClientHandle,
    key: &InputKey,
    terminal_height: u16,
) -> KeyAction {
    if key.is_quit() {
        return KeyAction::Quit;
    }

    let visible = pane_visible_rows(terminal_height);

    // Events tab has its own key handling (cursor, zoom, sync, detail).
    if state.events_tab_active() {
        handle_events_key(state, client, key, visible);
        return KeyAction::Continue;
    }

    match key.code {
        // ── Tab navigation ──
        KeyCode::Tab => cycle_tab(state, true),
        KeyCode::BackTab => cycle_tab(state, false),

        // ── Pane focus (within a 2-pane tab) ──
        KeyCode::Char('h') | KeyCode::Left if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            if !state.unwrap {
                state.active_pane = 0;
            }
        }
        KeyCode::Char('l') | KeyCode::Right if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            if !state.unwrap {
                let panes = state.active_tab_panes();
                if panes.len() > 1 {
                    state.active_pane = 1;
                }
            }
        }

        // ── Splitter resize ──
        KeyCode::Char('H') | KeyCode::Char('_') => {
            state.splitter = (state.splitter - 0.05).max(0.1);
        }
        KeyCode::Char('L') => {
            state.splitter = (state.splitter + 0.05).min(0.9);
        }

        // ── Scrolling ──
        KeyCode::Char('j') | KeyCode::Down => state.scroll_active(1, visible),
        KeyCode::Char('k') | KeyCode::Up => state.scroll_active(-1, visible),
        KeyCode::PageDown => state.scroll_active(visible as i32 / 2, visible),
        KeyCode::PageUp => state.scroll_active(-(visible as i32 / 2), visible),
        KeyCode::Char('g') => {
            if let Some(pane) = state.active_pane_id() {
                state.scroll_to_top(&pane);
            }
        }
        KeyCode::Char('G') => {
            if let Some(pane) = state.active_pane_id() {
                state.scroll_to_bottom(&pane, visible);
            }
        }
        // Ctrl-d / Ctrl-u: half-page scroll.
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.scroll_active(visible as i32 / 2, visible);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.scroll_active(-(visible as i32 / 2), visible);
        }

        // ── Sync: Enter syncs panes to the active line's timestamp ──
        KeyCode::Enter => {
            if let Some(pane) = state.active_pane_id() {
                let scroll = state.scroll_of(&pane);
                if let Some(ts) = state
                    .raw_lines
                    .get(&pane)
                    .and_then(|l| l.get(scroll))
                    .map(|l| l.abs_num)
                {
                    state.sync_panes_to_ts(ts, visible);
                }
            }
        }

        // ── Selection: Space toggles the current line ──
        KeyCode::Char(' ') => {
            if let Some(pane) = state.active_pane_id() {
                let scroll = state.scroll_of(&pane);
                if let Some(line_idx) = state
                    .raw_lines
                    .get(&pane)
                    .and_then(|l| l.get(scroll))
                    .map(|l| l.line_idx)
                {
                    state.toggle_select_active(line_idx);
                }
            }
        }
        // Visual range select: 'v' enters/extends from anchor.
        KeyCode::Char('v') => {
            if let Some(pane) = state.active_pane_id() {
                let scroll = state.scroll_of(&pane);
                let line_idx = state
                    .raw_lines
                    .get(&pane)
                    .and_then(|l| l.get(scroll))
                    .map(|l| l.line_idx);
                if let Some(idx) = line_idx {
                    match state.visual_anchor {
                        None => {
                            state.visual_anchor = Some(idx);
                            state.toggle_select_active(idx);
                        }
                        Some(anchor) => {
                            // Select range from anchor to current.
                            let set = state.selected.entry(pane.clone()).or_default();
                            let (lo, hi) = if anchor <= idx {
                                (anchor, idx)
                            } else {
                                (idx, anchor)
                            };
                            // Re-select the range.
                            set.clear();
                            for i in lo..=hi {
                                set.insert(i);
                            }
                            state.visual_anchor = Some(anchor);
                        }
                    }
                }
            }
        }
        // Clear selection.
        KeyCode::Esc => {
            state.clear_selection();
        }
        // Toggle scope: Exact ↔ Context.
        KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.selection_scope = match state.selection_scope {
                SelectionScope::Exact => SelectionScope::Context,
                SelectionScope::Context => SelectionScope::Exact,
            };
        }
        // Yank (copy) selection to clipboard.
        KeyCode::Char('y') => {
            let text = crate::selection::selection_text(state);
            if !text.is_empty() {
                let _ = crate::selection::copy_to_clipboard(&text);
            }
        }

        // ── Markers ──
        // Toggle marker on current line.
        KeyCode::Char('m') => {
            if let Some(pane) = state.active_pane_id() {
                let scroll = state.scroll_of(&pane);
                if let Some(line) = state.raw_lines.get(&pane).and_then(|l| l.get(scroll)) {
                    let markers = state.toggle_marker_active(line.line_idx, line.abs_num, "");
                    let _ = client.commands.try_send(ClientCommand::SaveMarkers {
                        markers: state.all_markers(),
                    });
                    let _ = markers; // markers already in state
                }
            }
        }
        // Marker navigation: [ prev, ] next.
        KeyCode::Char('[') => {
            if let Some(target) = state.nav_marker(false, state.include_event_markers) {
                if let Some(pane) = state.active_pane_id() {
                    let target = target as usize;
                    let center = target.saturating_sub(visible / 2);
                    state.set_scroll(&pane, center, visible);
                }
            }
        }
        KeyCode::Char(']') => {
            if let Some(target) = state.nav_marker(true, state.include_event_markers) {
                if let Some(pane) = state.active_pane_id() {
                    let target = target as usize;
                    let center = target.saturating_sub(visible / 2);
                    state.set_scroll(&pane, center, visible);
                }
            }
        }
        // Toggle include event markers in navigation.
        KeyCode::Char('M') => {
            state.include_event_markers = !state.include_event_markers;
        }

        // ── Toggles ──
        KeyCode::Char('u') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.unwrap = !state.unwrap;
            state.active_tab = 0;
            state.active_pane = 0;
        }
        KeyCode::Char('t') => {
            use crate::state::TimestampMode;
            state.timestamp_mode = match state.timestamp_mode {
                TimestampMode::Absolute => TimestampMode::Relative,
                TimestampMode::Relative => TimestampMode::Absolute,
            };
        }
        // Clear logs (active pane).
        KeyCode::Char('C') => {
            let pane = state.active_pane_id();
            let _ = client.commands.try_send(ClientCommand::ClearLogs { pane });
        }

        _ => {}
    }

    KeyAction::Continue
}

/// Cycle to the next/previous tab (or pane in unwrap mode).
fn cycle_tab(state: &mut State, forward: bool) {
    let len = state.tab_count();
    if len == 0 {
        return;
    }
    if forward {
        state.active_tab = (state.active_tab + 1) % len;
    } else {
        state.active_tab = (state.active_tab + len - 1) % len;
    }
    state.active_pane = 0;
}

/// Handle keys in the Events tab.
fn handle_events_key(
    state: &mut State,
    _client: &mut ClientHandle,
    key: &InputKey,
    visible: usize,
) {
    let events_len = crate::events::EventsView::visible_events(state, &state.events_view).len();

    match key.code {
        // Cursor movement.
        KeyCode::Char('j') | KeyCode::Down => {
            state.events_view.move_cursor(1, events_len);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.events_view.move_cursor(-1, events_len);
        }
        KeyCode::PageDown => {
            state.events_view.move_cursor(10, events_len);
        }
        KeyCode::PageUp => {
            state.events_view.move_cursor(-10, events_len);
        }

        // Zoom.
        KeyCode::Char('+') | KeyCode::Char('=') => {
            let data = crate::events::EventsView::data_range(state, &state.events_view);
            let current = crate::events::EventsView::effective_range(state, &state.events_view);
            state.events_view.zoom_ranges(data, current, 1.7);
        }
        KeyCode::Char('-') => {
            let data = crate::events::EventsView::data_range(state, &state.events_view);
            let current = crate::events::EventsView::effective_range(state, &state.events_view);
            state.events_view.zoom_ranges(data, current, 1.0 / 1.7);
        }
        KeyCode::Char('0') => {
            state.events_view.reset_zoom();
        }

        // Detail popup toggle.
        KeyCode::Char('K') => {
            state.events_view.show_detail = !state.events_view.show_detail;
        }
        KeyCode::Esc => {
            state.events_view.show_detail = false;
        }

        // Enter: sync log panes to the event's timestamp + switch to source tab.
        KeyCode::Enter => {
            let events = crate::events::EventsView::visible_events(state, &state.events_view);
            if let Some(ev) = events.get(state.events_view.cursor) {
                let ts = ev.timestamp_num;
                let source = ev.source_id.clone();
                // Switch to the tab containing the source.
                let tab_idx = state.tabs.iter().position(|t| t.pane_ids.contains(&source));
                if let Some(idx) = tab_idx {
                    state.active_tab = idx;
                    state.active_pane = 0;
                }
                // Sync panes to the event timestamp.
                state.sync_panes_to_ts(ts, visible);
            }
        }

        _ => {}
    }
}
