#![cfg(target_os = "linux")]

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_embed-log"))
}

fn temp_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("embed-log-tx-{}-{nonce}", std::process::id()));
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

fn invoke_with_stdin(runtime: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(binary())
        .args(args)
        .env("EMBED_LOG_RUNTIME_DIR", runtime)
        .env_remove("EMBED_LOG_INSTANCE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn start_simulator(root: &Path) -> (Child, PathBuf, PathBuf) {
    let script = root.join("device.py");
    let path_file = root.join("uart-path");
    let capture = root.join("uart-rx.hex");
    fs::write(
        &script,
        r#"import os, pty, select, sys
path_file, capture_file = sys.argv[1:]
master, slave = pty.openpty()
with open(path_file, 'w') as f:
    f.write(os.ttyname(slave))
os.close(slave)
buffer = b''
while True:
    ready, _, _ = select.select([master], [], [], 0.2)
    if not ready:
        continue
    try:
        chunk = os.read(master, 1024)
    except OSError:
        continue
    if not chunk:
        continue
    with open(capture_file, 'a') as f:
        f.write(chunk.hex() + '\n')
    buffer += chunk
    while b'\r' in buffer or b'\n' in buffer:
        positions = [p for p in (buffer.find(b'\r'), buffer.find(b'\n')) if p >= 0]
        pos = min(positions)
        command, buffer = buffer[:pos], buffer[pos + 1:]
        if command == b'status':
            os.write(master, b'noise before\r\nboot complete\r\n')
        elif command == b'PING':
            os.write(master, b'raw ok\r\n')
        elif command == b'nomatch':
            os.write(master, b'still waiting\r\n')
"#,
    )
    .unwrap();
    let child = Command::new("python3")
        .arg(&script)
        .arg(&path_file)
        .arg(&capture)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path_file.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(path_file.exists(), "PTY simulator did not publish its path");
    (child, path_file, capture)
}

#[test]
fn tx_line_raw_expect_timeout_and_persistence() {
    let root = temp_root();
    let runtime = root.join("runtime");
    let logs = root.join("logs");
    let (mut simulator, path_file, capture) = start_simulator(&root);
    let uart_path = fs::read_to_string(path_file).unwrap();
    let config = root.join("config.yml");
    let trace = root.join("trace.log");
    fs::write(&trace, "").unwrap();
    fs::write(
        &config,
        format!(
            "version: 2\nlogs:\n  dir: {}\nsources:\n  DUT_UART:\n    type: uart\n    path: {}\n    baud: 115200\n  TRACE:\n    type: file\n    path: {}\n",
            logs.display(),
            uart_path.trim(),
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

    let implicit = invoke(
        &runtime,
        &["tx", "--source", "DUT_UART", "--line", "status", "--json"],
    );
    assert!(!implicit.status.success());
    assert!(String::from_utf8_lossy(&implicit.stderr).contains("explicit target"));
    let non_writable = invoke(
        &runtime,
        &[
            "tx",
            "--instance",
            "bench",
            "--source",
            "TRACE",
            "--line",
            "status",
        ],
    );
    assert!(!non_writable.status.success());
    assert!(String::from_utf8_lossy(&non_writable.stderr).contains("not writable"));

    let expected = invoke(
        &runtime,
        &[
            "tx",
            "--instance",
            "bench",
            "--source",
            "DUT_UART",
            "--line",
            "status",
            "--expect",
            "boot complete",
            "--timeout",
            "3s",
            "--context",
            "3",
            "--json",
        ],
    );
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    let expected_json: serde_json::Value = serde_json::from_slice(&expected.stdout).unwrap();
    assert_eq!(expected_json["ok"], true);
    assert_eq!(expected_json["bytes_written"], 7);
    assert_eq!(expected_json["expectation"]["matched"], true);
    assert_eq!(
        expected_json["expectation"]["entry"]["message"],
        "boot complete"
    );
    assert!(expected_json["expectation"]["entry"]["sequence"]
        .as_u64()
        .is_some());
    assert_eq!(
        expected_json["next_cursor"],
        expected_json["expectation"]["entry"]["sequence"]
    );
    assert!(expected_json["context"].as_array().unwrap().len() <= 3);

    let raw = invoke(
        &runtime,
        &[
            "tx",
            "--instance",
            "bench",
            "--source",
            "DUT_UART",
            "--raw",
            "PING\n",
            "--expect-regex",
            "^raw ok$",
            "--timeout",
            "3s",
            "--json",
        ],
    );
    assert!(
        raw.status.success(),
        "{}",
        String::from_utf8_lossy(&raw.stderr)
    );
    let raw_json: serde_json::Value = serde_json::from_slice(&raw.stdout).unwrap();
    assert_eq!(raw_json["bytes_written"], 5);
    assert_eq!(raw_json["expectation"]["entry"]["message"], "raw ok");

    let timed_out = invoke(
        &runtime,
        &[
            "tx",
            "--instance",
            "bench",
            "--source",
            "DUT_UART",
            "--line",
            "nomatch",
            "--expect",
            "never arrives",
            "--timeout",
            "300ms",
            "--context",
            "2",
            "--json",
        ],
    );
    assert!(!timed_out.status.success());
    let timeout_json: serde_json::Value = serde_json::from_slice(&timed_out.stdout).unwrap();
    assert_eq!(timeout_json["ok"], false);
    assert_eq!(timeout_json["code"], "EXPECT_TIMEOUT");
    assert_eq!(timeout_json["bytes_written"], 8);
    assert_eq!(timeout_json["expectation"]["matched"], false);
    assert!(timeout_json["context"].as_array().unwrap().len() <= 2);

    let tx_file = root.join("tx.bin");
    fs::write(&tx_file, b"FILE\0").unwrap();
    let file_result = invoke(
        &runtime,
        &[
            "tx",
            "--instance",
            "bench",
            "--source",
            "DUT_UART",
            "--file",
            tx_file.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(file_result.status.success());
    let file_json: serde_json::Value = serde_json::from_slice(&file_result.stdout).unwrap();
    assert_eq!(file_json["bytes_written"], 5);
    assert!(file_json["expectation"].is_null());
    let stdin_result = invoke_with_stdin(
        &runtime,
        &[
            "tx",
            "--instance",
            "bench",
            "--source",
            "DUT_UART",
            "--stdin",
            "--json",
        ],
        b"STDIN\0",
    );
    assert!(stdin_result.status.success());
    let stdin_json: serde_json::Value = serde_json::from_slice(&stdin_result.stdout).unwrap();
    assert_eq!(stdin_json["bytes_written"], 6);

    thread::sleep(Duration::from_millis(200));
    let wire_hex = fs::read_to_string(capture).unwrap();
    assert!(wire_hex.contains("7374617475730d"), "{wire_hex}");
    assert!(wire_hex.contains("50494e470a"), "{wire_hex}");
    let compact_wire_hex = wire_hex.lines().collect::<String>();
    assert!(compact_wire_hex.contains("46494c4500"), "{wire_hex}");
    assert!(compact_wire_hex.contains("535444494e00"), "{wire_hex}");

    let session_id = expected_json["session_id"].as_str().unwrap();
    let combined = fs::read_to_string(logs.join(session_id).join("combined.jsonl")).unwrap();
    assert!(combined.contains("\"origin\":\"cli\""), "{combined}");
    assert!(combined.contains("\"type\":\"tx\""), "{combined}");
    assert!(combined.contains("boot complete"), "{combined}");
    assert!(combined.contains("raw ok"), "{combined}");

    let stopped = invoke(&runtime, &["stop", "--instance", "bench", "--json"]);
    assert!(stopped.status.success());
    let _ = simulator.kill();
    let _ = simulator.wait();
    fs::remove_dir_all(root).unwrap();
}
