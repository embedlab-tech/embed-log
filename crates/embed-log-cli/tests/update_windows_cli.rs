#![cfg(windows)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

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

fn archive(destination: &std::path::Path, replacement: &[u8]) -> String {
    let file = fs::File::create(destination).unwrap();
    let mut zip = ZipWriter::new(file);
    zip.start_file(
        "embed-log.exe",
        FileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(replacement).unwrap();
    zip.finish().unwrap();
    format!("{:x}", Sha256::digest(fs::read(destination).unwrap()))
}

fn serve(release: Vec<u8>, archive: Vec<u8>) -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut requests = 0;
        while requests < 2 && Instant::now() < deadline {
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
            let request = String::from_utf8_lossy(&request[..count]);
            let body = if request.starts_with("GET /release.json ") {
                &release
            } else if request.starts_with("GET /embed-log-test.zip ") {
                &archive
            } else {
                panic!("unexpected update request: {request}");
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

#[test]
fn update_defers_windows_executable_replacement_until_the_cli_exits() {
    let temp = TempDir::new().unwrap();
    let target = target();
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    let installed = install_dir.join("embed-log.exe");
    fs::copy(binary(), &installed).unwrap();
    fs::write(
        install_dir.join(".embed-log-install"),
        serde_json::to_vec(&serde_json::json!({
            "repository": "embedlab-tech/embed-log",
            "target": target.clone(),
            "executable": installed,
        }))
        .unwrap(),
    )
    .unwrap();

    let replacement = b"updated executable fixture";
    let archive_path = temp.path().join("embed-log-test.zip");
    let checksum = archive(&archive_path, replacement);
    let release = serde_json::json!({
        "version": "99.0.0",
        "assets": { target: { "archive": "embed-log-test.zip", "sha256": checksum } },
    });
    let (base_url, server) = serve(
        serde_json::to_vec(&release).unwrap(),
        fs::read(archive_path).unwrap(),
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("finish after embed-log exits"));

    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read(&installed).unwrap() != replacement && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(fs::read(installed).unwrap(), replacement);
    assert_eq!(
        server.join().unwrap(),
        2,
        "update did not fetch both fixture assets"
    );
}
