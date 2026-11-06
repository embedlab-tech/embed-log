#![cfg(target_os = "linux")]

use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn failed_or_missing_browser_prints_one_hint_without_helper_output() {
    for installed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let helper = root.path().join("xdg-open");
        if installed {
            fs::write(
                &helper,
                "#!/bin/sh\necho helper-stack-trace >&2\necho helper-stdout\nexit 7\n",
            )
            .unwrap();
            fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let input = root.path().join("input.log");
        fs::write(&input, "").unwrap();
        let config = root.path().join("config.yml");
        fs::write(&config, format!("version: 2\nserver:\n  listen: 127.0.0.1:{port}\nlogs:\n  dir: {}\nsources:\n  TEST:\n    type: file\n    path: {}\n", root.path().join("logs").display(), input.display())).unwrap();
        let stderr_path = root.path().join("stderr");
        let stdout_path = root.path().join("stdout");
        let mut child = Command::new(env!("CARGO_BIN_EXE_embed-log"))
            .args(["run", "--config", config.to_str().unwrap()])
            .env("PATH", root.path())
            .env("RUST_LOG", "warn")
            .stderr(fs::File::create(&stderr_path).unwrap())
            .stdout(fs::File::create(&stdout_path).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if fs::read_to_string(&stderr_path)
                .unwrap()
                .contains("Could not open a browser")
            {
                break;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        child.kill().ok();
        child.wait().unwrap();
        let stderr = fs::read_to_string(stderr_path).unwrap();
        assert_eq!(
            stderr.matches("Could not open a browser").count(),
            1,
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("open http://127.0.0.1:{port}/ manually")),
            "{stderr}"
        );
        assert!(stderr.contains("--no-open-browser"));
        assert!(!stderr.contains("helper-stack-trace"));
        assert!(!fs::read_to_string(stdout_path)
            .unwrap()
            .contains("helper-stdout"));
    }
}
