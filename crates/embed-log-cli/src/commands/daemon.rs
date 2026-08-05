//! Background daemon lifecycle and named-instance discovery.

use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use embed_log_core::config::{load_config, resolve_logs_root};

use crate::commands::run::RunOverrides;
use crate::config::resolve_config_path;

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InstanceRecord {
    pub instance: String,
    pub pid: u32,
    pub endpoint: String,
    #[serde(default)]
    pub bind_host: String,
    pub config_path: String,
    #[serde(default)]
    pub config_fingerprint: String,
    pub logs_dir: String,
    pub diagnostic_log: String,
    pub executable: String,
    pub started_at: String,
}

pub(crate) fn cmd_start_daemon(
    instance: &str,
    config_path: Option<&PathBuf>,
    frontend_dir: &Path,
    overrides: &RunOverrides,
    json: bool,
) -> Result<()> {
    validate_instance_name(instance)?;
    let config_path = absolute_path(&resolve_config_path(config_path))?;
    let config = load_config(&config_path).map_err(|error| anyhow::anyhow!(error))?;
    let frontend_dir = absolute_path(frontend_dir)?;
    let logs_dir = match overrides.log_dir.as_ref() {
        Some(path) => absolute_path(path)?,
        None => resolve_logs_root(&config_path, &config.logs.dir),
    };

    fs::create_dir_all(registry_dir()?).context("create daemon registry directory")?;
    cleanup_stale_records()?;

    let host = overrides
        .host
        .clone()
        .unwrap_or_else(|| config.server.host.clone());
    let port = overrides.ws_port.unwrap_or(config.server.ws_port);
    let connect_host = connect_host(&host);
    let endpoint = format!("http://{connect_host}:{port}");
    let config_fingerprint = fingerprint_file(&config_path)?;

    if let Some(record) = read_record(instance)? {
        if record.endpoint == endpoint
            && record.bind_host == host
            && record.config_path == config_path.display().to_string()
            && record.config_fingerprint == config_fingerprint
            && record.logs_dir == logs_dir.display().to_string()
        {
            let backend = http_get_json(&record.endpoint, "/api/v1/status").with_context(|| {
                format!("registered instance {instance:?} exists but is not ready")
            })?;
            return print_daemon_result(&record, &backend, true, json);
        }
        anyhow::bail!(
            "instance {instance:?} is already running with PID {} at {} using config {}; requested endpoint {} and config {}",
            record.pid,
            record.endpoint,
            record.config_path,
            endpoint,
            config_path.display()
        );
    }
    if let Some(record) = list_records()?
        .into_iter()
        .find(|record| record.endpoint == endpoint)
    {
        anyhow::bail!(
            "endpoint {endpoint} is already owned by instance {:?} (PID {}); use that instance or choose another explicit --port",
            record.instance,
            record.pid
        );
    }
    if !port_is_available(&host, port) {
        anyhow::bail!(
            "HTTP/WebSocket port {host}:{port} is already in use by an unregistered process"
        );
    }
    let diagnostic_log = registry_dir()?.join(format!("{instance}.log"));
    let log_file = File::create(&diagnostic_log)
        .with_context(|| format!("create daemon log {}", diagnostic_log.display()))?;
    let stderr_file = log_file.try_clone().context("clone daemon log handle")?;
    let executable = std::env::current_exe().context("resolve current executable")?;

    let mut command = Command::new(&executable);
    command
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .arg("--frontend-dir")
        .arg(&frontend_dir)
        .arg("--no-open-browser")
        .arg("--host")
        .arg(&host)
        .arg("--port")
        .arg(port.to_string())
        .arg("--daemon")
        .arg("--instance")
        .arg(instance)
        .arg("--daemon-child")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file));
    if let Some(path) = &overrides.log_dir {
        command.arg("--log-dir").arg(absolute_path(path)?);
    }

    let mut child = command.spawn().context("spawn embed-log daemon")?;
    let pid = child.id();
    if let Err(error) = wait_until_ready(&endpoint, &mut child, READY_TIMEOUT) {
        let _ = child.kill();
        let _ = child.wait();
        let tail = read_log_tail(&diagnostic_log, 20).unwrap_or_default();
        anyhow::bail!(
            "daemon {instance:?} did not become ready: {error}; diagnostic log: {}{}",
            diagnostic_log.display(),
            if tail.is_empty() {
                String::new()
            } else {
                format!("\n{tail}")
            }
        );
    }

    let record = InstanceRecord {
        instance: instance.to_string(),
        pid,
        endpoint,
        bind_host: host,
        config_path: config_path.display().to_string(),
        config_fingerprint,
        logs_dir: logs_dir.display().to_string(),
        diagnostic_log: diagnostic_log.display().to_string(),
        executable: executable.display().to_string(),
        started_at: Utc::now().to_rfc3339(),
    };
    write_record(&record)?;

    let backend = http_get_json(&record.endpoint, "/api/v1/status")?;
    print_daemon_result(&record, &backend, false, json)
}

