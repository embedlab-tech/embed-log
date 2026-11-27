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

use crate::protocol::{ConfigMessage, LogPayload, Marker, SessionInfo};

/// Per-pane cap on retained log lines. The browser virtualizes over the full array,
/// but a terminal viewer rarely needs more than this in memory; older lines are
/// dropped (the on-disk session log retains everything).
pub(crate) const MAX_LINES_PER_PANE: usize = 100_000;
/// A full pane may grow this far past the cap before old lines are dropped,
/// so the front of the `Vec` is shifted once per batch, not once per line.
pub(crate) const TRIM_SLACK: usize = MAX_LINES_PER_PANE / 10;

/// A stored log line (the post-parse, pre-render form).
///
/// Holds both timestamp variants so the abs/rel toggle is free (no recompute), and
/// keeps the raw `message` for copy/export and `data` for ANSI parity.
#[derive(Debug, Clone, Default)]
pub struct StoredLine {
    /// Original physical source id.
    pub source_id: String,
    /// Session-global sequence.
    pub sequence: u64,
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
            source_id: p.source_id.clone(),
            sequence: p.sequence,
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
    pub runtime_config: serde_json::Value,
    pub show_config: bool,
    pub config_scroll: u16,
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
    /// Physical source id → virtual merge pane ids.
    pub virtual_merges: HashMap<String, Vec<String>>,
    /// `pane_id → stored lines` (append-only except clear/rebuild).
    pub raw_lines: HashMap<String, Vec<StoredLine>>,
    /// `pane_id → current filter regex` (None = no filter).
    pub filters: HashMap<String, Option<Regex>>,
    /// `pane_id → scroll offset among the filter matches` (ignored when following).
    pub scroll: HashMap<String, usize>,
    /// Markers keyed by pane → list.
    pub markers: HashMap<String, Vec<Marker>>,
    /// Timestamp display mode.
    pub timestamp_mode: TimestampMode,
    /// First-log RFC3339 (drives relative timestamps).
    pub first_log_at: Option<String>,
    /// Sync timestamp (epoch millis) set by clicking/Enter on a line.
    pub sync_ts: Option<f64>,
    /// Pane and raw buffer offset of the most recently focused line.
    pub focused_line: Option<(String, usize)>,
    /// Last marker jump, so repeated jumps advance even when the row is centered.
    pub(crate) marker_jump: Option<(String, usize)>,
    /// Whether switching tabs should scroll to `sync_ts`.
    pub sync_tab_switch: bool,
    /// Connection state for the status bar.
    pub conn: ConnState,
    /// Active tab index.
    pub active_tab: usize,
    /// Active pane index within the active tab (0..=2 for tabs of 1–3 panes).
    pub active_pane: usize,
    /// Unwrap mode (each pane is its own tab).
    pub unwrap: bool,
    /// Selection: `pane_id → set of selected raw line indices`.
    pub selected: HashMap<String, std::collections::BTreeSet<usize>>,
    /// Selection scope: Exact (active pane) or Context (synced lines across tab panes).
    pub selection_scope: SelectionScope,
    /// Visual range start (pane and raw buffer offset).
    pub visual_anchor: Option<(String, usize)>,
    /// Splitter ratio for 2-pane tabs (0.0–1.0, fraction for the left pane).
    pub splitter: f32,
    /// Jump-to-bottom toggle per pane (true = follow new lines).
    pub at_bottom: HashMap<String, bool>,
    /// Regex filter input mode for the active pane.
    pub filter_mode: bool,
    /// Pending regex entered while `filter_mode` is active.
    pub filter_buffer: String,
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
    /// Whether the keyboard help overlay is visible.
    pub show_help: bool,
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

