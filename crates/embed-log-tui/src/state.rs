//! TUI application state — mirrors `frontend/state.js`.
//!
//! Owned by the app loop and mutated by [`crate::protocol::ServerMessage`]s arriving
//! from the WS client. The render layer reads this state each frame.
//!
//! Key invariants:
//! - `tabs` / `panes` / `pane_labels` / `pane_kinds` are rebuilt on `config` and on
//!   `session_rotated` (followed by a fresh config).
//! - `raw_lines[pane]` is append-only except for `clear_logs` (truncate) and session
//!   rebuild (drop all). A per-pane cap prevents unbounded growth during long runs.
//! - `first_log_at` is set from the first `session_info` that carries it and drives
//!   relative-timestamp rendering.
//! - `markers` is a full-replacement map keyed by pane → list (matches the server's
//!   `markers_update` semantics).

use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::protocol::{
    ConfigMessage, EventPayload, EventRuleSummary, LogPayload, Marker, SessionInfo,
};

/// Per-pane cap on retained log lines. The browser virtualizes over the full array,
/// but a terminal viewer rarely needs more than this in memory; older lines are
/// dropped (the on-disk session log retains everything).
const MAX_LINES_PER_PANE: usize = 100_000;

/// A stored log line (the post-parse, pre-render form).
///
/// Holds both timestamp variants so the abs/rel toggle is free (no recompute), and
/// keeps the raw `message` for copy/snippet/export and `data` for ANSI parity.
#[derive(Debug, Clone, Default)]
pub struct StoredLine {
    /// Display timestamp (absolute): `"06-14 09:30:45.123"`.
    pub abs_ts: String,
    /// Epoch millis (absolute).
    pub abs_num: f64,
    /// Relative timestamp: `"00:00:45.123"`.
    pub rel_ts: String,
    /// Relative millis.
    pub rel_num: f64,
    /// Raw logical message (no ANSI).
    pub message: String,
    /// ANSI-wrapped message as sent by the server.
    pub data: String,
    /// Color name or null.
    pub color: Option<String>,
    /// Origin (`"SERIAL"`, `"ui"`, …).
    pub origin: String,
    /// Stable per-source line index.
    pub line_idx: u64,
    /// Whether this is a TX line.
    pub is_tx: bool,
}

impl StoredLine {
    /// Build from a [`LogPayload`].
    pub fn from_payload(p: &LogPayload) -> Self {
        Self {
            abs_ts: p.abs_ts.clone(),
            abs_num: p.abs_num,
            rel_ts: p.rel_ts.clone(),
            rel_num: p.rel_num,
            message: p.message.clone(),
            data: p.data.clone(),
            color: p.color.clone(),
            origin: p.origin.clone(),
            line_idx: p.line_idx,
            is_tx: false, // set by caller based on rx/tx variant
        }
    }
}

/// A tab definition in TUI state.
#[derive(Debug, Clone, Default)]
pub struct Tab {
    pub label: String,
    pub pane_ids: Vec<String>,
}

/// Timestamp display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimestampMode {
    #[default]
    Absolute,
    Relative,
}

impl std::str::FromStr for TimestampMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("relative") {
            Ok(Self::Relative)
        } else {
            Ok(Self::Absolute)
        }
    }
}

/// Selection scope: Exact (active pane only) or Context (synced lines from
/// all panes in the active tab within the selection's time range).
/// Mirrors `frontend/selection.js` scope toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionScope {
    #[default]
    Exact,
    Context,
}

