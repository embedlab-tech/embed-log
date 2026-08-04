use std::process::Command;

use serde_json::Value;

const LIVE_SKILL: &str = include_str!("../../../skills/embed-log-live/SKILL.md");
const RECORDED_SKILL: &str = include_str!("../../../skills/embed-log-recorded/SKILL.md");

#[test]
fn skill_prints_selected_canonical_markdown_and_optional_json() {
    for (kind, canonical) in [("live", LIVE_SKILL), ("recorded", RECORDED_SKILL)] {
        let raw = Command::new(env!("CARGO_BIN_EXE_embed-log"))
            .args(["skill", kind])
            .output()
            .unwrap();
        assert!(raw.status.success());
        assert!(raw.stderr.is_empty());
        assert_eq!(raw.stdout, canonical.as_bytes());

        let structured = Command::new(env!("CARGO_BIN_EXE_embed-log"))
            .args(["skill", kind, "--json"])
            .output()
            .unwrap();
        assert!(structured.status.success());
        assert!(structured.stderr.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&structured.stdout).lines().count(),
            1
        );
        let value: Value = serde_json::from_slice(&structured.stdout).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["skill"], kind);
        assert_eq!(value["format"], "markdown");
        assert_eq!(value["content"], canonical);
        assert!(value["embed_log_version"].as_str().is_some());
    }
}

#[test]
fn skill_requires_a_known_investigation_mode() {
    for args in [["skill"].as_slice(), ["skill", "unknown"].as_slice()] {
        let output = Command::new(env!("CARGO_BIN_EXE_embed-log"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}
