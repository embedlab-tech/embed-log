//! The grab-bag of leaf subcommands: `version`, `doctor`, `validate`, `ports`,
//! and `hello`. None of them start the server.

use std::path::Path;

use anyhow::Result;

use embed_log_core::config::{load_config, resolve_logs_root};

/// `embed-log version` — package version plus optional config summary.
///
/// `git_sha`/`build_time` come from `build.rs` and don't change between
/// `cargo build` invocations unless the source actually changed — the
/// quickest way to tell a stale installed binary from a freshly built one.
pub(crate) fn cmd_version(config_path: Option<&Path>, json: bool) -> Result<()> {
    let mut out = version_report();
    if json {
        if let Some(path) = config_path {
            match load_config(path) {
                Ok(cfg) => {
                    out["config"] = serde_json::json!({
                        "path": path.display().to_string(),
                        "sources": cfg.sources.len(),
                        "tabs": cfg.tabs.len(),
                    });
                }
                Err(e) => {
                    out["config_error"] = serde_json::json!(e.to_string());
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "embed-log {} ({}, built {})",
            out["version"].as_str().unwrap_or("unknown"),
            out["git_sha"].as_str().unwrap_or("unknown"),
            out["build_time"].as_str().unwrap_or("unknown"),
        );
        println!(
            "  target:   {}",
            out["target"].as_str().unwrap_or("unknown")
        );
        println!(
            "  path:     {}",
            out["executable"].as_str().unwrap_or("unknown")
        );
        if let Some(path) = config_path {
            match load_config(path) {
                Ok(cfg) => {
                    println!("  config:   {}", path.display());
                    println!("  sources:  {}", cfg.sources.len());
                    println!("  tabs:     {}", cfg.tabs.len());
                }
                Err(e) => {
                    println!("  config error: {e}");
                }
            }
        }
    }
    Ok(())
}

fn version_report() -> serde_json::Value {
    let executable = std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "git_sha": env!("EMBED_LOG_GIT_SHA"),
        "build_time": env!("EMBED_LOG_BUILD_TIME"),
        "target": env!("EMBED_LOG_TARGET"),
        "executable": executable,
    })
}

/// `embed-log validate` — load/validate config and print resolved summary.
pub(crate) fn cmd_validate(config_path: &Path, json: bool) -> Result<()> {
    let cfg = load_config(config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let logs_root = resolve_logs_root(config_path, &cfg.logs.dir);
    let sources: Vec<_> = cfg
        .sources
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "label": s.label.as_deref().unwrap_or(&s.name),
                "kind": s.source_type,
                "parser": s.parser.parser_type,
                "writable": s.source_type.eq_ignore_ascii_case("uart"),
            })
        })
        .collect();
    let tabs: Vec<_> = cfg
        .tabs
        .iter()
        .map(|t| {
            serde_json::json!({
                "label": t.label,
                "panes": t.panes.iter().map(|p| p.source_name()).collect::<Vec<_>>(),
            })
        })
        .collect();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ok",
                "config": config_path.display().to_string(),
                "server": {
                    "host": cfg.server.host,
                    "ws_port": cfg.server.ws_port,
                    "app_name": cfg.server.app_name,
                    "control_api": cfg.server.control_api,
                },
                "logs_root": logs_root.display().to_string(),
                "sources": sources,
                "tabs": tabs,
            }))?
        );
    } else {
        println!("config ok: {}", config_path.display());
        println!(
            "  server:   http://{}:{}",
            cfg.server.host, cfg.server.ws_port
        );
        println!("  logs:     {}", logs_root.display());
        println!("  sources:  {}", cfg.sources.len());
        for source in &cfg.sources {
            println!(
                "    - {} [{}] label={}",
                source.name,
                source.source_type,
                source.label.as_deref().unwrap_or(&source.name)
            );
        }
        println!("  tabs:     {}", cfg.tabs.len());
        for tab in &cfg.tabs {
            let panes = tab
                .panes
                .iter()
                .map(|p| p.source_name())
                .collect::<Vec<_>>()
                .join(", ");
            println!("    - {}: {}", tab.label, panes);
        }
    }
    Ok(())
}

