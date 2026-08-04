use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use super::models::*;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),
    #[error("invalid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("unsupported config version: {0} (expected 1 or 2)")]
    UnsupportedVersion(u32),
    #[error("{0}")]
    Validation(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2Config {
    version: u32,
    #[serde(default)]
    server: V2ServerConfig,
    #[serde(default)]
    logs: LogsConfig,
    #[serde(default)]
    sources: serde_yaml::Mapping,
    #[serde(default)]
    ui: Option<V2UiConfig>,
    #[serde(default)]
    merges: Vec<MergeConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2ServerConfig {
    #[serde(default)]
    listen: Option<String>,
    #[serde(default)]
    app_name: Option<String>,
    #[serde(default)]
    verbosity: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    default_light_theme: Option<String>,
    #[serde(default)]
    default_dark_theme: Option<String>,
    #[serde(default)]
    timestamp_mode: crate::models::TimestampMode,
    #[serde(default)]
    queue_size: Option<usize>,
    #[serde(default)]
    control_api: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2SourceConfig {
    #[serde(rename = "type")]
    source_type: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    port: Option<serde_yaml::Value>,
    #[serde(default)]
    baud: Option<u32>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    parser: ParserConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2UiConfig {
    #[serde(default)]
    tabs: Vec<V2TabConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2TabConfig {
    title: String,
    sources: Vec<String>,
}

/// Load and validate an embed-log config from a YAML file.
///
/// Version 2 is the canonical format. Version 1 remains readable during the
/// migration so existing projects can move independently.
pub fn load_config(path: &Path) -> Result<AppConfig, ConfigError> {
    let text =
        std::fs::read_to_string(path).map_err(|_| ConfigError::NotFound(path.to_path_buf()))?;
    let raw: serde_yaml::Value = serde_yaml::from_str(&text)?;

    let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    if !matches!(version, 1 | 2) {
        return Err(ConfigError::UnsupportedVersion(version));
    }

    let mut config = if version == 2 {
        parse_v2_config(raw)?
    } else {
        reject_removed_fields(&raw)?;
        let mut config: AppConfig = serde_yaml::from_value(raw)?;
        config.version = 1;
        config
    };

    validate_config(&mut config, path)?;
    Ok(config)
}

fn parse_v2_config(raw: serde_yaml::Value) -> Result<AppConfig, ConfigError> {
    let v2: V2Config = serde_yaml::from_value(raw)?;
    if v2.version != 2 {
        return Err(ConfigError::UnsupportedVersion(v2.version));
    }

    let mut server = ServerConfig::default();
    if let Some(listen) = v2.server.listen.as_deref() {
        let (host, port) = parse_listen(listen)?;
        server.host = host;
        server.ws_port = port;
    }
    if let Some(value) = v2.server.app_name {
        server.app_name = value;
    }
    server.verbosity = v2.server.verbosity;
    server.job_id = v2.server.job_id;
    server.default_light_theme = v2.server.default_light_theme;
    server.default_dark_theme = v2.server.default_dark_theme;
    server.timestamp_mode = v2.server.timestamp_mode;
    if let Some(value) = v2.server.queue_size {
        server.queue_size = value;
    }
    if let Some(value) = v2.server.control_api {
        server.control_api = value;
    }

    let mut sources = Vec::with_capacity(v2.sources.len());
    for (name, raw_source) in v2.sources {
        let name = name
            .as_str()
            .ok_or_else(|| ConfigError::validation("version 2 source names must be strings"))?;
        let source: V2SourceConfig = serde_yaml::from_value(raw_source)?;
        let has_path = source.path.is_some();
        let has_port = source.port.is_some();
        if has_path && source.source_type == "udp" {
            return Err(ConfigError::validation(format!(
                "sources.{name}.path is not valid for udp sources; use port"
            )));
        }
        if has_port && source.source_type != "udp" {
            return Err(ConfigError::validation(format!(
                "sources.{name}.port is only valid for udp sources; use path"
            )));
        }
        if source.baud.is_some() && source.source_type != "uart" {
            return Err(ConfigError::validation(format!(
                "sources.{name}.baud is only valid for uart sources"
            )));
        }
        let port = match source.source_type.as_str() {
            "uart" | "file" => serde_yaml::Value::String(source.path.clone().ok_or_else(|| {
                ConfigError::validation(format!(
                    "sources.{name}.path is required for {} sources",
                    source.source_type
                ))
            })?),
            "udp" => {
                let udp_port = source.port.as_ref().and_then(yaml_u64).ok_or_else(|| {
                    ConfigError::validation(format!(
                        "sources.{name}.port must be an integer from 0 to 65535 for udp sources"
                    ))
                })?;
                if udp_port > u16::MAX as u64 {
                    return Err(ConfigError::validation(format!(
                        "sources.{name}.port must be an integer from 0 to 65535 for udp sources"
                    )));
                }
                serde_yaml::to_value(udp_port)?
            }
            _ => serde_yaml::Value::Null,
        };
        sources.push(SourceConfig {
            name: name.to_string(),
            source_type: source.source_type,
            port,
            parser: source.parser,
            baudrate: source.baud,
            label: source.label,
        });
    }

    let tabs = match v2.ui {
        Some(ui) => ui
            .tabs
            .into_iter()
            .map(|tab| TabConfig {
                label: tab.title,
                panes: tab.sources.into_iter().map(PaneConfig::Simple).collect(),
            })
            .collect(),
        None => sources
            .iter()
            .map(|source| TabConfig {
                label: source.label.clone().unwrap_or_else(|| source.name.clone()),
                panes: vec![PaneConfig::Simple(source.name.clone())],
            })
            .collect(),
    };

    Ok(AppConfig {
        version: 2,
        sources,
        tabs,
        merges: v2.merges,
        server,
        logs: v2.logs,
        baudrate: 115_200,
        frontend_plugins: Default::default(),
    })
}

/// Serialize the normalized runtime configuration using canonical YAML v2.
pub fn config_to_v2_yaml(config: &AppConfig) -> anyhow::Result<String> {
    let mut source_map = serde_json::Map::new();
    for source in &config.sources {
        let mut value = serde_json::Map::new();
        value.insert("type".to_string(), serde_json::json!(source.source_type));
        match source.source_type.as_str() {
            "udp" => {
                value.insert(
                    "port".to_string(),
                    serde_json::json!(yaml_u64(&source.port).unwrap_or_default()),
                );
            }
            _ => {
                value.insert(
                    "path".to_string(),
                    serde_json::json!(yaml_string(&source.port).unwrap_or_default()),
                );
            }
        }
        if let Some(baud) = source.baudrate {
            value.insert("baud".to_string(), serde_json::json!(baud));
        }
        if let Some(label) = &source.label {
            value.insert("label".to_string(), serde_json::json!(label));
        }
        if source.parser != ParserConfig::default() {
            value.insert("parser".to_string(), serde_json::to_value(&source.parser)?);
        }
        source_map.insert(source.name.clone(), serde_json::Value::Object(value));
    }

    let tabs: Vec<_> = config
        .tabs
        .iter()
        .map(|tab| {
            serde_json::json!({
                "title": tab.label,
                "sources": tab.panes.iter().map(PaneConfig::source_name).collect::<Vec<_>>(),
            })
        })
        .collect();
    let mut server = serde_json::Map::new();
    server.insert(
        "listen".to_string(),
        serde_json::json!(format!("{}:{}", config.server.host, config.server.ws_port)),
    );
    server.insert(
        "app_name".to_string(),
        serde_json::json!(config.server.app_name),
    );
    server.insert(
        "timestamp_mode".to_string(),
        serde_json::json!(config.server.timestamp_mode),
    );
    server.insert(
        "queue_size".to_string(),
        serde_json::json!(config.server.queue_size),
    );
    server.insert(
        "control_api".to_string(),
        serde_json::json!(config.server.control_api),
    );
    for (key, value) in [
        ("verbosity", config.server.verbosity.as_ref()),
        ("job_id", config.server.job_id.as_ref()),
        (
            "default_light_theme",
            config.server.default_light_theme.as_ref(),
        ),
        (
            "default_dark_theme",
            config.server.default_dark_theme.as_ref(),
        ),
    ] {
        if let Some(value) = value {
            server.insert(key.to_string(), serde_json::json!(value));
        }
    }

    let mut root = serde_json::json!({
        "version": 2,
        "server": server,
        "logs": { "dir": config.logs.dir },
        "sources": source_map,
        "ui": { "tabs": tabs },
    });
    if !config.merges.is_empty() {
        root["merges"] = serde_json::to_value(&config.merges)?;
    }
    Ok(serde_yaml::to_string(&root)?)
}

fn parse_listen(listen: &str) -> Result<(String, u16), ConfigError> {
    let (host, port) = listen.rsplit_once(':').ok_or_else(|| {
        ConfigError::validation(format!("server.listen must be HOST:PORT (got {listen:?})"))
    })?;
    if host.trim().is_empty() {
        return Err(ConfigError::validation(
            "server.listen host must not be empty",
        ));
    }
    let port = port.parse::<u16>().map_err(|_| {
        ConfigError::validation(format!(
            "server.listen port must be an integer from 1 to 65535 (got {port:?})"
        ))
    })?;
    if port == 0 {
        return Err(ConfigError::validation(
            "server.listen port must be an integer from 1 to 65535 (got 0)",
        ));
    }
    Ok((host.to_string(), port))
}

/// Reject compatibility-only fields that were removed from the runtime.
fn reject_removed_fields(raw: &serde_yaml::Value) -> Result<(), ConfigError> {
    if let Some(server) = raw.get("server").and_then(|v| v.as_mapping()) {
        for key in ["open_browser", "ws_ui", "verbose"] {
            if server.contains_key(serde_yaml::Value::String(key.to_string())) {
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
            if map.contains_key(serde_yaml::Value::String(key.to_string())) {
                return Err(ConfigError::validation(format!(
                    "sources[{i}].{key} was removed; use the /api/v1/control WebSocket API instead"
                )));
            }
        }
        if map
            .get(serde_yaml::Value::String("type".to_string()))
            .and_then(serde_yaml::Value::as_str)
            == Some("network_capture")
        {
            return Err(ConfigError::validation(format!(
                "sources[{i}].type 'network_capture' was removed; use an explicit UDP source instead"
            )));
        }
        if map
            .get(serde_yaml::Value::String("parser".to_string()))
            .and_then(serde_yaml::Value::as_mapping)
            .and_then(|parser| {
                parser
                    .get(serde_yaml::Value::String("type".to_string()))
                    .and_then(serde_yaml::Value::as_str)
            })
            == Some("cbor-datagram")
        {
            return Err(ConfigError::validation(format!(
                "sources[{i}].parser.type 'cbor-datagram' was removed; use text or a retained protocol parser"
            )));
        }
        for key in [
            "interface",
            "bpf_filter",
            "network_backend",
            "mock_interval",
            "udp",
            "snaplen",
            "promisc",
            "pcap",
            "payload",
        ] {
            if map.contains_key(serde_yaml::Value::String(key.to_string())) {
                return Err(ConfigError::validation(format!(
                    "sources[{i}].{key} was removed with network capture support"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the parsed config, applying defaults and checking constraints.
fn validate_config(config: &mut AppConfig, config_path: &Path) -> Result<(), ConfigError> {
    for (name, plugin) in &config.frontend_plugins {
        if plugin.builtin.as_deref() == Some("hex-coap") {
            return Err(ConfigError::validation(format!(
                "frontend_plugins.{name}.builtin 'hex-coap' was removed; attach parser.type 'hex-coap' to the source instead"
            )));
        }
    }

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
            other => {
                return Err(ConfigError::validation(format!(
                    "{}.type unsupported: {other:?} (use 'uart', 'udp', or 'file')",
                    ctx()
                )));
            }
        }

        // Validate parser type
        let parser_type = &src.parser.parser_type;
        if !matches!(
            parser_type.as_str(),
            "text" | "hex-coap" | "slip-coap" | "zephyr-dict"
        ) {
            return Err(ConfigError::validation(format!(
                "{}.parser.type unsupported: {parser_type:?} (use 'text', 'hex-coap', 'slip-coap', or 'zephyr-dict')",
                ctx()
            )));
        }
        if parser_type == "slip-coap" && stype != "uart" {
            return Err(ConfigError::validation(format!(
                "{}.parser.type 'slip-coap' is only valid for UART sources (got source type {stype:?})",
                ctx()
            )));
        }
        if parser_type == "zephyr-dict" {
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
    fn version_two_mapping_normalizes_sources_listen_and_ui() {
        let yaml = r#"
version: 2
server:
  listen: 0.0.0.0:19090
logs:
  dir: artifacts
sources:
  DUT:
    type: uart
    path: /dev/ttyUSB0
    baud: 921600
  HOST:
    type: udp
    port: 16000
ui:
  tabs:
    - title: Device
      sources: [DUT, HOST]
"#;
        let path = std::env::temp_dir().join("embed-log-v2-normalization-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.version, 2);
        assert_eq!(cfg.server.host, "0.0.0.0");
        assert_eq!(cfg.server.ws_port, 19090);
        assert_eq!(cfg.sources[0].name, "DUT");
        assert_eq!(cfg.sources[0].baudrate, Some(921_600));
        assert_eq!(cfg.sources[1].port.as_u64(), Some(16_000));
        assert_eq!(cfg.tabs[0].label, "Device");
        assert_eq!(cfg.tabs[0].panes.len(), 2);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn version_two_defaults_to_18080_and_generates_tabs() {
        let yaml = r#"
version: 2
sources:
  LOG:
    type: file
    path: device.log
"#;
        let path = std::env::temp_dir().join("embed-log-v2-defaults-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.server.host, "127.0.0.1");
        assert_eq!(cfg.server.ws_port, 18080);
        assert_eq!(cfg.tabs.len(), 1);
        assert_eq!(cfg.tabs[0].label, "LOG");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn version_two_rejects_legacy_source_fields() {
        let yaml = r#"
version: 2
server:
  listen: 127.0.0.1:18080
sources:
  DUT:
    type: uart
    port: /dev/ttyUSB0
"#;
        let path = std::env::temp_dir().join("embed-log-v2-legacy-fields-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(
            err.contains("port is only valid for udp sources; use path"),
            "{err}"
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn version_two_rejects_legacy_server_fields() {
        let yaml = r#"
version: 2
server:
  host: 127.0.0.1
sources: {}
"#;
        let path = std::env::temp_dir().join("embed-log-v2-legacy-server-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("unknown field `host`"), "{err}");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn version_two_rejects_invalid_listen_endpoint() {
        let yaml = "version: 2\nserver:\n  listen: localhost\nsources: {}\n";
        let path = std::env::temp_dir().join("embed-log-v2-listen-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("server.listen must be HOST:PORT"), "{err}");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn version_two_round_trip_serializer_uses_canonical_keys() {
        let mut cfg = AppConfig::default();
        cfg.sources.push(SourceConfig {
            name: "DUT".to_string(),
            source_type: "uart".to_string(),
            port: serde_yaml::Value::String("/dev/ttyUSB0".to_string()),
            parser: ParserConfig::default(),
            baudrate: Some(115_200),
            label: None,
        });
        cfg.tabs.push(TabConfig {
            label: "DUT".to_string(),
            panes: vec![PaneConfig::Simple("DUT".to_string())],
        });
        let yaml = config_to_v2_yaml(&cfg).unwrap();
        assert!(yaml.contains("version: 2"));
        assert!(yaml.contains("listen: 127.0.0.1:18080"));
        assert!(yaml.contains("path: /dev/ttyUSB0"));
        assert!(yaml.contains("baud: 115200"));
        assert!(!yaml.contains("baudrate:"));
        assert!(!yaml.contains("ws_port:"));

        let path = std::env::temp_dir().join("embed-log-v2-round-trip-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.sources[0].name, "DUT");
        assert_eq!(loaded.sources[0].baudrate, Some(115_200));
        assert_eq!(loaded.server.ws_port, 18080);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn sample_configs_use_version_two_without_legacy_fields() {
        // Verify maintained samples use canonical v2 and do not contain
        // removed inject/forward directives (except in comments).
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
                let config = load_config(&path)
                    .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
                assert_eq!(config.version, 2, "{} is not config v2", path.display());
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
        assert!(
            err.contains("collides with an existing source name"),
            "{err}"
        );
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
    fn removed_frontend_hex_coap_plugin_has_migration_error() {
        let yaml = r#"
version: 1
frontend_plugins:
  coap:
    builtin: hex-coap
sources:
  - name: COAP
    type: udp
    port: 6000
tabs:
  - label: CoAP
    panes: [COAP]
"#;
        let path = std::env::temp_dir().join("removed-frontend-hex-coap.yml");
        std::fs::write(&path, yaml).unwrap();
        let error = load_config(&path).unwrap_err().to_string();
        assert!(error.contains("parser.type 'hex-coap'"), "{error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn hex_coap_parser_is_valid_for_textual_sources() {
        let yaml = r#"
version: 2
sources:
  COAP:
    type: file
    path: capture.log
    parser:
      type: hex-coap
"#;
        let path = std::env::temp_dir().join("hex-coap-valid-test.yml");
        std::fs::write(&path, yaml).unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.sources[0].parser.parser_type, "hex-coap");
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

        let expected_database = if cfg!(windows) {
            "C:/tmp/database.json"
        } else {
            "/tmp/database.json"
        };

        assert_eq!(
            cfg.sources[0].parser.database.as_deref(),
            Some(expected_database)
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
        assert_eq!(cfg.version, 2);
        assert_eq!(cfg.sources.len(), 3);
        assert_eq!(cfg.tabs.len(), 2);
        assert_eq!(cfg.server.ws_port, 18080);
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
    fn parse_reference_full_annotated() {
        let cfg = load_sample("reference_full_annotated.yml").unwrap();
        assert_eq!(cfg.sources.len(), 2);
        assert_eq!(cfg.tabs.len(), 1);
        assert!(cfg.frontend_plugins.is_empty());
        assert_eq!(cfg.server.default_light_theme.as_deref(), Some("whitesand"));
        assert_eq!(cfg.server.default_dark_theme.as_deref(), Some("one-dark"));
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

    #[test]
    fn removed_cbor_parser_has_actionable_error() {
        let err = load_inline(
            "removed-cbor-parser",
            "version: 1\nsources:\n  - name: SENSOR\n    type: udp\n    port: 6000\n    parser:\n      type: cbor-datagram\ntabs:\n  - label: Sensor\n    panes: [SENSOR]\n",
        )
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::Validation(msg) if msg.contains("'cbor-datagram' was removed") && msg.contains("retained protocol parser")),
            "expected removed CBOR parser error"
        );
    }

    #[test]
    fn removed_network_capture_source_has_actionable_error() {
        let err = load_inline(
            "removed-network-capture",
            "version: 1\nsources:\n  - name: NET\n    type: network_capture\n    interface: lo\ntabs:\n  - label: Network\n    panes: [NET]\n",
        )
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::Validation(msg) if msg.contains("'network_capture' was removed") && msg.contains("explicit UDP source")),
            "expected removed network capture error"
        );
    }
}
