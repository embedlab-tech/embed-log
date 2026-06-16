use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use regex::Regex;
use serde_json::json;
use tracing::info;

use crate::frontend_assets::FrontendAssets;

/// Generates a self-contained HTML file from session log files.
///
/// The exported HTML embeds all frontend assets (CSS + JS with ES module syntax
/// stripped) and log data inline, matching the output of the original Python
/// `merge_logs.py` tool.
pub struct SessionExporter {
    html_path: PathBuf,
    source_files: HashMap<String, String>,
    tabs: Vec<serde_json::Value>,
    source_labels: HashMap<String, String>,
    frontend_dir: PathBuf,
    timestamp_mode: String,
    first_log_at: Option<String>,
    pane_plugins: serde_json::Value,
    frontend_plugins: serde_json::Value,
    plugin_scripts: serde_json::Value,
    markers: Vec<serde_json::Value>,
    events: Vec<serde_json::Value>,
    event_rules: serde_json::Value,
}

impl SessionExporter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        html_path: PathBuf,
        source_files: HashMap<String, String>,
        tabs: Vec<serde_json::Value>,
        source_labels: HashMap<String, String>,
        frontend_dir: PathBuf,
        timestamp_mode: String,
        first_log_at: Option<String>,
    ) -> Self {
        Self {
            html_path,
            source_files,
            tabs,
            source_labels,
            frontend_dir,
            timestamp_mode,
            first_log_at,
            pane_plugins: json!({}),
            frontend_plugins: json!({}),
            plugin_scripts: json!({}),
            markers: vec![],
            events: vec![],
            event_rules: json!({}),
        }
    }

    /// Set plugin data from the server's loaded plugins.
    pub fn with_plugins(
        mut self,
        frontend_plugins: serde_json::Value,
        pane_plugins: serde_json::Value,
        plugin_scripts: serde_json::Value,
    ) -> Self {
        self.frontend_plugins = frontend_plugins;
        self.pane_plugins = pane_plugins;
        self.plugin_scripts = plugin_scripts;
        self
    }

    /// Set markers for the exported session.
    pub fn with_markers(mut self, markers: Vec<serde_json::Value>) -> Self {
        self.markers = markers;
        self
    }

    /// Set detected events and event rules for the exported session.
    pub fn with_events(
        mut self,
        events: Vec<serde_json::Value>,
        event_rules: serde_json::Value,
    ) -> Self {
        self.events = events;
        self.event_rules = event_rules;
        self
    }

    /// Generate the self-contained session HTML file.
    pub fn export(&self) -> Result<PathBuf> {
        let css = self.read_frontend_asset("viewer.css").unwrap_or_default();

        // Parse log files and build entries.
        let mut log_data: HashMap<String, Vec<LogEntry>> = HashMap::new();
        for (source_name, log_path_str) in &self.source_files {
            let log_path = Path::new(log_path_str);
            if !log_path.exists() {
                continue;
            }
            let content = std::fs::read_to_string(log_path).unwrap_or_default();
            let entries = parse_log_file(
                &content,
                Some(source_name.as_str()),
                self.source_labels.get(source_name).map(|s| s.as_str()),
            );
            log_data.insert(source_name.clone(), entries);
        }

        // Enrich timestamp variants (compute rel from abs or vice versa).
        let effective_first_log_at =
            enrich_timestamps(&mut log_data, &self.timestamp_mode, &self.first_log_at);

        // Build pane list and labels.
        let mut all_pane_ids: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut pane_labels_map: HashMap<String, String> = HashMap::new();
        for tab in &self.tabs {
            if let Some(panes) = tab.get("panes").and_then(|p| p.as_array()) {
                for pane_id_val in panes {
                    if let Some(pane_id) = pane_id_val.as_str() {
                        if seen.insert(pane_id.to_string()) {
                            all_pane_ids.push(pane_id.to_string());
                        }
                        if let Some(label) = self.source_labels.get(pane_id) {
                            pane_labels_map.insert(pane_id.to_string(), label.clone());
                        } else if let Some(tab_labels) =
                            tab.get("pane_labels").and_then(|l| l.as_object())
                        {
                            if let Some(label) = tab_labels.get(pane_id).and_then(|v| v.as_str()) {
                                pane_labels_map.insert(pane_id.to_string(), label.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Compute pane stats.
        let mut total_lines = 0usize;
        let mut total_bytes = 0usize;
        let mut pane_stats: HashMap<String, (usize, usize)> = HashMap::new();
        for pane_id in &all_pane_ids {
            let entries = log_data.get(pane_id);
            let count = entries.map(|e| e.len()).unwrap_or(0);
            let bytes = entries
                .map(|e| e.iter().map(|entry| entry.text.len()).sum::<usize>())
                .unwrap_or(0);
            pane_stats.insert(pane_id.clone(), (count, bytes));
            total_lines += count;
            total_bytes += bytes;
        }

        // Build JSON serializations.
        let tabs_json = serde_json::to_string(&self.tabs)?;
        let panes_json = serde_json::to_string(&all_pane_ids)?;
        let pane_labels_json = serde_json::to_string(&pane_labels_map)?;
        let frontend_plugins_json = serde_json::to_string(&self.frontend_plugins)?;
        let pane_plugins_json = serde_json::to_string(&self.pane_plugins)?;
        let plugin_scripts_json = serde_json::to_string(&self.plugin_scripts)?;
        let markers_json = serde_json::to_string(&self.markers)?;
        let events_json = serde_json::to_string(&self.events)?;
        let event_rules_json = serde_json::to_string(&self.event_rules)?;

        // Build static profile.
        let static_profile = json!({
            "kind": "static",
            "capabilities": {
                "clearAll": false,
                "downloadRaw": true,
                "exportHtml": false,
                "fontSize": true,
                "paneSwap": true,
                "persistCache": false,
                "selectionExportHtml": true,
                "sessionApi": false,
                "themeToggle": true,
                "tx": false,
                "unwrap": true,
                "wsStatus": false,
                "dynamicTabs": false,
            },
        });
        let profile_json = serde_json::to_string(&static_profile)?;

        // Build config script.
        let config_js = esc_script_text(&format!(
            "window.__embedLogProfile = {profile_json};\n\
             window.TABS = {tabs_json};\n\
             window.PANES = {panes_json};\n\
             window.PANE_LABELS = {pane_labels_json};\n\
             window.__embedLogFrontendPlugins = {frontend_plugins_json};\n\
             window.__embedLogPanePlugins = {pane_plugins_json};\n\
             window.__embedLogPluginScripts = {plugin_scripts_json};\n\
             window.__embedLogInitialPanePluginUiState = {{}};\n\
             window.__embedLogInitialTimestampMode = {tm};\n\
             window.__embedLogFirstLogAt = {fla};\n\
             window.__embedLogInitialFontSize = 14;\n\
             window.__embedLogEventRules = {event_rules_json};\n\
             window.__embedLogEvents = {events_json};",
            tm = json!(self.timestamp_mode),
            fla = json!(effective_first_log_at),
        ));

        // Build pane data tags (lazy mode).
        let mut pane_data_tags = String::new();
        for pane_id in &all_pane_ids {
            let entries = log_data.get(pane_id);
            let compact: Vec<serde_json::Value> = entries
                .map(|es| {
                    es.iter()
                        .map(|e| {
                            let mut meta = json!({});
                            if let Some(ref abs_ts) = e.abs_ts {
                                meta["absTs"] = json!(abs_ts);
                            }
                            if let Some(abs_num) = e.abs_num {
                                meta["absNum"] = json!(abs_num);
                            }
                            if let Some(ref rel_ts) = e.rel_ts {
                                meta["relTs"] = json!(rel_ts);
                            }
                            if let Some(rel_num) = e.rel_num {
                                meta["relNum"] = json!(rel_num);
                            }
                            let meta_val =
                                if meta.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                                    meta
                                } else {
                                    json!(null)
                                };
                            json!([e.ts, e.text, e.is_tx, meta_val])
                        })
                        .collect()
                })
                .unwrap_or_default();
            let compact_json = serde_json::to_string(&compact)?;
            let escaped = compact_json.replace("</", "<\\/");
            pane_data_tags.push_str(&format!(
                "<script type=\"application/json\" data-pane=\"{pane_id}\">{escaped}</script>\n"
            ));
        }

        // Build bootstrap script.
        let bootstrap_js = esc_script_text(&format!(
            "(function () {{\n\
             \"use strict\";\n\
             window.wsSend = function () {{}};\n\
             if (typeof hydratePanesFromJson === \"function\") {{\n\
                 hydratePanesFromJson();\n\
             }}\n\
             if (typeof window.__embedLogUpdateTimestampModeUi === \"function\") {{\n\
                 window.__embedLogUpdateTimestampModeUi();\n\
             }}\n\
             var _markers = {markers_json};\n\
             if (_markers.length) {{\n\
                 state.markers = {{}};\n\
                 _markers.forEach(function (m) {{\n\
                     if (!m.paneId) return;\n\
                     state.markers[m.paneId] = state.markers[m.paneId] || [];\n\
                     state.markers[m.paneId].push(m);\n\
                 }});\n\
                 if (typeof applyMarkers === \"function\") applyMarkers();\n\
                 if (typeof window.__embedLogOnMarkers === \"function\") window.__embedLogOnMarkers();\n\
             }}\n\
             var _eventRules = window.__embedLogEventRules || {{}};\n\
             var _hasRules = Object.values(_eventRules).some(function (r) {{ return Array.isArray(r) && r.length > 0; }});\n\
             if (_hasRules) {{\n\
                 state.eventRules = _eventRules;\n\
                 state.eventsEnabled = true;\n\
                 if (typeof initEventsTab === \"function\") initEventsTab();\n\
                 var _events = window.__embedLogEvents || [];\n\
                 _events.forEach(function (ev) {{ if (typeof addEvent === \"function\") addEvent(ev); }});\n\
                 if (typeof renderTabBar === \"function\") renderTabBar();\n\
             }}\n\
             }})();"
        ));

        // Read and strip frontend JS files.
        let js_files = [
            "profile.js",
            "renderPane.js",
            "renderToolbar.js",
            "pluginRuntime.js",
            "state.js",
            "themes.js",
            "settings.js",
            "fontsize.js",
            "ansi.js",
            "lines.js",
            "tabs.js",
            "tabcreate.js",
            "ui.js",
            "export.js",
            "selection.js",
            "events.js",
            "tsparse.js",
            "import.js",
        ];
        let mut js_blocks = String::new();
        for &filename in &js_files {
            if let Some(src) = self.read_frontend_asset(filename) {
                let stripped = strip_module_syntax(&src);
                let escaped = esc_script_text(&stripped);
                js_blocks.push_str("<script>");
                js_blocks.push_str(&escaped);
                js_blocks.push_str("</script>\n");
            }
        }

        // Add plugin script tags.
        let mut plugin_script_tags = String::new();
        if let Some(scripts) = self.plugin_scripts.as_object() {
            for (name, script) in scripts {
                if let Some(script_str) = script.as_str() {
                    let escaped = esc_script_text(script_str);
                    plugin_script_tags.push_str("<script>");
                    plugin_script_tags.push_str(&escaped);
                    plugin_script_tags.push_str("</script>\n");
                    let _ = name; // plugin name not needed in tag
                }
            }
        }

        // Build pane HTML.
        let mut tab_contents = String::new();
        for (tab_idx, tab) in self.tabs.iter().enumerate() {
            let panes = tab.get("panes").and_then(|p| p.as_array());
            tab_contents.push_str(&format!(
                "    <div class=\"tab-content\" id=\"tab-content-{tab_idx}\">\n"
            ));
            if let Some(panes) = panes {
                for (i, pane_id_val) in panes.iter().enumerate() {
                    if let Some(pane_id) = pane_id_val.as_str() {
                        if i > 0 {
                            tab_contents.push_str("        <div class=\"splitter\"></div>\n");
                        }
                        let label = pane_labels_map
                            .get(pane_id)
                            .map(|s| s.as_str())
                            .unwrap_or(pane_id);
                        let stats = pane_stats.get(pane_id).copied().unwrap_or((0, 0));
                        let stats_text = stats_text(stats.0, stats.1);
                        tab_contents.push_str(&pane_html(pane_id, label, &stats_text));
                        tab_contents.push('\n');
                    }
                }
            }
            tab_contents.push_str("    </div>\n");
        }

        let total_stats = stats_text(total_lines, total_bytes);
        let title = self
            .tabs
            .iter()
            .filter_map(|t| t.get("label").and_then(|l| l.as_str()))
            .collect::<Vec<_>>()
            .join(" + ");

        // Assemble HTML.
        let mut html = String::with_capacity(
            css.len() + js_blocks.len() + config_js.len() + pane_data_tags.len() + 8192,
        );
        html.push_str("<!DOCTYPE html>\n");
        html.push_str("<html lang=\"en\" data-theme=\"whitesand\">\n");
        html.push_str("<head>\n");
        html.push_str("<meta charset=\"UTF-8\">\n");
        html.push_str(&format!("<title>embed-log — {title}</title>\n"));
        html.push_str("<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n");
        html.push_str("<link href=\"https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap\" rel=\"stylesheet\">\n");
        html.push_str("<style>");
        html.push_str(&css);
        html.push_str("</style>\n");
        html.push_str("</head>\n");
        html.push_str("<body>\n\n");

        html.push_str(&render_toolbar(&total_stats));
        html.push_str("\n\n");

        html.push_str("<div id=\"download-raw-menu\">\n");
        html.push_str("    <div class=\"download-raw-head\">Download raw logs</div>\n");
        html.push_str("    <div class=\"download-raw-body\">\n");
        html.push_str("        <button id=\"btn-download-merged\" class=\"download-raw-opt\">Merged (.log) — all panes interleaved</button>\n");
        html.push_str("        <button id=\"btn-download-split\" class=\"download-raw-opt\">Per pane (.log files) — one file per source</button>\n");
        html.push_str("    </div>\n");
        html.push_str("</div>\n\n");

        html.push_str("<div id=\"tab-bar\"></div>\n\n");
        html.push_str("<div id=\"container\">\n");
        html.push_str(&tab_contents);
        html.push_str("</div>\n\n");

        html.push_str("<script>");
        html.push_str(&config_js);
        html.push_str("</script>\n");
        html.push_str(&pane_data_tags);
        html.push_str(&js_blocks);
        html.push_str(&plugin_script_tags);
        html.push_str("<script>");
        html.push_str(&bootstrap_js);
        html.push_str("</script>\n");

        html.push_str("</body>\n");
        html.push_str("</html>\n");

        // Write.
        if let Some(parent) = self.html_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.html_path, &html)
            .with_context(|| format!("write session HTML {}", self.html_path.display()))?;
        info!("session HTML exported: {}", self.html_path.display());
        Ok(self.html_path.clone())
    }

    /// Read a frontend asset from embedded assets or filesystem.
    fn read_frontend_asset(&self, filename: &str) -> Option<String> {
        // Try embedded assets first.
        if let Some(file) = FrontendAssets::get(filename) {
            return String::from_utf8(file.data.to_vec()).ok();
        }
        // Fall back to filesystem.
        let path = self.frontend_dir.join(filename);
        std::fs::read_to_string(&path).ok()
    }
}

/// A parsed log entry with timestamp variants.
struct LogEntry {
    ts: String,
    text: String,
    is_tx: bool,
    abs_ts: Option<String>,
    abs_num: Option<i64>,
    rel_ts: Option<String>,
    rel_num: Option<i64>,
}

fn parse_log_file(content: &str, pane_id: Option<&str>, pane_label: Option<&str>) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    let mut pending: Option<LogEntry> = None;

    for raw_line in content.lines() {
        if let Some((ts, text)) = parse_line(raw_line) {
            // Flush previous entry.
            if let Some(entry) = pending.take() {
                entries.push(entry);
            }
            let is_tx = text.contains("[TX::");
            let clean_text = strip_embedlog_prefixes(&text, pane_id, pane_label);

            let (abs_ts, abs_num, rel_ts, rel_num) = if let Some(ms) = relative_ts_to_ms(&ts) {
                (None, None, Some(ts.clone()), Some(ms))
            } else {
                let abs_num = parse_absolute_to_ms(raw_line);
                (Some(ts.clone()), abs_num, None, None)
            };

            pending = Some(LogEntry {
                ts,
                text: clean_text,
                is_tx,
                abs_ts,
                abs_num,
                rel_ts,
                rel_num,
            });
        } else if raw_line.trim().is_empty() {
            continue;
        } else if let Some(ref mut entry) = pending {
            // Continuation line — append to previous entry.
            entry.text.push(' ');
            entry.text.push_str(raw_line.trim());
        }
    }
    if let Some(entry) = pending {
        entries.push(entry);
    }
    entries
}

fn parse_line(raw: &str) -> Option<(String, String)> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }

    // Strip ANSI prefix.
    let re = ansi_prefix_re();
    let (line, ansi_prefix) = if let Some(m) = re.find(line) {
        (&line[m.end()..], &line[..m.end()])
    } else {
        (line, "")
    };

    // [MM-DD HH:MM:SS.mmm] message
    let re = short_space_bracket_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[1],
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            ms3(caps.get(6).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[7])));
    }

    // [T+HH:MM:SS.mmm] message
    let re = relative_bracket_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "T+{}:{}:{}.{}",
            &caps[1],
            &caps[2],
            &caps[3],
            ms3(caps.get(4).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[5])));
    }

    // [YYYY-MM-DDTHH:MM:SS.mmm] message (full ISO bracket)
    let re = full_iso_bracket_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            &caps[6],
            ms3(caps.get(7).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[8])));
    }

    // Bare ISO: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DD HH:MM:SS
    let re = bare_iso_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            &caps[6],
            ms3(caps.get(7).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[8])));
    }

    let re = space_iso_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "{}-{} {}:{}:{}.{}",
            &caps[2],
            &caps[3],
            &caps[4],
            &caps[5],
            &caps[6],
            ms3(caps.get(7).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[8])));
    }

    // Bare relative: T+HH:MM:SS.mmm
    let re = bare_relative_re();
    if let Some(caps) = re.captures(line) {
        let ts = format!(
            "T+{}:{}:{}.{}",
            &caps[1],
            &caps[2],
            &caps[3],
            ms3(caps.get(4).map(|m| m.as_str()))
        );
        return Some((ts, format!("{ansi_prefix}{}", &caps[5])));
    }

    None
}

