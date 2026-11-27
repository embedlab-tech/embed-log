import { STATIC_PROFILE } from './profile.js';
import { renderPaneShell } from './renderPane.js';
import { renderToolbar } from './renderToolbar.js';
import {
    getConfiguredFrontendPlugins, getConfiguredPanePlugins, getConfiguredPluginScripts, getPanePluginUiState,
} from './pluginRuntime.js';
const STATIC_EXPORT_PROFILE =
    typeof STATIC_PROFILE !== 'undefined' ? STATIC_PROFILE : window.__embedLogProfile;

import { renderPaneWindow, updateJumpBtn, getLine, hydratePaneRows, refreshStatsUi, markerForExport } from './lines.js';

import { state, TABS, PANES, PANE_LABELS } from './state.js';
import { isEditableTarget, isComposingEvent } from './keyboard.js';

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
            "profile.js", "keyboard.js", "renderPane.js", "renderToolbar.js", "viewer.css",
            "pluginRuntime.js", "state.js", "themes.js", "settings.js", "fontsize.js",
            "ansi.js", "levels.js", "search.js", "timeSync.js", "lines.js", "tabs.js", "tabcreate.js",
            "ui.js", "export.js", "postprocess.js", "selection.js", "tsparse.js", "import.js",
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
        const [profileJs, keyboardJs, renderPaneJs, renderToolbarJs, css, pluginRuntimeJs, stateJs, themesJs, settingsJs, fontsizeJs,
               ansiJs, levelsJs, searchJs, timeSyncJs, linesJs, tabsJs, tabcreateJs, uiJs, exportJs, postprocessJs, selectionJs, tsparseJs, importJs] = texts;


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
        const panePluginUiState = getPanePluginUiState();
        const frontendPlugins = getConfiguredFrontendPlugins();
        const panePlugins = getConfiguredPanePlugins();
        const existingPluginScripts = getConfiguredPluginScripts();
        const activePluginNames = [...new Set(Object.values(panePlugins)
            .flatMap(refs => Array.isArray(refs) ? refs : [])
            .map(ref => ref?.name)
            .filter(name => typeof name === "string" && name && frontendPlugins[name]))];
        const pluginScripts = {};
        for (const name of activePluginNames) {
            if (typeof existingPluginScripts[name] === "string" && existingPluginScripts[name]) {
                pluginScripts[name] = existingPluginScripts[name];
            }
        }
        const pluginScriptTags = activePluginNames
            .map(name => pluginScripts[name])
            .filter(Boolean)
            .map(src => `<script>${_escJs(src)}</script>`)
            .join("\n");

        // ------------------------------------------------------------------
        // Config: inject TABS + PANES + initial theme state before scripts run
        // ------------------------------------------------------------------
        const configJs =
            `window.__embedLogProfile = ${_safeJson(STATIC_EXPORT_PROFILE)};\n` +
            `window.TABS = ${_safeJson(TABS)};\n` +
            `window.PANES = ${_safeJson(PANES)};\n` +
            `window.PANE_LABELS = ${_safeJson(PANE_LABELS)};\n` +
            `window.__embedLogFrontendPlugins = ${_safeJson(Object.fromEntries(activePluginNames.map(name => [name, frontendPlugins[name]])))};\n` +
            `window.__embedLogPanePlugins = ${_safeJson(panePlugins)};\n` +
            `window.__embedLogPluginScripts = ${_safeJson(pluginScripts)};\n` +
            `window.__embedLogMerges = ${_safeJson(window.__embedLogMerges || [])};\n` +
            (window.__embedLogSourceLevels ? `window.__embedLogSourceLevels = ${_safeJson(window.__embedLogSourceLevels)};\n` : "") +
            `window.__embedLogInitialPanePluginUiState = ${_safeJson(panePluginUiState)};\n` +
            `window.__embedLogInitialThemeState = ${_safeJson(themeState)};\n` +
            `window.__embedLogInitialTimestampMode = ${_safeJson(state.timestampMode === "hidden" ? state.sessionTimestampMode : state.timestampMode)};\n` +
            `window.__embedLogFirstLogAt = ${_safeJson(state.firstLogAt)};\n` +
            `window.__embedLogInitialFontSize = ${state.fontSize};`;



        // ------------------------------------------------------------------
        // Serialize selection-export pane data as compact JSON tuples for
        // fast hydration with windowed rendering.
        // rawText may be absent on lines loaded before this session; fall back
        // to decoding the stored HTML via a temporary element.
        // ------------------------------------------------------------------
        const _tmpEl = document.createElement("div");
        function _rawOf(line) {
            if (Array.isArray(line)) return line[1];

            if (line.rawText !== undefined) return line.rawText;
            _tmpEl.innerHTML = line.html;
            return _tmpEl.textContent;
        }

        // Compact tuple: [ts, text, isTx, meta|null]
        function _compactEntry(line, text) {
            if (Array.isArray(line)) return line;
            const meta = {};
            if (line.absTs != null) meta.absTs = line.absTs;
            if (Number.isFinite(line.absNum)) meta.absNum = line.absNum;
            if (line.relTs != null) meta.relTs = line.relTs;
            if (Number.isFinite(line.relNum)) meta.relNum = line.relNum;
            if (line.sourceId) meta.sourceId = line.sourceId;
            if (line.level) meta.level = line.level;
            if (line.sessionId) meta.sessionId = line.sessionId;
            // Markers name server line indexes; without it the page would use positions.
            const lineIdx = line.serverLineIdx ?? line.lineIdx;
            if (Number.isFinite(lineIdx)) meta.lineIdx = lineIdx;
            if (Number.isFinite(line.sequence)) meta.sequence = line.sequence;
            if (Number.isFinite(line.recordOrdinal)) meta.recordOrdinal = line.recordOrdinal;
            if (Number.isFinite(line.originalBytes)) meta.originalBytes = line.originalBytes;
            return [line.ts, text ?? _rawOf(line), line.isTx, Object.keys(meta).length ? meta : null];
        }

        const logData = options.logData || {};
        let paneDataTags = "";
        if (!options.logData) {
            PANES.forEach(id => {
                const lines = state.rawLines[id] || [];
                if (!lines.length) return;
                paneDataTags += `<script type="application/json" data-pane="${id}">${_safeJson(lines.map(line => _compactEntry(line)))}</script>\n`;
            });
        } else {
            // Custom logData passed in (e.g. from selection export)
            Object.entries(options.logData).forEach(([paneId, entries]) => {
                if (!entries || !entries.length) return;
                paneDataTags += `<script type="application/json" data-pane="${paneId}">${_safeJson(entries.map(e => _compactEntry(e, e.text)))}</script>\n`;
            });
        }
        // ------------------------------------------------------------------
        // Build pane + tab HTML for selection-only snapshots, with the TX
        // input row hidden in static mode.
        // ------------------------------------------------------------------
        function _paneHtml(paneId) {
            const raw = document.querySelector(`#pane-${paneId} .pane-name`)?.textContent.trim() || paneId;
            return renderPaneShell(paneId, raw, { showTx: false });
        }

        const tabContentsHtml = TABS.map((tab, i) => {
            const inner = tab.panes.map((paneId, j) =>
                (j > 0 ? '        <div class="splitter"></div>\n' : "") + _paneHtml(paneId)
            ).join("\n");
            return `    <div class="tab-content" id="tab-content-${i}">\n${inner}\n    </div>`;
        }).join("\n");

        // Panes removed from every view are still in PANES, so the page can add them
        // back: give them elements, parked out of sight like in the live viewer.
        const hiddenPanesHtml = PANES.filter(id => !TABS.some(tab => tab.panes.includes(id)))
            .map(id => _paneHtml(id)).join("\n");

        // ------------------------------------------------------------------
        // Bootstrap: lazy hydration via compact JSON + windowed rendering
        // ------------------------------------------------------------------
        const activeTabIdx = Number.isInteger(options.activeTab) ? options.activeTab : state.activeTab;
        const bootstrapJs = `(function () {
    "use strict";
    window.wsSend = function () {};
    if (typeof hydratePanesFromJson === "function") {
        hydratePanesFromJson();
    }
    if (typeof window.__embedLogUpdateTimestampModeUi === "function") {
        window.__embedLogUpdateTimestampModeUi();
    }
    var _markers = ${_safeJson(Object.entries(state.markers).flatMap(([paneId, list]) => (list || []).map(marker => markerForExport(paneId, marker))))};
    if (_markers.length) {
        state.markers = {};
        _markers.forEach(function (m) {
            if (!m.paneId) return;
            state.markers[m.paneId] = state.markers[m.paneId] || [];
            state.markers[m.paneId].push(m);
        });
        if (typeof applyMarkers === "function") applyMarkers();
        if (typeof window.__embedLogOnMarkers === "function") window.__embedLogOnMarkers();
    }
    if (${activeTabIdx} !== 0) switchTab(${activeTabIdx});
    (function () {
        var m = window.location.hash.match(/^#marker-(\\d+)$/);
        if (!m) return;
        var idx = parseInt(m[1], 10);
        if (!Number.isFinite(idx) || idx < 1) return;
        var flat = [];
        Object.keys(state.markers).forEach(function (pid) {
            (state.markers[pid] || []).forEach(function (mk) {
                flat.push(Object.assign({}, mk, { paneId: pid }));
            });
        });
        flat.sort(function (a, b) { return (a.numTs || 0) - (b.numTs || 0); });
        if (idx > flat.length) return;
        var target = flat[idx - 1];
        var tabIdx = -1;
        for (var t = 0; t < TABS.length; t++) {
            if (TABS[t].panes.indexOf(target.paneId) >= 0) { tabIdx = t; break; }
        }
        if (tabIdx >= 0 && typeof switchTab === 'function') switchTab(tabIdx);
        // Markers name server line indexes; find the row that holds that line.
        var rawIdx = typeof markerRawIndex === 'function' ? markerRawIndex(target.paneId, target) : target.lineIdx;
        if (rawIdx === null) return;
        if (typeof ensureLineVisible === 'function') ensureLineVisible(target.paneId, rawIdx, { align: 'center' });
        var div = document.querySelector('#log-' + target.paneId + ' [data-idx="' + rawIdx + '"]');
        if (!div) return;
        var logEl = document.getElementById('log-' + target.paneId);
        if (!logEl) return;
        state.atBottom[target.paneId] = false;
        if (typeof onLineClick === 'function') onLineClick(target.paneId, target.numTs, div);
    })();
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
<style>${css}</style>
</head>
<body>

                ${renderToolbar(STATIC_EXPORT_PROFILE)}

<div id="download-raw-menu">
    <div class="download-raw-head">Download raw logs</div>
    <div class="download-raw-body">
        <button id="btn-download-merged" class="download-raw-opt">Merged (.log) — all panes interleaved</button>
        <button id="btn-download-split" class="download-raw-opt">Per pane (.zip) — all sources</button>
    </div>
</div>

<div id="tab-bar"></div>

<div id="container">
${tabContentsHtml}
<div id="hidden-panes">
${hiddenPanesHtml}
</div>
</div>

<script>${configJs}</script>
${paneDataTags}
<script>${profileJs}</script>
<script>${keyboardJs}</script>
<script>${renderPaneJs}</script>
<script>${renderToolbarJs}</script>
<script>${pluginRuntimeJs}</script>
${pluginScriptTags}
<script>${stateJs}</script>
<script>${themesJs}</script>
<script>${settingsJs}</script>
<script>${fontsizeJs}</script>
<script>${ansiJs}</script>
<script>${levelsJs}</script>
<script>${searchJs}</script>
<script>${timeSyncJs}</script>
<script>${linesJs}</script>
<script>${tabsJs}</script>
<script>${tabcreateJs}</script>
<script>${uiJs}</script>
<script>${exportJs}</script>
<script>${postprocessJs}</script>
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
    const btn = document.getElementById("btn-export");
    const previous = btn?.textContent;
    if (btn) {
        btn.textContent = "…";
        btn.disabled = true;
    }
    try {
        const response = await fetch("/api/session/export?download=true", {
            method: "POST",
            headers: { "Accept": "text/html" },
        });
        if (!response.ok || !response.headers.get("content-type")?.includes("text/html")) {
            const result = await response.json().catch(() => ({}));
            throw new Error(result.html_error || `server returned ${response.status}`);
        }

        // This response is the exact file atomically published in the session
        // directory; the browser does not run a second full-session renderer.
        const disposition = response.headers.get("content-disposition") || "";
        const filename = disposition.match(/filename="([^"]+)"/)?.[1]
            || "embed-log-session.html";
        const blobUrl = URL.createObjectURL(await response.blob());
        const link = document.createElement("a");
        link.href = blobUrl;
        link.download = filename;
        document.body.appendChild(link);
        link.click();
        link.remove();
        setTimeout(() => URL.revokeObjectURL(blobUrl), 0);
    } catch (err) {
        console.error("Export failed:", err);
        alert("Export failed: " + err.message);
    } finally {
        if (btn) {
            btn.textContent = previous;
            btn.disabled = false;
        }
    }
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
// Matches _selectionMessageText in selection.js.
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
        (state.rawLines[id] || []).forEach((_, idx) => {
            const line = getLine(id, idx);
            if (line) entries.push({ paneId: id, idx, line });
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
async function downloadRawSplit() {
    const files = [];
    const names = new Set();
    try {
        for (const id of PANES) {
            const session = window.__embedLogSession;
            const sourceFile = session?.source_files?.[id];
            if (session?.id && sourceFile) {
                const filename = sourceFile.split(/[\\/]/).pop();
                const response = await fetch(`/sessions/${encodeURIComponent(session.id)}/${encodeURIComponent(filename)}`);
                if (!response.ok) throw new Error(`Cannot read saved logs for ${id}: HTTP ${response.status}`);
                files.push({ name: _zipEntryName(id, names), data: new Uint8Array(await response.arrayBuffer()) });
                continue;
            }
            const lines = state.rawLines[id] || [];
            if (!lines.length) continue;
            const text = lines.map((_, idx) => {
                const line = getLine(id, idx);
                return line ? `[${line.ts}] ${_cleanMessage(line.rawText)}` : "";
            }).filter(Boolean).join("\n");
            files.push({ name: _zipEntryName(id, names), data: new TextEncoder().encode(text + '\n') });
        }
        if (!files.length) return;
        const url = URL.createObjectURL(_zipLogs(files));
        const a = document.createElement('a');
        a.href = url;
        a.download = 'embed-log-panes.zip';
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (error) {
        alert(`Unable to download logs: ${error.message}`);
    }
}

// Readable, portable file name for a pane inside the ZIP: whitespace becomes
// "_", characters that are invalid on Windows/macOS/Linux become "_", Windows
// device names get a "_" suffix, and remaining (case-insensitive) collisions
// get a numeric suffix.
function _zipEntryName(paneId, used) {
    const base = String(paneId)
        .replace(/\s+/g, "_")
        .replace(/[\\/:*?"<>|\x00-\x1f\x7f]/g, "_")
        .replace(/^\.+/, "_") || "pane";
    // Windows refuses device names as file names, even with an extension.
    const safeBase = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])$/i.test(base) ? `${base}_` : base;
    let name = `${safeBase}.log`;
    for (let n = 2; used.has(name.toLowerCase()); n++) name = `${safeBase}_${n}.log`;
    used.add(name.toLowerCase());
    return name;
}

// Table-driven CRC-32 (IEEE 802.3): about 8x faster than the bitwise loop,
// which matters because hashing runs on the UI thread.
let _crcTable = null;
function _crc32(data) {
    if (!_crcTable) {
        _crcTable = new Uint32Array(256);
        for (let n = 0; n < 256; n++) {
            let c = n;
            for (let bit = 0; bit < 8; bit++) c = (c >>> 1) ^ ((c & 1) ? 0xedb88320 : 0);
            _crcTable[n] = c >>> 0;
        }
    }
    let crc = 0xffffffff;
    for (let i = 0; i < data.length; i++) crc = _crcTable[(crc ^ data[i]) & 0xff] ^ (crc >>> 8);
    return (crc ^ 0xffffffff) >>> 0;
}

// Stored ZIP needs no external library and works in offline HTML exports.
function _zipLogs(files) {
    const parts = [], directory = [];
    let offset = 0, directorySize = 0;
    if (files.length > 65535) throw new Error('Too many panes for ZIP');
    for (const { name, data } of files) {
        const filename = new TextEncoder().encode(name);
        // shortcut: classic ZIP is limited to 4 GiB; use ZIP64 for larger browser exports.
        if (filename.length > 65535 || offset + data.length + filename.length + 30 > 0xffffffff) {
            throw new Error('Logs exceed the 4 GiB ZIP limit');
        }
        const crc = _crc32(data);
        const local = new Uint8Array(30), lv = new DataView(local.buffer);
        lv.setUint32(0, 0x04034b50, true);
        lv.setUint16(4, 20, true);
        lv.setUint16(6, 0x800, true);
        lv.setUint16(12, 33, true); // 1980-01-01
        lv.setUint32(14, crc, true);
        lv.setUint32(18, data.length, true);
        lv.setUint32(22, data.length, true);
        lv.setUint16(26, filename.length, true);
        parts.push(local, filename, data);
        const central = new Uint8Array(46), cv = new DataView(central.buffer);
        cv.setUint32(0, 0x02014b50, true);
        cv.setUint16(4, 20, true);
        cv.setUint16(6, 20, true);
        cv.setUint16(8, 0x800, true);
        cv.setUint16(14, 33, true);
        cv.setUint32(16, crc, true);
        cv.setUint32(20, data.length, true);
        cv.setUint32(24, data.length, true);
        cv.setUint16(28, filename.length, true);
        cv.setUint32(42, offset, true);
        directory.push(central, filename);
        offset += local.length + filename.length + data.length;
        directorySize += central.length + filename.length;
    }
    if (offset + directorySize + 22 > 0xffffffff) throw new Error('Logs exceed the 4 GiB ZIP limit');
    const end = new Uint8Array(22), ev = new DataView(end.buffer);
    ev.setUint32(0, 0x06054b50, true);
    ev.setUint16(8, files.length, true);
    ev.setUint16(10, files.length, true);
    ev.setUint32(12, directorySize, true);
    ev.setUint32(16, offset, true);
    return new Blob([...parts, ...directory, end], { type: 'application/zip' });
}

function _toggleRawMenu() {
    const btn = document.getElementById("btn-download-raw");
    const menu = document.getElementById("download-raw-menu");
    if (!menu || !btn) return;

    const isOpen = menu.classList.contains("open");
    document.querySelectorAll("#clip-peek-menu.open").forEach(el =>
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
    if (isComposingEvent(e) || isEditableTarget(e.target)) return;
    if (e.key === "Escape") _closeRawMenu();
});

// ---------------------------------------------------------------------------
// Lazy hydration — reads compact JSON from <script data-pane> tags,
// populates state.rawLines[] without creating DOM elements, then
// renders only the visible window of lines for instant load with 100k+ lines.
// ---------------------------------------------------------------------------
export function hydratePanesFromJson() {
    const scripts = document.querySelectorAll('script[type="application/json"][data-pane]');
    if (!scripts.length) return;

    scripts.forEach(script => {
        const paneId = script.dataset.pane;
        if (!paneId) return;
        try {
            const tuples = JSON.parse(script.textContent);
            if (!Array.isArray(tuples) || !tuples.length) return;

            // Keep compact tuples in state; lines are parsed/analyzed lazily
            // when the virtual renderer needs to display a raw index.
            if (!PANES.includes(paneId)) return;
            hydratePaneRows(paneId, tuples);
            // The compact arrays own the data now; release the duplicate JSON payload.
            script.remove();

            // Scroll to top on initial load (consistent with legacy behavior)
            state.atBottom[paneId] = false;
        } catch (e) {
            console.error("Failed to parse JSON data for pane", paneId, e);
        }
    });

    refreshStatsUi();

    // Render the visible window for each hydrated pane
    PANES.forEach(id => {
        const lines = state.rawLines[id];
        if (lines && lines.length) {
            renderPaneWindow(id, { targetIdx: 0 });
            const logEl = document.getElementById("log-" + id);
            if (logEl) logEl.scrollTop = 0;
            updateJumpBtn(id);
        }
    });

    window.__embedLogUpdateTimestampModeUi?.();
}

window.__embedLogHydratePanes = hydratePanesFromJson;
