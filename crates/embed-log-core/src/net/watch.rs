//! Temporary, process-local watches used by automation clients.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Value};

pub const MAX_WATCH_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_WATCHES: usize = 1_024;

#[derive(Debug, Clone)]
pub struct TemporaryWatch {
    pub id: String,
    pub source_id: String,
    pub kind: String,
    pub pattern: String,
    pub regex: Regex,
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

pub fn expire_watch(watches: &Arc<RwLock<HashMap<String, TemporaryWatch>>>, watch_id: &str) {
    if let Ok(mut watches) = watches.write() {
        if let Some(watch) = watches.get_mut(watch_id) {
            if watch.status == WatchStatus::Active {
                watch.status = WatchStatus::Expired;
            }
        }
    }
}

/// Match a committed log record against active watches for its source. Matches
/// are retained in process memory before a waiter needs to connect. TX records
/// never satisfy a watch.
pub fn retain_matching_watches(
    watches: &Arc<RwLock<HashMap<String, TemporaryWatch>>>,
    source_id: &str,
    is_tx: bool,
    message: &str,
    record: &Value,
) {
    if is_tx {
        return;
    }
    let Ok(mut watches) = watches.write() else {
        return;
    };
    let now = Utc::now();
    for watch in watches.values_mut() {
        if watch.source_id != source_id || watch.status != WatchStatus::Active {
            continue;
        }
        if now >= watch.expires_at {
            watch.status = WatchStatus::Expired;
            continue;
        }
        let Some(captures) = watch.regex.captures(message) else {
            continue;
        };
        let captures = captures
            .iter()
            .map(|capture| capture.map_or_else(String::new, |value| value.as_str().to_string()))
            .collect::<Vec<_>>();
        let mut matched = record.clone();
        if let Some(object) = matched.as_object_mut() {
            object.insert("captures".to_string(), json!(captures));
        }
        watch.matched = Some(matched);
        watch.status = WatchStatus::Matched;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_watch(id: &str) -> TemporaryWatch {
        TemporaryWatch {
            id: id.to_string(),
            source_id: "DUT".to_string(),
            kind: "contains".to_string(),
            pattern: "ready".to_string(),
            regex: Regex::new("ready").unwrap(),
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::seconds(30),
            once: true,
            status: WatchStatus::Active,
            matched: None,
        }
    }

    #[test]
    fn match_is_retained_without_event_persistence() {
        let watches = Arc::new(RwLock::new(HashMap::from([(
            "watch-1".to_string(),
            active_watch("watch-1"),
        )])));
        let record = json!({
            "type":"rx",
            "source_id":"DUT",
            "message":"system ready",
            "sequence":42,
            "session_id":"session-a"
        });

        retain_matching_watches(&watches, "DUT", false, "system ready", &record);

        let watches = watches.read().unwrap();
        let watch = watches.get("watch-1").unwrap();
        assert_eq!(watch.status, WatchStatus::Matched);
        assert_eq!(watch.matched.as_ref().unwrap()["sequence"], 42);
        assert_eq!(watch.matched.as_ref().unwrap()["session_id"], "session-a");
    }

    #[test]
    fn tx_does_not_satisfy_watch() {
        let watches = Arc::new(RwLock::new(HashMap::from([(
            "watch-1".to_string(),
            active_watch("watch-1"),
        )])));
        retain_matching_watches(&watches, "DUT", true, "ready", &json!({"message":"ready"}));
        assert_eq!(
            watches.read().unwrap()["watch-1"].status,
            WatchStatus::Active
        );
    }

    #[test]
    fn expiration_updates_active_watch() {
        let watches = Arc::new(RwLock::new(HashMap::from([(
            "watch-1".to_string(),
            active_watch("watch-1"),
        )])));
        expire_watch(&watches, "watch-1");
        assert_eq!(
            watches.read().unwrap()["watch-1"].status,
            WatchStatus::Expired
        );
    }
}