/// `embed-log doctor` — environment/config/runtime diagnostics.
pub(crate) fn cmd_doctor(
    config_path: Option<&Path>,
    serial_paths: &[std::path::PathBuf],
    json: bool,
) -> Result<()> {
    let report = build_doctor_report_with_serial(config_path, serial_paths);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("embed-log doctor");
    println!(
        "  version:  {}",
        report["version"].as_str().unwrap_or("unknown")
    );
    println!(
        "  status:   {}",
        report["status"].as_str().unwrap_or("unknown")
    );
    if let Some(system) = report.get("system") {
        let os = system["os"].as_str().unwrap_or("unknown");
        let arch = system["arch"].as_str().unwrap_or("unknown");
        let family = system["family"].as_str().unwrap_or("unknown");
        println!("  system:   {os} / {arch} ({family})");
        if let Some(detail) = system.get("detail").and_then(|v| v.as_str()) {
            println!("  detail:   {detail}");
        }
    }

    if let Some(value) = report
        .get("config_env")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
    {
        println!("  config env: EMBED_LOG_CONFIG_YML_PATH={value}");
    }
    if let Some(resolved) = report.get("resolved_config_path").and_then(|v| v.as_str()) {
        println!("  resolved config: {resolved}");
    }

    if let Some(config) = report.get("config") {
        if let Some(path) = config.get("path").and_then(|v| v.as_str()) {
            println!("  config:   {path}");
        }
        if let Some(sources) = config.get("sources").and_then(|v| v.as_u64()) {
            println!("  sources:  {sources}");
        }
        if let Some(tabs) = config.get("tabs").and_then(|v| v.as_u64()) {
            println!("  tabs:     {tabs}");
        }
    }
    if let Some(error) = report.get("config_error").and_then(|v| v.as_str()) {
        println!("  config error: {error}");
    }
    if let Some(serial) = report.get("serial").and_then(|v| v.as_array()) {
        for port in serial {
            let path = port["path"].as_str().unwrap_or("unknown");
            let status = port["status"].as_str().unwrap_or("unknown");
            let detail = port["detail"].as_str().unwrap_or("");
            println!("  serial:   {path} — {status}{detail}");
        }
    }

    if let Some(hints) = report.get("hints").and_then(|v| v.as_array()) {
        for hint in hints.iter().filter_map(|v| v.as_str()) {
            println!("  hint:     {hint}");
        }
    }
    Ok(())
}

fn build_doctor_report_with_serial(
    config_path: Option<&Path>,
    serial_paths: &[std::path::PathBuf],
) -> serde_json::Value {
    build_doctor_report_with_env_and_serial(
        config_path,
        std::env::var("EMBED_LOG_CONFIG_YML_PATH").ok(),
        serial_paths,
    )
}

/// Same as [`build_doctor_report`], but with the `EMBED_LOG_CONFIG_YML_PATH`
/// value injected rather than read from the real process environment — kept
/// separate so the env-var precedence is testable without touching global
/// state (same split `crate::config::resolve_config_path_with_env` already uses).
#[cfg(test)]
fn build_doctor_report_with_env(
    config_path: Option<&Path>,
    env_value: Option<String>,
) -> serde_json::Value {
    build_doctor_report_with_env_and_serial(config_path, env_value, &[])
}

fn build_doctor_report_with_env_and_serial(
    config_path: Option<&Path>,
    env_value: Option<String>,
    requested_serial_paths: &[std::path::PathBuf],
) -> serde_json::Value {
    let version = env!("CARGO_PKG_VERSION");
    let mut status = "ok".to_string();
    let resolved_path = crate::config::resolve_config_path_with_env(
        config_path.map(Path::to_path_buf).as_ref(),
        env_value.clone().map(std::path::PathBuf::from),
    );
    let mut out = serde_json::json!({
        "version": version,
        "status": status,
        "system": detect_system_info(),
        "resolved_config_path": resolved_path.display().to_string(),
        "hints": [],
    });
    if let Some(value) = &env_value {
        out["config_env"] = serde_json::json!({
            "var": "EMBED_LOG_CONFIG_YML_PATH",
            "value": value,
        });
    }

    // Reflect the same config `run` would actually load — not just an
    // explicitly-passed --config — so `doctor` never hides what's really
    // going to happen. A config that simply doesn't exist yet (fresh
    // checkout, no --config given) is normal, not a warning.
    let mut config_info = None;
    let mut serial_paths = requested_serial_paths.to_vec();
    if resolved_path.exists() {
        match load_config(&resolved_path) {
            Ok(cfg) => {
                config_info = Some(serde_json::json!({
                    "path": resolved_path.display().to_string(),
                    "sources": cfg.sources.len(),
                    "tabs": cfg.tabs.len(),
                }));
                serial_paths.extend(cfg.sources.iter().filter_map(|source| {
                    source
                        .source_type
                        .eq_ignore_ascii_case("uart")
                        .then(|| source.port.as_str().map(std::path::PathBuf::from))
                        .flatten()
                }));
            }
            Err(error) => {
                status = "warn".to_string();
                out["status"] = serde_json::json!(status);
                out["config_error"] = serde_json::json!(error.to_string());
            }
        }
    }
    if let Some(config) = config_info {
        out["config"] = config;
    }
    serial_paths.sort();
    serial_paths.dedup();
    let serial = serial_paths
        .iter()
        .map(|path| inspect_serial_path(path))
        .collect::<Vec<_>>();
    if serial.iter().any(|port| port["status"] != "ok") {
        out["status"] = serde_json::json!("warn");
    }
    out["serial"] = serde_json::Value::Array(serial);
    out
}

