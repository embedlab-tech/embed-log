use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::models::*;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),
    #[error("invalid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("unsupported config version: {0} (expected 1)")]
    UnsupportedVersion(u32),
    #[error("{0}")]
    Validation(String),
}

/// Load and validate an embed-log config from a YAML file.
pub fn load_config(path: &Path) -> Result<AppConfig, ConfigError> {
    let text =
        std::fs::read_to_string(path).map_err(|_| ConfigError::NotFound(path.to_path_buf()))?;
    let raw: serde_yaml::Value = serde_yaml::from_str(&text)?;

    // Extract version before full deserialization
    let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    if version != 1 {
        return Err(ConfigError::UnsupportedVersion(version));
    }
    reject_removed_fields(&raw)?;

    let mut config: AppConfig = serde_yaml::from_value(raw)?;
    config.version = version;

    validate_config(&mut config, path)?;
    Ok(config)
}

/// Reject compatibility-only fields that were removed from the runtime.
fn reject_removed_fields(raw: &serde_yaml::Value) -> Result<(), ConfigError> {
    if let Some(server) = raw.get("server").and_then(|v| v.as_mapping()) {
        for key in ["open_browser", "ws_ui", "verbose"] {
            if server.contains_key(&serde_yaml::Value::String(key.to_string())) {
                return Err(ConfigError::validation(format!(
                    "server.{key} was removed because it had no effect"
                )));
            }
        }
    }

    let Some(sources) = raw.get("sources").and_then(|v| v.as_sequence()) else {
        return Ok(());
    };
    for (i, source) in sources.iter().enumerate() {
        let Some(map) = source.as_mapping() else {
            continue;
        };
        for key in ["inject_port", "forward_port", "forward_ports"] {
            if map.contains_key(&serde_yaml::Value::String(key.to_string())) {
                return Err(ConfigError::validation(format!(
                    "sources[{i}].{key} was removed; use the /api/v1/control WebSocket API instead"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the parsed config, applying defaults and checking constraints.
fn validate_config(config: &mut AppConfig, config_path: &Path) -> Result<(), ConfigError> {
    let mut source_names = HashSet::new();

    // ── Validate sources ──
    for (i, src) in config.sources.iter_mut().enumerate() {
        let ctx = || format!("sources[{i}]");

        if src.name.is_empty() {
            return Err(ConfigError::validation(format!(
                "{}.name must be non-empty",
                ctx()
            )));
        }
        if !source_names.insert(src.name.clone()) {
            return Err(ConfigError::validation(format!(
                "{}.name duplicate: {:?}",
                ctx(),
                src.name
            )));
        }

        let stype = src.source_type.to_lowercase();
        match stype.as_str() {
            "uart" => {
                // port must be a non-empty string
                let port = yaml_string(&src.port).ok_or_else(|| {
                    ConfigError::validation(format!(
                        "{}.port must be a string for uart sources",
                        ctx()
                    ))
                })?;
                if port.is_empty() {
                    return Err(ConfigError::validation(format!(
                        "{}.port must not be empty",
                        ctx()
                    )));
                }
            }
            "udp" => {
                // port must be an integer
                yaml_u64(&src.port).ok_or_else(|| {
                    ConfigError::validation(format!(
                        "{}.port must be an integer for udp sources",
                        ctx()
                    ))
                })?;
            }
            "file" => {
                let port = yaml_string(&src.port).ok_or_else(|| {
                    ConfigError::validation(format!(
                        "{}.port must be a string for file sources",
                        ctx()
                    ))
                })?;
                if port.is_empty() {
                    return Err(ConfigError::validation(format!(
                        "{}.port must not be empty",
                        ctx()
                    )));
                }
            }
            "network_capture" => {
                if src.interface.is_none() {
                    return Err(ConfigError::validation(format!(
                        "{}.interface is required for network_capture sources",
                        ctx()
                    )));
                }
                let backend = src.network_backend.as_deref().unwrap_or("mock");
                if backend != "mock" && backend != "pcap" {
                    return Err(ConfigError::validation(format!(
                        "{}.network_backend must be 'mock' or 'pcap'",
                        ctx()
                    )));
                }
                if let Some(snaplen) = src.snaplen {
                    if snaplen == 0 {
                        return Err(ConfigError::validation(format!(
                            "{}.snaplen must be > 0",
                            ctx()
                        )));
                    }
                }
                if let Some(udp) = &src.udp {
                    if udp.ports.is_empty()
                        && udp.host.is_none()
                        && udp.src_ips.is_empty()
                        && udp.dst_ips.is_empty()
                    {
                        return Err(ConfigError::validation(format!(
                            "{}.udp must set at least one of: ports, host, src_ips, dst_ips",
                            ctx()
                        )));
                    }
                    if udp.ports.contains(&0) {
                        return Err(ConfigError::validation(format!(
                            "{}.udp.ports must contain valid UDP port numbers",
                            ctx()
                        )));
                    }
                    for (field, values) in [("src_ips", &udp.src_ips), ("dst_ips", &udp.dst_ips)] {
                        for value in values {
                            if value.parse::<std::net::IpAddr>().is_err() {
                                return Err(ConfigError::validation(format!(
                                    "{}.udp.{field} contains invalid IP address {:?}",
                                    ctx(),
                                    value
                                )));
                            }
                        }
                    }
                    if let Some(host) = &udp.host {
                        if host.parse::<std::net::IpAddr>().is_err() {
                            return Err(ConfigError::validation(format!(
                                "{}.udp.host must be a valid IP address",
                                ctx()
                            )));
                        }
                    }
                }
                if backend == "pcap" && src.udp.is_none() && src.bpf_filter.trim().is_empty() {
                    return Err(ConfigError::validation(format!(
                        "{}.network_backend 'pcap' requires either udp.* filters or bpf_filter",
                        ctx()
                    )));
                }
            }
            other => {
                return Err(ConfigError::validation(format!(
                    "{}.type unsupported: {other:?} (use 'uart', 'udp', 'file', or 'network_capture')",
                    ctx()
                )));
            }
        }

        // Validate parser type
        let parser_type = &src.parser.parser_type;
        if !matches!(
            parser_type.as_str(),
            "text" | "cbor-datagram" | "slip-coap" | "zephyr-dict" | "gwl-dict"
        ) {
            return Err(ConfigError::validation(format!(
                "{}.parser.type unsupported: {parser_type:?} (use 'text', 'cbor-datagram', 'slip-coap', 'zephyr-dict', or 'gwl-dict')",
                ctx()
            )));
        }
        if parser_type == "cbor-datagram" && stype != "udp" {
            return Err(ConfigError::validation(format!(
                "{}.parser.type 'cbor-datagram' is only valid for UDP sources (got source type {stype:?})",
                ctx()
            )));
        }
        if parser_type == "slip-coap" && stype != "uart" {
            return Err(ConfigError::validation(format!(
                "{}.parser.type 'slip-coap' is only valid for UART sources (got source type {stype:?})",
                ctx()
            )));
        }
        if parser_type == "zephyr-dict" || parser_type == "gwl-dict" {
            let db = src.parser.database.as_deref().unwrap_or("");
            if db.trim().is_empty() {
                return Err(ConfigError::validation(format!(
                    "{}.parser.database is required for parser.type '{parser_type}'",
                    ctx()
                )));
            }
            let resolved = super::paths::resolve_relative_to_config(config_path, db);
            src.parser.database = Some(resolved.display().to_string());
        }
        if let Some(wire_format) = src.parser.wire_format.as_deref() {
            if !matches!(wire_format, "binary" | "hex") {
                return Err(ConfigError::validation(format!(
                    "{}.parser.wire_format unsupported: {wire_format:?} (use 'binary' or 'hex')",
                    ctx()
                )));
            }
            if parser_type != "zephyr-dict" {
                return Err(ConfigError::validation(format!(
                    "{}.parser.wire_format is only valid for parser.type 'zephyr-dict'",
                    ctx()
                )));
            }
        }
    }

    // ── Validate merges ──
    let mut merge_names: HashSet<String> = HashSet::new();
    for (i, merge) in config.merges.iter().enumerate() {
        let ctx = || format!("merges[{i}]");

        if merge.name.is_empty() {
            return Err(ConfigError::validation(format!(
                "{}.name must be non-empty",
                ctx()
            )));
        }
        if source_names.contains(&merge.name) {
            return Err(ConfigError::validation(format!(
                "{}.name {:?} collides with an existing source name",
                ctx(),
                merge.name
            )));
        }
        if !merge_names.insert(merge.name.clone()) {
            return Err(ConfigError::validation(format!(
                "{}.name duplicate: {:?}",
                ctx(),
                merge.name
            )));
        }
        if merge.of.len() < 2 {
            return Err(ConfigError::validation(format!(
                "{}.of must list at least 2 source names",
                ctx()
            )));
        }
        let mut seen = HashSet::new();
        for name in &merge.of {
            if !source_names.contains(name) {
                return Err(ConfigError::validation(format!(
                    "{}.of references unknown source: {name:?}",
                    ctx()
                )));
            }
            if !seen.insert(name.clone()) {
                return Err(ConfigError::validation(format!(
                    "{}.of lists {name:?} more than once",
                    ctx()
                )));
            }
        }
    }

    // ── Validate tabs ──
    if config.tabs.is_empty() && !config.sources.is_empty() {
        // Tabs are optional; if missing, each source gets its own tab.
        // We'll handle this in the runtime, not here.
    }

    for (i, tab) in config.tabs.iter().enumerate() {
        let ctx = || format!("tabs[{i}]");

        if tab.label.is_empty() {
            return Err(ConfigError::validation(format!(
                "{}.label must be non-empty",
                ctx()
            )));
        }
        if tab.panes.is_empty() || tab.panes.len() > 2 {
            return Err(ConfigError::validation(format!(
                "{}.panes must contain 1 or 2 pane definitions",
                ctx()
            )));
        }

        for (j, pane) in tab.panes.iter().enumerate() {
            let pane_source = pane.source_name();
            if !source_names.contains(pane_source) && !merge_names.contains(pane_source) {
                return Err(ConfigError::validation(format!(
                    "{}.panes[{j}] unknown source: {pane_source:?}",
                    ctx()
                )));
            }
        }
    }

    // ── Validate server verbosity ──
    if let Some(ref v) = config.server.verbosity {
        if v != "quiet" && v != "events" && v != "full" {
            return Err(ConfigError::validation(format!(
                "server.verbosity must be one of: quiet, events, full (got {v:?})"
            )));
        }
    }

    Ok(())
}

impl ConfigError {
    fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
}

/// Extract a string from a serde_yaml::Value.
fn yaml_string(value: &serde_yaml::Value) -> Option<&str> {
    match value {
        serde_yaml::Value::String(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Extract a u64 from a serde_yaml::Value.
fn yaml_u64(value: &serde_yaml::Value) -> Option<u64> {
    match value {
        serde_yaml::Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_config_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("config-samples")
    }

    fn load_sample(name: &str) -> Result<AppConfig, ConfigError> {
        let path = sample_config_dir().join(name);
        load_config(&path)
    }

    #[test]
    fn sample_configs_have_no_legacy_fields() {
        // Verify that sample config YAML files do NOT contain legacy
        // inject_port / forward_port / forward_ports directives (except
        // in comments).  Also verify that each file still parses.
        let dir = sample_config_dir();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().map(|e| e == "yml").unwrap_or(false) {
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, line) in text.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('#') {
                        continue;
                    }
                    assert!(
                        !trimmed.starts_with("inject_port:"),
                        "{}:{} has inject_port: {}",
                        path.display(),
                        i + 1,
                        trimmed
                    );
                    assert!(
                        !trimmed.starts_with("forward_port:"),
                        "{}:{} has forward_port: {}",
                        path.display(),
                        i + 1,
                        trimmed
                    );
                    assert!(
                        !trimmed.starts_with("forward_ports:"),
                        "{}:{} has forward_ports: {}",
                        path.display(),
                        i + 1,
                        trimmed
                    );
                }
                // Also verify the config still parses.
                let result = load_config(&path);
                assert!(
                    result.is_ok(),
                    "failed to parse {}: {}",
                    path.display(),
                    result.unwrap_err()
                );
            }
        }
    }

    #[test]
    fn removed_server_noop_fields_are_rejected() {
        let yaml = r#"
version: 1
server:
  open_browser: false
sources:
  - name: DUT
    type: udp
    port: 6000
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("noop-server-field-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("server.open_browser was removed"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn legacy_inject_forward_fields_are_rejected() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    inject_port: 5001
    forward_port: 5002
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("legacy-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("inject_port was removed"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn merge_config_parses_and_pane_can_reference_it() {
        let yaml = r#"
version: 1
sources:
  - name: MCU_LINK_TX
    type: uart
    port: /dev/ttyUSB0
  - name: MCU_LINK_RX
    type: uart
    port: /dev/ttyUSB1
merges:
  - name: MCU_LINK
    label: MCU Link
    of: [MCU_LINK_TX, MCU_LINK_RX]
tabs:
  - label: T
    panes: [MCU_LINK]
"#;
        let path = std::env::temp_dir().join("merge-valid-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.merges.len(), 1);
        assert_eq!(cfg.merges[0].of, vec!["MCU_LINK_TX", "MCU_LINK_RX"]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn merge_name_colliding_with_source_is_rejected() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: udp
    port: 6000
  - name: OTHER
    type: udp
    port: 6001
merges:
  - name: DUT
    of: [DUT, OTHER]
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("merge-collide-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("collides with an existing source name"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn merge_of_unknown_source_is_rejected() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: udp
    port: 6000
merges:
  - name: MERGED
    of: [DUT, GHOST]
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("merge-unknown-source-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("unknown source"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn merge_of_with_fewer_than_two_sources_is_rejected() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: udp
    port: 6000
merges:
  - name: MERGED
    of: [DUT]
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("merge-too-few-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("at least 2 source names"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn zephyr_dict_parser_parses_with_database_path() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    parser:
      type: zephyr-dict
      database: /tmp/database.json
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("zephyr-dict-valid-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.sources[0].parser.parser_type, "zephyr-dict");
        assert_eq!(
            cfg.sources[0].parser.database.as_deref(),
            Some("/tmp/database.json")
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn zephyr_dict_parser_without_database_is_rejected() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    parser:
      type: zephyr-dict
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("zephyr-dict-missing-db-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("parser.database is required"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn control_api_defaults_to_true() {
        let yaml = r#"
version: 1
sources:
  - name: DUT
    type: udp
    port: 6000
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("control-api-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert!(cfg.server.control_api, "control_api should default to true");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn control_api_can_be_disabled() {
        let yaml = r#"
version: 1
server:
  control_api: false
sources:
  - name: DUT
    type: udp
    port: 6000
tabs:
  - label: T
    panes: [DUT]
"#;
        let path = std::env::temp_dir().join("control-api-disabled.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert!(!cfg.server.control_api, "control_api should be false");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_double_uart_udp_two_tabs() {
        let cfg = load_sample("double_uart_udp_two_tabs.yml").unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.sources.len(), 3);
        assert_eq!(cfg.tabs.len(), 2);
        assert_eq!(cfg.server.ws_port, 8080);
        assert_eq!(cfg.baudrate, 115200);
    }

    #[test]
    fn parse_single_uart_single_tab() {
        let cfg = load_sample("single_uart_single_tab.yml").unwrap();
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].source_type, "uart");
        assert_eq!(cfg.tabs.len(), 1);
    }

    #[test]
    fn parse_single_file_single_tab() {
        let cfg = load_sample("single_file_single_tab.yml").unwrap();
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].source_type, "file");
    }

    #[test]
    fn parse_three_udp_cbor_two_tabs() {
        let cfg = load_sample("three_udp_cbor_two_tabs.yml").unwrap();
        assert_eq!(cfg.sources.len(), 3);
        // First two sources use cbor-datagram
        assert_eq!(cfg.sources[0].parser.parser_type, "cbor-datagram");
        assert_eq!(cfg.sources[1].parser.parser_type, "cbor-datagram");
        // Third uses default text
        assert_eq!(cfg.sources[2].parser.parser_type, "text");
    }

    #[test]
    fn parse_reference_full_annotated() {
        let cfg = load_sample("reference_full_annotated.yml").unwrap();
        assert_eq!(cfg.sources.len(), 4);
        assert_eq!(cfg.tabs.len(), 3);
        assert!(cfg.frontend_plugins.contains_key("hex-coap"));
        assert_eq!(cfg.server.default_light_theme.as_deref(), Some("whitesand"));
        assert_eq!(cfg.server.default_dark_theme.as_deref(), Some("one-dark"));
    }

    #[test]
    fn parse_single_network_single_tab() {
        let cfg = load_sample("single_network_single_tab.yml").unwrap();
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].source_type, "network_capture");
        assert_eq!(cfg.sources[0].network_backend.as_deref(), Some("mock"));
    }

    #[test]
    fn reject_unknown_source_type() {
        let yaml = r#"
version: 1
sources:
  - name: BAD
    type: bluetooth
    port: "hci0"
tabs:
  - label: T
    panes: [BAD]
"#;
        let result: Result<AppConfig, _> = serde_yaml::from_str(yaml);
        // serde will parse it, but validation should catch it
        if let Ok(mut cfg) = result {
            let err = validate_config(&mut cfg, Path::new("test")).unwrap_err();
            assert!(err.to_string().contains("unsupported"), "got: {err}");
        }
    }

    #[test]
    fn reject_cbor_on_non_udp() {
        let yaml = r#"
version: 1
sources:
  - name: UART_A
    type: uart
    port: "/dev/ttyUSB0"
    parser:
      type: cbor-datagram
tabs:
  - label: T
    panes: [UART_A]
"#;
        let mut cfg: AppConfig = serde_yaml::from_str(yaml).unwrap();
        let err = validate_config(&mut cfg, Path::new("test")).unwrap_err();
        assert!(err.to_string().contains("cbor-datagram"), "got: {err}");
    }

    #[test]
    fn reject_tab_with_unknown_source() {
        let yaml = r#"
version: 1
sources:
  - name: A
    type: udp
    port: 6000
tabs:
  - label: T
    panes: [A, NONEXISTENT]
"#;
        let mut cfg: AppConfig = serde_yaml::from_str(yaml).unwrap();
        let err = validate_config(&mut cfg, Path::new("test")).unwrap_err();
        assert!(err.to_string().contains("unknown source"), "got: {err}");
    }

    #[test]
    fn reject_too_many_panes() {
        let yaml = r#"
version: 1
sources:
  - name: A
    type: udp
    port: 6000
  - name: B
    type: udp
    port: 6001
  - name: C
    type: udp
    port: 6002
tabs:
  - label: T
    panes: [A, B, C]
"#;
        let mut cfg: AppConfig = serde_yaml::from_str(yaml).unwrap();
        let err = validate_config(&mut cfg, Path::new("test")).unwrap_err();
        assert!(err.to_string().contains("1 or 2 pane"), "got: {err}");
    }

    #[test]
    fn parse_all_sample_configs() {
        let dir = sample_config_dir();
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "yml"))
            .collect();

        assert!(
            !entries.is_empty(),
            "no sample configs found in {}",
            dir.display()
        );

        for entry in entries {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy();
            let result = load_config(&path);
            assert!(
                result.is_ok(),
                "failed to parse {name}: {}",
                result.unwrap_err()
            );
        }
    }

    #[test]
    fn missing_config_file() {
        let err = load_config(Path::new("/nonexistent/config.yml")).unwrap_err();
        assert!(matches!(err, ConfigError::NotFound(_)));
    }

    #[test]
    fn invalid_yaml() {
        let dir = std::env::temp_dir().join("embed-log-test-invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.yml");
        std::fs::write(&path, "{{invalid yaml: [").unwrap();
        let err = load_config(&path).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidYaml(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Write a config to a uniquely-named temp file and load it.
    fn load_inline(test: &str, body: &str) -> Result<AppConfig, ConfigError> {
        let path = std::env::temp_dir().join(format!(
            "embed-log-loader-{}-{test}.yml",
            std::process::id()
        ));
        std::fs::write(&path, body).unwrap();
        let result = load_config(&path);
        std::fs::remove_file(&path).ok();
        result
    }

    fn network_config(backend_line: &str) -> String {
        format!(
            "version: 1\n\
             logs:\n  dir: logs\n\
             sources:\n  - name: NET\n    type: network_capture\n    interface: lo\n{backend_line}\
             tabs:\n  - label: Network\n    panes: [NET]\n"
        )
    }

    #[test]
    fn network_capture_accepts_mock_backend() {
        let cfg = load_inline("net-mock", &network_config("    network_backend: mock\n")).unwrap();
        assert_eq!(cfg.sources[0].network_backend.as_deref(), Some("mock"));
    }

    #[test]
    fn network_capture_defaults_to_mock_when_backend_omitted() {
        // No network_backend line: must validate (default is mock, not scapy).
        load_inline("net-default", &network_config("")).unwrap();
    }

    #[test]
    fn network_capture_accepts_pcap_backend_with_udp_ports() {
        let cfg = load_inline(
            "net-pcap",
            &network_config(
                "    network_backend: pcap\n    udp:\n      ports: [5683, 5684]\n    snaplen: 256\n",
            ),
        )
        .unwrap();
        assert_eq!(cfg.sources[0].network_backend.as_deref(), Some("pcap"));
        assert_eq!(cfg.sources[0].udp.as_ref().unwrap().ports, vec![5683, 5684]);
        assert_eq!(cfg.sources[0].snaplen, Some(256));
    }

    #[test]
    fn network_capture_rejects_unknown_backend() {
        let err =
            load_inline("net-scapy", &network_config("    network_backend: scapy\n")).unwrap_err();
        assert!(
            matches!(err, ConfigError::Validation(msg) if msg.contains("must be 'mock' or 'pcap'")),
            "expected backend validation error"
        );
    }

    #[test]
    fn network_capture_rejects_pcap_without_filter() {
        let err = load_inline(
            "net-pcap-empty",
            &network_config("    network_backend: pcap\n"),
        )
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::Validation(msg) if msg.contains("requires either udp.* filters or bpf_filter")),
            "expected pcap filter validation error"
        );
    }
}
