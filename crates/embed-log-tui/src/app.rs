//! The TUI application: terminal lifecycle + main event loop.
//!
//! Owns [`State`], a WS client handle, and an input receiver. The loop
//! `select!`s on:
//! - inbound server events → mutate `State`
//! - inbound key events → mutate `State` or quit
//!
//! Drawing is rate-limited by a short tick so bursts of WS messages coalesce
//! into one frame, and so the loop wakes even when idle to refresh the status
//! bar (connection state can change in the background).

use std::{
    io::{self, stdout},
    time::Duration,
};

use crate::{
    client::{ClientHandle, ConnectionState, ServerEvent},
    draw,
    input::{spawn_input, InputEvent},
    protocol::ServerMessage,
    state::State,
};
use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, MouseButton, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use tokio::select;
use tracing::warn;

/// Frame coalesce interval: when many server messages arrive at once, we draw
/// at most one frame per this window.
const FRAME_TICK: Duration = Duration::from_millis(33);

/// Run the TUI app against an already-spawned WS client.
///
/// Takes ownership of the client handle (for stop-on-quit) and the app name
/// (shown in the status bar). Restores the terminal on exit, including on
/// error/panic paths.
pub async fn run(mut client: ClientHandle, app_name: &str) -> Result<()> {
    let mut state = State {
        app_name: app_name.to_string(),
        ..Default::default()
    };

    // Terminal setup.
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    // Input polling task.
    let mut keys = spawn_input();

    // Drive the loop; ensure teardown happens no matter how we exit.
    let result = run_loop(&mut terminal, &mut state, &mut client, &mut keys).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut State,
    client: &mut ClientHandle,
    inputs: &mut tokio::sync::mpsc::Receiver<InputEvent>,
) -> Result<()> {
    let mut interval = tokio::time::interval(FRAME_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        // Draw the current frame.
        terminal.draw(|f| draw::draw(f, state))?;

        select! {
            // Server event inbound.
            maybe_ev = client.events.recv() => {
                let Some(ev) = maybe_ev else {
                    // Event channel closed: client task exited. Keep looping so
                    // the user sees the disconnect and can quit cleanly.
                    continue;
                };
                handle_server_event(state, ev);
            }
            // Input event inbound.
            maybe_input = inputs.recv() => {
                let Some(input) = maybe_input else { continue; };
                let size = terminal.size().unwrap_or(ratatui::layout::Size { width: 80, height: 24 });
                match input {
                    InputEvent::Key(key) => {
                        match crate::keys::handle_key(state, client, &key, size.height) {
                            crate::keys::KeyAction::Quit => break,
                            crate::keys::KeyAction::Continue => {}
                        }
                    }
                    InputEvent::Mouse(mouse) => {
                        handle_mouse(state, &mouse, size.width, size.height);
                    }
                }
            }
            // Frame tick: forces a redraw even if no events (status bar updates,
            // connection state changes observed in background).
            _ = interval.tick() => {}
        }
    }
    Ok(())
}

/// Apply one server event to state.
fn handle_server_event(state: &mut State, ev: ServerEvent) {
    match ev {
        ServerEvent::State(s) => {
            state.conn = match s {
                ConnectionState::Disconnected => crate::state::ConnState::Disconnected,
                ConnectionState::Connecting => crate::state::ConnState::Connecting,
                ConnectionState::Connected => crate::state::ConnState::Connected,
                ConnectionState::Reconnecting => crate::state::ConnState::Reconnecting,
            };
        }
        ServerEvent::Message(msg) => handle_message(state, *msg),
        ServerEvent::Unparsed(s) => {
            warn!("unparsed server frame ({} bytes)", s.len());
        }
    }
}

/// Apply one parsed server message to state.
fn handle_message(state: &mut State, msg: ServerMessage) {
    match msg {
        ServerMessage::Config(c) => state.apply_config(&c),
        ServerMessage::Rx(p) => {
            let mut line = crate::state::StoredLine::from_payload(&p);
            line.is_tx = false;
            state.append_line(&p.source_id, line);
        }
        ServerMessage::Tx(p) => {
            let mut line = crate::state::StoredLine::from_payload(&p);
            line.is_tx = true;
            state.append_line(&p.source_id, line);
        }
        ServerMessage::Event(e) => state.push_event(e),
        ServerMessage::SessionInfo(s) => state.apply_session_info(&s.session),
        ServerMessage::MarkersUpdate(m) => state.apply_markers(&m.markers),
        ServerMessage::SessionHtmlStatus(_) => {
            // Export status is currently handled by the browser UI/CLI.
        }
        ServerMessage::SessionRotated(_) => {
            // Server follows with a fresh config; tear down and await it.
            state.teardown_layout();
        }
        ServerMessage::ClearLogs(c) => state.clear(c.pane.as_deref()),
        ServerMessage::FilterResult(_) => {
            // Reserved for future interactive filter UI feedback.
        }
        ServerMessage::SendRawResult(v) => {
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            let source = v.get("source_id").and_then(|x| x.as_str()).unwrap_or("");
            if ok {
                let bytes = v.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0);
                state.tx_status = Some(format!("{source} {bytes}B ok"));
            } else {
                let err = v
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("send failed");
                state.tx_status = Some(format!("{source} {err}"));
            }
        }
        ServerMessage::Unknown => {}
    }
}

/// Handle mouse interaction: wheel scroll, left-click focus, left-click sync.
fn handle_mouse(state: &mut State, mouse: &crossterm::event::MouseEvent, width: u16, height: u16) {
    let area = Rect::new(0, 0, width, height);
    let Some(hit) = draw::hit_test_pane(state, area, mouse.column, mouse.row) else {
        return;
    };
    let visible = height.saturating_sub(6).max(1) as usize;

    match mouse.kind {
        MouseEventKind::ScrollDown => {
            state.active_pane = hit.pane_index;
            state.scroll_active(3, visible);
        }
        MouseEventKind::ScrollUp => {
            state.active_pane = hit.pane_index;
            state.scroll_active(-3, visible);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            state.active_pane = hit.pane_index;
            if let Some(raw_idx) = hit.raw_idx {
                if let Some(line) = state
                    .raw_lines
                    .get(&hit.pane_id)
                    .and_then(|l| l.get(raw_idx))
                {
                    state.focused_raw_idx = Some(line.line_idx);
                    state.sync_panes_to_ts(line.abs_num, visible);
                }
            }
        }
        _ => {}
    }
}

/// Restore the terminal to its original state.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")?;
    Ok(())
}
