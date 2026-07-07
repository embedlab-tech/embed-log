use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::TimestampMode;

/// Parser configuration attached to a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserConfig {
    #[serde(rename = "type", default = "ParserConfig::default_type")]
    pub parser_type: String,
    /// Path to a Zephyr dictionary-logging `database.json`. Required when
    /// `type: zephyr-dict` or `gwl-dict`; ignored otherwise.
    #[serde(default)]
    pub database: Option<String>,
    /// Wire format for dictionary packets: `binary` (default) or `hex`.
    /// GWL firmware uses ASCII hex with optional `##ZLOGV1##` separators.
    #[serde(default)]
    pub wire_format: Option<String>,
}

impl ParserConfig {
    fn default_type() -> String {
        "text".to_string()
    }
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            parser_type: "text".to_string(),
            database: None,
            wire_format: None,
        }
    }
}

/// A single log source definition from the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub source_type: String, // "uart", "udp", "file", "network_capture"
    #[serde(default)]
    pub port: serde_yaml::Value, // string for uart/file, int for udp
    #[serde(default)]
    pub parser: ParserConfig,
    pub baudrate: Option<u32>,
    // label
    pub label: Option<String>,
    // network_capture fields
    pub interface: Option<String>,
    #[serde(default)]
    pub bpf_filter: String,
    pub network_backend: Option<String>,
    pub mock_interval: Option<f64>,
    #[serde(default)]
    pub udp: Option<NetworkUdpCaptureConfig>,
    pub snaplen: Option<u32>,
    pub promisc: Option<bool>,
    // pcap sub-config
    pub pcap: Option<PcapConfig>,
    // payload sub-config
    pub payload: Option<PayloadConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkUdpCaptureConfig {
    #[serde(default)]
    pub ports: Vec<u16>,
    pub host: Option<String>,
    #[serde(default)]
    pub src_ips: Vec<String>,
    #[serde(default)]
    pub dst_ips: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PcapConfig {
    #[serde(default)]
    pub enabled: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadConfig {
    #[serde(default = "PayloadConfig::default_include_preview")]
    pub include_preview: bool,
    #[serde(default = "PayloadConfig::default_max_preview_bytes")]
    pub max_preview_bytes: u32,
}

impl Default for PayloadConfig {
    fn default() -> Self {
        Self {
            include_preview: Self::default_include_preview(),
            max_preview_bytes: Self::default_max_preview_bytes(),
        }
    }
}

impl PayloadConfig {
    fn default_include_preview() -> bool {
        true
    }
    fn default_max_preview_bytes() -> u32 {
        128
    }
}

/// A compiled event rule loaded from a companion .events.yml file.
#[derive(Debug, Clone)]
pub struct EventRule {
    /// Unique name within its source.
    pub name: String,
    /// Raw regex pattern string (as written in the YAML).
    pub pattern: String,
    /// Severity label: "info", "warn", "error", or "fatal".
    pub severity: String,
    /// Compiled regex for fast matching.
    pub regex: regex::Regex,
}

/// Frontend plugin definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendPluginDefinition {
    pub builtin: Option<String>,
    pub path: Option<String>,
}

/// Per-pane plugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanePluginConfig {
    pub name: String,
    #[serde(default)]
    pub options: HashMap<String, serde_yaml::Value>,
}

impl PanePluginConfig {
    /// Return a stable signature for comparing plugin sets across tabs.
    pub fn signature(&self) -> String {
        serde_json::to_string(&self.options).unwrap_or_default()
    }
}

/// A pane within a tab — references a source and optional plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PaneConfig {
    /// Simple form: just a source name string.
    Simple(String),
    /// Full form: source + plugins.
    Detailed {
        source: String,
        #[serde(default)]
        plugins: Vec<PanePluginEntry>,
    },
}

impl PaneConfig {
    pub fn source_name(&self) -> &str {
        match self {
            Self::Simple(s) => s,
            Self::Detailed { source, .. } => source,
        }
    }
}

/// A plugin entry in a pane config — either a bare name or a {name, options} dict.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PanePluginEntry {
    Name(String),
    Detailed(PanePluginConfig),
}

/// A tab in the UI, containing 1–2 panes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabConfig {
    pub label: String,
    pub panes: Vec<PaneConfig>,
}

/// A virtual pseudo-source that interleaves other sources' entries into one
/// stream, each line tagged with its origin source's label. Referenced from
/// `tabs[].panes` exactly like a real source name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConfig {
    pub name: String,
    pub label: Option<String>,
    pub of: Vec<String>,
}

/// Server settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "ServerConfig::default_host")]
    pub host: String,
    #[serde(default = "ServerConfig::default_ws_port")]
    pub ws_port: u16,
    #[serde(default = "ServerConfig::default_app_name")]
    pub app_name: String,
    pub verbosity: Option<String>,
    pub job_id: Option<String>,
    pub default_light_theme: Option<String>,
    pub default_dark_theme: Option<String>,
    #[serde(default)]
    pub timestamp_mode: TimestampMode,
    #[serde(default = "ServerConfig::default_queue_size")]
    pub queue_size: usize,
    #[serde(default = "ServerConfig::default_control_api")]
    pub control_api: bool,
}

impl ServerConfig {
    fn default_host() -> String {
        "127.0.0.1".to_string()
    }
    fn default_ws_port() -> u16 {
        8080
    }
    fn default_app_name() -> String {
        "embed-log".to_string()
    }
    fn default_queue_size() -> usize {
        20_000
    }
    fn default_control_api() -> bool {
        true
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: Self::default_host(),
            ws_port: Self::default_ws_port(),
            app_name: Self::default_app_name(),
            verbosity: None,
            job_id: None,
            default_light_theme: None,
            default_dark_theme: None,
            timestamp_mode: TimestampMode::default(),
            queue_size: Self::default_queue_size(),
            control_api: Self::default_control_api(),
        }
    }
}

/// Log output directory settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsConfig {
    #[serde(default = "LogsConfig::default_dir")]
    pub dir: String,
}

impl LogsConfig {
    fn default_dir() -> String {
        "logs/".to_string()
    }
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            dir: Self::default_dir(),
        }
    }
}

/// Top-level application configuration parsed from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    #[serde(default)]
    pub tabs: Vec<TabConfig>,
    #[serde(default)]
    pub merges: Vec<MergeConfig>,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub logs: LogsConfig,
    #[serde(default = "AppConfig::default_baudrate")]
    pub baudrate: u32,
    #[serde(default)]
    pub frontend_plugins: HashMap<String, FrontendPluginDefinition>,
}

impl AppConfig {
    fn default_baudrate() -> u32 {
        115200
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            sources: Vec::new(),
            tabs: Vec::new(),
            merges: Vec::new(),
            server: ServerConfig::default(),
            logs: LogsConfig::default(),
            baudrate: Self::default_baudrate(),
            frontend_plugins: HashMap::new(),
        }
    }
}
