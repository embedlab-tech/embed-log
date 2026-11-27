//! Key dispatch — maps crossterm key events to State mutations + client commands.
//!
//! This is the interaction layer: scrolling, pane focus, sync, selection,
//! copy, markers, filter, unwrap, timestamp mode, help, and TX.

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

/// Handle one key event. Returns whether to quit.
pub fn handle_key(
    state: &mut State,
    client: &mut ClientHandle,
    key: &InputKey,
    terminal_height: u16,
) -> KeyAction {
    if state.tx_mode {
        handle_tx_key(state, client, key);
        return KeyAction::Continue;
    }
    if state.filter_mode {
        handle_filter_key(state, key, terminal_height);
        return KeyAction::Continue;
    }
    if key.is_quit() {
        return KeyAction::Quit;
    }
    if state.show_config {
        // shortcut: byte-length bounds allow blank overscroll; measure wrapped rows if exact bounds are needed.
        let max_scroll = serde_json::to_string_pretty(&state.runtime_config)
            .map(|text| text.len().saturating_sub(1).min(u16::MAX as usize) as u16)
            .unwrap_or(0);
        match key.code {
            KeyCode::Char('I') | KeyCode::Esc => state.show_config = false,
            KeyCode::Down | KeyCode::Char('j') => {
                state.config_scroll = state.config_scroll.saturating_add(1).min(max_scroll)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.config_scroll = state.config_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => {
                state.config_scroll = state.config_scroll.saturating_add(10).min(max_scroll)
            }
            KeyCode::PageUp => state.config_scroll = state.config_scroll.saturating_sub(10),
            _ => {}
        }
        return KeyAction::Continue;
    }
    if state.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => state.show_help = false,
            _ => {}
        }
        return KeyAction::Continue;
    }

    let visible = crate::draw::active_visible_rows(state, terminal_height);

    match key.code {
        // ── Help / TX open ──
        KeyCode::Char('?') => state.show_help = true,
        KeyCode::Char('I') => {
            state.show_config = true;
            state.config_scroll = 0;
        }
        KeyCode::Char('/') => {
            state.filter_mode = true;
            state.filter_buffer.clear();
        }
        KeyCode::Char(':') => state.open_tx_mode(),
        KeyCode::Char('i')
            if state
                .active_pane_id()
                .as_deref()
                .is_some_and(|p| state.is_writable(p)) =>
        {
            state.open_tx_mode();
        }
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
                state.scroll_to_bottom(&pane);
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
            if let Some((pane, idx)) = state.active_top(visible) {
                state.focus_line(pane, idx, visible);
            }
        }

        // ── Selection: Space toggles the current line ──
        KeyCode::Char(' ') => {
            if let Some((_, idx)) = state.active_top(visible) {
                state.toggle_select_active(idx);
            }
        }
        // Visual range select: 'v' enters/extends from anchor.
        KeyCode::Char('v') => {
            if let Some((pane, idx)) = state.active_top(visible) {
                match &state.visual_anchor {
                    Some((id, anchor)) if id == &pane => {
                        let range = idx.min(*anchor)..=idx.max(*anchor);
                        state.selected.insert(pane, range.collect());
                    }
                    _ => {
                        state.selected.entry(pane.clone()).or_default().insert(idx);
                        state.visual_anchor = Some((pane, idx));
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
            if let Some((pane, idx)) = state.active_top(visible) {
                state.toggle_marker_active(pane, idx);
                let _ = client.commands.try_send(ClientCommand::SaveMarkers {
                    markers: state.all_markers(),
                });
            }
        }
        // Marker navigation: [ prev, ] next.
        KeyCode::Char('[' | ']') => {
            if let Some(pane) = state.active_pane_id() {
                state.jump_marker(pane, key.code == KeyCode::Char(']'), visible);
            }
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
        // Export the current session as self-contained HTML. The server reports
        // completion through session_html_status in the status bar.
        KeyCode::Char('x') => {
            let _ = client.commands.try_send(ClientCommand::ExportSessionHtml);
            state.tx_status = Some("exporting session HTML…".to_string());
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

fn handle_filter_key(state: &mut State, key: &InputKey, terminal_height: u16) {
    match key.code {
        KeyCode::Esc => {
            state.filter_mode = false;
            state.filter_buffer.clear();
        }
        KeyCode::Enter => {
            let pattern = state.filter_buffer.trim();
            match regex::Regex::new(pattern) {
                Ok(regex) => {
                    let filter = (!pattern.is_empty()).then_some(regex);
                    state.tx_status = Some(if filter.is_none() {
                        "filter cleared".to_string()
                    } else {
                        format!("filter: {pattern}")
                    });
                    if let Some(pane) = state.active_pane_id() {
                        let visible = crate::draw::active_visible_rows(state, terminal_height);
                        state.set_filter(pane, filter, visible);
                    }
                    state.filter_mode = false;
                    state.filter_buffer.clear();
                }
                Err(error) => state.tx_status = Some(format!("invalid filter: {error}")),
            }
        }
        KeyCode::Backspace => {
            state.filter_buffer.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.filter_buffer.push(c);
        }
        _ => {}
    }
}

fn handle_tx_key(state: &mut State, client: &mut ClientHandle, key: &InputKey) {
    match key.code {
        KeyCode::Esc => state.close_tx_mode(),
        KeyCode::Enter => {
            if let Some(pane) = state.active_pane_id() {
                let text = state.tx_buffer.trim_end().to_string();
                if !text.is_empty() {
                    let _ = client.commands.try_send(ClientCommand::SendRaw {
                        id: pane,
                        data: format!("{text}\n"),
                    });
                    state.tx_status = Some(format!("sent {} bytes", text.len() + 1));
                }
                state.tx_buffer.clear();
                state.close_tx_mode();
            }
        }
        KeyCode::Backspace => {
            state.tx_buffer.pop();
            state.refresh_tx_matches();
        }
        KeyCode::Tab => state.cycle_tx_suggestion(false),
        KeyCode::BackTab => state.cycle_tx_suggestion(true),
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.tx_buffer.push(c);
            state.refresh_tx_matches();
        }
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ConfigMessage, TabDef};

    #[tokio::test]
    async fn configuration_opens_scrolls_and_closes_without_changing_log_state() {
        let mut state = State {
            runtime_config: serde_json::json!({"sources":["DUT"]}),
            ..Default::default()
        };
        let mut client = crate::client::spawn_client("ws://127.0.0.1:0".into());
        for code in [KeyCode::Char('I'), KeyCode::Down] {
            handle_key(
                &mut state,
                &mut client,
                &InputKey {
                    code,
                    modifiers: KeyModifiers::NONE,
                },
                24,
            );
        }
        assert!(state.show_config);
        assert_eq!(state.config_scroll, 1);
        assert_eq!(state.active_tab, 0);
        handle_key(
            &mut state,
            &mut client,
            &InputKey {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
            },
            24,
        );
        assert!(!state.show_config);
        client.stop();
    }

    #[tokio::test]
    async fn keys_keep_row_identity_after_eviction_in_a_merged_pane() {
        use crate::protocol::Marker;
        use crate::state::{StoredLine, MAX_LINES_PER_PANE, TRIM_SLACK};

        let mut s = State::default();
        s.apply_config(&ConfigMessage {
            tabs: vec![TabDef {
                panes: vec!["merged".into()],
                ..Default::default()
            }],
            ..Default::default()
        });
        let mut client = crate::client::spawn_client("ws://127.0.0.1:0".into());
        let mut press = |s: &mut State, code| {
            handle_key(
                s,
                &mut client,
                &InputKey {
                    code,
                    modifiers: KeyModifiers::NONE,
                },
                10,
            );
        };
        let append = |s: &mut State, i: usize| {
            s.append_line(
                "merged",
                StoredLine {
                    source_id: if i % 2 == 0 { "A" } else { "B" }.into(),
                    sequence: i as u64 + 1,
                    line_idx: (i / 2) as u64,
                    abs_num: i as f64,
                    message: format!("row-{i}"),
                    ..Default::default()
                },
            );
        };
        let limit = MAX_LINES_PER_PANE + TRIM_SLACK;
        for i in 0..limit {
            append(&mut s, i);
        }
        s.scroll.insert("merged".into(), 1);
        press(&mut s, KeyCode::Char(' ')); // evicted selection
        s.scroll.insert("merged".into(), 50_000);
        press(&mut s, KeyCode::Char('v'));
        press(&mut s, KeyCode::Enter);
        append(&mut s, limit);

        let first = 50_000 - (TRIM_SLACK + 1);
        assert_eq!(s.selected_for("merged"), vec![first]);
        assert_eq!(s.focused_line, Some(("merged".into(), first)));
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.selected_for("merged"), vec![first, first + 1]);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char('v'));
        let copied = crate::selection::selection_text(&s);
        assert_eq!(
            copied,
            "  [merged]  row-50000\n  [merged]  row-50001\n  [merged]  row-50002"
        );
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.focused_line, Some(("merged".into(), first + 2)));

        s.apply_markers(&[1, 70_001, 80_000].map(|i| Marker {
            pane_id: "merged".into(),
            line_idx: i / 2,
            num_ts: i as f64,
            ..Default::default()
        }));
        press(&mut s, KeyCode::Char('[')); // the only earlier marker was evicted
        assert_eq!(s.scroll_of("merged"), first + 2);
        for (key, id) in [(']', 70_001), (']', 80_000), ('[', 70_001)] {
            press(&mut s, KeyCode::Char(key));
            let center = s.viewport("merged", 4).2[2];
            assert_eq!(s.raw_lines["merged"][center].abs_num, id as f64);
        }
        assert_eq!(s.all_markers().len(), 3);
        client.stop();
    }

    #[test]
    fn cycle_tab_wraps_and_resets_active_pane() {
        let mut state = State::default();
        state.apply_config(&ConfigMessage {
            tabs: vec![
                TabDef {
                    label: "Device".into(),
                    panes: vec!["DUT".into(), "HOST".into()],
                    ..Default::default()
                },
                TabDef {
                    label: "UART".into(),
                    panes: vec!["UART".into()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        state.active_pane = 1;
        cycle_tab(&mut state, true);
        assert_eq!(state.active_tab, 1);
        assert_eq!(state.active_pane, 0);

        cycle_tab(&mut state, true);
        assert_eq!(state.active_tab, 0);
        cycle_tab(&mut state, false);
        assert_eq!(state.active_tab, 1);
    }
}
