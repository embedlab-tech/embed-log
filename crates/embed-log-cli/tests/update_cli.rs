#![cfg(not(windows))]

use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use flate2::{write::GzEncoder, Compression};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tar::Builder;
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_embed-log")
}

fn target() -> String {
    let output = Command::new(binary())
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["target"]
        .as_str()
        .unwrap()
        .to_string()
}

fn archive(source: &Path, destination: &Path) -> String {
    let file = fs::File::create(destination).unwrap();
    // The fixture only needs the tar.gz wire format; avoid spending test time
    // compressing the debug CLI binary and its embedded frontend assets.
    let encoder = GzEncoder::new(file, Compression::none());
    let mut tar = Builder::new(encoder);
    tar.append_path_with_name(source, "embed-log").unwrap();
    tar.finish().unwrap();
    let encoder = tar.into_inner().unwrap();
    encoder.finish().unwrap();
    let bytes = fs::read(destination).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}

fn serve(
    release: Vec<u8>,
    archive: Vec<u8>,
    expected_requests: usize,
) -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut requests = 0;
        while requests < expected_requests && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("test server failed: {error}"),
            };
            // Accepted sockets inherit the listener's non-blocking mode on
            // macOS. Restore blocking mode before reading the complete HTTP
            // request, rather than racing the client for its first bytes.
            stream.set_nonblocking(false).unwrap();
            let mut request = [0_u8; 1024];
            let count = stream.read(&mut request).unwrap();
            let path = String::from_utf8_lossy(&request[..count]);
            let body = if path.starts_with("GET /release.json ") {
                &release
            } else if path.starts_with("GET /embed-log-test.tar.gz ") {
                &archive
            } else {
                panic!("unexpected update request: {path}");
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
            requests += 1;
        }
        requests
    });
    (url, server)
}

