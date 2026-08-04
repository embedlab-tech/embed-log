//! Temporary, process-local watches used by automation clients.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::config::EventRule;

pub const MAX_WATCH_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_WATCHES: usize = 1_024;

#[derive(Debug, Clone)]
pub struct TemporaryWatch {
    pub id: String,
    pub source_id: String,
    pub kind: String,
    pub pattern: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub once: bool,
    pub status: WatchStatus,
    pub matched: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchStatus {
    Active,
    Matched,
    Expired,
}

impl WatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Matched => "matched",
            Self::Expired => "expired",
        }
    }
}

impl TemporaryWatch {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "source_id": self.source_id,
            "kind": self.kind,
            "pattern": self.pattern,
            "once": self.once,
            "status": self.status.as_str(),
            "created_at": self.created_at.to_rfc3339(),
            "expires_at": self.expires_at.to_rfc3339(),
            "match": self.matched,
        })
    }
}

pub fn remove_runtime_rule(
    rules: &Arc<RwLock<HashMap<String, Vec<EventRule>>>>,
    source_id: &str,
    watch_id: &str,
) {
    if let Ok(mut rules) = rules.write() {
        if let Some(source_rules) = rules.get_mut(source_id) {
            source_rules.retain(|rule| rule.name != watch_id);
            if source_rules.is_empty() {
                rules.remove(source_id);
            }
        }
    }
}

/// Mark an active watch expired and deactivate its event rule.
pub fn expire_watch(
    watches: &Arc<RwLock<HashMap<String, TemporaryWatch>>>,
    rules: &Arc<RwLock<HashMap<String, Vec<EventRule>>>>,
    watch_id: &str,
) {
    let source = if let Ok(mut watches) = watches.write() {
        watches.get_mut(watch_id).and_then(|watch| {
            if watch.status == WatchStatus::Active {
                watch.status = WatchStatus::Expired;
                Some(watch.source_id.clone())
            } else {
                None
            }
        })
    } else {
        None
    };
    if let Some(source) = source {
        remove_runtime_rule(rules, &source, watch_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMatchResult {
    NotWatch,
    Ignored,
    Retained,
}

/// Retain a match before any waiter needs to connect. TX and expired matches
/// are ignored and do not produce persisted watch events.
pub fn retain_watch_match(
    watches: &Arc<RwLock<HashMap<String, TemporaryWatch>>>,
    rules: &Arc<RwLock<HashMap<String, Vec<EventRule>>>>,
    watch_id: &str,
    is_tx: bool,
    event_payload: &Value,
    session_id: &str,
) -> WatchMatchResult {
    let (source, once) = if let Ok(mut watches) = watches.write() {
        let Some(watch) = watches.get_mut(watch_id) else {
            return WatchMatchResult::NotWatch;
        };
        if watch.status != WatchStatus::Active {
            return WatchMatchResult::Ignored;
        }
        if Utc::now() >= watch.expires_at {
            watch.status = WatchStatus::Expired;
            let source = watch.source_id.clone();
            drop(watches);
            remove_runtime_rule(rules, &source, watch_id);
            return WatchMatchResult::Ignored;
        }
        if is_tx {
            return WatchMatchResult::Ignored;
        }
        let mut retained = event_payload.clone();
        if let Some(object) = retained.as_object_mut() {
            object.insert("session_id".to_string(), json!(session_id));
            // Global sequence is added by the next MVP milestone. Keep the
            // output shape stable without pretending line_idx is global.
            object.insert("sequence".to_string(), Value::Null);
        }
        watch.matched = Some(retained);
        watch.status = WatchStatus::Matched;
        (watch.source_id.clone(), watch.once)
    } else {
        return WatchMatchResult::Ignored;
    };
    if once {
        remove_runtime_rule(rules, &source, watch_id);
    }
    WatchMatchResult::Retained
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn rule(id: &str) -> EventRule {
        EventRule {
            name: id.to_string(),
            pattern: "ready".to_string(),
            severity: "info".to_string(),
            regex: Regex::new("ready").unwrap(),
        }
    }

    fn active_watch(id: &str) -> TemporaryWatch {
        TemporaryWatch {
            id: id.to_string(),
            source_id: "DUT".to_string(),
            kind: "contains".to_string(),
            pattern: "ready".to_string(),
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::seconds(10),
            once: true,
            status: WatchStatus::Active,
            matched: None,
        }
    }

    #[test]
    fn match_is_retained_and_once_rule_is_removed() {
        let watches = Arc::new(RwLock::new(HashMap::from([(
            "watch-1".to_string(),
            active_watch("watch-1"),
        )])));
        let rules = Arc::new(RwLock::new(HashMap::from([(
            "DUT".to_string(),
            vec![rule("watch-1")],
        )])));
        assert_eq!(
            retain_watch_match(
                &watches,
                &rules,
                "watch-1",
                false,
                &json!({"message":"ready","line_idx":2}),
                "session-a",
            ),
            WatchMatchResult::Retained
        );
        let watch = watches.read().unwrap()["watch-1"].clone();
        assert_eq!(watch.status, WatchStatus::Matched);
        assert_eq!(watch.matched.unwrap()["session_id"], "session-a");
        assert!(rules.read().unwrap().get("DUT").is_none());
    }

    #[test]
    fn tx_does_not_consume_watch() {
        let watches = Arc::new(RwLock::new(HashMap::from([(
            "watch-1".to_string(),
            active_watch("watch-1"),
        )])));
        let rules = Arc::new(RwLock::new(HashMap::new()));
        assert_eq!(
            retain_watch_match(
                &watches,
                &rules,
                "watch-1",
                true,
                &json!({"message":"ready"}),
                "session-a",
            ),
            WatchMatchResult::Ignored
        );
        assert_eq!(
            watches.read().unwrap()["watch-1"].status,
            WatchStatus::Active
        );
    }
}
