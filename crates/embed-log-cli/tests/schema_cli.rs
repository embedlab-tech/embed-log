use std::process::{Command, Output};

use serde_json::Value;

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_embed-log"))
        .args(args)
        .output()
        .expect("run embed-log")
}

fn parse_success(args: &[&str]) -> Value {
    let output = invoke(args);
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "schema polluted stderr");
    serde_json::from_slice(&output.stdout).expect("schema stdout is one JSON document")
}

#[test]
fn schema_discovers_capabilities_and_targeted_commands_without_runtime_state() {
    let output = invoke(&["schema"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).lines().count(),
        1,
        "default schema must be compact"
    );
    let index: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(index["schema_version"], 1);
    assert_eq!(index["kind"], "embed-log.capabilities");
    assert_eq!(index["defaults"]["endpoint"], "127.0.0.1:18080");
    assert_eq!(index["limits"]["read_records_max"], 1000);
    let commands = index["commands"].as_array().unwrap();
    assert!(commands.iter().any(|command| command == "sessions.read"));
    assert!(commands.iter().any(|command| command == "watch.wait"));
    assert!(!commands.iter().any(|command| command == "help"));
    assert!(!commands.iter().any(|command| command == "hello"));

    let read = parse_success(&["schema", "sessions.read"]);
    assert_eq!(read["kind"], "embed-log.command");
    assert_eq!(read["command"], "sessions.read");
    assert_eq!(read["output"]["next_cursor"], "next_cursor");
    assert_eq!(read["targeting"]["mode"], "offline_session");
    let limit = read["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|argument| argument["id"] == "limit")
        .unwrap();
    assert_eq!(limit["type"], "integer");
    assert_eq!(limit["maximum"], 1000);

    let tx = parse_success(&["schema", "tx", "--json"]);
    assert_eq!(tx["mutates"], true);
    assert_eq!(tx["targeting"]["sole_instance_inference"], false);
    assert!(tx["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|code| code == "EXPECT_TIMEOUT"));

    let split = parse_success(&["schema", "watch", "wait"]);
    assert_eq!(split["command"], "watch.wait");
    assert_eq!(split["output"]["next_cursor_on_match"], true);

    let errors = parse_success(&["schema", "errors"]);
    assert_eq!(errors["coverage"], "all_json_invocations");
    let config = parse_success(&["schema", "config"]);
    assert_eq!(config["canonical_version"], 2);
}

#[test]
fn schema_pretty_is_valid_json_and_unknown_selector_fails_actionably() {
    let pretty = invoke(&["schema", "sessions", "around", "--pretty"]);
    assert!(pretty.status.success());
    assert!(String::from_utf8_lossy(&pretty.stdout).lines().count() > 1);
    let value: Value = serde_json::from_slice(&pretty.stdout).unwrap();
    assert_eq!(value["command"], "sessions.around");

    let unknown = invoke(&["schema", "not-a-command"]);
    assert!(!unknown.status.success());
    assert!(unknown.stderr.is_empty());
    let failure: Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(failure["error"]["code"], "SCHEMA_SELECTOR_NOT_FOUND");
    assert!(failure["error"]["message"]
        .as_str()
        .unwrap()
        .contains("run `embed-log schema`"));
}