fn print_daemon_result(
    record: &InstanceRecord,
    backend: &serde_json::Value,
    reused: bool,
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": true,
                "reused": reused,
                "instance": record,
                "backend": backend,
            }))?
        );
    } else if reused {
        println!(
            "reused daemon {} at {} (PID {})",
            record.instance, record.endpoint, record.pid
        );
    } else {
        println!(
            "daemon {} ready at {} (PID {})",
            record.instance, record.endpoint, record.pid
        );
        println!("  log: {}", record.diagnostic_log);
    }
    Ok(())
}

pub(crate) fn cmd_mark(
    instance: Option<&str>,
    url: Option<&str>,
    action: &str,
    label: Option<&str>,
    json: bool,
) -> Result<()> {
    anyhow::ensure!(!action.trim().is_empty(), "--action must not be empty");
    let (_, endpoint) = resolve_mutating_endpoint(instance, url)?;
    let marker = http_post_json(
        &endpoint,
        "/api/session/marker",
        &serde_json::json!({"action": action, "label": label}),
    )?;
    if json {
        println!("{}", serde_json::to_string(&marker)?);
    } else {
        println!(
            "marked {} at sequence {}",
            action,
            marker["marker"]["sequence"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
}

pub(crate) fn cmd_stats(
    instance: Option<&str>,
    url: Option<&str>,
    json: bool,
    brief: bool,
    sources: &[String],
) -> Result<()> {
    let (_, endpoint) = resolve_endpoint(instance, url)?;
    let mut stats = http_get_json(&endpoint, "/api/stats")?;
    if !sources.is_empty() {
        if let Some(source_map) = stats
            .pointer_mut("/scopes/process_lifetime/sources")
            .and_then(|value| value.as_object_mut())
        {
            source_map.retain(|name, _| sources.iter().any(|requested| requested == name));
        }
    }
    if json {
        println!("{}", serde_json::to_string(&stats)?);
    } else if brief {
        println!(
            "session={} records={} ws_clients={}",
            stats["session_id"].as_str().unwrap_or("-"),
            stats
                .pointer("/scopes/current_session/records")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            stats["ws_clients"].as_u64().unwrap_or(0)
        );
    } else {
        println!(
            "process_lifetime: {}",
            serde_json::to_string(&stats["scopes"]["process_lifetime"])?
        );
        println!(
            "current_session: {}",
            serde_json::to_string(&stats["scopes"]["current_session"])?
        );
    }
    Ok(())
}

pub(crate) fn cmd_status(
    instance: Option<&str>,
    url: Option<&str>,
    json: bool,
    brief: bool,
    sources: &[String],
) -> Result<()> {
    let (record, endpoint) = resolve_endpoint(instance, url)?;
    let mut backend = http_get_json(&endpoint, "/api/v1/status")?;
    if !sources.is_empty() {
        if let Some(source_map) = backend
            .get_mut("sources")
            .and_then(|value| value.as_object_mut())
        {
            source_map.retain(|name, _| sources.iter().any(|requested| requested == name));
        }
    }
    let output = serde_json::json!({
        "ok": true,
        "instance": record,
        "endpoint": endpoint,
        "backend": backend,
    });
    if json {
        println!("{}", serde_json::to_string(&output)?);
    } else if brief {
        let session = output["backend"]["session_id"].as_str().unwrap_or("-");
        let source_count = output["backend"]["sources"]
            .as_object()
            .map_or(0, serde_json::Map::len);
        println!(
            "ready endpoint={} session={} sources={source_count}",
            output["endpoint"].as_str().unwrap_or("-"),
            session
        );
    } else {
        if let Some(record) = output.get("instance").filter(|value| !value.is_null()) {
            println!(
                "instance {} (PID {})",
                record["instance"].as_str().unwrap_or("unknown"),
                record["pid"].as_u64().unwrap_or_default()
            );
        }
        println!("  endpoint: {}", output["endpoint"].as_str().unwrap_or(""));
        println!(
            "  session:  {}",
            output["backend"]["session_id"]
                .as_str()
                .unwrap_or("unavailable")
        );
        let source_count = output["backend"]["sources"]
            .as_object()
            .map_or(0, serde_json::Map::len);
        println!("  sources:  {source_count}");
    }
    Ok(())
}

pub(crate) fn cmd_stop(instance: Option<&str>, url: Option<&str>, json: bool) -> Result<()> {
    cleanup_stale_records()?;
    let record = if let Some(url) = url {
        let endpoint = url.trim_end_matches('/');
        list_records()?
            .into_iter()
            .find(|record| record.endpoint.trim_end_matches('/') == endpoint)
            .ok_or_else(|| anyhow::anyhow!(
                "no registered daemon matches {endpoint}; URL stop requires a registry record so the correct process can be signaled safely"
            ))?
    } else {
        resolve_mutating_instance(instance)?
    };
    if !process_matches_record(&record) {
        remove_record(&record.instance)?;
        anyhow::bail!(
            "instance {:?} has a stale PID record; removed it without signaling PID {}",
            record.instance,
            record.pid
        );
    }

    signal_interrupt(record.pid)?;
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline && process_matches_record(&record) {
        thread::sleep(Duration::from_millis(100));
    }
    if process_matches_record(&record) {
        anyhow::bail!(
            "instance {:?} did not stop within {} seconds; see {}",
            record.instance,
            STOP_TIMEOUT.as_secs(),
            record.diagnostic_log
        );
    }
    remove_record(&record.instance)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": true,
                "instance": record.instance,
                "pid": record.pid,
            }))?
        );
    } else {
        println!("stopped daemon {} (PID {})", record.instance, record.pid);
    }
    Ok(())
}