/// Enrich timestamp variants and return effective first_log_at.
fn enrich_timestamps(
    log_data: &mut HashMap<String, Vec<LogEntry>>,
    timestamp_mode: &str,
    first_log_at: &Option<String>,
) -> Option<String> {
    // Try to parse origin from first_log_at. Keep its fixed-offset wall clock
    // when deriving absolute display timestamps; Python merge_logs.py strips
    // timezone suffixes and preserves the supplied clock time.
    let mut origin_fixed: Option<DateTime<FixedOffset>> = None;
    if let Some(fla) = first_log_at {
        let token = if fla.ends_with('Z') {
            format!("{}+00:00", &fla[..fla.len() - 1])
        } else {
            fla.clone()
        };
        origin_fixed = DateTime::parse_from_rfc3339(&token).ok();
    }

    let mut origin_ms = origin_fixed.map(|dt| dt.timestamp_millis());

    // If no origin from first_log_at, try min absNum.
    if origin_ms.is_none() {
        origin_ms = log_data
            .values()
            .flat_map(|entries| entries.iter().filter_map(|e| e.abs_num))
            .min();
    }

    for entries in log_data.values_mut() {
        for entry in entries.iter_mut() {
            // Compute rel from abs.
            if let (None, Some(abs_num), Some(origin_ms)) =
                (entry.rel_num, entry.abs_num, origin_ms)
            {
                let rel = (abs_num - origin_ms).max(0);
                entry.rel_num = Some(rel);
                entry.rel_ts = Some(format_relative_ms(rel));
            }
            // Compute abs from rel. If the user supplied a fixed-offset origin,
            // preserve that origin's displayed clock rather than converting to
            // the machine's local timezone.
            if let (None, Some(rel_num)) = (entry.abs_num, entry.rel_num) {
                if let Some(origin) = origin_fixed {
                    let abs_dt = origin + chrono::Duration::milliseconds(rel_num);
                    entry.abs_num = Some(abs_dt.timestamp_millis());
                    entry.abs_ts = Some(abs_dt.format("%m-%d %H:%M:%S%.3f").to_string());
                } else if let Some(ms) = origin_ms {
                    if let Some(abs_utc) = Utc.timestamp_millis_opt(ms + rel_num).single() {
                        entry.abs_num = Some(abs_utc.timestamp_millis());
                        let local = abs_utc.with_timezone(&Local);
                        entry.abs_ts = Some(format_absolute_display(&local));
                    }
                }
            }
            // Set display ts based on mode.
            if timestamp_mode == "relative" {
                if let Some(rel_ts) = &entry.rel_ts {
                    entry.ts = rel_ts.clone();
                } else if let Some(abs_ts) = &entry.abs_ts {
                    entry.ts = abs_ts.clone();
                }
            } else if let Some(abs_ts) = &entry.abs_ts {
                entry.ts = abs_ts.clone();
            } else if let Some(rel_ts) = &entry.rel_ts {
                entry.ts = rel_ts.clone();
            }
        }
    }

    origin_fixed
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, false))
        .or_else(|| first_log_at.clone())
}

