use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::json;
use tracing::info;

use super::log_parse::{enrich_timestamps, parse_log_file, LogEntry};
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
        let css = self.inline_font_urls(&css);

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
                "markers": false,
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
             window.__embedLogInitialThemeState = {{\"mode\":\"light\",\"lightKey\":\"whitesand\",\"darkKey\":\"one-dark\"}};\n\
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
             var _events = window.__embedLogEvents || [];\n\
             var _hasRules = Object.values(_eventRules).some(function (r) {{ return Array.isArray(r) && r.length > 0; }});\n\
             if (_hasRules || _events.length) {{\n\
                 state.eventRules = _eventRules;\n\
                 state.eventsEnabled = true;\n\
                 if (typeof initEventsTab === \"function\") initEventsTab();\n\
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
            "postprocess.js",
            "selection.js",
            "events.js",
            "tsparse.js",
            "import.js",
        ];
        let mut js_blocks = String::new();
        for &filename in &js_files {
            if filename == "state.js" {
                js_blocks.push_str(&plugin_script_tags(&self.plugin_scripts));
            }
            if let Some(src) = self.read_frontend_asset(filename) {
                let stripped = strip_module_syntax(&src);
                let escaped = esc_script_text(&stripped);
                js_blocks.push_str("<script>");
                js_blocks.push_str(&escaped);
                js_blocks.push_str("</script>\n");
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
                        tab_contents.push_str(&pane_html(pane_id, label));
                        tab_contents.push('\n');
                    }
                }
            }
            tab_contents.push_str("    </div>\n");
        }

        let title = self
            .tabs
            .iter()
            .filter_map(|t| t.get("label").and_then(|l| l.as_str()))
            .collect::<Vec<_>>()
            .join(" + ");
        let safe_title = html_escape(&title);

        // Assemble HTML.
        let mut html = String::with_capacity(
            css.len() + js_blocks.len() + config_js.len() + pane_data_tags.len() + 8192,
        );
        html.push_str("<!DOCTYPE html>\n");
        html.push_str("<html lang=\"en\" data-theme=\"whitesand\">\n");
        html.push_str("<head>\n");
        html.push_str("<meta charset=\"UTF-8\">\n");
        html.push_str(&format!("<title>embed-log — {safe_title}</title>\n"));
        html.push_str("<style>");
        html.push_str(&css);
        html.push_str("</style>\n");
        html.push_str("</head>\n");
        html.push_str("<body>\n\n");

        html.push_str(&render_toolbar());
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

    /// Read a binary frontend asset (e.g. a font file) from embedded assets or filesystem.
    fn read_frontend_asset_bytes(&self, filename: &str) -> Option<Vec<u8>> {
        if let Some(file) = FrontendAssets::get(filename) {
            return Some(file.data.to_vec());
        }
        let path = self.frontend_dir.join(filename);
        std::fs::read(&path).ok()
    }

    /// Replace `url('fonts/...')` references in the CSS with base64 data URIs.
    /// The exported HTML is a standalone file (often opened via `file://`), so
    /// relative font URLs and the CDN `@font-face` fallback both 404 — embedding
    /// the bytes directly is the only way the bundled font renders offline.
    fn inline_font_urls(&self, css: &str) -> String {
        use base64::Engine;
        font_url_re()
            .replace_all(css, |caps: &regex::Captures| {
                let rel_path = &caps[1];
                match self.read_frontend_asset_bytes(rel_path) {
                    Some(bytes) => {
                        let mime = mime_guess::from_path(rel_path).first_or_octet_stream();
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        format!("url(data:{mime};base64,{b64})")
                    }
                    None => caps[0].to_string(),
                }
            })
            .into_owned()
    }
}

fn font_url_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"url\('(fonts/[^']+)'\)"#).unwrap())
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

fn plugin_script_tags(plugin_scripts: &serde_json::Value) -> String {
    let mut tags = String::new();
    if let Some(scripts) = plugin_scripts.as_object() {
        for script in scripts.values() {
            if let Some(script_str) = script.as_str() {
                tags.push_str("<script>");
                tags.push_str(&esc_script_text(script_str));
                tags.push_str("</script>\n");
            }
        }
    }
    tags
}

fn pane_html(pane_id: &str, label: &str) -> String {
    let safe_label = html_escape(label);
    format!(
        "        <div class=\"pane\" id=\"pane-{pane_id}\">\n\
         \x20           <div class=\"pane-header\">\n\
         \x20               <span class=\"pane-name\">{safe_label}</span>\n\
         \x20               <span class=\"pane-stats\" data-pane-stats=\"{pane_id}\"></span>\n\n\
         \x20               <button class=\"pane-wrap-btn\" title=\"Toggle word wrap in this pane\">Wrap</button>\n\
         \x20               <button class=\"pane-download-btn\" title=\"Download raw .log for this pane\">Download</button>\n\
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

fn render_toolbar() -> String {
    [
        "<div id=\"toolbar\">",
        "    <div class=\"toolbar-group toolbar-left\">",
        "        <span class=\"app-name\">embed-log</span>",
        "        <button id=\"btn-unwrap\" title=\"Unwrap multi-pane tabs into single-pane tabs\">Unwrap</button>",
        "        <button id=\"btn-timestamp-mode\" title=\"Switch timestamps\">Absolute</button>",
        "        <div class=\"sep\"></div>",
        "        <button id=\"btn-theme\" title=\"Toggle light / dark theme\">&#x1F319;</button>",
        "    </div>",
        "    <button id=\"btn-jump-all\" class=\"btn-live\" title=\"Jump every pane to its latest line and keep tab switches live (Shift+L)\">Live</button>",
        "    <div class=\"toolbar-group toolbar-right\">",
        "        <div id=\"toolbar-stats\" class=\"toolbar-stats\"></div>",
        "        <div id=\"marker-nav\" class=\"marker-nav\" style=\"display:none\">",
        "            <button id=\"marker-nav-prev\" title=\"Previous marker\">&#x25C0;</button>",
        "            <span id=\"marker-nav-idx\">1</span>/<span id=\"marker-nav-total\">0</span>",
        "            <button id=\"marker-nav-next\" title=\"Next marker\">&#x25B6;</button>",
        "        </div>",
        "    </div>",
        "</div>",
    ]
    .join("\n")
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
    fn inline_font_urls_embeds_bundled_font_as_data_uri() {
        let exporter = SessionExporter::new(
            PathBuf::from("/tmp/unused.html"),
            HashMap::new(),
            vec![],
            HashMap::new(),
            PathBuf::from("frontend"), // relative to crates/embed-log-core, matches rust-embed folder
            "absolute".to_string(),
            None,
        );
        let css = "@font-face { src: url('fonts/JetBrainsMono-Regular.woff2'); }";
        let out = exporter.inline_font_urls(css);
        assert!(!out.contains("fonts/JetBrainsMono-Regular.woff2"));
        assert!(out.contains("url(data:font/"));
        assert!(out.contains(";base64,"));
    }
}
