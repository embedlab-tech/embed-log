#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_embed-log")
}

fn temp_root(name: &str) -> PathBuf {
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "embed-log-daemon-e2e-{name}-{}-{counter}",
        std::process::id()
    ));
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

fn write_config(root: &Path, name: &str, start_port: u16) -> PathBuf {
    let path = root.join(format!("{name}.yml"));
    let logs = root.join(format!("{name}-logs"));
    let input = root.join(format!("{name}.log"));
    fs::write(
        &path,
        format!(
            "version: 2\nserver:\n  listen: 127.0.0.1:{start_port}\nlogs:\n  dir: {}\nsources:\n  TEST:\n    type: file\n    path: {}\n",
            logs.display(),
            input.display()
        ),
    )
    .unwrap();
    path
}

fn invoke(runtime: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .env("EMBED_LOG_RUNTIME_DIR", runtime)
        .output()
        .unwrap()
}

fn start(runtime: &Path, config: &Path, instance: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "run",
        "--daemon",
        "--instance",
        instance,
        "--config",
        config.to_str().unwrap(),
        "--frontend-dir",
        "/definitely/not/a/frontend",
        "--json",
    ];
    args.extend_from_slice(extra);
    invoke(runtime, &args)
}

fn stop(runtime: &Path, instance: &str) {
    let _ = invoke(runtime, &["stop", "--instance", instance, "--json"]);
}

struct DaemonGuard {
    runtime: PathBuf,
    instances: Vec<&'static str>,
}

impl DaemonGuard {
    fn new(runtime: &Path, instance: &'static str) -> Self {
        Self {
            runtime: runtime.to_path_buf(),
            instances: vec![instance],
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        for instance in &self.instances {
            stop(&self.runtime, instance);
        }
    }
}

#[test]
fn daemon_start_status_duplicate_and_graceful_stop() {
    let root = temp_root("lifecycle");
    let runtime = root.join("runtime");
    let config = write_config(&root, "daemon", free_port());
    let port = free_port().to_string();
    fs::create_dir_all(&runtime).unwrap();
    fs::write(
        runtime.join("bench-a.json"),
        serde_json::to_vec(&serde_json::json!({
            "instance": "bench-a",
            "pid": u32::MAX,
            "endpoint": "http://127.0.0.1:1",
            "config_path": config,
            "logs_dir": root.join("stale-logs"),
            "diagnostic_log": root.join("stale.log"),
            "executable": binary(),
            "started_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();

    let started = start(&runtime, &config, "bench-a", &["--port", &port]);
    assert!(
        started.status.success(),
        "startup failed: {}",
        String::from_utf8_lossy(&started.stderr)
    );
    let _guard = DaemonGuard::new(&runtime, "bench-a");
    let started_json: serde_json::Value = serde_json::from_slice(&started.stdout).unwrap();
    assert_eq!(started_json["ok"], true);
    assert_eq!(started_json["instance"]["instance"], "bench-a");
    assert_eq!(
        started_json["instance"]["endpoint"],
        format!("http://127.0.0.1:{port}")
    );
    assert!(runtime.join("bench-a.json").exists());

    let status = invoke(&runtime, &["status", "--instance", "bench-a", "--json"]);
    assert!(status.status.success());
    let status_json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_json["backend"]["ok"], true);
    assert!(status_json["backend"]["session_id"].is_string());
    assert_eq!(status_json["backend"]["sources"]["TEST"]["type"], "file");
    let original_session = status_json["backend"]["session_id"].as_str().unwrap();
    let daemon_pid = started_json["instance"]["pid"].as_u64().unwrap();

    let implicit_rotation = invoke(
        &runtime,
        &["sessions", "new", "--title", "implicit mutation", "--json"],
    );
    assert!(!implicit_rotation.status.success());
    assert!(String::from_utf8_lossy(&implicit_rotation.stderr).contains("explicit target"));

    let rotated = invoke(
        &runtime,
        &[
            "sessions",
            "new",
            "--instance",
            "bench-a",
            "--title",
            "Reconnect Attempt #3",
            "--json",
        ],
    );
    assert!(
        rotated.status.success(),
        "rotation failed: {}",
        String::from_utf8_lossy(&rotated.stderr)
    );
    let rotated_json: serde_json::Value = serde_json::from_slice(&rotated.stdout).unwrap();
    let new_session = rotated_json["session_id"].as_str().unwrap();
    assert_ne!(new_session, original_session);
    assert!(new_session.contains("_reconnect-attempt-3"));
    assert_eq!(rotated_json["title"], "Reconnect Attempt #3");
    assert_eq!(rotated_json["session"]["title"], "Reconnect Attempt #3");
    let manifest_path = root
        .join("daemon-logs")
        .join(new_session)
        .join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["title"], "Reconnect Attempt #3");

    let input_path = root.join("daemon.log");
    writeln!(
        OpenOptions::new().append(true).open(&input_path).unwrap(),
        "after titled rotation"
    )
    .unwrap();
    let new_log = PathBuf::from(manifest["source_files"]["TEST"].as_str().unwrap());
    for _ in 0..50 {
        if fs::read_to_string(&new_log).is_ok_and(|text| text.contains("after titled rotation")) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        fs::read_to_string(&new_log)
            .unwrap()
            .contains("after titled rotation"),
        "existing source task did not route data into the titled session"
    );

    let auto_selected = invoke(&runtime, &["status", "--json"]);
    assert!(auto_selected.status.success());
    let auto_json: serde_json::Value = serde_json::from_slice(&auto_selected.stdout).unwrap();
    assert_eq!(auto_json["instance"]["pid"], daemon_pid);
    assert_eq!(auto_json["backend"]["session_id"], new_session);
    let env_selected = Command::new(binary())
        .args(["status", "--json"])
        .env("EMBED_LOG_RUNTIME_DIR", &runtime)
        .env("EMBED_LOG_INSTANCE", "bench-a")
        .output()
        .unwrap();
    assert!(env_selected.status.success());
    let endpoint = format!("http://127.0.0.1:{port}");
    let direct = invoke(&runtime, &["status", "--url", &endpoint, "--json"]);
    assert!(direct.status.success());
    let direct_json: serde_json::Value = serde_json::from_slice(&direct.stdout).unwrap();
    assert!(direct_json["instance"].is_null());
    assert_eq!(direct_json["backend"]["ok"], true);

    let duplicate = start(&runtime, &config, "bench-a", &["--port", &port]);
    assert!(duplicate.status.success());
    let reused_json: serde_json::Value = serde_json::from_slice(&duplicate.stdout).unwrap();
    assert_eq!(reused_json["reused"], true);
    assert_eq!(reused_json["instance"]["pid"], daemon_pid);

    fs::write(
        &config,
        format!("{}\n# changed\n", fs::read_to_string(&config).unwrap()),
    )
    .unwrap();
    let changed_config = start(&runtime, &config, "bench-a", &["--port", &port]);
    assert!(!changed_config.status.success());
    assert!(String::from_utf8_lossy(&changed_config.stderr).contains("already running"));

    let implicit_stop = invoke(&runtime, &["stop", "--json"]);
    assert!(!implicit_stop.status.success());
    assert!(String::from_utf8_lossy(&implicit_stop.stderr).contains("explicit target"));

    let stopped = invoke(&runtime, &["stop", "--instance", "bench-a", "--json"]);
    assert!(
        stopped.status.success(),
        "stop failed: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(!runtime.join("bench-a.json").exists());
    let html_count = fs::read_dir(root.join("daemon-logs"))
        .unwrap()
        .flat_map(|entry| fs::read_dir(entry.unwrap().path()).unwrap())
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "html")
        })
        .count();
    assert_eq!(html_count, 0, "daemon shutdown must skip automatic HTML");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn occupied_port_is_never_reassigned_and_foreground_bind_failure_exits() {
    let root = temp_root("occupied");
    let runtime = root.join("runtime");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let config = write_config(&root, "occupied", free_port());
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("broken.json"), "not json").unwrap();
    let malformed = invoke(&runtime, &["status", "--json"]);
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("parse daemon registry"));
    fs::remove_file(runtime.join("broken.json")).unwrap();

