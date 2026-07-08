//! Crossterm terminal input.
//!
//! Spawns a background task that polls crossterm events and forwards them to
//! the app via an `mpsc<InputEvent>`. The app's main loop selects on input and
//! server events, so terminal interaction and WS traffic never block each other.
//!
//! Mouse support is required for wheel scrolling and click-to-sync.

use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use tokio::sync::mpsc;
use tracing::debug;

/// Poll interval for the input task. Crossterm's `poll` is blocking, so we
/// poll for a short window then loop; this lets the task check the stop flag
/// regularly without a separate watcher.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A normalized key event the app acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputKey {
    /// The key code.
    pub code: KeyCode,
    /// Active modifiers (ctrl, shift, alt).
    pub modifiers: KeyModifiers,
}

impl InputKey {
    /// Whether this is the quit binding (`q` or `Ctrl+c`).
    pub fn is_quit(&self) -> bool {
        matches!(self.code, KeyCode::Char('q'))
            || (matches!(self.code, KeyCode::Char('c'))
                && self.modifiers.contains(KeyModifiers::CONTROL))
    }

    /// Whether this is a tab-cycle forward (`Tab`) or backward (`BackTab`).
    pub fn is_tab_cycle(&self) -> bool {
        matches!(self.code, KeyCode::Tab | KeyCode::BackTab)
    }
}

impl From<KeyEvent> for InputKey {
    fn from(e: KeyEvent) -> Self {
        Self {
            code: e.code,
            modifiers: e.modifiers,
        }
    }
}

/// Any terminal input the app handles.
#[derive(Debug, Clone)]
pub enum InputEvent {
    Key(InputKey),
    Mouse(MouseEvent),
}

/// Spawn the input polling task. Returns a receiver of input events.
pub fn spawn_input() -> mpsc::Receiver<InputEvent> {
    let (tx, rx) = mpsc::channel::<InputEvent>(128);
    tokio::task::spawn_blocking(move || loop {
        if let Ok(true) = event::poll(POLL_INTERVAL) {
            match event::read() {
                Ok(Event::Key(k)) => {
                    use crossterm::event::KeyEventKind;
                    if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        && tx.blocking_send(InputEvent::Key(k.into())).is_err()
                    {
                        debug!("input channel closed; exiting input task");
                        return;
                    }
                }
                Ok(Event::Mouse(m)) => {
                    if tx.blocking_send(InputEvent::Mouse(m)).is_err() {
                        debug!("input channel closed; exiting input task");
                        return;
                    }
                }
                Ok(_) => { /* ignore resize/focus for now */ }
                Err(e) => {
                    debug!("crossterm read error: {e}; exiting input task");
                    return;
                }
            }
        }
        if tx.is_closed() {
            return;
        }
    });
    rx
}
