use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::models::TimestampMode;

/// One log level of a level profile, e.g. `err` / "Error".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelDefinition {
    /// Stable identifier stored in records (`level` field) and used by the UI filter.
    pub id: String,
    /// Display name; defaults to `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Regex matched against the ANSI-stripped line. Required unless the
    /// profile uses a single `pattern` with a named `level` group.
    #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
    pub match_regex: Option<String>,
    /// CSS color (`#rgb`, `#rrggbb`, a color name or `var(--theme-variable)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Whether the level is shown before the viewer changes the filter.
    #[serde(default = "LevelDefinition::default_visible")]
    pub default_visible: bool,
}

impl LevelDefinition {
    fn default_visible() -> bool {
        true
    }
}

/// What happens to a line that matches no level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnmatchedLevel {
    /// The line has no level ("No level" in the UI).
    #[default]
    None,
    /// The line takes the level of the previous leveled line of the same source.
    Inherit,
}

/// How to detect log levels in a source's lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelProfile {
    /// Ordered by importance; with per-level `match` the first match wins.
    pub levels: Vec<LevelDefinition>,
    #[serde(default)]
    pub case_insensitive: bool,
    /// Alternative to per-level `match`: one regex with a named group `level`
    /// whose text (or its alias) is a level `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub unmatched: UnmatchedLevel,
    /// Lines matching this regex always take the previous line's level
    /// (multi-line messages, hexdumps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
}

/// `levels:` of a source: a profile name (`zephyr`, `off` or a key of
/// `level_profiles`) or an inline profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SourceLevels {
    Named(String),
    Inline(LevelProfile),
}

/// One dictionary path (legacy format) or an ordered list; the first is preferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DictionaryPaths {
    Single(String),
    Multiple(Vec<String>),
}

impl DictionaryPaths {
    pub fn paths(&self) -> &[String] {
        match self {
            Self::Single(path) => std::slice::from_ref(path),
            Self::Multiple(paths) => paths,
        }
    }

    pub fn paths_mut(&mut self) -> &mut [String] {
        match self {
            Self::Single(path) => std::slice::from_mut(path),
            Self::Multiple(paths) => paths,
        }
    }
}

impl std::ops::Deref for DictionaryPaths {
    type Target = str;

    fn deref(&self) -> &str {
        self.paths().first().map(String::as_str).unwrap_or_default()
    }
}

impl From<String> for DictionaryPaths {
    fn from(path: String) -> Self {
        Self::Single(path)
    }
}

/// Parser configuration attached to a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserConfig {
    #[serde(rename = "type", default = "ParserConfig::default_type")]
    pub parser_type: String,
    /// Path or ordered paths to Zephyr dictionary-logging databases. Required when
    /// `type: zephyr-dict` (binary) or `zephyr-dict-hex`; ignored otherwise.
    #[serde(default)]
    pub database: Option<DictionaryPaths>,
    /// Reload the active dictionary file automatically when a rebuild overwrites
    /// it. Only for `zephyr-dict` / `zephyr-dict-hex`; needs the control API.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watch: bool,
    /// Also write every received byte to the `.log` as `[dict-capture:v1] <base64>`
    /// lines, in arrival order, next to the decoded text. Only for `zephyr-dict`
    /// (binary). `embed-log dictionary decode` replays them, so a stream the live
    /// decoder lost can be analysed offline.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raw_capture: bool,
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
            watch: false,
            raw_capture: false,
        }
    }
}

/// A single log source definition from the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub source_type: String, // "uart", "udp", "file"
    #[serde(default)]
    pub port: serde_yaml::Value, // string for uart/file, int for udp
    #[serde(default)]
    pub parser: ParserConfig,
    pub baudrate: Option<u32>,
    // label
    pub label: Option<String>,
    /// Log level detection; `None` means the built-in `zephyr` profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<SourceLevels>,
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

/// A tab in the UI, containing 1–3 panes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabConfig {
    pub label: String,
    pub panes: Vec<PaneConfig>,
}

/// A presentation-only virtual source that interleaves original member
/// records without persisting copies. Referenced from `tabs[].panes` exactly
/// like a physical source name.
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
        18080
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
    /// Named level profiles referenced by `sources.<name>.levels`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub level_profiles: BTreeMap<String, LevelProfile>,
}

impl AppConfig {
    fn default_baudrate() -> u32 {
        115200
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 2,
            sources: Vec::new(),
            tabs: Vec::new(),
            merges: Vec::new(),
            server: ServerConfig::default(),
            logs: LogsConfig::default(),
            baudrate: Self::default_baudrate(),
            frontend_plugins: HashMap::new(),
            level_profiles: BTreeMap::new(),
        }
    }
}