/// Connection state shown in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// The root application state.
#[derive(Debug, Default)]
pub struct State {
    /// App name from config (status bar).
    pub app_name: String,
    /// Active session metadata.
    pub session: SessionInfo,
    /// Tabs (label + pane ids). Rebuilt on config.
    pub tabs: Vec<Tab>,
    /// All pane ids across all tabs (deduped, insertion order).
    pub panes: Vec<String>,
    /// `pane_id → human label`.
    pub pane_labels: HashMap<String, String>,
    /// `pane_id → source type`.
    pub pane_kinds: HashMap<String, String>,
    /// `pane_id → UART command suggestions`.
    pub pane_commands: HashMap<String, Vec<String>>,
    /// `pane_id → list of plugin names` (from config).
    pub pane_plugins: HashMap<String, Vec<String>>,
    /// `pane_id → stored lines` (append-only except clear/rebuild).
    pub raw_lines: HashMap<String, Vec<StoredLine>>,
    /// `pane_id → current filter regex` (None = no filter).
    pub filters: HashMap<String, Option<Regex>>,
    /// `pane_id → scroll offset (raw index of the top visible line)`.
    pub scroll: HashMap<String, usize>,
    /// Markers keyed by pane → list.
    pub markers: HashMap<String, Vec<Marker>>,
    /// Whether event-marker navigation includes event markers.
    pub include_event_markers: bool,
    /// Detected events (append-only except rebuild).
    pub events: Vec<EventPayload>,
    /// `source_id → rule summaries` (non-empty ⇒ events tab shown).
    pub event_rules: HashMap<String, Vec<EventRuleSummary>>,
    /// Cached "events enabled" flag (any source has ≥1 rule).
    pub events_enabled: bool,
    /// Timestamp display mode.
    pub timestamp_mode: TimestampMode,
    /// First-log RFC3339 (drives relative timestamps).
    pub first_log_at: Option<String>,
    /// Sync timestamp (epoch millis) set by clicking/Enter on a line.
    pub sync_ts: Option<f64>,
    /// The exact raw line index most recently focused/clicked in the active pane.
    pub focused_raw_idx: Option<u64>,
    /// Whether switching tabs should scroll to `sync_ts`.
    pub sync_tab_switch: bool,
    /// Connection state for the status bar.
    pub conn: ConnState,
    /// Active tab index.
    pub active_tab: usize,
    /// Active pane index within the active tab (0 or 1 for 1–2 pane tabs).
    pub active_pane: usize,
    /// Unwrap mode (each pane is its own tab).
    pub unwrap: bool,
    /// Selection: `pane_id → set of selected raw line indices`.
    pub selected: HashMap<String, std::collections::BTreeSet<u64>>,
    /// Selection scope: Exact (active pane) or Context (synced lines across tab panes).
    pub selection_scope: SelectionScope,
    /// Visual range select mode anchor (raw index of the start line).
    pub visual_anchor: Option<u64>,
    /// Splitter ratio for 2-pane tabs (0.0–1.0, fraction for the left pane).
    pub splitter: f32,
    /// Jump-to-bottom toggle per pane (true = follow new lines).
    pub at_bottom: HashMap<String, bool>,
    /// TX input mode active (bottom input row shown for the active UART pane).
    pub tx_mode: bool,
    /// Current TX input buffer.
    pub tx_buffer: String,
    /// Matching command indices within `pane_commands[active_pane]`.
    pub tx_matches: Vec<usize>,
    /// Which match is currently selected when cycling suggestions.
    pub tx_match_pos: Option<usize>,
    /// Last TX result/status line shown in the status bar.
    pub tx_status: Option<String>,
    /// Events view state (cursor, zoom, filters, detail popup).
    pub events_view: crate::events::EventsView,
}

impl State {
    /// Human label for a pane, falling back to its id.
    pub fn pane_label(&self, pane_id: &str) -> String {
        self.pane_labels
            .get(pane_id)
            .cloned()
            .unwrap_or_else(|| pane_id.to_string())
    }

    /// Pane ids of the active tab (or, in unwrap mode, the single active pane).
    pub fn active_tab_panes(&self) -> Vec<String> {
        if self.unwrap {
            self.panes
                .get(self.active_tab)
                .map(|p| vec![p.clone()])
                .unwrap_or_default()
        } else {
            self.tabs
                .get(self.active_tab)
                .map(|t| t.pane_ids.clone())
                .unwrap_or_default()
        }
    }

    /// Whether the events tab is currently active.
    /// The events tab sits at index `tabs.len()` (appended when events_enabled).
    pub fn events_tab_active(&self) -> bool {
        self.events_enabled && !self.unwrap && self.active_tab == self.tabs.len()
    }

    /// Number of tab slots (tabs + 1 for events if enabled, or panes in unwrap).
    pub fn tab_count(&self) -> usize {
        if self.unwrap {
            self.panes.len()
        } else {
            let n = self.tabs.len();
            if self.events_enabled {
                n + 1
            } else {
                n
            }
        }
    }

