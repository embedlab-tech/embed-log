#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_embed-log"))
}

fn root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("embed-log-sequence-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn invoke(runtime: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .env("EMBED_LOG_RUNTIME_DIR", runtime)
        .env_remove("EMBED_LOG_INSTANCE")
        .output()
        .unwrap()
}

fn append(path: &Path, message: &str) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{message}").unwrap();
    file.flush().unwrap();
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn global_sequence_bounded_reads_context_and_rotation() {
    let root = root();
    let runtime = root.join("runtime");
    let logs = root.join("logs");
    let dut = root.join("dut.log");
    let host = root.join("host.log");
    fs::write(&dut, "").unwrap();
    fs::write(&host, "").unwrap();
    let config = root.join("config.yml");
    fs::write(
        &config,
        format!(
            "version: 2\nlogs:\n  dir: {}\nsources:\n  DUT:\n    type: file\n    path: {}\n  HOST:\n    type: file\n    path: {}\n",
            logs.display(),
            dut.display(),
            host.display()
        ),
    )
    .unwrap();
    let port = free_port().to_string();
    let started = invoke(
        &runtime,
        &[
            "run",
            "--daemon",
            "--instance",
            "bench",
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
    thread::sleep(Duration::from_millis(200));

    let added = invoke(
        &runtime,
        &[
            "watch",
            "add",
            "--instance",
            "bench",
            "--source",
            "DUT",
            "--contains",
            "target event",
            "--ttl",
            "3s",
            "--json",
        ],
    );
    assert!(added.status.success());
    let watch_id = json(&added)["watch"]["id"].as_str().unwrap().to_string();

    append(&dut, "alpha");
    append(&host, "beta");
    append(&dut, "target event");
    append(&host, "delta");
    append(&dut, "epsilon");

    let waited = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            &watch_id,
            "--instance",
            "bench",
            "--timeout",
            "3s",
            "--json",
        ],
    );
    assert!(waited.status.success());
    let waited = json(&waited);
    let session_id = waited["match"]["session_id"].as_str().unwrap();
    let event_sequence = waited["match"]["sequence"].as_u64().unwrap();
    thread::sleep(Duration::from_millis(200));

    let page1 = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--limit",
            "2",
            "--json",
        ],
    );
    assert!(
        page1.status.success(),
        "{}",
        String::from_utf8_lossy(&page1.stderr)
    );
    let page1 = json(&page1);
    assert_eq!(
        page1["fields"],
        serde_json::json!(["time", "sequence", "source", "index", "message"])
    );
    assert_eq!(page1["records"].as_array().unwrap().len(), 2);
    assert_eq!(page1["records"][0][1], 1);
    assert_eq!(page1["records"][1][1], 2);
    assert!(page1["records"][0][0].as_str().unwrap().starts_with("+"));
    assert_eq!(page1["truncated"], true);
    assert_eq!(page1["next_cursor"], 2);

    let page2 = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--after",
            "2",
            "--limit",
            "2",
            "--json",
        ],
    );
    assert!(page2.status.success());
    let page2 = json(&page2);
    assert_eq!(
        page2["fields"],
        serde_json::json!(["time", "sequence", "source", "index", "message"])
    );
    assert_eq!(page2["records"][0][1], 3);
    assert_eq!(page2["records"][1][1], 4);

    let absolute = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--limit",
            "1",
            "--time",
            "absolute",
            "--json",
        ],
    );
    assert!(absolute.status.success());
    let absolute = json(&absolute);
    assert_eq!(absolute["fields"][0], "time");
    assert!(absolute["records"][0][0].as_str().unwrap().contains('T'));

    let text = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--limit",
            "1",
        ],
    );
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.starts_with("@session="), "{text}");
    assert!(text.contains("next=1 count=1 more=1"), "{text}");
    assert!(text.contains("\n+"), "{text}");
    assert!(text.contains("seq=1"), "{text}");
    assert!(text.contains("src="), "{text}");

    let source_only = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--source",
            "HOST",
            "--last",
            "10",
            "--json",
        ],
    );
    assert!(source_only.status.success());
    let source_only = json(&source_only);
    assert!(source_only["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record[2] == "HOST"));

    let around = invoke(
        &runtime,
        &[
            "sessions",
            "around",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--sequence",
            &event_sequence.to_string(),
            "--before",
            "1",
            "--after",
            "1",
            "--json",
        ],
    );
    assert!(around.status.success());
    let around = json(&around);
    assert_eq!(around["target"]["sequence"], event_sequence);
    assert!(around["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record[1] == event_sequence));
    assert!(around["records"].as_array().unwrap().len() <= 3);

    let invalid = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--after",
            "999999",
            "--json",
        ],
    );
    assert!(!invalid.status.success());
    assert!(invalid.stderr.is_empty());
    let invalid_json: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(invalid_json["error"]["code"], "CURSOR_INVALID");
    assert!(invalid_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("beyond the final sequence"));

    let rotated = invoke(
        &runtime,
        &[
            "sessions",
            "new",
            "--instance",
            "bench",
            "--title",
            "sequence reset",
            "--json",
        ],
    );
    assert!(rotated.status.success());
    let new_session = json(&rotated)["session_id"].as_str().unwrap().to_string();
    append(&dut, "new session first");
    let mut new_read = None;
    for _ in 0..50 {
        let output = invoke(
            &runtime,
            &[
                "sessions",
                "read",
                &new_session,
                "--dir",
                logs.to_str().unwrap(),
                "--limit",
                "1",
                "--json",
            ],
        );
        if output.status.success() {
            let value = json(&output);
            if !value["records"].as_array().unwrap().is_empty() {
                new_read = Some(value);
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    let new_read = new_read.expect("new session record was not persisted");
    assert_eq!(new_read["records"][0][1], 1);
    assert_eq!(new_read["records"][0][3], 0);

    let removed = invoke(
        &runtime,
        &[
            "watch",
            "remove",
            &watch_id,
            "--instance",
            "bench",
            "--json",
        ],
    );
    assert!(removed.status.success());
    let stopped = invoke(&runtime, &["stop", "--instance", "bench", "--json"]);
    assert!(stopped.status.success());
    fs::remove_dir_all(root).unwrap();
}
