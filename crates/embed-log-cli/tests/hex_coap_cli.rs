#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn invoke(runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_embed-log"))
        .args(args)
        .env("EMBED_LOG_RUNTIME_DIR", runtime)
        .env_remove("EMBED_LOG_INSTANCE")
        .output()
        .unwrap()
}

#[test]
fn configured_hex_coap_source_replaces_wire_hex_before_persistence() {
    let root = std::env::temp_dir().join(format!(
        "embed-log-hex-coap-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let runtime = root.join("runtime");
    let logs = root.join("logs");
    let input = root.join("capture.log");
    fs::create_dir_all(&root).unwrap();
    fs::write(&input, "").unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
        .to_string();
    let config = root.join("embed-log.yml");
    fs::write(
        &config,
        format!(
            "version: 2\nlogs:\n  dir: {}\nsources:\n  RADIO:\n    type: file\n    path: {}\n    parser:\n      type: hex-coap\n",
            logs.display(),
            input.display()
        ),
    )
    .unwrap();

    let started = invoke(
        &runtime,
        &[
            "run",
            "--daemon",
            "--instance",
            "coap",
            "--config",
            config.to_str().unwrap(),
            "--port",
            &port,
            "--json",
            "--frontend-dir",
            "/definitely/not/a/frontend",
        ],
    );
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let started_json: serde_json::Value = serde_json::from_slice(&started.stdout).unwrap();
    let session_id = started_json["backend"]["session_id"].as_str().unwrap();

    writeln!(
        OpenOptions::new().append(true).open(&input).unwrap(),
        "radio rx: 40 01 12 34 b3 66 6f 6f 03 62 61 72"
    )
    .unwrap();

    let mut records = serde_json::Value::Null;
    for _ in 0..100 {
        let read = invoke(
            &runtime,
            &[
                "sessions",
                "read",
                session_id,
                "--dir",
                logs.to_str().unwrap(),
                "--last",
                "1",
                "--json",
            ],
        );
        if read.status.success() {
            records = serde_json::from_slice(&read.stdout).unwrap();
            if !records["records"].as_array().unwrap().is_empty() {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    let message = records["records"][0][4].as_str().unwrap();
    assert!(
        message.starts_with("radio rx: [CoAP] t:CON c:GET"),
        "{message}"
    );
    assert!(message.contains("i:1234"), "{message}");
    assert!(message.contains("Uri-Path: foo"), "{message}");
    assert!(!message.contains("40 01 12 34"), "{message}");

    let stopped = invoke(&runtime, &["stop", "--instance", "coap", "--json"]);
    assert!(stopped.status.success());
    fs::remove_dir_all(root).unwrap();
}
