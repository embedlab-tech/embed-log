//! The grab-bag of leaf subcommands: `version`, `doctor`, `validate`, `ports`,
//! `hello`, `init`, `merge`, `parse`. None of them start the server.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use embed_log_core::config::{load_config, resolve_logs_root, AppConfig};
use embed_log_core::session::SessionExporter;

use crate::demo_config::DEMO_CONFIG;

/// `embed-log version` — package version plus optional config summary.
///
/// `git_sha`/`build_time` come from `build.rs` and don't change between
/// `cargo build` invocations unless the source actually changed — the
/// quickest way to tell a stale installed binary from a freshly built one.
pub(crate) fn cmd_version(config_path: Option<&Path>, json: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let git_sha = env!("EMBED_LOG_GIT_SHA");
    let build_time = env!("EMBED_LOG_BUILD_TIME");
    if json {
        let mut out = serde_json::json!({
            "version": version,
            "git_sha": git_sha,
            "build_time": build_time,
        });
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
        println!("embed-log {version} ({git_sha}, built {build_time})");
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

/// `embed-log validate` — load/validate config and print resolved summary.
pub(crate) fn cmd_validate(config_path: &Path, json: bool) -> Result<()> {
    let cfg = load_config(config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let logs_root = resolve_logs_root(config_path, &cfg.logs.dir);
    let pcap_sources = count_pcap_sources(&cfg);
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
                "pcap_sources": pcap_sources,
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
        if pcap_sources > 0 {
            println!("  pcap sources: {pcap_sources}");
            println!(
                "  hint: run `embed-log doctor --config {}` for packet-capture diagnostics",
                config_path.display()
            );
        }
    }
    Ok(())
}

/// `embed-log doctor` — environment/config/runtime diagnostics.
pub(crate) fn cmd_doctor(config_path: Option<&Path>, json: bool) -> Result<()> {
    let report = build_doctor_report(config_path);
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
        if let Some(pcap_sources) = config.get("pcap_sources").and_then(|v| v.as_u64()) {
            println!("  pcap sources: {pcap_sources}");
        }
    }
    if let Some(error) = report.get("config_error").and_then(|v| v.as_str()) {
        println!("  config error: {error}");
    }

    let capture = &report["packet_capture"];
    println!(
        "  pcap feature: {}",
        if capture["feature_enabled"].as_bool().unwrap_or(false) {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  native library: {} ({})",
        if capture["library_found"].as_bool().unwrap_or(false) {
            "found"
        } else {
            "missing"
        },
        capture["provider"].as_str().unwrap_or("unknown")
    );
    if let Some(loaded) = capture.get("loaded_from").and_then(|v| v.as_str()) {
        println!("  loaded from: {loaded}");
    }
    if let Some(message) = capture.get("message").and_then(|v| v.as_str()) {
        println!("  capture:  {message}");
    }
    if let Some(hints) = report.get("hints").and_then(|v| v.as_array()) {
        for hint in hints.iter().filter_map(|v| v.as_str()) {
            println!("  hint:     {hint}");
        }
    }
    Ok(())
}

fn build_doctor_report(config_path: Option<&Path>) -> serde_json::Value {
    let version = env!("CARGO_PKG_VERSION");
    let packet_capture = detect_packet_capture_support();
    let mut status = "ok".to_string();
    let mut out = serde_json::json!({
        "version": version,
        "status": status,
        "system": detect_system_info(),
        "packet_capture": packet_capture,
        "hints": [],
    });

    let mut config_info = None;
    if let Some(path) = config_path {
        match load_config(path) {
            Ok(cfg) => {
                let pcap_sources = count_pcap_sources(&cfg);
                config_info = Some(serde_json::json!({
                    "path": path.display().to_string(),
                    "sources": cfg.sources.len(),
                    "tabs": cfg.tabs.len(),
                    "pcap_sources": pcap_sources,
                }));
                if pcap_sources > 0 {
                    apply_packet_capture_warnings(&mut out, pcap_sources);
                }
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
    out
}

fn apply_packet_capture_warnings(out: &mut serde_json::Value, pcap_sources: usize) {
    let feature_enabled = out["packet_capture"]["feature_enabled"]
        .as_bool()
        .unwrap_or(false);
    let library_found = out["packet_capture"]["library_found"]
        .as_bool()
        .unwrap_or(false);
    if !feature_enabled || !library_found {
        out["status"] = serde_json::json!("warn");
    }
    if !feature_enabled {
        out["hints"].as_array_mut().unwrap().push(serde_json::json!(
            format!(
                "config uses {pcap_sources} pcap network_capture source(s), but this embed-log binary was built without the 'pcap-capture' feature"
            )
        ));
    }
    if !library_found {
        out["hints"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(format!(
                "native packet-capture library not found; install {}",
                packet_capture_provider_name()
            )));
    }
}

fn count_pcap_sources(cfg: &AppConfig) -> usize {
    cfg.sources
        .iter()
        .filter(|source| source.source_type.eq_ignore_ascii_case("network_capture"))
        .filter(|source| {
            source
                .network_backend
                .as_deref()
                .unwrap_or("mock")
                .eq_ignore_ascii_case("pcap")
        })
        .count()
}

fn detect_packet_capture_support() -> serde_json::Value {
    let feature_enabled = cfg!(feature = "pcap-capture");
    let provider = packet_capture_provider_name();
    let candidates = packet_capture_library_candidates();
    let mut loaded_from = None;
    let mut errors = Vec::new();

    for candidate in &candidates {
        // SAFETY: We only probe whether the dynamic library can be opened.
        // The handle is dropped immediately without resolving symbols.
        let attempt = unsafe { libloading::Library::new(candidate) };
        match attempt {
            Ok(lib) => {
                loaded_from = Some((*candidate).to_string());
                drop(lib);
                break;
            }
            Err(error) => errors.push(format!("{candidate}: {error}")),
        }
    }

    let library_found = loaded_from.is_some();
    if library_found {
        errors.clear();
    }
    let message = if feature_enabled && library_found {
        "pcap capture is available"
    } else if feature_enabled {
        "pcap capture feature is enabled, but the native packet-capture library is missing"
    } else if library_found {
        "native packet-capture library is installed, but this binary was built without the pcap-capture feature"
    } else {
        "pcap capture is unavailable in this binary and no native packet-capture library was found"
    };

    serde_json::json!({
        "feature_enabled": feature_enabled,
        "provider": provider,
        "library_found": library_found,
        "loaded_from": loaded_from,
        "candidate_libraries": candidates,
        "message": message,
        "errors": errors,
    })
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

fn packet_capture_provider_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Npcap/WinPcap"
    }
    #[cfg(target_os = "macos")]
    {
        "libpcap"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "libpcap"
    }
}

fn packet_capture_library_candidates() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        vec!["wpcap.dll", "Packet.dll"]
    }
    #[cfg(target_os = "macos")]
    {
        vec!["libpcap.A.dylib", "libpcap.dylib"]
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        vec!["libpcap.so.1", "libpcap.so", "libpcap.so.0.8"]
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

/// `embed-log init` — write the sample config template.
pub(crate) fn cmd_init(output: &Path) -> Result<()> {
    std::fs::write(output, DEMO_CONFIG).with_context(|| format!("write {}", output.display()))?;
    println!("wrote {}", output.display());
    println!("edit it and run: embed-log --config {}", output.display());
    Ok(())
}

/// `embed-log parse` — extract raw log files from an exported session HTML.
pub(crate) fn cmd_parse(html_path: &Path, output_dir: &Path) -> Result<()> {
    let html = std::fs::read_to_string(html_path)
        .with_context(|| format!("read {}", html_path.display()))?;

    let entries = extract_log_data(&html)?;
    std::fs::create_dir_all(output_dir)?;
    let by_source = group_entries_by_source(&entries);

    for (source, lines) in &by_source {
        let path = output_dir.join(format!("{}.log", source));
        std::fs::write(&path, lines.join("\n") + "\n")?;
        println!("  {}  {} lines", path.display(), lines.len());
    }
    println!(
        "parsed {} sources → {}",
        by_source.len(),
        output_dir.display()
    );
    Ok(())
}

/// Pull the `const logData = [...];` array out of a session HTML document.
fn extract_log_data(html: &str) -> Result<Vec<serde_json::Value>> {
    let marker = "const logData = ";
    let start = html
        .find(marker)
        .ok_or_else(|| anyhow::anyhow!("not an embed-log session HTML: missing logData"))?;
    let data_start = start + marker.len();
    let end = html[data_start..]
        .find(";\n")
        .ok_or_else(|| anyhow::anyhow!("malformed logData in HTML"))?;
    let json_str = &html[data_start..data_start + end];
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(json_str).with_context(|| "parse logData JSON")?;
    Ok(entries)
}

/// Group log entries by their `source_id` (missing → "unknown"), preserving order.
fn group_entries_by_source(entries: &[serde_json::Value]) -> HashMap<String, Vec<String>> {
    let mut by_source: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries {
        let source_id = entry
            .get("source_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let data = entry.get("data").and_then(|v| v.as_str()).unwrap_or("");
        by_source
            .entry(source_id.to_string())
            .or_default()
            .push(data.to_string());
    }
    by_source
}

/// `embed-log merge` — merge raw log files into a static HTML session.
pub(crate) fn cmd_merge(
    tabs: &[String],
    output: &Path,
    timestamp_mode: &str,
    first_log_at: Option<String>,
) -> Result<()> {
    let inputs = parse_merge_tabs(tabs)?;

    let frontend_dir = std::env::current_dir()?.join("frontend");
    let exporter = SessionExporter::new(
        output.to_path_buf(),
        inputs.source_files,
        inputs.tab_configs,
        inputs.pane_labels,
        frontend_dir,
        timestamp_mode.to_string(),
        first_log_at,
    );
    exporter.export()?;
    println!("{}", output.display());
    Ok(())
}

/// Parsed `--tab` groups, ready to hand to [`SessionExporter`].
#[derive(Debug)]
struct MergeInputs {
    tab_configs: Vec<serde_json::Value>,
    source_files: HashMap<String, String>,
    pane_labels: HashMap<String, String>,
}

/// Parse the flat `--tab` argument vector into structured merge inputs.
///
/// Each `--tab` group is `LABEL PANE FILE [PANE FILE]...` where `PANE` may be
/// `id=Label`. The clap definition collects every `--tab`'s args into one flat
/// `Vec<String>`, so we re-split with a heuristic: a token containing `.log`
/// or `.txt` is a FILE; once a group has an odd length >1, the next non-file
/// token starts a new group.
fn parse_merge_tabs(tabs: &[String]) -> Result<MergeInputs> {
    let groups = group_tab_specs(tabs);

    let mut tab_configs: Vec<serde_json::Value> = Vec::new();
    let mut source_files: HashMap<String, String> = HashMap::new();
    let mut pane_labels: HashMap<String, String> = HashMap::new();

    for group in &groups {
        if group.len() < 3 {
            anyhow::bail!("each --tab needs LABEL PANE FILE [PANE FILE]");
        }
        let label = &group[0];
        let mut panes: Vec<String> = Vec::new();
        let mut i = 1;
        while i < group.len() {
            if i + 1 >= group.len() {
                anyhow::bail!("each pane needs FILE after PANE name in --tab {}", label);
            }
            let pane_spec = &group[i];
            let file = group[i + 1].clone();
            let (pane_id, pane_label) = pane_spec
                .split_once('=')
                .map(|(id, label)| (id.to_string(), label.to_string()))
                .unwrap_or_else(|| (pane_spec.clone(), pane_spec.clone()));
            source_files.insert(pane_id.clone(), file);
            pane_labels.insert(pane_id.clone(), pane_label);
            panes.push(pane_id);
            i += 2;
        }
        tab_configs.push(serde_json::json!({
            "label": label,
            "panes": panes,
        }));
    }

    Ok(MergeInputs {
        tab_configs,
        source_files,
        pane_labels,
    })
}

/// Re-split the flat `--tab` argument vector into per-tab groups.
fn group_tab_specs(tabs: &[String]) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    for arg in tabs {
        if groups.is_empty() {
            groups.push(vec![arg.clone()]);
        } else if arg.contains(".log") || arg.contains(".txt") {
            groups.last_mut().unwrap().push(arg.clone());
        } else if groups
            .last()
            .map(|g| g.len() > 1 && g.len() % 2 == 1)
            .unwrap_or(false)
        {
            groups.push(vec![arg.clone()]);
        } else {
            groups.last_mut().unwrap().push(arg.clone());
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_tab_specs_single_group() {
        let groups = group_tab_specs(&[
            "DevA".to_string(),
            "SENSOR_A".to_string(),
            "a.log".to_string(),
        ]);
        assert_eq!(groups, vec![vec!["DevA", "SENSOR_A", "a.log"]]);
    }

    #[test]
    fn group_tab_specs_two_groups() {
        let groups = group_tab_specs(&[
            "DevA".to_string(),
            "SENSOR_A".to_string(),
            "a.log".to_string(),
            "DevB".to_string(),
            "SENSOR_B".to_string(),
            "b.log".to_string(),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec!["DevA", "SENSOR_A", "a.log"]);
        assert_eq!(groups[1], vec!["DevB", "SENSOR_B", "b.log"]);
    }

    #[test]
    fn parse_merge_tabs_bare_pane_name() {
        let inputs =
            parse_merge_tabs(&["Dev".to_string(), "SENSOR".to_string(), "a.log".to_string()])
                .unwrap();
        assert_eq!(inputs.tab_configs.len(), 1);
        assert_eq!(inputs.tab_configs[0]["label"], "Dev");
        assert_eq!(inputs.tab_configs[0]["panes"][0], "SENSOR");
        assert_eq!(inputs.source_files["SENSOR"], "a.log");
        assert_eq!(inputs.pane_labels["SENSOR"], "SENSOR"); // label defaults to id
    }

    #[test]
    fn parse_merge_tabs_labeled_pane() {
        let inputs = parse_merge_tabs(&[
            "Dev".to_string(),
            "SENSOR=My Sensor".to_string(),
            "a.log".to_string(),
        ])
        .unwrap();
        assert_eq!(inputs.source_files["SENSOR"], "a.log");
        assert_eq!(inputs.pane_labels["SENSOR"], "My Sensor");
    }

    #[test]
    fn parse_merge_tabs_two_separate_tabs() {
        // The --tab heuristic re-splits after each LABEL PANE FILE group, so
        // two separate tabs each with one pane is the supported multi-tab shape.
        let inputs = parse_merge_tabs(&[
            "Dual".to_string(),
            "A".to_string(),
            "a.log".to_string(),
            "Second".to_string(),
            "B".to_string(),
            "b.log".to_string(),
        ])
        .unwrap();
        assert_eq!(inputs.tab_configs.len(), 2);
        assert_eq!(inputs.tab_configs[0]["label"], "Dual");
        assert_eq!(inputs.tab_configs[1]["label"], "Second");
        assert_eq!(inputs.source_files.len(), 2);
    }

    #[test]
    fn parse_merge_tabs_short_group_is_error() {
        let err = parse_merge_tabs(&["Lonely".to_string()]).unwrap_err();
        assert!(err.to_string().contains("each --tab needs"));
    }

    #[test]
    fn extract_log_data_round_trip() {
        let entries = serde_json::json!([
            { "source_id": "dut", "data": "boot" },
            { "source_id": "host", "data": "hello" },
        ]);
        let html = format!("const logData = {};\n</script>", entries);
        let parsed = extract_log_data(&html).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["source_id"], "dut");
    }

    #[test]
    fn extract_log_data_missing_marker_is_error() {
        let err = extract_log_data("<html>no logs here</html>").unwrap_err();
        assert!(err.to_string().contains("missing logData"));
    }

    #[test]
    fn extract_log_data_malformed_terminator_is_error() {
        let err = extract_log_data("const logData = [1, 2] no semicolon newline").unwrap_err();
        assert!(err.to_string().contains("malformed logData"));
    }

    #[test]
    fn group_entries_by_source_preserves_unknown() {
        let entries: Vec<serde_json::Value> = serde_json::json!([
            { "source_id": "a", "data": "x" },
            { "data": "no source" },
            { "source_id": "a", "data": "y" }
        ])
        .as_array()
        .unwrap()
        .clone();
        let grouped = group_entries_by_source(&entries);
        assert_eq!(grouped["a"], vec!["x".to_string(), "y".to_string()]);
        assert_eq!(grouped["unknown"], vec!["no source".to_string()]);
    }

    #[test]
    fn packet_capture_candidates_are_non_empty() {
        assert!(!packet_capture_library_candidates().is_empty());
    }

    #[test]
    fn count_pcap_sources_only_counts_network_capture_pcap_backends() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("embed-log-doctor-{}-pcap.yml", std::process::id()));
        std::fs::write(
            &path,
            "version: 1\nlogs:\n  dir: logs\nsources:\n  - name: NET_PCAP\n    type: network_capture\n    interface: lo\n    network_backend: pcap\n    udp:\n      ports: [5683]\n  - name: NET_MOCK\n    type: network_capture\n    interface: lo\n    network_backend: mock\n    bpf_filter: udp\n  - name: UDP_TEXT\n    type: udp\n    port: 6000\ntabs:\n  - label: One\n    panes: [NET_PCAP]\n",
        )
        .unwrap();
        let cfg = load_config(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(count_pcap_sources(&cfg), 1);
    }

    #[test]
    fn doctor_report_includes_packet_capture_section() {
        let report = build_doctor_report(None);
        assert!(report.get("packet_capture").is_some());
        assert!(report["packet_capture"].get("feature_enabled").is_some());
        assert!(report["packet_capture"]
            .get("candidate_libraries")
            .is_some());
    }
}