    let daemon = start(&runtime, &config, "bench-a", &["--port", &port]);
    assert!(!daemon.status.success());
    let daemon_error = String::from_utf8_lossy(&daemon.stderr);
    assert!(
        daemon_error.contains("unregistered process"),
        "{daemon_error}"
    );
    assert!(!runtime.join("bench-a.json").exists());

    let foreground = invoke(
        &runtime,
        &[
            "run",
            "--config",
            config.to_str().unwrap(),
            "--port",
            &port,
            "--no-open-browser",
            "--frontend-dir",
            "/definitely/not/a/frontend",
        ],
    );
    assert!(!foreground.status.success());
    let foreground_error = String::from_utf8_lossy(&foreground.stderr);
    assert!(
        foreground_error.contains("HTTP/WebSocket server failed"),
        "{foreground_error}"
    );

    drop(listener);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn multiple_instances_require_explicit_ports_and_selection() {
    let root = temp_root("multiple");
    let runtime = root.join("runtime");
    let port_a = free_port();
    let port_b = free_port();
    let config_a = write_config(&root, "a", port_a);
    let config_b = write_config(&root, "b", port_b);

    let missing_port = start(&runtime, &config_a, "missing-port", &[]);
    assert!(!missing_port.status.success());
    assert!(String::from_utf8_lossy(&missing_port.stderr).contains("--port"));

    let port_a_text = port_a.to_string();
    let first = start(&runtime, &config_a, "bench-a", &["--port", &port_a_text]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let mut guard = DaemonGuard::new(&runtime, "bench-a");
    let same_endpoint = start(&runtime, &config_b, "bench-c", &["--port", &port_a_text]);
    assert!(!same_endpoint.status.success());
    assert!(
        String::from_utf8_lossy(&same_endpoint.stderr).contains("owned by instance \"bench-a\"")
    );

    let port_b_text = port_b.to_string();
    let second = start(&runtime, &config_b, "bench-b", &["--port", &port_b_text]);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    guard.instances.push("bench-b");

    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let second_json: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(
        first_json["instance"]["endpoint"],
        second_json["instance"]["endpoint"]
    );

    let ambiguous = invoke(&runtime, &["status", "--json"]);
    assert!(!ambiguous.status.success());
    let error = String::from_utf8_lossy(&ambiguous.stderr);
    assert!(error.contains("multiple Embed-log instances"), "{error}");
    assert!(error.contains("bench-a"), "{error}");
    assert!(error.contains("bench-b"), "{error}");
    assert!(error.contains("--instance"), "{error}");

    stop(&runtime, "bench-a");
    stop(&runtime, "bench-b");
    fs::remove_dir_all(root).unwrap();
}