    /// Number of tab slots, or panes while unwrapped.
    pub fn tab_count(&self) -> usize {
        if self.unwrap {
            self.panes.len()
        } else {
            self.tabs.len()
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
    ///
    /// Old lines are dropped in batches of `TRIM_SLACK` (amortised O(1) per
    /// line); dropping one line per append moved the whole buffer every time.
    /// Rebase view and row offsets so they keep pointing at the same lines.
    pub fn append_line(&mut self, pane_id: &str, line: StoredLine) {
        let v = self.raw_lines.entry(pane_id.to_string()).or_default();
        v.push(line);
        if v.len() <= MAX_LINES_PER_PANE + TRIM_SLACK {
            return;
        }
        let drop = v.len() - MAX_LINES_PER_PANE;
        if let Some(scroll) = self.scroll.get_mut(pane_id) {
            let matched = match self.filters.get(pane_id).and_then(Option::as_ref) {
                Some(rx) => v[..drop].iter().filter(|l| rx.is_match(&l.message)).count(),
                None => drop,
            };
            *scroll = scroll.saturating_sub(matched);
        }
        v.drain(..drop);
        if let Some(selected) = self.selected.get_mut(pane_id) {
            *selected = selected
                .iter()
                .filter_map(|idx| idx.checked_sub(drop))
                .collect();
        }
        for position in [
            &mut self.visual_anchor,
            &mut self.focused_line,
            &mut self.marker_jump,
        ] {
            if let Some((pane, idx)) = position {
                if pane == pane_id {
                    if let Some(shifted) = idx.checked_sub(drop) {
                        *idx = shifted;
                    } else {
                        *position = None;
                    }
                }
            }
        }
    }

    /// Append one physical payload to its direct pane and any virtual merge panes.
    pub fn append_payload(&mut self, payload: &LogPayload, is_tx: bool) {
        let mut line = StoredLine::from_payload(payload);
        line.is_tx = is_tx;
        if self.panes.iter().any(|pane| pane == &payload.source_id) {
            self.append_line(&payload.source_id, line.clone());
        }
        let virtual_panes = self
            .virtual_merges
            .get(&payload.source_id)
            .cloned()
            .unwrap_or_default();
        let source_label = self.pane_label(&payload.source_id).to_string();
        for pane in virtual_panes {
            let mut virtual_line = line.clone();
            virtual_line.message = format!("{source_label}: {}", virtual_line.message);
            virtual_line.data = format!("{source_label}: {}", virtual_line.data);
            self.append_line(&pane, virtual_line);
        }
    }

    /// Clear one pane (or all panes if `pane` is None).
    pub fn clear(&mut self, pane: Option<&str>) {
        match pane {
            Some(id) => {
                if let Some(v) = self.raw_lines.get_mut(id) {
                    v.clear();
                }
                self.scroll.remove(id);
                self.selected.remove(id);
            }
            None => {
                for v in self.raw_lines.values_mut() {
                    v.clear();
                }
                self.scroll.clear();
                self.selected.clear();
            }
        }
        for position in [
            &mut self.visual_anchor,
            &mut self.focused_line,
            &mut self.marker_jump,
        ] {
            if position
                .as_ref()
                .is_some_and(|(id, _)| pane.is_none() || pane == Some(id.as_str()))
            {
                *position = None;
            }
        }
    }

    /// Apply a config message: rebuild tabs/panes/labels, reset per-pane stores.
    pub fn apply_config(&mut self, cfg: &ConfigMessage) {
        self.app_name = cfg.app_name.clone();
        self.runtime_config = cfg.runtime_config.clone();
        self.config_scroll = 0;
        self.session = serde_json::from_value(cfg.session.clone()).unwrap_or_default();
        self.pane_labels = cfg.pane_labels.clone();
        self.pane_kinds = cfg.pane_kinds.clone();
        self.pane_commands = cfg.pane_commands.clone();
        self.virtual_merges.clear();
        for merge in &cfg.merges {
            for member in &merge.of {
                self.virtual_merges
                    .entry(member.clone())
                    .or_default()
                    .push(merge.name.clone());
            }
        }
        self.pane_plugins = cfg
            .pane_plugins
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().map(|e| e.name().to_string()).collect()))
            .collect();
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
        self.clear(None);
        self.raw_lines.clear();
        self.filters.clear();
        self.at_bottom.clear();
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
        self.clear(None);
        self.raw_lines.clear();
        self.filters.clear();
        self.at_bottom.clear();
        self.markers.clear();
        self.active_tab = 0;
        self.active_pane = 0;
        self.sync_ts = None;
        self.sync_tab_switch = false;
        self.splitter = 0.5;
    }

    // ── Scrolling ───────────────────────────────────────────────────────

    /// Get the scroll offset for a pane (default 0).
    pub fn scroll_of(&self, pane_id: &str) -> usize {
        self.scroll.get(pane_id).copied().unwrap_or(0)
    }

    pub fn filter_for(&self, pane_id: &str) -> Option<&Regex> {
        self.filters.get(pane_id).and_then(Option::as_ref)
    }

