import { STATIC_PROFILE } from './profile.js';
import { renderPaneShell } from './renderPane.js';
import { renderToolbar } from './renderToolbar.js';
const STATIC_EXPORT_PROFILE =
    typeof STATIC_PROFILE !== 'undefined' ? STATIC_PROFILE : window.__embedLogProfile;

import { state, TABS, PANES, PANE_LABELS } from './state.js';

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
            "profile.js", "renderPane.js", "renderToolbar.js", "viewer.css",
            "state.js", "themes.js", "settings.js", "fontsize.js",
            "ansi.js", "lines.js", "tabs.js", "tabcreate.js",
            "ui.js", "export.js", "selection.js", "tsparse.js", "import.js",
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
                // Remove multi-line imports (import { ... } from '...')
                .replace(/^import\s*\{[^}]*\}\s*from\s*['"][^'"]*['"]\s*;?\s*/gm, '')
                // Remove export keyword from declarations
                .replace(/^export\s+(async\s+)?(function|class|const|let|var)\b/gm, '$1$2')
                // Remove standalone export { ... } statements
                .replace(/^export\s*\{[^}]*\}\s*(?:from\s*['"][^'"]*['"])?\s*;?\r?\n?/gm, '');
        }

        const texts = await Promise.all(ASSETS.map(async a => {
            const r = await fetch(a, {cache: "no-store"});
            if (!r.ok) throw new Error(`Failed to fetch ${a}: ${r.status}`);
            const src = await r.text();
            return a.endsWith(".js") ? _escJs(_stripModuleSyntax(src)) : src;
        }));
        const [profileJs, renderPaneJs, renderToolbarJs, css, stateJs, themesJs, settingsJs, fontsizeJs,
               ansiJs, linesJs, tabsJs, tabcreateJs, uiJs, exportJs, selectionJs, tsparseJs, importJs] = texts;


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
            `window.__embedLogProfile = ${_safeJson(STATIC_EXPORT_PROFILE)};\n` +
            `window.TABS = ${_safeJson(TABS)};\n` +
            `window.PANES = ${_safeJson(PANES)};\n` +
            `window.PANE_LABELS = ${_safeJson(PANE_LABELS)};\n` +
            `window.__embedLogInitialThemeState = ${_safeJson(themeState)};\n` +
            `window.__embedLogInitialTimestampMode = ${_safeJson(state.timestampMode)};\n` +
            `window.__embedLogFirstLogAt = ${_safeJson(state.firstLogAt)};\n` +
            `window.__embedLogInitialFontSize = ${state.fontSize};`;



        // ------------------------------------------------------------------
        // Serialize all pane data with both timestamp representations when known.
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
                    ts: line.ts,
                    text: _rawOf(line),
                    isTx: line.isTx,
                    absTs: line.absTs ?? null,
                    absNum: Number.isFinite(line.absNum) ? line.absNum : null,
                    relTs: line.relTs ?? null,
                    relNum: Number.isFinite(line.relNum) ? line.relNum : null,
                }));
            });
        }

        // ------------------------------------------------------------------
        // Build pane + tab HTML (mirrors merge_logs.py's _pane_html /
        // _tab_content_html, with TX input row hidden in static mode)
        // ------------------------------------------------------------------
        function _paneHtml(paneId) {
            // Read the display name from the live DOM, fall back to paneId.
            const raw = document.querySelector(`#pane-${paneId} .pane-name`)?.textContent.trim() || paneId;
            // TX row is hidden in the static export output.
            return renderPaneShell(paneId, raw, { showTx: false });
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
        entries.forEach(function (e) { appendLine(paneId, e.ts, e.text, e.isTx, e); });
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

                ${renderToolbar(STATIC_EXPORT_PROFILE)}



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
<script>${profileJs}</script>
<script>${renderPaneJs}</script>
<script>${renderToolbarJs}</script>
<script>${stateJs}</script>
<script>${themesJs}</script>
<script>${settingsJs}</script>
<script>${fontsizeJs}</script>
<script>${ansiJs}</script>
<script>${linesJs}</script>
<script>${tabsJs}</script>
<script>${tabcreateJs}</script>
<script>${uiJs}</script>
<script>${exportJs}</script>
<script>${selectionJs}</script>
<script>${tsparseJs}</script>
<script>${importJs}</script>
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

if (window.__embedLogProfile?.capabilities?.exportHtml) {
    document.getElementById("btn-export")?.addEventListener("click", exportToHtml);
}
// Strip ANSI escape sequences from raw text.
function _stripAnsi(text) {
    return text.replace(/\x1b(?:\[[0-9;]*[A-Za-z]|\][^\x07]*\x07|[^[\]])/g, "");
}

// from raw text, producing clean plain text for download.
// Matches _snippetMessageText in selection.js.
function _cleanMessage(rawText) {
    let text = _stripAnsi(rawText ?? "").trim();
    for (let i = 0; i < 4; i++) {
        const before = text;
        text = text
            // Strip ISO timestamp prefix like [2026-05-24T22:59:41.773+02:00]
            .replace(/^\[\d{4}-\d{2}-\d{2}T[^\]]+\]\s*/, "")
            // Strip relative timestamp prefix like [T+00:00:01.234]
            .replace(/^\[T\+\d+:\d{2}:\d{2}\.\d{3}\]\s*/, "")
            // Strip bare short timestamp prefix like 05-24 23:05:51.109
            .replace(/^\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3}\s*/, "")
            // Strip bare relative timestamp prefix like T+00:00:01.234
            .replace(/^T\+\d+:\d{2}:\d{2}\.\d{3}\s*/, "")
            // Strip source label prefix like [SENSOR_A]
            .replace(/^\[[A-Za-z_][A-Za-z0-9_-]*\]\s*/, "")
            .trim();
        if (text === before) break;
    }
    return text;
}

function _lineRawFull(line) {
    return _cleanMessage(line.rawText);
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
        const text = lines.map(line => `[${line.ts}] ${_cleanMessage(line.rawText)}`).join("\n");
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