fn managed_copy(temp: &TempDir, target: &str) -> PathBuf {
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    let installed = install_dir.join("embed-log");
    fs::copy(binary(), &installed).unwrap();
    let marker = serde_json::json!({
        "repository": "embedlab-tech/embed-log",
        "target": target,
        "executable": installed,
    });
    fs::write(
        install_dir.join(".embed-log-install"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    installed
}

#[test]
fn update_replaces_an_installer_managed_unix_binary_after_checksum_verification() {
    let temp = TempDir::new().unwrap();
    let target = target();
    let archive_path = temp.path().join("embed-log-test.tar.gz");
    let checksum = archive(Path::new(binary()), &archive_path);
    let release = serde_json::json!({
        "version": "99.0.0",
        "assets": { target.clone(): { "archive": "embed-log-test.tar.gz", "sha256": checksum } },
    });
    let installed = managed_copy(&temp, &target);
    let (base_url, server) = serve(
        serde_json::to_vec(&release).unwrap(),
        fs::read(archive_path).unwrap(),
        2,
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("EMBED_LOG_UPDATE_BASE_URL", base_url)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("updated to v99.0.0"));
    assert!(Command::new(installed)
        .arg("version")
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(
        server.join().unwrap(),
        2,
        "update did not fetch both fixture assets"
    );
}

#[test]
fn update_rejects_a_bad_checksum_without_replacing_the_binary() {
    let temp = TempDir::new().unwrap();
    let target = target();
    let archive_path = temp.path().join("embed-log-test.tar.gz");
    archive(Path::new(binary()), &archive_path);
    let release = serde_json::json!({
        "version": "99.0.0",
        "assets": { target.clone(): { "archive": "embed-log-test.tar.gz", "sha256": "0".repeat(64) } },
    });
    let installed = managed_copy(&temp, &target);
    let before = fs::read(&installed).unwrap();
    let (base_url, server) = serve(
        serde_json::to_vec(&release).unwrap(),
        fs::read(archive_path).unwrap(),
        2,
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("EMBED_LOG_UPDATE_BASE_URL", base_url)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum"));
    assert_eq!(fs::read(installed).unwrap(), before);
    assert_eq!(
        server.join().unwrap(),
        2,
        "update did not fetch both fixture assets"
    );
}

#[test]
fn update_refuses_an_unmanaged_binary_without_downloading_an_archive() {
    let temp = TempDir::new().unwrap();
    let target = target();
    let install_dir = temp.path().join("manual-bin");
    fs::create_dir_all(&install_dir).unwrap();
    let installed = install_dir.join("embed-log");
    fs::copy(binary(), &installed).unwrap();
    let before = fs::read(&installed).unwrap();
    let release = serde_json::json!({
        "version": "99.0.0",
        "assets": { target: { "archive": "embed-log-test.tar.gz", "sha256": "a".repeat(64) } },
    });
    let (base_url, server) = serve(serde_json::to_vec(&release).unwrap(), Vec::new(), 1);

    let output = Command::new(&installed)
        .arg("update")
        .env("EMBED_LOG_UPDATE_BASE_URL", base_url)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not managed"));
    assert_eq!(fs::read(installed).unwrap(), before);
    assert_eq!(
        server.join().unwrap(),
        1,
        "unmanaged update fetched an archive"
    );
}

#[test]
fn explicit_update_check_reports_an_offline_connection_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let output = Command::new(binary())
        .args(["update", "--check"])
        .env("EMBED_LOG_UPDATE_BASE_URL", format!("http://{address}"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("error sending request"));
}

/// Follow-up step 3: the background hint must not delay or blacken `run` even
/// when the update endpoint is completely unreachable. The hint runs on a
/// detached thread, so the server must come up immediately and the captured
/// stderr must stay free of any update/network noise.
#[test]
fn background_update_hint_is_quiet_when_the_update_endpoint_is_unreachable() {
    let temp = TempDir::new().unwrap();
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config = hint_config(&temp, port);

    // A dropped listener address guarantees immediate connection refusal for
    // any background fetch attempt.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_address = dead.local_addr().unwrap();
    drop(dead);

    let stdout = temp.path().join("run.out");
    let stderr = temp.path().join("run.err");
    let started = Instant::now();
    let mut child = spawn_run(
        &config,
        port,
        &format!("http://{dead_address}"),
        &stdout,
        &stderr,
        false,
    );
    assert!(
        wait_until_ready(port, started + Duration::from_secs(15)),
        "embed-log run did not start within the deadline"
    );
    // Leave the detached hint ample time to attempt (and fail at) its fetch.
    thread::sleep(Duration::from_secs(1));
    child.kill().ok();
    let _ = child.wait();

    let out = fs::read_to_string(&stdout).unwrap();
    assert!(out.contains("embed-log v"), "run banner missing: {out}");
    let err = fs::read_to_string(&stderr).unwrap();
    let lowered = err.to_lowercase();
    assert!(
        !lowered.contains("update"),
        "update hint leaked noise to stderr: {err}"
    );
    assert!(
        !lowered.contains("network"),
        "update hint leaked noise to stderr: {err}"
    );
    assert!(
        !lowered.contains("error sending request"),
        "update hint leaked fetch errors to stderr: {err}"
    );
}

/// Follow-up step 3: EMBED_LOG_NO_UPDATE_CHECK=1 must suppress the background
/// hint entirely — no request may reach the update endpoint while a normal
/// `run` stays up and healthy.
#[test]
fn background_update_hint_can_be_disabled_entirely() {
    let temp = TempDir::new().unwrap();
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config = hint_config(&temp, port);

    // Live update endpoint that observes any background request.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let requests_observer = Arc::clone(&requests);
    let done_observer = Arc::clone(&done);
    let observer = thread::spawn(move || {
        while done_observer.load(Ordering::SeqCst) == 0 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    requests_observer.fetch_add(1, Ordering::SeqCst);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let stdout = temp.path().join("run.out");
    let stderr = temp.path().join("run.err");
    let mut child = spawn_run(&config, port, &base_url, &stdout, &stderr, true);

    let started = Instant::now();
    assert!(
        wait_until_ready(port, started + Duration::from_secs(15)),
        "embed-log run did not start within the deadline"
    );
    thread::sleep(Duration::from_millis(300));
    child.kill().ok();
    let _ = child.wait();
    done.store(1, Ordering::SeqCst);

    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "EMBED_LOG_NO_UPDATE_CHECK did not suppress the background update check"
    );
    assert!(fs::read_to_string(&stderr).unwrap().is_empty());
    let _ = observer.join();
}

/// Writes a minimal v2 config exposing one quiet file source.
fn hint_config(temp: &TempDir, port: u16) -> PathBuf {
    let root = temp.path();
    let logs = root.join("hint-logs");
    let input = root.join("hint-input.log");
    fs::create_dir_all(&logs).unwrap();
    fs::write(&input, "hint fixture line\n").unwrap();
    let path = root.join("hint.yml");
    fs::write(
        &path,
        format!(
            "version: 2\nserver:\n  listen: 127.0.0.1:{port}\nlogs:\n  dir: {}\nsources:\n  TEST:\n    type: file\n    path: {}\n",
            logs.display(),
            input.display()
        ),
    )
    .unwrap();
    path
}

/// Launches a foreground `embed-log run` whose background update hint points at
/// `base_url`. Output goes to files so a full pipe buffer cannot deadlock the
/// child while it is being killed. Set `suppress` to also export
/// `EMBED_LOG_NO_UPDATE_CHECK=1`.
fn spawn_run(
    config: &Path,
    port: u16,
    base_url: &str,
    stdout: &Path,
    stderr: &Path,
    suppress: bool,
) -> Child {
    let mut command = Command::new(binary());
    command
        .args([
            "run",
            "--config",
            config.to_str().unwrap(),
            "--port",
            &port.to_string(),
            "--no-open-browser",
            "--frontend-dir",
            "/definitely/not/a/frontend",
        ])
        .env("RUST_LOG", "warn")
        .env("EMBED_LOG_UPDATE_BASE_URL", base_url)
        .stdout(Stdio::from(fs::File::create(stdout).unwrap()))
        .stderr(Stdio::from(fs::File::create(stderr).unwrap()));
    if suppress {
        command.env("EMBED_LOG_NO_UPDATE_CHECK", "1");
    }
    command.spawn().unwrap()
}

/// Polls the run server's /api/v1/status endpoint until it answers.
fn wait_until_ready(port: u16, deadline: Instant) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut response = [0_u8; 4096];
    while Instant::now() < deadline {
        let result = std::net::TcpStream::connect_timeout(&address, Duration::from_millis(250));
        if let Ok(mut stream) = result {
            let _ = write!(
                stream,
                "GET /api/v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            );
            if let Ok(count) = stream.read(&mut response) {
                let text = String::from_utf8_lossy(&response[..count]);
                if text.contains("200") && text.contains("api_version") {
                    return true;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}