    fn effective_scroll(&self, pane_id: &str, max: usize) -> usize {
        if self.at_bottom.get(pane_id) == Some(&true) {
            max
        } else {
            self.scroll_of(pane_id).min(max)
        }
    }

    /// Effective scroll, match count, and raw offsets of the on-screen rows.
    pub fn viewport(&self, pane_id: &str, visible: usize) -> (usize, usize, Vec<usize>) {
        let Some(lines) = self.raw_lines.get(pane_id) else {
            return (0, 0, Vec::new());
        };
        let matches = self
            .filter_for(pane_id)
            .map(|rx| crate::lines::filtered_indices(lines, Some(rx)));
        let len = matches.as_ref().map_or(lines.len(), Vec::len);
        let start = self.effective_scroll(pane_id, len.saturating_sub(visible));
        let range = start..(start + visible).min(len);
        let rows = match matches {
            Some(matches) => matches[range].to_vec(),
            None => range.collect(),
        };
        (start, len, rows)
    }

    pub fn active_top(&self, visible: usize) -> Option<(String, usize)> {
        let pane = self.active_pane_id()?;
        let raw_idx = *self.viewport(&pane, visible).2.first()?;
        Some((pane, raw_idx))
    }

    /// Scroll the active pane by `delta` matching rows (positive = down).
    pub fn scroll_active(&mut self, delta: i32, visible: usize) {
        if let Some(pane) = self.active_pane_id() {
            let (cur, len, _) = self.viewport(&pane, visible);
            let max = len.saturating_sub(visible);
            let next = cur.saturating_add_signed(delta as isize).min(max);
            self.scroll.insert(pane.clone(), next);
            self.at_bottom.insert(pane, next == max);
            self.marker_jump = None;
        }
    }

    /// Scroll a pane to its bottom (follow new lines).
    pub fn scroll_to_bottom(&mut self, pane_id: &str) {
        self.at_bottom.insert(pane_id.to_string(), true);
        self.marker_jump = None;
    }

    /// Scroll a pane to its top.
    pub fn scroll_to_top(&mut self, pane_id: &str) {
        self.scroll.insert(pane_id.to_string(), 0);
        self.at_bottom.insert(pane_id.to_string(), false);
        self.marker_jump = None;
    }

    pub fn set_filter(&mut self, pane_id: String, filter: Option<Regex>, visible: usize) {
        self.filters.insert(pane_id.clone(), filter);
        let (scroll, _, _) = self.viewport(&pane_id, visible);
        self.scroll.insert(pane_id, scroll);
        self.marker_jump = None;
    }

    pub fn focus_line(&mut self, pane: String, raw_idx: usize, visible: usize) {
        if let Some(line) = self
            .raw_lines
            .get(&pane)
            .and_then(|lines| lines.get(raw_idx))
        {
            let ts = line.abs_num;
            self.focused_line = Some((pane, raw_idx));
            self.marker_jump = None;
            self.sync_panes_to_ts(ts, visible);
        }
    }

