use std::process::Command;

use serde_json::Value;

const SKILL: &str = include_str!("../../../skills/embed-log/SKILL.md");

#[test]
fn skill_prints_canonical_markdown_and_optional_json() {
    let raw = Command::new(env!("CARGO_BIN_EXE_embed-log"))
        .args(["skill"])
        .output()
        .unwrap();
    assert!(raw.status.success());
    assert!(raw.stderr.is_empty());
    assert_eq!(raw.stdout, SKILL.as_bytes());

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
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["skill"], "embed-log");
    assert_eq!(value["format"], "markdown");
    assert_eq!(value["content"], SKILL);
    assert!(value["embed_log_version"].as_str().is_some());
}

#[test]
fn skill_rejects_removed_mode_arguments() {
    for args in [
        ["skill", "live"].as_slice(),
        ["skill", "recorded"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_embed-log"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}
