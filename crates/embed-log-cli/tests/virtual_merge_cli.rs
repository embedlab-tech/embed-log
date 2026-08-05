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
fn merge_is_virtual_and_cli_expands_it_to_original_records() {
    let root = std::env::temp_dir().join(format!(
        "embed-log-virtual-merge-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let runtime = root.join("runtime");
    let logs = root.join("logs");
    let tx_log = root.join("tx.log");
    let rx_log = root.join("rx.log");
    fs::create_dir_all(&root).unwrap();
    fs::write(&tx_log, "").unwrap();
    fs::write(&rx_log, "").unwrap();
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
            "version: 2\nserver:\n  listen: 127.0.0.1:{port}\nlogs:\n  dir: {}\nsources:\n  MCU_TX:\n    type: file\n    path: {}\n  MCU_RX:\n    type: file\n    path: {}\nmerges:\n  - name: MCU_LINK\n    label: MCU Link\n    of: [MCU_TX, MCU_RX]\nui:\n  tabs:\n    - title: Link\n      sources: [MCU_LINK]\n",
            logs.display(),
            tx_log.display(),
            rx_log.display(),
        ),
    )
    .unwrap();

    let started = invoke(
        &runtime,
        &[
            "run",
            "--daemon",
            "--instance",
            "merge",
            "--config",
            config.to_str().unwrap(),
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
        OpenOptions::new().append(true).open(&tx_log).unwrap(),
        "sent"
    )
    .unwrap();
    writeln!(
        OpenOptions::new().append(true).open(&rx_log).unwrap(),
        "received"
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
                "--source",
                "MCU_LINK",
                "--limit",
                "10",
                "--format",
                "full-json",
            ],
        );
        if read.status.success() {
            records = serde_json::from_slice(&read.stdout).unwrap();
            if records["records"]
                .as_array()
                .is_some_and(|rows| rows.len() == 2)
            {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }

    let rows = records["records"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{records}");
    let source_ids = rows
        .iter()
        .map(|record| record["source_id"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        source_ids,
        std::collections::HashSet::from(["MCU_TX", "MCU_RX"])
    );
    assert!(rows.iter().all(|record| record["source_kind"] != "merge"));

    let session_dir = logs.join(session_id);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(session_dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["merges"][0]["name"], "MCU_LINK");
    assert_eq!(
        manifest["merges"][0]["of"],
        serde_json::json!(["MCU_TX", "MCU_RX"])
    );
    assert!(manifest["source_files"].get("MCU_LINK").is_none());
    assert!(fs::read_dir(&session_dir).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("mcu-link")));

    let combined = fs::read_to_string(session_dir.join("combined.jsonl")).unwrap();
    assert_eq!(combined.lines().count(), 2, "{combined}");
    assert!(!combined.contains("\"source_kind\":\"merge\""));

    let active_export = invoke(&runtime, &["export", "--instance", "merge", "--json"]);
    assert!(
        active_export.status.success(),
        "{}",
        String::from_utf8_lossy(&active_export.stderr)
    );
    let active_result: serde_json::Value = serde_json::from_slice(&active_export.stdout).unwrap();
    assert_eq!(active_result["export"]["html_status"], "ready");
    assert_eq!(
        active_result["export"]["download_url"],
        format!("/sessions/{session_id}/session.html")
    );
    let canonical_html_path = session_dir.join("session.html");
    let canonical_html = fs::read(&canonical_html_path).unwrap();

    // Post-factum CLI export uses the same Rust renderer and is byte-identical
    // for the same combined.jsonl/manifest/markers snapshot.
    let copy_path = root.join("session-copy.html");
    let recorded_export = invoke(
        &runtime,
        &[
            "sessions",
            "export",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--format",
            "html",
            "--output",
            copy_path.to_str().unwrap(),
        ],
    );
    assert!(
        recorded_export.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded_export.stderr)
    );
    let recorded_html = fs::read(&copy_path).unwrap();
    assert_eq!(recorded_html, canonical_html);

    let html = String::from_utf8(canonical_html).unwrap();
    assert!(html.contains("data-pane=\"MCU_LINK\""));
    assert!(html.contains("MCU_TX: sent") || html.contains("MCU TX: sent"));
    assert!(html.contains("sourceId"));
    assert!(html.contains("sequence"));

    let stopped = invoke(&runtime, &["stop", "--instance", "merge", "--json"]);
    assert!(stopped.status.success());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn legacy_materialized_merge_records_are_hidden_unless_requested() {
    let root = std::env::temp_dir().join(format!(
        "embed-log-legacy-merge-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let runtime = root.join("runtime");
    let logs = root.join("logs");
    let session_id = "legacy";
    let session_dir = logs.join(session_id);
    fs::create_dir_all(&session_dir).unwrap();
    let combined_path = session_dir.join("combined.jsonl");
    fs::write(
        &combined_path,
        concat!(
            "{\"sequence\":1,\"source_id\":\"MCU_TX\",\"source_kind\":\"file\",\"message\":\"sent\"}\n",
            "{\"sequence\":2,\"source_id\":\"MCU_LINK\",\"source_kind\":\"merge\",\"message\":\"MCU TX: sent\"}\n",
            "{\"sequence\":3,\"source_id\":\"MCU_RX\",\"source_kind\":\"file\",\"message\":\"received\"}\n",
        ),
    )
    .unwrap();
    fs::write(
        session_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "session_id": session_id,
            "session_dir": session_dir.display().to_string(),
            "combined_file": combined_path.display().to_string(),
            "source_files": {},
            "pane_kinds": {"MCU_TX":"file","MCU_RX":"file","MCU_LINK":"merge"},
            "merges": [{"name":"MCU_LINK","label":"MCU Link","of":["MCU_TX","MCU_RX"]}],
        }))
        .unwrap(),
    )
    .unwrap();

    let read = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--limit",
            "10",
            "--format",
            "full-json",
        ],
    );
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(output["records"].as_array().unwrap().len(), 2);
    assert_eq!(output["next_cursor"], 3);

    let compatible = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--limit",
            "10",
            "--format",
            "full-json",
            "--include-materialized-merges",
        ],
    );
    assert!(compatible.status.success());
    let output: serde_json::Value = serde_json::from_slice(&compatible.stdout).unwrap();
    assert_eq!(output["records"].as_array().unwrap().len(), 3);

    let virtual_read = invoke(
        &runtime,
        &[
            "sessions",
            "read",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
            "--source",
            "MCU_LINK",
            "--limit",
            "10",
            "--format",
            "full-json",
        ],
    );
    assert!(virtual_read.status.success());
    let output: serde_json::Value = serde_json::from_slice(&virtual_read.stdout).unwrap();
    assert_eq!(output["records"].as_array().unwrap().len(), 2);
    assert!(output["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record["source_id"] != "MCU_LINK"));

    let combined = invoke(
        &runtime,
        &[
            "sessions",
            "combined",
            session_id,
            "--dir",
            logs.to_str().unwrap(),
        ],
    );
    assert!(combined.status.success());
    assert_eq!(
        String::from_utf8(combined.stdout).unwrap().lines().count(),
        2
    );

    fs::remove_dir_all(root).unwrap();
}
