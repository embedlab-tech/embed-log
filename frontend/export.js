"use strict";

async function exportToHtml() {
    const btn  = document.getElementById("btn-export");
    const prev = btn.textContent;
    btn.textContent = "…";
    btn.disabled    = true;

    // Escape <\/script> sequences in embedded JSON so they can't break the
    // surrounding <\/script> tag — same issue affects any log message.
    function _safeJson(obj) {
        return JSON.stringify(obj).replace(/<\//g, "<\\/");
    }

    try {
        // ------------------------------------------------------------------
        // Fetch all frontend assets in parallel (same order as index.html).
        // ws.js is intentionally omitted — no WebSocket in a static file.
        // cache: "no-store" ensures we always bundle the latest files, not
        // whatever the browser has cached from a previous page load.
        // ------------------------------------------------------------------
        const ASSETS = [
            "viewer.css", "state.js", "ansi.js",   "lines.js",  "tabs.js",
            "ui.js",      "settings.js", "tsparse.js", "import.js", "selection.js",
            "themes.js",  "tabcreate.js", "export.js",
        ];
        // _escJs: make a JS source string safe to embed inside <script>…<\/script>.
        // The HTML parser ends a script block at the first <\/script it sees
        // (case-insensitive), even inside comments or string literals.
        // Replacing <\/script with <\/script is invisible to the JS engine but
        // prevents the HTML tokeniser from closing the tag early.
        const _escJs = src => src.replace(/<\/script/gi, "<\\/script");

        const texts = await Promise.all(ASSETS.map(async a => {
            const r = await fetch(a, {cache: "no-store"});
            if (!r.ok) throw new Error(`Failed to fetch ${a}: ${r.status}`);
            return _escJs(await r.text());
        }));
        const [css, stateJs, ansiJs, linesJs, tabsJs, uiJs, settingsJs,
               tsparseJs, importJs, selectionJs, themesJs, tabcreateJs, exportJs] = texts;

        // ------------------------------------------------------------------
        // Capture current theme and all CSS variable values so the exported
        // file looks identical to what the user sees right now.
        // ------------------------------------------------------------------
        const currentTheme = document.documentElement.dataset.theme || "";
        const cs = getComputedStyle(document.documentElement);
        const v  = name => cs.getPropertyValue(name).trim();
        const CSS_VARS = [
            "--bg", "--panel", "--header", "--text", "--accent",
            "--border", "--selection", "--input-bg",
            "--ansi-0", "--ansi-1", "--ansi-2", "--ansi-3",
            "--ansi-4", "--ansi-5", "--ansi-6", "--ansi-7",
            "--font-family", "--font-size",
        ];
        const cssOverride =
            `:root {\n` +
            CSS_VARS.map(p => `    ${p}: ${v(p) || "inherit"};`).join("\n") +
            `\n}`;

        // ------------------------------------------------------------------
        // Config: inject TABS + PANES before state.js so it skips its defaults
        // ------------------------------------------------------------------
        const configJs =
            `var TABS = ${_safeJson(TABS)};\n` +
            `var PANES = ${_safeJson(PANES)};`;

        // ------------------------------------------------------------------
        // Serialize all pane data as { ts, text, isTx }.
        // rawText may be absent on lines loaded before this session; fall back
        // to decoding the stored HTML via a temporary element (strips tags,
        // decodes entities) so the export always has something to render.
        // ------------------------------------------------------------------
        const _tmpEl = document.createElement("div");
        function _rawOf(line) {
            if (line.rawText !== undefined) return line.rawText;
            _tmpEl.innerHTML = line.html;
            return _tmpEl.textContent;
        }

        const logData = {};
        PANES.forEach(id => {
            logData[id] = state.rawLines[id].map(line => ({
                ts: line.ts, text: _rawOf(line), isTx: line.isTx,
            }));
        });

        // ------------------------------------------------------------------
        // Build pane + tab HTML (mirrors merge_logs.py's _pane_html /
        // _tab_content_html, with TX input row hidden in static mode)
        // ------------------------------------------------------------------
        function _paneHtml(paneId) {
            const raw   = document.querySelector(`#pane-${paneId} .pane-name`)?.textContent.trim() || paneId;
            const label = raw.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
            return `        <div class="pane" id="pane-${paneId}">
            <div class="pane-header">
                <span class="pane-name">${label}</span>
                <button class="pane-clear-btn" data-pane="${paneId}">clear</button>
            </div>
            <div class="filter-bar">
                <input class="filter-input" data-pane="${paneId}" placeholder="Filter (regex)\u2026">
            </div>
            <div class="pane-body">
                <div class="log-area" id="log-${paneId}"></div>
                <button class="jump-btn" id="jump-${paneId}">jump to bottom</button>
            </div>
            <div class="input-row" style="display:none">
                <input class="serial-input" id="input-${paneId}" autocomplete="off">
                <button class="send-btn" data-pane="${paneId}">Send</button>
            </div>
        </div>`;
        }

        const tabContentsHtml = TABS.map((tab, i) => {
            const inner = tab.panes.map((paneId, j) =>
                (j > 0 ? '        <div class="splitter"></div>\n' : "") + _paneHtml(paneId)
            ).join("\n");
            return `    <div class="tab-content" id="tab-content-${i}">\n${inner}\n    </div>`;
        }).join("\n");

        // ------------------------------------------------------------------
        // Bootstrap: runs last, populates all panes with log data
        // ------------------------------------------------------------------
        const activeTabIdx = state.activeTab;
        const bootstrapJs = `(function () {
    "use strict";
    window.wsSend = function () {};
    var _logData = ${_safeJson(logData)};
    function _loadPane(paneId) {
        var entries = _logData[paneId];
        if (!entries || !entries.length) return;
        state.atBottom[paneId] = false;
        entries.forEach(function (e) { appendLine(paneId, e.ts, e.text, e.isTx); });
        document.getElementById("log-" + paneId).scrollTop = 0;
        state.atBottom[paneId] = false;
        updateJumpBtn(paneId);
    }
    PANES.forEach(_loadPane);
    // Restore the tab that was active when the export was taken
    if (${activeTabIdx} !== 0) switchTab(${activeTabIdx});
})();`;

        // ------------------------------------------------------------------
        // Assemble final HTML
        // ------------------------------------------------------------------
        const ts    = new Date().toISOString().replace(/[:.]/g, "-").slice(0, -1);
        const title = TABS.map(t => t.label).join(" + ")
            .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        const themeAttr = currentTheme ? ` data-theme="${currentTheme}"` : "";

        const html = `<!DOCTYPE html>
<html lang="en"${themeAttr}>
<head>
<meta charset="UTF-8">
<title>log-viewer \u2014 ${title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
<style>${css}</style>
<style>${cssOverride}</style>
</head>
<body>

<div id="toolbar">
    <span class="app-name">log-viewer</span>
    <button id="btn-wrap"     title="Toggle word wrap">Wrap</button>
    <button id="btn-ts"       title="Toggle timestamps" class="active">Time</button>
    <button id="btn-sync"     title="Click a line to sync the other pane to the same timestamp" class="active">Sync</button>
    <div class="sep"></div>
    <button id="btn-font-dec" title="Decrease font size">A-</button>
    <button id="btn-font-inc" title="Increase font size">A+</button>
    <div class="sep"></div>
    <button id="btn-clear"    title="Clear all panes">Clear</button>
    <button id="btn-export"   title="Export current tab to a self-contained HTML file">Export</button>
    <div class="sep"></div>
    <button id="btn-theme" title="Toggle light / dark theme">\uD83C\uDF19</button>
    <div id="ws-status" style="display:none"></div>
</div>

<div id="tab-bar"></div>

<div id="container">
${tabContentsHtml}
</div>

<script>${configJs}</script>
<script>${stateJs}</script>
<script>${ansiJs}</script>
<script>${linesJs}</script>
<script>${tabsJs}</script>
<script>${uiJs}</script>
<script>${settingsJs}</script>
<script>${tsparseJs}</script>
<script>${importJs}</script>
<script>${selectionJs}</script>
<script>${themesJs}</script>
<script>${tabcreateJs}</script>
<script>${exportJs}</script>
<script>${bootstrapJs}</script>
</body>
</html>`;

        const blob = new Blob([html], { type: "text/html" });
        const url  = URL.createObjectURL(blob);
        const a    = document.createElement("a");
        a.href     = url;
        a.download = `log-viewer-${ts}.html`;
        a.click();
        URL.revokeObjectURL(url);

    } catch (err) {
        console.error("Export failed:", err);
        alert("Export failed: " + err.message);
    } finally {
        btn.textContent = prev;
        btn.disabled    = false;
    }
}

document.getElementById("btn-export").addEventListener("click", exportToHtml);
