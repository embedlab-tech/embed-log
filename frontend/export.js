import { state, TABS, PANES } from './state.js';

export async function exportHtmlSnapshot(options = {}) {
    const btn = options.button === undefined ? document.getElementById("btn-export") : options.button;
    const prev = btn?.textContent;
    if (btn) {
        btn.textContent = "…";
        btn.disabled = true;
    }

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
        const _escJs = src => src.replace(/<\/script/gi, "<\\/script");

        // Strip ES module import/export syntax so JS files can be embedded as
        // classic <script> blocks in the self-contained static HTML output.
        function _stripModuleSyntax(src) {
            return src
                // Remove import statements (single-line)
                .replace(/^import\s+.*?['"][^'"]*['"]\s*;?\r?\n?/gm, '')
                // Remove export keyword from declarations
                .replace(/^export\s+(async\s+)?(function|class|const|let|var)\b/gm, '$2')
                // Remove standalone export { ... } statements
                .replace(/^export\s*\{[^}]*\}\s*(?:from\s*['"][^'"]*['"])?\s*;?\r?\n?/gm, '');
        }

        const texts = await Promise.all(ASSETS.map(async a => {
            const r = await fetch(a, {cache: "no-store"});
            if (!r.ok) throw new Error(`Failed to fetch ${a}: ${r.status}`);
            const src = await r.text();
            return a.endsWith(".js") ? _escJs(_stripModuleSyntax(src)) : src;
        }));
        const [css, stateJs, ansiJs, linesJs, tabsJs, uiJs, settingsJs,
               tsparseJs, importJs, selectionJs, themesJs, tabcreateJs, exportJs] = texts;

        // ------------------------------------------------------------------
        // Capture current theme mode/palette and pass it to the exported file.
        // Avoid hard-overriding CSS vars in <style>, because that freezes the
        // theme toggle in static HTML.
        // ------------------------------------------------------------------
        const currentTheme = document.documentElement.dataset.theme || "";
        const themeState = window.__embedLogTheme?.getState?.() || {
            mode: currentTheme === "whitesand" ? "light" : "dark",
            lightKey: "whitesand",
            darkKey: "one-dark",
        };

        // ------------------------------------------------------------------
        // Config: inject TABS + PANES + initial theme state before scripts run
        // ------------------------------------------------------------------
        const configJs =
            `window.TABS = ${_safeJson(TABS)};\n` +
            `window.PANES = ${_safeJson(PANES)};\n` +
            `window.__embedLogInitialThemeState = ${_safeJson(themeState)};`;

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

        const logData = options.logData || {};
        if (!options.logData) {
            PANES.forEach(id => {
                logData[id] = state.rawLines[id].map(line => ({
                    ts: line.ts, text: _rawOf(line), isTx: line.isTx,
                }));
            });
        }

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

                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>

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
        const activeTabIdx = Number.isInteger(options.activeTab) ? options.activeTab : state.activeTab;
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
        const rawTitle = options.title || TABS.map(t => t.label).join(" + ");
        const title = rawTitle.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        const themeAttr = currentTheme ? ` data-theme="${currentTheme}"` : "";

        const html = `<!DOCTYPE html>
<html lang="en"${themeAttr}>
<head>
<meta charset="UTF-8">
<title>embed-log \u2014 ${title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
<style>${css}</style>
</head>
<body>

<div id="toolbar">
    <span class="app-name">embed-log</span>
    <button id="btn-clear"    title="Clear all panes">Clear</button>
    <button id="btn-export"   title="Export current session as a self-contained HTML file">Export HTML</button>
    <button id="btn-download-raw" title="Download all logs as merged raw text file">Download raw</button>
    <button id="btn-unwrap"  title="Unwrap multi-pane tabs into single-pane tabs">Unwrap</button>
    <div class="sep"></div>
    <button id="btn-theme" title="Toggle light / dark theme">\uD83C\uDF19</button>
    <div id="ws-status" style="display:none"></div>
</div>


<div id="download-raw-menu">
    <div class="download-raw-head">Download raw logs</div>
    <div class="download-raw-body">
        <button id="btn-download-merged" class="download-raw-opt">Merged (.log) — all panes interleaved</button>
        <button id="btn-download-split" class="download-raw-opt">Per pane (.log files) — one file per source</button>
    </div>
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
        a.download = `${options.filenamePrefix || "embed-log"}-${ts}.html`;
        a.click();
        URL.revokeObjectURL(url);

    } catch (err) {
        console.error("Export failed:", err);
        alert("Export failed: " + err.message);
    } finally {
        if (btn) {
            btn.textContent = prev;
            btn.disabled = false;
        }
    }
}

async function exportToHtml() {
    return exportHtmlSnapshot();
}

window.__embedLogExportSnapshot = exportHtmlSnapshot;

document.getElementById("btn-export")?.addEventListener("click", exportToHtml);

// ---------------------------------------------------------------------------
// Download all logs as merged raw text (one file, all panes interleaved)
// ---------------------------------------------------------------------------
function _lineRawFull(line) {
    return `${line.ts}  ${line.rawText ?? ""}`;
}

function downloadRawMerged() {
    const entries = [];
    PANES.forEach(id => {
        (state.rawLines[id] || []).forEach((line, idx) => {
            entries.push({ paneId: id, idx, line });
        });
    });
    entries.sort((a, b) =>
        (a.line.numTs - b.line.numTs) || a.paneId.localeCompare(b.paneId) || (a.idx - b.idx)
    );

    const text = entries.map(e => {
        return `[${e.line.ts}] [${e.paneId}] ${_lineRawFull(e.line)}`;
    }).join("\n");

    const now = new Date().toISOString().replace(/[:.]/g, "-").slice(0, -1);
    const blob = new Blob([text + "\n"], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `embed-log-merged-${now}.log`;
    a.click();
    URL.revokeObjectURL(url);
}

// ---------------------------------------------------------------------------
// Download raw — popup menu (merged or per pane)
// ---------------------------------------------------------------------------
function downloadRawSplit() {
    PANES.forEach(id => {
        const lines = state.rawLines[id] || [];
        if (!lines.length) return;
        const text = lines.map(line => `${line.ts}  ${line.rawText ?? ""}`).join("\n");
        const blob = new Blob([text + "\n"], { type: "text/plain" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = `${id}.log`;
        a.click();
        URL.revokeObjectURL(url);
    });
}

function _toggleRawMenu() {
    const btn = document.getElementById("btn-download-raw");
    const menu = document.getElementById("download-raw-menu");
    if (!menu || !btn) return;

    const isOpen = menu.classList.contains("open");
    document.querySelectorAll("#clip-peek-menu.open, #sessions-menu.open").forEach(el =>
        el.classList.remove("open")
    );

    if (isOpen) {
        menu.classList.remove("open");
        return;
    }

    const rect = btn.getBoundingClientRect();
    menu.style.left = `${Math.max(8, rect.left)}px`;
    menu.style.top = `${rect.bottom + 6}px`;
    menu.classList.add("open");
}

function _closeRawMenu() {
    document.getElementById("download-raw-menu")?.classList.remove("open");
}

document.getElementById("btn-download-raw")?.addEventListener("click", e => {
    e.stopPropagation();
    _toggleRawMenu();
});

document.getElementById("btn-download-merged")?.addEventListener("click", e => {
    e.stopPropagation();
    downloadRawMerged();
    _closeRawMenu();
});

document.getElementById("btn-download-split")?.addEventListener("click", e => {
    e.stopPropagation();
    downloadRawSplit();
    _closeRawMenu();
});

document.addEventListener("click", e => {
    const menu = document.getElementById("download-raw-menu");
    const btn = document.getElementById("btn-download-raw");
    if (!menu?.classList.contains("open")) return;
    if (menu.contains(e.target) || btn?.contains(e.target)) return;
    _closeRawMenu();
}, true);

document.addEventListener("keydown", e => {
    if (e.key === "Escape") _closeRawMenu();
});