    fn center_row(&mut self, pane_id: &str, row: usize, len: usize, visible: usize) {
        let start = row
            .saturating_sub(visible / 2)
            .min(len.saturating_sub(visible));
        self.scroll.insert(pane_id.to_string(), start);
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
            let raw_idx = self
                .focused_line
                .as_ref()
                .filter(|(id, idx)| {
                    id == pane_id && lines.get(*idx).is_some_and(|line| line.abs_num == num_ts)
                })
                .map_or_else(|| nearest_line_by_ts(lines, num_ts), |(_, idx)| *idx);
            let (row, len) = match self.filter_for(pane_id) {
                Some(rx) => {
                    let matches = crate::lines::filtered_indices(lines, Some(rx));
                    (matches.partition_point(|&idx| idx < raw_idx), matches.len())
                }
                None => (raw_idx, lines.len()),
            };
            self.center_row(pane_id, row, len, visible);
        }
        self.sync_ts = Some(num_ts);
        self.sync_tab_switch = true;
    }

    // ── Selection ──────────────────────────────────────────────────────

    /// Toggle selection of a line in the active pane.
    pub fn toggle_select_active(&mut self, line_idx: usize) {
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
            if self
                .visual_anchor
                .as_ref()
                .is_some_and(|(id, _)| id == &pane)
            {
                self.visual_anchor = None;
            }
        }
    }

    /// Whether a line is selected in a pane.
    pub fn is_selected(&self, pane_id: &str, line_idx: usize) -> bool {
        self.selected
            .get(pane_id)
            .is_some_and(|s| s.contains(&line_idx))
    }

    /// Selected line indices in a pane, sorted.
    pub fn selected_for(&self, pane_id: &str) -> Vec<usize> {
        self.selected
            .get(pane_id)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Collect context-scope entries: lines from all panes in the active tab
    /// whose `abs_num` falls within the selection's [min, max] time range.
    /// Mirrors `frontend/selection.js::_collectRangeEntries`.
    pub fn collect_context_entries(&self) -> Vec<(String, usize, f64)> {
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
            .filter_map(|&idx| lines.get(idx).map(|l| l.abs_num))
            .fold((f64::MAX, f64::MIN), |(mn, mx), n| (mn.min(n), mx.max(n)));
        if min_ts > max_ts {
            return Vec::new();
        }

        let mut entries = Vec::new();
        for pane_id in &panes {
            if let Some(pl) = self.raw_lines.get(pane_id) {
                for (i, line) in pl.iter().enumerate() {
                    if line.abs_num >= min_ts && line.abs_num <= max_ts {
                        entries.push((pane_id.clone(), i, line.abs_num));
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

    /// Toggle a persisted marker on the given row in the active pane.
    pub fn toggle_marker_active(&mut self, pane: String, raw_idx: usize) {
        let line = &self.raw_lines[&pane][raw_idx];
        let line_idx = line.line_idx;
        let num_ts = line.abs_num;
        let merged = !line.source_id.is_empty() && line.source_id != pane;
        let markers = self.markers.entry(pane.clone()).or_default();
        // Toggle: remove if one exists at this line, else add.
        let exists = markers
            .iter()
            .position(|m| m.line_idx == line_idx && (!merged || m.num_ts == num_ts));
        if let Some(pos) = exists {
            markers.remove(pos);
        } else {
            markers.push(Marker {
                pane_id: pane.clone(),
                line_idx,
                end_idx: line_idx,
                num_ts,
                description: String::new(),
                created_at: chrono::Local::now().to_rfc3339(),
            });
        }
    }

    /// All markers across all panes (for the `save_markers` WS command).
    pub fn all_markers(&self) -> Vec<Marker> {
        let mut all: Vec<Marker> = self.markers.values().flatten().cloned().collect();
        all.sort_by_key(|m| (m.pane_id.clone(), m.line_idx));
        all
    }

    /// Jump to the previous/next marker whose start row is retained and visible to the filter.
    pub fn jump_marker(&mut self, pane: String, forward: bool, visible: usize) {
        let Some(markers) = self.markers.get(&pane).filter(|m| !m.is_empty()) else {
            return;
        };
        let (_, len, rows) = self.viewport(&pane, visible);
        let Some(&top) = rows.first() else { return };
        let current = match &self.marker_jump {
            Some((id, idx)) if id == &pane => *idx,
            _ => top,
        };
        let lines = &self.raw_lines[&pane];
        let filter = self.filter_for(&pane);
        let merged = lines
            .iter()
            .any(|l| !l.source_id.is_empty() && l.source_id != pane);
        // shortcut: legacy merge markers can collide on index and timestamp; use source IDs if the format gains them.
        let key = |idx, ts: f64| (idx, merged.then(|| ts.to_bits()));
        let targets: HashSet<_> = markers.iter().map(|m| key(m.line_idx, m.num_ts)).collect();
        let mut matches = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| filter.map_or(true, |rx| rx.is_match(&line.message)))
            .enumerate()
            .filter(|(_, (raw, line))| {
                (if forward {
                    *raw > current
                } else {
                    *raw < current
                }) && targets.contains(&key(line.line_idx, line.abs_num))
            });
        let target = if forward {
            matches.next()
        } else {
            matches.last()
        };
        if let Some((row, (raw, _))) = target {
            self.center_row(&pane, row, len, visible);
            self.marker_jump = Some((pane, raw));
        }
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
    use crate::protocol::{LogPayload, Marker, MergeDef, TabDef};
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
    fn physical_payload_is_projected_into_virtual_merge_without_changing_identity() {
        let mut cfg = cfg_with(vec![("Link", vec!["MCU_LINK"])]);
        cfg.pane_labels
            .insert("MCU_RX".to_string(), "MCU RX".to_string());
        cfg.merges.push(MergeDef {
            name: "MCU_LINK".to_string(),
            label: Some("MCU Link".to_string()),
            of: vec!["MCU_TX".to_string(), "MCU_RX".to_string()],
        });
        let mut state = State::default();
        state.apply_config(&cfg);
        state.append_payload(
            &LogPayload {
                source_id: "MCU_RX".to_string(),
                sequence: 42,
                line_idx: 7,
                message: "reply".to_string(),
                data: "reply".to_string(),
                ..Default::default()
            },
            false,
        );

        let line = &state.raw_lines["MCU_LINK"][0];
        assert_eq!(line.source_id, "MCU_RX");
        assert_eq!(line.sequence, 42);
        assert_eq!(line.line_idx, 7);
        assert_eq!(line.message, "MCU RX: reply");
        assert!(!state.raw_lines.contains_key("MCU_RX"));
    }

    #[test]
    fn append_line_caps_per_pane() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT"])]));
        let total = MAX_LINES_PER_PANE + TRIM_SLACK + 1;
        for i in 0..total {
            s.append_line(
                "DUT",
                StoredLine {
                    line_idx: i as u64,
                    message: format!("m{i}"),
                    ..Default::default()
                },
            );
            assert!(s.raw_lines["DUT"].len() <= MAX_LINES_PER_PANE + TRIM_SLACK);
        }
        let v = s.raw_lines.get("DUT").unwrap();
        assert_eq!(v.len(), MAX_LINES_PER_PANE);
        // Oldest dropped; newest retained.
        assert_eq!(v.first().unwrap().line_idx, (TRIM_SLACK + 1) as u64);
        assert_eq!(v.last().unwrap().line_idx, (total - 1) as u64);
    }

    #[test]
    fn trimming_a_filtered_pane_keeps_the_same_top_match() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT"])]));
        s.set_filter("DUT".into(), Some(Regex::new("keep").unwrap()), 3);
        let line = |i: usize| StoredLine {
            line_idx: i as u64,
            message: if i % 2 == 0 {
                "keep".into()
            } else {
                String::new()
            },
            ..Default::default()
        };
        for i in 0..MAX_LINES_PER_PANE + TRIM_SLACK {
            s.append_line("DUT", line(i));
        }
        s.scroll_active(25_000, 3);
        let top_id = |s: &State| s.raw_lines["DUT"][s.viewport("DUT", 3).2[0]].line_idx;
        assert_eq!(top_id(&s), 50_000);
        s.append_line("DUT", line(MAX_LINES_PER_PANE + TRIM_SLACK));
        assert_eq!(top_id(&s), 50_000);
    }

    #[test]
    fn appending_to_a_full_pane_is_cheap() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![("T", vec!["DUT"])]));
        for i in 0..MAX_LINES_PER_PANE {
            s.append_line(
                "DUT",
                StoredLine {
                    line_idx: i as u64,
                    ..Default::default()
                },
            );
        }
        let started = std::time::Instant::now();
        for i in MAX_LINES_PER_PANE..2 * MAX_LINES_PER_PANE {
            s.append_line(
                "DUT",
                StoredLine {
                    line_idx: i as u64,
                    ..Default::default()
                },
            );
        }
        let elapsed = started.elapsed();
        eprintln!("100,000 appends to a full pane: {elapsed:?}");
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
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
    fn syncs_all_panes_in_active_tab_to_nearest_timestamp() {
        let mut s = State::default();
        s.apply_config(&cfg_with(vec![
            ("Device", vec!["DUT", "HOST"]),
            ("Other", vec!["OTHER"]),
        ]));
        for i in 0..5 {
            s.append_line(
                "DUT",
                StoredLine {
                    line_idx: i,
                    abs_num: (i * 100) as f64,
                    ..Default::default()
                },
            );
            s.append_line(
                "HOST",
                StoredLine {
                    line_idx: i,
                    abs_num: (i * 100 + 20) as f64,
                    ..Default::default()
                },
            );
        }

        s.sync_panes_to_ts(250.0, 2);

        // DUT's 200/300 timestamps are equidistant from 250, so the
        // deterministic binary-search tie-break chooses the later line.
        assert_eq!(s.scroll_of("DUT"), 2);
        assert_eq!(s.scroll_of("HOST"), 1);
        assert_eq!(s.sync_ts, Some(250.0));
        assert!(s.sync_tab_switch);
        assert_eq!(s.scroll_of("OTHER"), 0);
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