// ── Regex patterns (compiled once via OnceLock) ──

fn ansi_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:\x1b\[[0-9;]*m)+").unwrap())
}

fn short_space_bracket_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\[(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})\.(\d+)\]\s?(.*)").unwrap()
    })
}

fn relative_bracket_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[T\+(\d{1,2}):(\d{2}):(\d{2})\.(\d+)\]\s?(.*)").unwrap())
}

fn full_iso_bracket_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\[(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})\.(\d+)(?:[Zz]|[+-]\d{2}:?\d{2})?\]\s?(.*)").unwrap()
    })
}

fn bare_iso_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})\.(\d+)(?:[Zz]|[+-]\d{2}:?\d{2})?\s?(.*)").unwrap()
    })
}

fn space_iso_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})\.(\d+)(?:[Zz]|[+-]\d{2}:?\d{2})?\s?(.*)").unwrap()
    })
}

fn bare_relative_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^T\+(\d{1,2}):(\d{2}):(\d{2})\.(\d+)\s?(.*)").unwrap())
}

fn import_single_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?m)^import\s+.*?['"].*?['"]\s*;?\r?\n?"#).unwrap())
}

fn import_multi_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?m)^import\s*\{[^}]*\}\s*from\s*['"].*?['"]\s*;?\s*"#).unwrap())
}

