//! Messages published to live WebSocket clients.

use std::sync::Arc;

use axum::extract::ws::{Message, Utf8Bytes};
use serde_json::Value;

/// One message for live clients, shared by the broadcast channel and the
/// replay buffer.
///
/// The JSON text is serialized once when the message is created. Every `/ws`
/// client sends the same reference-counted buffer, and control clients read the
/// structured value instead of parsing the text again.
#[derive(Debug)]
pub struct LiveEvent {
    text: Utf8Bytes,
    value: Value,
}

/// Cheap-to-clone handle passed through `broadcast` and kept in the replay.
pub type LiveMessage = Arc<LiveEvent>;

impl LiveEvent {
    pub fn new(value: Value) -> LiveMessage {
        Arc::new(Self {
            text: value.to_string().into(),
            value,
        })
    }

    /// Build from a value and its already-serialized JSON text, avoiding a
    /// second serialization. `text` must be `value` serialized.
    pub fn with_text(value: Value, text: String) -> LiveMessage {
        debug_assert_eq!(
            serde_json::from_str::<Value>(&text).ok(),
            Some(value.clone())
        );
        Arc::new(Self {
            text: text.into(),
            value,
        })
    }

    /// The structured message.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The serialized JSON text.
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// A WebSocket text frame sharing this message's buffer (no copy).
    pub fn ws_message(&self) -> Message {
        Message::Text(self.text.clone())
    }
}