/// Check filesystem-level serial access without opening/configuring the port;
/// opening a TTY through the serial library can reset attached hardware.
fn inspect_serial_path(path: &Path) -> serde_json::Value {
    let display = path.display().to_string();
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(_) => serde_json::json!({
            "path": display,
            "status": "ok",
            "detail": " readable and writable",
        }),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => serde_json::json!({
            "path": display,
            "status": "permission_denied",
            "detail": " — permission denied; on Linux add your user to the device group (commonly dialout or uucp), then sign in again",
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({
            "path": display,
            "status": "missing",
            "detail": " — device path does not exist; reconnect the device or run `embed-log ports`",
        }),
        Err(error) => serde_json::json!({
            "path": display,
            "status": "unavailable",
            "detail": format!(" — {error}"),
        }),
    }
}

fn detect_system_info() -> serde_json::Value {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let family = std::env::consts::FAMILY;
    let detail = detect_system_detail();
    serde_json::json!({
        "os": os,
        "arch": arch,
        "family": family,
        "detail": detail,
    })
}

fn detect_system_detail() -> Option<String> {
    #[cfg(target_family = "unix")]
    {
        let output = std::process::Command::new("uname")
            .args(["-srm"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
    #[cfg(not(target_family = "unix"))]
    {
        None
    }
}

/// `embed-log ports` — list detected serial ports.
pub(crate) fn cmd_ports(json: bool) -> Result<()> {
    let ports = serialport::available_ports().unwrap_or_default();

    if json {
        let port_list: Vec<serde_json::Value> = ports
            .iter()
            .map(|p| {
                let port_type = match &p.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        serde_json::json!({
                            "type": "usb",
                            "vid": info.vid,
                            "pid": info.pid,
                            "product": info.product,
                            "manufacturer": info.manufacturer,
                        })
                    }
                    _ => serde_json::json!({"type": "other"}),
                };
                serde_json::json!({
                    "name": p.port_name,
                    "port_type": port_type,
                })
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ports": port_list,
            }))?
        );
    } else if ports.is_empty() {
        println!("No serial ports detected.");
    } else {
        println!("Detected serial ports:");
        for p in &ports {
            match &p.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    let product = info.product.as_deref().unwrap_or("unknown");
                    let mfr = info.manufacturer.as_deref().unwrap_or("unknown");
                    println!(
                        "  {}  USB {:04x}:{:04x}  {} ({})",
                        p.port_name, info.vid, info.pid, product, mfr
                    );
                }
                _ => {
                    println!("  {}", p.port_name);
                }
            }
        }
    }
    Ok(())
}

/// `embed-log hello` — smoke-test target.
pub(crate) fn cmd_hello() -> Result<()> {
    println!("Hello from embed-log!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_report_includes_release_diagnostics() {
        let report = version_report();
        assert!(!report["version"].as_str().unwrap_or_default().is_empty());
        assert!(!report["git_sha"].as_str().unwrap_or_default().is_empty());
        assert!(!report["build_time"].as_str().unwrap_or_default().is_empty());
        assert!(!report["target"].as_str().unwrap_or_default().is_empty());
        assert!(!report["executable"].as_str().unwrap_or_default().is_empty());
    }

    #[test]
    fn doctor_report_marks_missing_explicit_serial_path() {
        let path =
            std::env::temp_dir().join(format!("embed-log-missing-serial-{}", std::process::id()));
        std::fs::remove_file(&path).ok();
        let report = build_doctor_report_with_env_and_serial(None, None, &[path]);
        assert_eq!(report["status"], "warn");
        assert_eq!(report["serial"][0]["status"], "missing");
    }

    #[test]
    fn doctor_report_includes_resolved_config_path_even_without_explicit_flag() {
        let report = build_doctor_report_with_env(None, None);
        assert_eq!(report["resolved_config_path"], "embed-log.yml");
        assert!(report.get("config_env").is_none());
    }

    #[test]
    fn doctor_report_surfaces_config_env_var_when_set() {
        let report = build_doctor_report_with_env(None, Some("/set/by/env.yml".to_string()));
        assert_eq!(report["resolved_config_path"], "/set/by/env.yml");
        assert_eq!(report["config_env"]["value"], "/set/by/env.yml");
        assert_eq!(report["config_env"]["var"], "EMBED_LOG_CONFIG_YML_PATH");
    }

    #[test]
    fn doctor_report_missing_config_is_not_a_warning() {
        let report =
            build_doctor_report_with_env(None, Some("/nonexistent/embed-log.yml".to_string()));
        assert_eq!(report["status"], "ok");
        assert!(report.get("config").is_none());
        assert!(report.get("config_error").is_none());
    }
}