fn export_decl_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?m)^export\s+(async\s+)?(function|class|const|let|var)\b").unwrap()
    })
}

fn export_stmt_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?m)^export\s*\{[^}]*\}\s*(?:from\s*['"].*?['"])?\s*;?\r?\n?"#).unwrap()
    })
}

fn script_close_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)</script").unwrap())
}

// ── Helpers ──

fn ms3(frac: Option<&str>) -> String {
    match frac {
        Some(f) => {
            let f = f.trim_end_matches('Z').trim_end_matches('z');
            if f.len() >= 3 {
                f[..3].to_string()
            } else {
                format!("{f:0<3}")
            }
        }
        None => "000".to_string(),
    }
}

fn relative_ts_to_ms(ts: &str) -> Option<i64> {
    let re = Regex::new(r"^T\+(\d{1,2}):(\d{2}):(\d{2})\.(\d+)$").unwrap();
    let caps = re.captures(ts)?;
    let h: i64 = caps[1].parse().ok()?;
    let m: i64 = caps[2].parse().ok()?;
    let s: i64 = caps[3].parse().ok()?;
    let ms_str = &caps[4];
    let ms: i64 = if ms_str.len() >= 3 {
        ms_str[..3].parse().ok()?
    } else {
        format!("{ms_str:0<3}")[..3].parse().ok()?
    };
    Some(h * 3_600_000 + m * 60_000 + s * 1000 + ms)
}