pub(crate) fn resolve_endpoint(
    instance: Option<&str>,
    url: Option<&str>,
) -> Result<(Option<InstanceRecord>, String)> {
    cleanup_stale_records()?;
    if let Some(url) = url {
        return Ok((None, url.trim_end_matches('/').to_string()));
    }
    let record = resolve_instance(instance)?;
    let endpoint = record.endpoint.clone();
    Ok((Some(record), endpoint))
}

pub(crate) fn resolve_mutating_endpoint(
    instance: Option<&str>,
    url: Option<&str>,
) -> Result<(Option<InstanceRecord>, String)> {
    cleanup_stale_records()?;
    if let Some(url) = url {
        return Ok((None, url.trim_end_matches('/').to_string()));
    }
    let record = resolve_mutating_instance(instance)?;
    let endpoint = record.endpoint.clone();
    Ok((Some(record), endpoint))
}

fn selected_instance(explicit: Option<&str>) -> Option<String> {
    explicit
        .map(str::to_string)
        .or_else(|| std::env::var("EMBED_LOG_INSTANCE").ok())
}

fn resolve_mutating_instance(explicit: Option<&str>) -> Result<InstanceRecord> {
    let name = selected_instance(explicit).context(
        "an explicit target is required; pass --instance, set EMBED_LOG_INSTANCE, or use --url where supported",
    )?;
    validate_instance_name(&name)?;
    read_record(&name)?.ok_or_else(|| anyhow::anyhow!("instance {name:?} is not running"))
}

