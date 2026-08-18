#![cfg(not(windows))]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
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
        let deadline = Instant::now() + Duration::from_secs(3);
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
    let (base_url, server) = serve(
        serde_json::to_vec(&release).unwrap(),
        fs::read(archive_path).unwrap(),
        2,
    );
    let installed = managed_copy(&temp, &target);

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
    let (base_url, server) = serve(
        serde_json::to_vec(&release).unwrap(),
        fs::read(archive_path).unwrap(),
        2,
    );
    let installed = managed_copy(&temp, &target);
    let before = fs::read(&installed).unwrap();

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
