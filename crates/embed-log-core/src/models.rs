use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// Timestamp display/storage mode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampMode {
    #[default]
    Absolute,
    Relative,
}

impl TimestampMode {
    pub const ALL: [Self; 2] = [Self::Absolute, Self::Relative];
}

impl std::fmt::Display for TimestampMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absolute => write!(f, "absolute"),
            Self::Relative => write!(f, "relative"),
        }
    }
}

impl std::str::FromStr for TimestampMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "absolute" => Ok(Self::Absolute),
            "relative" => Ok(Self::Relative),
            _ => Err(format!(
                "unknown timestamp mode: {s:?} (use 'absolute' or 'relative')"
            )),
        }
    }
}

/// A single log entry flowing through the system.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub source: String,
    pub message: String,
    pub color: Option<String>,
    pub no_ws: bool,
}

impl LogEntry {
    pub fn new(
        timestamp: DateTime<Local>,
        source: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp,
            source: source.into(),
            message: message.into(),
            color: None,
            no_ws: false,
        }
    }

    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn with_no_ws(mut self, no_ws: bool) -> Self {
        self.no_ws = no_ws;
        self
    }
}

/// Queue saturation statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub maxsize: usize,
    pub depth: usize,
    pub utilization_pct: f64,
    pub enqueued: u64,
    pub dequeued: u64,
    pub peak_depth: usize,
    pub near_full_events: u64,
}

/// ANSI color codes for terminal output.
pub struct Ansi;

impl Ansi {
    pub const RESET: &'static str = "\x1b[0m";
    pub const RED: &'static str = "\x1b[31m";
    pub const GREEN: &'static str = "\x1b[32m";
    pub const YELLOW: &'static str = "\x1b[33m";
    pub const BLUE: &'static str = "\x1b[34m";
    pub const MAGENTA: &'static str = "\x1b[35m";
    pub const CYAN: &'static str = "\x1b[36m";

    /// Map a color name (as used in LogEntry.color) to an ANSI escape code.
    pub fn code(name: &str) -> Option<&'static str> {
        match name {
            "red" => Some(Self::RED),
            "green" => Some(Self::GREEN),
            "yellow" => Some(Self::YELLOW),
            "blue" => Some(Self::BLUE),
            "magenta" => Some(Self::MAGENTA),
            "cyan" => Some(Self::CYAN),
            _ => None,
        }
    }
}