fn format_relative_ms(total_ms: i64) -> String {
    let neg = total_ms < 0;
    let total = total_ms.unsigned_abs();
    let hours = total / 3_600_000;
    let minutes = (total % 3_600_000) / 60_000;
    let seconds = (total % 60_000) / 1000;
    let millis = total % 1000;
    if neg {
        format!("T+-{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("T+{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    }
}

fn format_absolute_display(dt: &DateTime<Local>) -> String {
    dt.format("%m-%d %H:%M:%S%.3f").to_string()
}

fn parse_absolute_to_ms(raw: &str) -> Option<i64> {
    // Try full ISO: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DD HH:MM:SS
    let stripped = ansi_prefix_re().replace(raw, "").to_string();
    let line = stripped.trim();

    // Try bracket format first.
    let inner = if line.starts_with('[') {
        line.find(']').map(|end| &line[1..end])
    } else {
        Some(line)
    }?;

    // Parse YYYY-MM-DD[THH:MM:SS.mmm]
    let re =
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?").unwrap();
    let caps = re.captures(inner)?;
    let year: i32 = caps[1].parse().ok()?;
    let month: u32 = caps[2].parse().ok()?;
    let day: u32 = caps[3].parse().ok()?;
    let hour: u32 = caps[4].parse().ok()?;
    let min: u32 = caps[5].parse().ok()?;
    let sec: u32 = caps[6].parse().ok()?;
    let nano = caps
        .get(7)
        .map(|m| {
            let frac = m.as_str();
            let padded = format!("{frac:0<9}");
            padded[..9].parse::<u32>().unwrap_or(0)
        })
        .unwrap_or(0);

    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_nano_opt(hour, min, sec, nano)?;
    let ndt = NaiveDateTime::new(date, time);
    let local = Local.from_local_datetime(&ndt).single()?;
    Some(local.timestamp_millis())
}

fn strip_embedlog_prefixes(text: &str, pane_id: Option<&str>, pane_label: Option<&str>) -> String {
    let mut result = text.to_string();
    let mut variants = std::collections::HashSet::new();
    for value in [pane_id, pane_label].into_iter().flatten() {
        variants.insert(value.to_string());
        variants.insert(value.replace('-', "_"));
        variants.insert(value.replace('_', "-"));
    }
    for variant in &variants {
        let pattern = format!(r"(?i)^\s*\[{}\]\s*", regex::escape(variant));
        if let Ok(re) = Regex::new(&pattern) {
            result = re.replace(&result, "").to_string();
        }
    }
    // Remove [SERIAL] prefix.
    let re = Regex::new(r"(?i)^\s*\[SERIAL\]\s*").unwrap();
    result = re.replace(&result, "").to_string();
    result
}

fn strip_module_syntax(src: &str) -> String {
    let src = import_single_re().replace_all(src, "");
    let src = import_multi_re().replace_all(&src, "");
    let src = export_decl_re().replace_all(&src, "$1$2");
    let src = export_stmt_re().replace_all(&src, "");
    src.to_string()
}

fn esc_script_text(src: &str) -> String {
    script_close_re().replace_all(src, "<\\/script").to_string()
}

fn stats_text(line_count: usize, byte_count: usize) -> String {
    if line_count == 0 {
        return String::new();
    }
    format!("{} lines · {}", fmt_int(line_count), fmt_bytes(byte_count))
}

fn fmt_int(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(bytes.len() + bytes.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(b as char);
    }
    result
}

fn fmt_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        if n < 10 * 1024 {
            format!("{:.1} kB", n as f64 / 1024.0)
        } else {
            format!("{:.0} kB", n as f64 / 1024.0)
        }
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

fn pane_html(pane_id: &str, label: &str, stats_text: &str) -> String {
    let safe_label = html_escape(label);
    let stats_span = if stats_text.is_empty() {
        format!("<span class=\"pane-stats\" data-pane-stats=\"{pane_id}\"></span>")
    } else {
        format!(
            "<span class=\"pane-stats\" data-pane-stats=\"{pane_id}\">{}</span>",
            html_escape(stats_text)
        )
    };
    format!(
        "        <div class=\"pane\" id=\"pane-{pane_id}\">\n\
         \x20           <div class=\"pane-header\">\n\
         \x20               <span class=\"pane-name\">{safe_label}</span>\n\
         \x20               {stats_span}\n\n\
         \x20               <button class=\"pane-wrap-btn\" title=\"Toggle word wrap in this pane\">Wrap</button>\n\n\
         \x20           </div>\n\
         \x20           <div class=\"filter-bar\">\n\
         \x20               <input class=\"filter-input\" data-pane=\"{pane_id}\" placeholder=\"Filter (regex)…\">\n\
         \x20           </div>\n\
         \x20           <div class=\"pane-body\">\n\
         \x20               <div class=\"log-area\" id=\"log-{pane_id}\"><div class=\"log-spacer\"><div class=\"log-window\"></div></div></div>\n\
         \x20               <button class=\"jump-btn\" id=\"jump-{pane_id}\">jump to bottom</button>\n\
         \x20           </div>\n\
         \x20           <div class=\"input-row\" style=\"display:none\">\n\
         \x20               <input class=\"serial-input\" id=\"input-{pane_id}\" autocomplete=\"off\">\n\
         \x20               <button class=\"send-btn\" data-pane=\"{pane_id}\">Send</button>\n\
         \x20           </div>\n\
         \x20       </div>"
    )
}

fn render_toolbar(total_stats: &str) -> String {
    let safe_total = html_escape(total_stats);
    let stats_div = if safe_total.is_empty() {
        "<div id=\"toolbar-stats\" class=\"toolbar-stats\"></div>".to_string()
    } else {
        format!("<div id=\"toolbar-stats\" class=\"toolbar-stats\">· {safe_total}</div>")
    };
    format!(
        "<div id=\"toolbar\">\n\
         \x20   <span class=\"app-name\">embed-log</span>\n\
         \x20   <div class=\"sep\"></div>\n\
         \x20   <button id=\"btn-download-raw\" title=\"Download all logs as merged raw text file\">Download raw</button>\n\
         \x20   <button id=\"btn-unwrap\" title=\"Unwrap multi-pane tabs into single-pane tabs\">Unwrap</button>\n\
         \x20   <button id=\"btn-timestamp-mode\" title=\"Switch timestamps\">Absolute</button>\n\
         \x20   <div class=\"sep\"></div>\n\
         \x20   <button id=\"btn-theme\" title=\"Toggle light / dark theme\">&#x1F319;</button>\n\
         \x20   {stats_div}\n\
         \x20   <div id=\"marker-nav\" class=\"marker-nav\" style=\"display:none\">\n\
         \x20       <button id=\"marker-nav-prev\" title=\"Previous marker\">&#x25C0;</button>\n\
         \x20       <span id=\"marker-nav-idx\">1</span>/<span id=\"marker-nav-total\">0</span>\n\
         \x20       <button id=\"marker-nav-next\" title=\"Next marker\">&#x25B6;</button>\n\
         \x20   </div>\n\
         </div>"
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_short_space_bracket() {
        let (ts, msg) = parse_line("[06-15 14:30:05.123] hello world").unwrap();
        assert_eq!(ts, "06-15 14:30:05.123");
        assert_eq!(msg, "hello world");
    }

    #[test]
    fn parse_relative_bracket() {
        let (ts, msg) = parse_line("[T+00:00:05.250] boot ok").unwrap();
        assert_eq!(ts, "T+00:00:05.250");
        assert_eq!(msg, "boot ok");
    }

    #[test]
    fn parse_full_iso_bracket() {
        let (ts, msg) = parse_line("[2024-06-15T14:30:05.123] test").unwrap();
        assert_eq!(ts, "06-15 14:30:05.123");
        assert_eq!(msg, "test");
    }

    #[test]
    fn parse_line_no_timestamp_returns_none() {
        assert!(parse_line("raw message without timestamp").is_none());
    }

    #[test]
    fn parse_line_empty_returns_none() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
    }

    #[test]
    fn continuation_lines_join_previous() {
        let content = "[T+00:00:00.000] boot ok\nstack trace line 2\n[T+00:00:01.000] next";
        let entries = parse_log_file(content, None, None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "boot ok stack trace line 2");
        assert_eq!(entries[1].text, "next");
    }

    #[test]
    fn tx_detection() {
        let content = "[T+00:00:00.000] [TX::UI] ping";
        let entries = parse_log_file(content, None, None);
        assert!(entries[0].is_tx);
    }

    #[test]
    fn relative_ts_to_ms_correct() {
        assert_eq!(relative_ts_to_ms("T+00:00:00.000"), Some(0));
        assert_eq!(relative_ts_to_ms("T+00:00:01.250"), Some(1250));
        assert_eq!(relative_ts_to_ms("T+01:02:03.456"), Some(3_723_456));
    }

    #[test]
    fn strip_module_removes_imports() {
        let src = "import { foo } from './foo.js';\nexport function bar() {}\nconst x = 1;";
        let stripped = strip_module_syntax(src);
        assert!(!stripped.contains("import"));
        assert!(stripped.contains("function bar"));
        assert!(!stripped.contains("export"));
    }

    #[test]
    fn esc_script_replaces_close_tag() {
        let src = "var x = '</script>';";
        let escaped = esc_script_text(src);
        assert!(escaped.contains("<\\/script"));
        assert!(!escaped.contains("</script>"));
    }

    #[test]
    fn fmt_int_with_commas() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(1000), "1,000");
        assert_eq!(fmt_int(1234567), "1,234,567");
    }

    #[test]
    fn fmt_bytes_thresholds() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1024), "1.0 kB");
        assert_eq!(fmt_bytes(5120), "5.0 kB");
        assert_eq!(fmt_bytes(10240), "10 kB");
        assert_eq!(fmt_bytes(1048576), "1.0 MB");
    }
}