fn resolve_instance(explicit: Option<&str>) -> Result<InstanceRecord> {
    let selected = selected_instance(explicit);
    if let Some(name) = selected {
        validate_instance_name(&name)?;
        return read_record(&name)?.ok_or_else(|| {
            anyhow::anyhow!(
                "instance {name:?} is not running; use `embed-log status` to list choices"
            )
        });
    }

    let records = list_records()?;
    match records.as_slice() {
        [only] => Ok(only.clone()),
        [] => anyhow::bail!("no Embed-log daemon instances are running"),
        many => {
            let names = many
                .iter()
                .map(|record| record.instance.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple Embed-log instances are running: {names}; repeat with --instance <name>"
            )
        }
    }
}

fn validate_instance_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        anyhow::bail!("invalid instance name {name:?}; use ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

fn registry_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("EMBED_LOG_RUNTIME_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Ok(PathBuf::from(path).join("embed-log"));
    }
    let home = std::env::var_os("HOME").context(
        "cannot resolve daemon registry: set XDG_RUNTIME_DIR, HOME, or EMBED_LOG_RUNTIME_DIR",
    )?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("embed-log")
        .join("runtime"))
}

fn record_path(instance: &str) -> Result<PathBuf> {
    Ok(registry_dir()?.join(format!("{instance}.json")))
}

fn write_record(record: &InstanceRecord) -> Result<()> {
    let path = record_path(&record.instance)?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec_pretty(record)?)
        .with_context(|| format!("write daemon registry {}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .with_context(|| format!("publish daemon registry {}", path.display()))?;
    Ok(())
}

fn read_record(instance: &str) -> Result<Option<InstanceRecord>> {
    let path = record_path(instance)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read registry {}", path.display()))?;
    let record = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse registry {}", path.display()))?;
    Ok(Some(record))
}

fn list_records() -> Result<Vec<InstanceRecord>> {
    let directory = registry_dir()?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let bytes =
            fs::read(&path).with_context(|| format!("read daemon registry {}", path.display()))?;
        let record = serde_json::from_slice::<InstanceRecord>(&bytes)
            .with_context(|| format!("parse daemon registry {}", path.display()))?;
        records.push(record);
    }
    records.sort_by(|left, right| left.instance.cmp(&right.instance));
    Ok(records)
}

fn cleanup_stale_records() -> Result<()> {
    for mut record in list_records()? {
        if process_matches_record(&record) {
            continue;
        }

        // A release install can replace the executable while an older daemon
        // remains alive. If its endpoint still answers, adopt the live
        // process instead of deleting the only way to stop it.
        let live_executable = PathBuf::from(format!("/proc/{}/exe", record.pid));
        if let Ok(actual) = fs::read_link(&live_executable) {
            if http_get_json(&record.endpoint, "/api/v1/status").is_ok() {
                record.executable = actual.display().to_string();
                write_record(&record)?;
                eprintln!(
                    "repaired daemon record for instance {:?}: adopted live PID {} at {}",
                    record.instance, record.pid, record.endpoint
                );
                continue;
            }
        }

        remove_record(&record.instance)?;
        eprintln!(
            "removed stale daemon record for instance {:?} (PID {})",
            record.instance, record.pid
        );
    }
    Ok(())
}

fn remove_record(instance: &str) -> Result<()> {
    let path = record_path(instance)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove registry {}", path.display())),
    }
}