    /// The currently focused pane id (if any).
    pub fn active_pane_id(&self) -> Option<String> {
        let panes = self.active_tab_panes();
        panes.get(self.active_pane).cloned()
    }

    /// Whether a pane is writable (UART) and thus supports TX.
    pub fn is_writable(&self, pane_id: &str) -> bool {
        self.pane_kinds
            .get(pane_id)
            .map(|k| k == "uart")
            .unwrap_or(false)
    }

    /// Append a log line to its pane, enforcing the per-pane cap.
    pub fn append_line(&mut self, pane_id: &str, line: StoredLine) {
        let v = self.raw_lines.entry(pane_id.to_string()).or_default();
        v.push(line);
        if v.len() > MAX_LINES_PER_PANE {
            let drop = v.len() - MAX_LINES_PER_PANE;
            v.drain(..drop);
        }
    }

    /// Clear one pane (or all panes if `pane` is None).
    pub fn clear(&mut self, pane: Option<&str>) {
        match pane {
            Some(id) => {
                if let Some(v) = self.raw_lines.get_mut(id) {
                    v.clear();
                }
            }
            None => {
                for v in self.raw_lines.values_mut() {
                    v.clear();
                }
            }
        }
    }

    /// Apply a config message: rebuild tabs/panes/labels, reset per-pane stores.
    pub fn apply_config(&mut self, cfg: &ConfigMessage) {
        self.app_name = cfg.app_name.clone();
        self.session = serde_json::from_value(cfg.session.clone()).unwrap_or_default();
        self.pane_labels = cfg.pane_labels.clone();
        self.pane_kinds = cfg.pane_kinds.clone();
        self.pane_commands = cfg.pane_commands.clone();
        self.pane_plugins = cfg
            .pane_plugins
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().map(|e| e.name().to_string()).collect()))
            .collect();
        self.event_rules = cfg.event_rules.clone();
        self.events_enabled = self.event_rules.values().any(|rs| !rs.is_empty());

        // Rebuild tabs + deduped pane list (insertion order).
        self.tabs = cfg
            .tabs
            .iter()
            .map(|t| Tab {
                label: t.label.clone(),
                pane_ids: t.panes.clone(),
            })
            .collect();
        let mut seen = HashSet::new();
        self.panes = self
            .tabs
            .iter()
            .flat_map(|t| t.pane_ids.iter().cloned())
            .filter(|p| seen.insert(p.clone()))
            .collect();

        // Reset per-pane stores for the new layout.
        self.raw_lines.clear();
        self.filters.clear();
        self.scroll.clear();
        for p in &self.panes {
            self.raw_lines.entry(p.clone()).or_default();
            self.filters.insert(p.clone(), None);
            self.scroll.insert(p.clone(), 0);
        }

        // Markers: re-key by pane.
        self.markers.clear();
        for m in &cfg.markers {
            self.markers
                .entry(m.pane_id.clone())
                .or_default()
                .push(m.clone());
        }

        // Events: a fresh config means a fresh session — drop old events.
        self.events.clear();

        // Timestamp context from session.
        self.timestamp_mode = self.session.timestamp_mode.parse().unwrap_or_default();
        self.first_log_at = self.session.first_log_at.clone();

        // Reset navigation/view state.
        self.active_tab = 0;
        self.active_pane = 0;
        self.sync_ts = None;
        self.sync_tab_switch = false;
        self.splitter = 0.5;
    }

    /// Apply a `session_info` update (first_log_at set or rotation metadata).
    pub fn apply_session_info(&mut self, info: &SessionInfo) {
        self.session = info.clone();
        if let Some(ref fla) = info.first_log_at {
            self.first_log_at = Some(fla.clone());
        }
        self.timestamp_mode = info.timestamp_mode.parse().unwrap_or_default();
    }

    /// Replace the full marker set.
    pub fn apply_markers(&mut self, markers: &[Marker]) {
        self.markers.clear();
        for m in markers {
            self.markers
                .entry(m.pane_id.clone())
                .or_default()
                .push(m.clone());
        }
    }

    /// Push a detected event.
    pub fn push_event(&mut self, ev: EventPayload) {
        self.events.push(ev);
    }

    /// Markers for a pane, sorted by line_idx (stable for navigation).
    pub fn markers_for(&self, pane_id: &str) -> Vec<Marker> {
        let mut ms = self.markers.get(pane_id).cloned().unwrap_or_default();
        ms.sort_by_key(|m| m.line_idx);
        ms
    }

    /// Teardown for session rotation / full reconnect: drop layout + lines, keep
    /// connection state. The next `config` message rebuilds everything.
    pub fn teardown_layout(&mut self) {
        self.tabs.clear();
        self.panes.clear();
        self.pane_labels.clear();
        self.pane_kinds.clear();
        self.pane_commands.clear();
        self.pane_plugins.clear();
        self.raw_lines.clear();
        self.filters.clear();
        self.scroll.clear();
        self.selected.clear();
        self.at_bottom.clear();
        self.markers.clear();
        self.events.clear();
        self.event_rules.clear();
        self.events_enabled = false;
        self.active_tab = 0;
        self.active_pane = 0;
        self.sync_ts = None;
        self.sync_tab_switch = false;
        self.visual_anchor = None;
        self.splitter = 0.5;
    }

    // ── Scrolling ───────────────────────────────────────────────────────

    /// Get the scroll offset for a pane (default 0).
    pub fn scroll_of(&self, pane_id: &str) -> usize {
        self.scroll.get(pane_id).copied().unwrap_or(0)
    }

    /// Set the scroll offset for a pane, clamping to valid range.
    /// `visible` is the number of rows that fit in the pane.
    pub fn set_scroll(&mut self, pane_id: &str, scroll: usize, visible: usize) {
        let len = self.raw_lines.get(pane_id).map(|v| v.len()).unwrap_or(0);
        let max = len.saturating_sub(visible);
        let clamped = scroll.min(max);
        self.scroll.insert(pane_id.to_string(), clamped);
        // Scrolling away from the bottom disables follow mode.
        if clamped < max {
            self.at_bottom.insert(pane_id.to_string(), false);
        } else {
            self.at_bottom.insert(pane_id.to_string(), true);
        }
    }

    /// Scroll the active pane by `delta` rows (positive = down).
    pub fn scroll_active(&mut self, delta: i32, visible: usize) {
        if let Some(pane) = self.active_pane_id() {
            let cur = self.scroll_of(&pane) as i32;
            let next = (cur + delta).max(0) as usize;
            self.set_scroll(&pane, next, visible);
        }
    }

    /// Scroll a pane to its bottom (follow new lines).
    pub fn scroll_to_bottom(&mut self, pane_id: &str, visible: usize) {
        let len = self.raw_lines.get(pane_id).map(|v| v.len()).unwrap_or(0);
        self.set_scroll(pane_id, len, visible);
        self.at_bottom.insert(pane_id.to_string(), true);
    }

    /// Scroll a pane to its top.
    pub fn scroll_to_top(&mut self, pane_id: &str) {
        self.scroll.insert(pane_id.to_string(), 0);
        self.at_bottom.insert(pane_id.to_string(), false);
    }

    // ── Sync ────────────────────────────────────────────────────────────

    /// Sync all panes in the active tab to `num_ts`: binary-search each pane
    /// for the nearest line by `abs_num`, center it in the viewport.
    /// Mirrors `frontend/lines.js::syncPanes`.
    pub fn sync_panes_to_ts(&mut self, num_ts: f64, visible: usize) {
        if self.unwrap {
            return;
        }
        let panes = self.active_tab_panes();
        if panes.len() < 2 {
            return;
        }
        for pane_id in &panes {
            let Some(lines) = self.raw_lines.get(pane_id) else {
                continue;
            };
            if lines.is_empty() {
                continue;
            }
            let idx = nearest_line_by_ts(lines, num_ts);
            // Center the line in the viewport.
            let target = idx.saturating_sub(visible / 2);
            self.set_scroll(pane_id, target, visible);
        }
        self.sync_ts = Some(num_ts);
        self.sync_tab_switch = true;
    }

    // ── Selection ──────────────────────────────────────────────────────

    /// Toggle selection of a line in the active pane.
    pub fn toggle_select_active(&mut self, line_idx: u64) {
        if let Some(pane) = self.active_pane_id() {
            let set = self.selected.entry(pane.clone()).or_default();
            if set.contains(&line_idx) {
                set.remove(&line_idx);
            } else {
                set.insert(line_idx);
            }
            if set.is_empty() {
                self.selected.remove(&pane);
            }
        }
    }

    /// Clear all selection in the active pane.
    pub fn clear_selection(&mut self) {
        if let Some(pane) = self.active_pane_id() {
            self.selected.remove(&pane);
        }
        self.visual_anchor = None;
    }

    /// Whether a line is selected in a pane.
    pub fn is_selected(&self, pane_id: &str, line_idx: u64) -> bool {
        self.selected
            .get(pane_id)
            .is_some_and(|s| s.contains(&line_idx))
    }

    /// Selected line indices in a pane, sorted.
    pub fn selected_for(&self, pane_id: &str) -> Vec<u64> {
        self.selected
            .get(pane_id)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Collect context-scope entries: lines from all panes in the active tab
    /// whose `abs_num` falls within the selection's [min, max] time range.
    /// Mirrors `frontend/selection.js::_collectRangeEntries`.
    pub fn collect_context_entries(&self) -> Vec<(String, u64, f64)> {
        let panes = self.active_tab_panes();
        // Compute the time range from the active pane's selection.
        let Some(active) = self.active_pane_id() else {
            return Vec::new();
        };
        let Some(sel) = self.selected.get(&active) else {
            return Vec::new();
        };
        if sel.is_empty() {
            return Vec::new();
        }
        let lines = match self.raw_lines.get(&active) {
            Some(l) => l,
            None => return Vec::new(),
        };
        let (min_ts, max_ts) = sel
            .iter()
            .filter_map(|&idx| lines.get(idx as usize).map(|l| l.abs_num))
            .fold((f64::MAX, f64::MIN), |(mn, mx), n| (mn.min(n), mx.max(n)));
        if min_ts > max_ts {
            return Vec::new();
        }

        let mut entries: Vec<(String, u64, f64)> = Vec::new();
        for pane_id in &panes {
            if let Some(pl) = self.raw_lines.get(pane_id) {
                for (i, line) in pl.iter().enumerate() {
                    if line.abs_num >= min_ts && line.abs_num <= max_ts {
                        entries.push((pane_id.clone(), i as u64, line.abs_num));
                    }
                }
            }
        }
        // Sort by timestamp, then pane id, then index (mirrors _collectRangeEntries).
        entries.sort_by(|a, b| {
            a.2.partial_cmp(&b.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
                .then(a.1.cmp(&b.1))
        });
        entries
    }

    // ── Markers ────────────────────────────────────────────────────────

    /// Toggle a user marker on a line in the active pane. Returns the full
    /// marker set for that pane (for the `save_markers` WS command).
    pub fn toggle_marker_active(
        &mut self,
        line_idx: u64,
        num_ts: f64,
        description: &str,
    ) -> Vec<Marker> {
        let Some(pane) = self.active_pane_id() else {
            return Vec::new();
        };
        let markers = self.markers.entry(pane.clone()).or_default();
        // Toggle: remove if exists (user marker only), else add.
        let exists = markers
            .iter()
            .position(|m| m.pane_id == pane && m.line_idx == line_idx && !m.is_event());
        if let Some(pos) = exists {
            markers.remove(pos);
        } else {
            markers.push(Marker {
                pane_id: pane.clone(),
                line_idx,
                end_idx: line_idx,
                num_ts,
                description: description.to_string(),
                kind: "user".to_string(),
                severity: String::new(),
                created_at: chrono::Local::now().to_rfc3339(),
            });
        }
        // Return the full set for this pane (save_markers sends per-pane or all).
        self.markers_for(&pane)
    }

    /// All markers across all panes (for the `save_markers` WS command).
    pub fn all_markers(&self) -> Vec<Marker> {
        let mut all: Vec<Marker> = self.markers.values().flatten().cloned().collect();
        all.sort_by_key(|m| (m.pane_id.clone(), m.line_idx));
        all
    }

    /// Navigate to the prev/next marker in the active pane.
    /// `include_events`: whether to consider event markers.
    /// Returns the line_idx of the target marker, or None.
    pub fn nav_marker(&self, forward: bool, include_events: bool) -> Option<u64> {
        let pane = self.active_pane_id()?;
        let scroll = self.scroll_of(&pane);
        let markers = self.markers_for(&pane);
        let current_ts = self
            .raw_lines
            .get(&pane)
            .and_then(|l| l.get(scroll))
            .map(|l| l.abs_num);
        let target = if forward {
            markers
                .iter()
                .filter(|m| include_events || !m.is_event())
                .find(|m| m.num_ts > current_ts.unwrap_or(f64::MIN))
                .map(|m| m.line_idx)
        } else {
            markers
                .iter()
                .filter(|m| include_events || !m.is_event())
                .rev()
                .find(|m| m.num_ts < current_ts.unwrap_or(f64::MAX))
                .map(|m| m.line_idx)
        };
        target
    }

    // ── TX input ───────────────────────────────────────────────────────

    pub fn open_tx_mode(&mut self) {
        if self
            .active_pane_id()
            .as_deref()
            .is_some_and(|p| self.is_writable(p))
        {
            self.tx_mode = true;
            self.refresh_tx_matches();
        }
    }

    pub fn close_tx_mode(&mut self) {
        self.tx_mode = false;
        self.tx_matches.clear();
        self.tx_match_pos = None;
    }

    pub fn active_pane_commands(&self) -> Vec<String> {
        let Some(pane) = self.active_pane_id() else {
            return Vec::new();
        };
        self.pane_commands.get(&pane).cloned().unwrap_or_default()
    }

    pub fn refresh_tx_matches(&mut self) {
        let typed = self.tx_buffer.to_lowercase();
        let commands = self.active_pane_commands();
        self.tx_matches = if typed.is_empty() {
            (0..commands.len()).collect()
        } else {
            let mut scored: Vec<(usize, usize, usize)> = commands
                .iter()
                .enumerate()
                .filter_map(|(i, cmd)| {
                    let lower = cmd.to_lowercase();
                    lower.find(&typed).map(|score| (i, score, cmd.len()))
                })
                .collect();
            scored.sort_by_key(|(_, score, len)| (*score, *len));
            scored.into_iter().map(|(i, _, _)| i).collect()
        };
        self.tx_match_pos = None;
    }

    pub fn cycle_tx_suggestion(&mut self, backward: bool) {
        let commands = self.active_pane_commands();
        if commands.is_empty() {
            return;
        }
        if self.tx_matches.is_empty() {
            self.tx_matches = (0..commands.len()).collect();
        }
        let len = self.tx_matches.len();
        if len == 0 {
            return;
        }
        let next = match self.tx_match_pos {
            None => {
                if backward {
                    len - 1
                } else {
                    0
                }
            }
            Some(cur) => {
                if backward {
                    (cur + len - 1) % len
                } else {
                    (cur + 1) % len
                }
            }
        };
        self.tx_match_pos = Some(next);
        if let Some(cmd_idx) = self.tx_matches.get(next).copied() {
            if let Some(cmd) = commands.get(cmd_idx) {
                self.tx_buffer = cmd.clone();
            }
        }
    }
}

/// Binary search for the line nearest to `num_ts` by `abs_num`.
/// Mirrors `frontend/lines.js::syncPanes` binary search.
fn nearest_line_by_ts(lines: &[StoredLine], num_ts: f64) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let mut lo = 0;
    let mut hi = lines.len() - 1;
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if lines[mid].abs_num < num_ts {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // Check if lo-1 is closer.
    if lo > 0 && (lines[lo - 1].abs_num - num_ts).abs() < (lines[lo].abs_num - num_ts).abs() {
        lo - 1
    } else {
        lo
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{LogPayload, Marker, TabDef};
    use std::collections::HashMap;

    fn cfg_with(tabs: Vec<(&str, Vec<&str>)>) -> ConfigMessage {
        ConfigMessage {
            app_name: "t".into(),
            tabs: tabs
                .into_iter()
                .map(|(label, panes)| TabDef {
                    label: label.into(),
                    panes: panes.into_iter().map(String::from).collect(),
                    ..Default::default()
                })
                .collect(),
            pane_labels: HashMap::new(),
            pane_kinds: HashMap::new(),
            ..Default::default()
        }
    }

    #[test]
    fn apply_config_builds_tabs_and_deduped_panes() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![
            ("Device", vec!["DUT", "HOST"]),
            ("UART", vec!["UART_DUT", "HOST"]), // HOST reused
        ]));
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.tabs[0].pane_ids, ["DUT", "HOST"]);
        // Deduped pane list preserves first-seen order: DUT, HOST, UART_DUT.
        assert_eq!(s.panes, ["DUT", "HOST", "UART_DUT"]);
        // Per-pane stores initialized.
        for p in &s.panes {
            assert!(s.raw_lines.get(p).unwrap().is_empty());
            assert!(s.filters.get(p).unwrap().is_none());
        }
        assert_eq!(s.active_tab, 0);
    }

    #[test]
    fn append_line_caps_per_pane() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT"])]));
        for i in 0..(MAX_LINES_PER_PANE + 50) {
            s.append_line(
                "DUT",
                StoredLine {
                    line_idx: i as u64,
                    message: format!("m{i}"),
                    ..Default::default()
                },
            );
        }
        let v = s.raw_lines.get("DUT").unwrap();
        assert_eq!(v.len(), MAX_LINES_PER_PANE);
        // Oldest dropped; newest retained.
        assert_eq!(v.first().unwrap().line_idx, 50);
        assert_eq!(v.last().unwrap().line_idx, (MAX_LINES_PER_PANE + 49) as u64);
    }

    #[test]
    fn clear_one_pane_leaves_others() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT", "HOST"])]));
        s.append_line("DUT", StoredLine::default());
        s.append_line("HOST", StoredLine::default());
        s.clear(Some("DUT"));
        assert!(s.raw_lines.get("DUT").unwrap().is_empty());
        assert_eq!(s.raw_lines.get("HOST").unwrap().len(), 1);
    }

    #[test]
    fn clear_all_panes() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT", "HOST"])]));
        s.append_line("DUT", StoredLine::default());
        s.append_line("HOST", StoredLine::default());
        s.clear(None);
        assert!(s.raw_lines.values().all(|v| v.is_empty()));
    }

    #[test]
    fn apply_markers_keys_by_pane() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT", "HOST"])]));
        s.apply_markers(&[
            Marker {
                pane_id: "DUT".into(),
                line_idx: 2,
                ..Default::default()
            },
            Marker {
                pane_id: "DUT".into(),
                line_idx: 1,
                ..Default::default()
            },
            Marker {
                pane_id: "HOST".into(),
                line_idx: 5,
                ..Default::default()
            },
        ]);
        // Sorted by line_idx.
        let dut = s.markers_for("DUT");
        assert_eq!(dut.len(), 2);
        assert_eq!(dut[0].line_idx, 1);
        assert_eq!(dut[1].line_idx, 2);
        assert_eq!(s.markers_for("HOST").len(), 1);
    }

    #[test]
    fn events_enabled_flag_reflects_rules() {
        let mut s = State::default();
        let mut cfg = cfg_with(vec![("T", vec!["DUT"])]);
        cfg.event_rules
            .insert("DUT".into(), vec![EventRuleSummary::default()]);
        s.apply_config(&cfg);
        assert!(s.events_enabled);

        let mut s2 = State::default();
        s2.apply_config(&cfg_with(vec![("T", vec!["DUT"])]));
        assert!(!s2.events_enabled);
    }

    #[test]
    fn apply_session_info_sets_first_log_at_and_mode() {
        let mut s = State::default();
        let info = SessionInfo {
            first_log_at: Some("2026-06-14T09:30:45.123Z".into()),
            timestamp_mode: "relative".into(),
            ..Default::default()
        };
        s.apply_session_info(&info);
        assert_eq!(s.first_log_at.as_deref(), Some("2026-06-14T09:30:45.123Z"));
        assert_eq!(s.timestamp_mode, TimestampMode::Relative);
    }

    #[test]
    fn teardown_drops_layout_but_keeps_conn() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT"])]));
        s.append_line("DUT", StoredLine::default());
        s.conn = ConnState::Connected;
        s.teardown_layout();
        assert!(s.tabs.is_empty());
        assert!(s.panes.is_empty());
        assert!(s.raw_lines.is_empty());
        assert_eq!(s.conn, ConnState::Connected);
    }

    #[test]
    fn active_pane_id_resolves_through_tab() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("Device", vec!["DUT", "HOST"])]));
        assert_eq!(s.active_pane_id().as_deref(), Some("DUT"));
        s.active_pane = 1;
        assert_eq!(s.active_pane_id().as_deref(), Some("HOST"));
    }

    #[test]
    fn is_writable_only_for_uart() {
        let mut s = State::default();
        let mut cfg = cfg_with(vec![("T", vec!["DUT", "UART_DUT"])]);
        cfg.pane_kinds.insert("DUT".into(), "udp".into());
        cfg.pane_kinds.insert("UART_DUT".into(), "uart".into());
        s.apply_config(&cfg);
        assert!(!s.is_writable("DUT"));
        assert!(s.is_writable("UART_DUT"));
        assert!(!s.is_writable("nope"));
    }

    #[test]
    fn from_payload_copies_fields() {
        let p = LogPayload {
            abs_ts: "06-14 09:30:45.123".into(),
            abs_num: 1718347845123.0,
            rel_ts: "00:00:45.123".into(),
            rel_num: 45123.0,
            message: "boot".into(),
            data: "\u{1b}[36mboot\u{1b}[0m".into(),
            color: Some("cyan".into()),
            origin: "SERIAL".into(),
            line_idx: 42,
            ..Default::default()
        };
        let l = StoredLine::from_payload(&p);
        assert_eq!(l.abs_num, 1718347845123.0);
        assert_eq!(l.rel_num, 45123.0);
        assert_eq!(l.message, "boot");
        assert_eq!(l.color.as_deref(), Some("cyan"));
        assert_eq!(l.line_idx, 42);
    }

    #[test]
    fn open_tx_mode_only_for_uart() {
        let mut s = State::default();
        let mut cfg = cfg_with(vec![("T", vec!["DUT", "UART_DUT"])]);
        cfg.pane_kinds.insert("DUT".into(), "udp".into());
        cfg.pane_kinds.insert("UART_DUT".into(), "uart".into());
        cfg.pane_commands
            .insert("UART_DUT".into(), vec!["help".into(), "version".into()]);
        s.apply_config(&cfg);

        s.active_pane = 0;
        s.open_tx_mode();
        assert!(!s.tx_mode);

        s.active_pane = 1;
        s.open_tx_mode();
        assert!(s.tx_mode);
    }

    #[test]
    fn refresh_tx_matches_fuzzy_sorts_by_position_then_len() {
        let mut s = State::default();
        let mut cfg = cfg_with(vec![("T", vec!["UART_DUT"])]);
        cfg.pane_kinds.insert("UART_DUT".into(), "uart".into());
        cfg.pane_commands.insert(
            "UART_DUT".into(),
            vec!["version".into(), "get version".into(), "help".into()],
        );
        s.apply_config(&cfg);
        s.tx_buffer = "ver".into();
        s.refresh_tx_matches();
        assert_eq!(s.tx_matches, vec![0, 1]);
    }

    #[test]
    fn cycle_tx_suggestion_replaces_buffer() {
        let mut s = State::default();
        let mut cfg = cfg_with(vec![("T", vec!["UART_DUT"])]);
        cfg.pane_kinds.insert("UART_DUT".into(), "uart".into());
        cfg.pane_commands
            .insert("UART_DUT".into(), vec!["help".into(), "version".into()]);
        s.apply_config(&cfg);
        s.open_tx_mode();
        s.cycle_tx_suggestion(false);
        assert_eq!(s.tx_buffer, "help");
        s.cycle_tx_suggestion(false);
        assert_eq!(s.tx_buffer, "version");
        s.cycle_tx_suggestion(true);
        assert_eq!(s.tx_buffer, "help");
    }
}
