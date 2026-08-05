use std::process::{Command, Output};

use serde_json::Value;

fn invoke(runtime: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_embed-log"))
        .args(args)
        .env("EMBED_LOG_RUNTIME_DIR", runtime)
        .env_remove("EMBED_LOG_INSTANCE")
        .output()
        .expect("run embed-log")
}

fn assert_json_failure(output: Output, code: &str) -> Value {
    assert!(!output.status.success());
    assert!(
        output.stderr.is_empty(),
        "JSON failure polluted stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON failure document");
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], code);
    assert!(value["error"]["message"].as_str().is_some());
    assert!(value["error"].get("details").is_some());
    value
}

#[test]
fn json_usage_and_runtime_failures_share_one_envelope() {
    let temp = std::env::temp_dir().join(format!(
        "embed-log-json-errors-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp).unwrap();

    assert_json_failure(invoke(&temp, &["tx", "--json"]), "CLI_USAGE");
    assert_json_failure(invoke(&temp, &["stop", "--json"]), "INSTANCE_REQUIRED");
    let human = invoke(&temp, &["stop"]);
    assert!(!human.status.success());
    assert!(human.stdout.is_empty());
    assert!(String::from_utf8_lossy(&human.stderr).starts_with("Error: "));

    let _ = std::fs::remove_dir_all(temp);
}
