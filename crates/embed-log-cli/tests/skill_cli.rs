use std::process::Command;

use serde_json::Value;

const CANONICAL_SKILL: &str = include_str!("../../../skills/embed-log/SKILL.md");

#[test]
fn skill_prints_canonical_markdown_and_optional_json_without_runtime_state() {
    let raw = Command::new(env!("CARGO_BIN_EXE_embed-log"))
        .arg("skill")
        .output()
        .unwrap();
    assert!(raw.status.success());
    assert!(raw.stderr.is_empty());
    assert_eq!(raw.stdout, CANONICAL_SKILL.as_bytes());

    let structured = Command::new(env!("CARGO_BIN_EXE_embed-log"))
        .args(["skill", "--json"])
        .output()
        .unwrap();
    assert!(structured.status.success());
    assert!(structured.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&structured.stdout).lines().count(),
        1
    );
    let value: Value = serde_json::from_slice(&structured.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["format"], "markdown");
    assert_eq!(value["content"], CANONICAL_SKILL);
    assert!(value["embed_log_version"].as_str().is_some());
}
