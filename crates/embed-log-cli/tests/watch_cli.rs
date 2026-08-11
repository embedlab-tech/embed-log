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

fn temp_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("embed-log-watch-{}-{nonce}", std::process::id()));
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

fn append(path: &Path, line: &str) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
    file.flush().unwrap();
}

fn add_watch(runtime: &Path, matcher_flag: &str, pattern: &str, ttl: &str) -> serde_json::Value {
    let output = invoke(
        runtime,
        &[
            "watch",
            "add",
            "--instance",
            "bench",
            "--source",
            "TRACE",
            matcher_flag,
            pattern,
            "--ttl",
            ttl,
            "--json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn watches_retain_matches_expire_timeout_and_remove() {
    let root = temp_root();
    let runtime = root.join("runtime");
    let logs = root.join("logs");
    let trace = root.join("trace.log");
    fs::write(&trace, "existing line is not replayed\n").unwrap();
    let config = root.join("config.yml");
    fs::write(
        &config,
        format!(
            "version: 2\nlogs:\n  dir: {}\nsources:\n  TRACE:\n    type: file\n    path: {}\n",
            logs.display(),
            trace.display()
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

    let implicit = invoke(
        &runtime,
        &["watch", "add", "--source", "TRACE", "--contains", "ready"],
    );
    assert!(!implicit.status.success());
    assert!(String::from_utf8_lossy(&implicit.stderr).contains("explicit target"));

    let added = add_watch(&runtime, "--contains", "device ready", "3s");
    let watch_id = added["watch"]["id"].as_str().unwrap().to_string();
    assert_eq!(added["watch"]["status"], "active");
    assert_eq!(added["watch"]["once"], true);

    // Match before `watch wait` starts: server-side retained state must preserve it.
    append(&trace, "noise");
    append(&trace, "device ready now");
    thread::sleep(Duration::from_millis(200));
    let waited = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            &watch_id,
            "--instance",
            "bench",
            "--timeout",
            "2s",
            "--json",
        ],
    );
    assert!(
        waited.status.success(),
        "{}",
        String::from_utf8_lossy(&waited.stderr)
    );
    let waited_json: serde_json::Value = serde_json::from_slice(&waited.stdout).unwrap();
    assert_eq!(waited_json["status"], "matched");
    assert_eq!(waited_json["match"]["message"], "device ready now");
    assert_eq!(waited_json["match"]["source_id"], "TRACE");
    assert!(waited_json["match"]["line_idx"].as_u64().is_some());
    assert!(waited_json["match"]["timestamp_iso"].as_str().is_some());
    assert!(waited_json["match"]["sequence"].as_u64().is_some());
    assert_eq!(waited_json["next_cursor"], waited_json["match"]["sequence"]);
    assert_eq!(waited_json["match"]["captures"][0], "device ready");

    // Waiting again returns the same retained match instead of losing it.
    let retained = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            &watch_id,
            "--instance",
            "bench",
            "--timeout",
            "100ms",
            "--json",
        ],
    );
    assert!(retained.status.success());
    let retained_json: serde_json::Value = serde_json::from_slice(&retained.stdout).unwrap();
    assert_eq!(retained_json["match"], waited_json["match"]);

    let regex_added = add_watch(&runtime, "--regex", r"value=(\d+)", "3s");
    let regex_id = regex_added["watch"]["id"].as_str().unwrap();
    append(&trace, "sensor value=42");
    let regex_wait = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            regex_id,
            "--instance",
            "bench",
            "--timeout",
            "2s",
            "--json",
        ],
    );
    assert!(regex_wait.status.success());
    let regex_json: serde_json::Value = serde_json::from_slice(&regex_wait.stdout).unwrap();
    assert_eq!(regex_json["match"]["captures"][1], "42");

    let expiring = add_watch(&runtime, "--contains", "too late", "150ms");
    let expiring_id = expiring["watch"]["id"].as_str().unwrap();
    thread::sleep(Duration::from_millis(250));
    append(&trace, "too late");
    let expired = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            expiring_id,
            "--instance",
            "bench",
            "--timeout",
            "1s",
            "--json",
        ],
    );
    assert!(!expired.status.success());
    let expired_json: serde_json::Value = serde_json::from_slice(&expired.stdout).unwrap();
    assert_eq!(expired_json["error"]["code"], "WATCH_EXPIRED");
    assert_eq!(
        expired_json["error"]["details"]["watch"]["status"],
        "expired"
    );
    assert!(expired_json["error"]["details"]["watch"]["match"].is_null());
    assert!(expired.stderr.is_empty());

    let pending = add_watch(&runtime, "--contains", "not emitted", "3s");
    let pending_id = pending["watch"]["id"].as_str().unwrap();
    let timed_out = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            pending_id,
            "--instance",
            "bench",
            "--timeout",
            "150ms",
            "--json",
        ],
    );
    assert!(!timed_out.status.success());
    let timed_out_json: serde_json::Value = serde_json::from_slice(&timed_out.stdout).unwrap();
    assert_eq!(timed_out_json["error"]["code"], "WATCH_WAIT_TIMEOUT");
    assert_eq!(
        timed_out_json["error"]["details"]["watch"]["status"],
        "active"
    );

    for id in [&watch_id, regex_id, expiring_id, pending_id] {
        let removed = invoke(
            &runtime,
            &["watch", "remove", id, "--instance", "bench", "--json"],
        );
        assert!(removed.status.success());
    }

    let missing = invoke(
        &runtime,
        &[
            "watch",
            "wait",
            &watch_id,
            "--instance",
            "bench",
            "--timeout",
            "100ms",
            "--json",
        ],
    );
    assert!(!missing.status.success());
    let missing_json: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(missing_json["error"]["code"], "WATCH_NOT_FOUND");

    let session_id = waited_json["match"]["session_id"].as_str().unwrap();
    let session_dir = logs.join(session_id);
    let combined = fs::read_to_string(session_dir.join("combined.jsonl")).unwrap();
    assert!(combined.contains("device ready now"), "{combined}");
    assert!(!session_dir.join("events.jsonl").exists());

    let stopped = invoke(&runtime, &["stop", "--instance", "bench", "--json"]);
    assert!(stopped.status.success());
    fs::remove_dir_all(root).unwrap();
}