fn process_matches_record(record: &InstanceRecord) -> bool {
    let proc_exe = PathBuf::from(format!("/proc/{}/exe", record.pid));
    let Ok(actual) = fs::read_link(proc_exe) else {
        return false;
    };
    let expected =
        fs::canonicalize(&record.executable).unwrap_or_else(|_| PathBuf::from(&record.executable));
    fs::canonicalize(actual).is_ok_and(|actual| actual == expected)
}

fn signal_interrupt(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .context("execute kill -INT")?;
        if !status.success() {
            anyhow::bail!("failed to signal daemon PID {pid}");
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        anyhow::bail!("daemon stop is currently supported on Unix only")
    }
}

fn fingerprint_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read config {}", path.display()))?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn port_is_available(host: &str, port: u16) -> bool {
    let bind_host = if host == "localhost" {
        "127.0.0.1"
    } else {
        host
    };
    TcpListener::bind((bind_host, port)).is_ok()
}

fn connect_host(bind_host: &str) -> &str {
    match bind_host {
        "0.0.0.0" => "127.0.0.1",
        "::" => "[::1]",
        other => other,
    }
}

fn wait_until_ready(
    endpoint: &str,
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().context("inspect daemon child")? {
            anyhow::bail!("daemon process exited before readiness with {status}");
        }
        match http_get_json(endpoint, "/api/v1/status") {
            Ok(status) if status.get("ok").and_then(|value| value.as_bool()) == Some(true) => {
                return Ok(())
            }
            Ok(_) => last_error = Some("status response did not report ok=true".to_string()),
            Err(error) => last_error = Some(error.to_string()),
        }
        thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!(
        "readiness timed out after {} seconds ({})",
        timeout.as_secs(),
        last_error.unwrap_or_else(|| "no response".to_string())
    )
}

pub(crate) fn http_post_json(
    endpoint: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    http_request_json(
        endpoint,
        "POST",
        path,
        Some(&body.to_string()),
        Duration::from_secs(2),
    )
}

pub(crate) fn http_post_json_with_timeout(
    endpoint: &str,
    path: &str,
    body: &serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value> {
    http_request_json(endpoint, "POST", path, Some(&body.to_string()), timeout)
}

fn http_get_json(endpoint: &str, path: &str) -> Result<serde_json::Value> {
    http_request_json(endpoint, "GET", path, None, Duration::from_secs(2))
}

fn http_request_json(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let address = endpoint
        .strip_prefix("http://")
        .context("only http:// daemon endpoints are supported")?;
    if address.contains('/') {
        anyhow::bail!("endpoint must not contain a path: {endpoint}");
    }
    let socket_address = address
        .to_socket_addrs()
        .with_context(|| format!("resolve endpoint {endpoint}"))?
        .next()
        .with_context(|| format!("endpoint did not resolve: {endpoint}"))?;
    let mut stream = TcpStream::connect_timeout(&socket_address, Duration::from_secs(2))
        .with_context(|| format!("connect to {endpoint}"))?;
    stream.set_read_timeout(Some(timeout))?;
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let response = String::from_utf8(response).context("HTTP response was not UTF-8")?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .context("malformed HTTP response")?;
    let status = headers.lines().next().unwrap_or_default();
    if !status.contains(" 200 ") {
        anyhow::bail!("daemon returned {status}");
    }
    serde_json::from_str(body).context("parse daemon status JSON")
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn read_log_tail(path: &Path, lines: usize) -> Result<String> {
    let text = fs::read_to_string(path)?;
    Ok(text
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_are_safe_for_registry_paths() {
        assert!(validate_instance_name("bench-a_2").is_ok());
        assert!(validate_instance_name("").is_err());
        assert!(validate_instance_name("../other").is_err());
        assert!(validate_instance_name("bench a").is_err());
    }

    #[test]
    fn connect_host_maps_wildcard_addresses() {
        assert_eq!(connect_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(connect_host("::"), "[::1]");
        assert_eq!(connect_host("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn explicit_port_preflight_detects_listener() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!port_is_available("127.0.0.1", port));
    }
}
