import { STATIC_PROFILE } from './profile.js';
import { renderPaneShell } from './renderPane.js';
import { renderToolbar } from './renderToolbar.js';
import {
    getConfiguredFrontendPlugins, getConfiguredPanePlugins, getConfiguredPluginScripts, getPanePluginUiState,
} from './pluginRuntime.js';
const STATIC_EXPORT_PROFILE =
    typeof STATIC_PROFILE !== 'undefined' ? STATIC_PROFILE : window.__embedLogProfile;

import { renderPaneWindow, updateJumpBtn, getLine, hydratePaneRows, refreshStatsUi } from './lines.js';

import { state, TABS, PANES, PANE_LABELS } from './state.js';
import { isEditableTarget, isComposingEvent } from './keyboard.js';

export async function fetchCrashReport(sessionId, item) {
    const response = await fetch(`/api/sessions/${encodeURIComponent(sessionId)}/crashes/${item.sequence}?source=${encodeURIComponent(item.sourceId)}`, {
        cache: "no-store", signal: AbortSignal.timeout(5000),
    });
    const result = await response.json().catch(() => null);
    const report = result?.report;
    if (!response.ok) throw new Error(result?.error || `HTTP ${response.status}`);
    if (report?.source !== item.sourceId || report?.sequence !== item.sequence || !Array.isArray(report.addresses)) {
        throw new Error("Invalid crash report");
    }
    return report;
}

async function _snapshotCrashReports(items, sources, session) {
    const offline = window.__embedLogProfile?.kind === "static";
    const key = (source, sequence) => JSON.stringify([source, sequence]);
    const saved = new Map((window.__embedLogCrashReports || []).map(report => [key(report.source, report.sequence), report]));
    const crashes = new Map(items.map(item => [key(item.sourceId, item.sequence), item]));
    const reports = [];
    for (const [id, item] of crashes) {
        const { sourceId: source, sequence } = item;
        let report = saved.get(id);
        if (!offline && session?.id && item.sessionId === session.id && Object.hasOwn(session.symbols || {}, source)) {
            try { report = await fetchCrashReport(session.id, item); }
            catch { /* Decoder failure must not prevent exporting the logs. */ }
        }
        const decoded = report?.addresses || [];
        const addresses = decoded.filter(address => Number.isFinite(address?.address)
            && Array.isArray(address.frames) && sources.get(source)?.has(address.record_sequence))
            .map(({ label, address, record_sequence, frames }) => ({ label, address, record_sequence,
                frames: frames.map(({ function: name, file, line, inlined }) => ({
                    function: name, file: file?.split(/[\\/\\\\]/).pop(), line, inlined,
                })),
            }));
        reports.push({ source, sequence, addresses, unavailable: !report || !!report.unavailable,
            partial: !!report?.partial || addresses.length < decoded.length });
    }
    return reports;
}

function _configurationSnapshot(summary, sourceIds) {
    if (!Array.isArray(summary?.sources)) return null;
    return {
        server: { timestamp_mode: summary.server?.timestamp_mode ?? null },
        sources: summary.sources.filter(source => sourceIds.has(source.name))
            .map(({ name, type, baudrate, parser }) => ({
                name, type, baudrate: baudrate ?? null, parser: { type: parser?.type ?? null },
            })),
    };
}

export async function exportHtmlSnapshot(options = {}) {
    const btn = options.button === undefined ? document.getElementById("btn-export") : options.button;
    const prev = btn?.textContent;
    if (btn) {
        btn.textContent = "…";
        btn.disabled = true;
    }

    const _safeJson = obj => JSON.stringify(obj).replace(/</g, "\\u003c");
    const _attr = text => text.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
    try {
        const session = window.__embedLogSession;
        const paneData = options.logData || Object.fromEntries(PANES.map(id => [id, state.rawLines[id] || []]));
        const allMerges = window.__embedLogMerges || [];
        const sources = new Map();
        if (!options.logData) {
            for (const paneId of PANES) {
                for (const source of allMerges.find(merge => merge.name === paneId)?.of || [paneId]) {
                    sources.set(source, new Set());
                }
            }
        }
        const textTemplate = document.createElement("template");
        const rawText = entry => {
            if (entry.text != null || entry.rawText != null) return entry.text ?? entry.rawText;
            textTemplate.innerHTML = entry.html || "";
            return textTemplate.content.textContent;
        };
        const crashes = [];
        let paneDataTags = "";
        for (const [paneId, entries] of Object.entries(paneData)) {
            if (!entries.length) continue;
            const tuples = entries.map(entry => {
                const [ts, text, isTx, meta] = Array.isArray(entry)
                    ? entry : [entry.ts, rawText(entry), entry.isTx, entry];
                const { absTs, absNum, relTs, relNum, timeDomain, sequence, recordOrdinal, originalBytes, level, event, sessionId } = meta || {};
                const sourceId = meta?.sourceId || paneId;
                if (!sources.has(sourceId)) sources.set(sourceId, new Set());
                if (!isTx && Number.isFinite(sequence) && (!session || sessionId === session.id)) {
                    sources.get(sourceId).add(sequence);
                    if (event === "crash") crashes.push({ sourceId, sequence, sessionId });
                }
                return [ts, text, isTx, { absTs, absNum, relTs, relNum, timeDomain, sourceId, sequence,
                    sessionId, recordOrdinal, originalBytes, level, event }];
            });
            paneDataTags += `<script type="application/json" data-pane="${_attr(paneId)}">${_safeJson(tuples)}</script>\n`;
        }
        const layout = TABS.map((tab, index) => ({ label: tab.label, index,
            panes: tab.panes.filter(id => !options.logData || paneData[id]?.length),
        })).filter(tab => tab.panes.length);
        if (!layout.length) return;
        const tabs = layout.map(({ label, panes }) => ({ label, panes }));
        const panes = [...new Set(tabs.flatMap(tab => tab.panes))];
        const visible = id => panes.includes(id) || sources.has(id);
        const labels = Object.fromEntries(Object.entries(PANE_LABELS).filter(([id]) => visible(id)));
        const merges = allMerges.filter(merge => panes.includes(merge.name))
            .map(({ name, of }) => ({ name, of: of.filter(id => sources.has(id)) }));
        const levels = Object.fromEntries(Object.entries(window.__embedLogSourceLevels || {}).filter(([id]) => visible(id)));
        const config = _configurationSnapshot(window.__embedLogRuntimeConfig, sources);
        const markers = options.logData ? [] : Object.values(state.markers).flat();
        const activeTabIdx = Math.max(0, layout.findIndex(tab => tab.index === (options.activeTab ?? state.activeTab)));
        const timestampMode = state.timestampMode === "hidden" ? state.sessionTimestampMode : state.timestampMode;
        const firstLogAt = state.firstLogAt;
        const fontSize = state.fontSize;
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

        const assets = await Promise.all(ASSETS.map(async a => {
            const bundled = document.querySelector(`[data-asset="${a}"]`);
            if (bundled) return bundled.outerHTML;
            const r = await fetch(a, {cache: "no-store"});
            if (!r.ok) throw new Error(`Failed to fetch ${a}: ${r.status}`);
            const src = await r.text();
            return a.endsWith(".js")
                ? `<script data-asset="${a}">${_escJs(_stripModuleSyntax(src))}</script>`
                : `<style data-asset="${a}">${src.replace(/@font-face\s*\{[^}]*\}/g, "")}</style>`;
        }));
        const css = assets[ASSETS.indexOf("viewer.css")];
        const scopePlugins = value => Object.fromEntries(Object.entries(value).filter(([id]) => panes.includes(id)));
        const panePlugins = scopePlugins(getConfiguredPanePlugins());
        const panePluginUiState = scopePlugins(getPanePluginUiState());
        const frontendPlugins = getConfiguredFrontendPlugins();
        const existingPluginScripts = getConfiguredPluginScripts();
        const activePluginNames = [...new Set(Object.values(panePlugins).flatMap(refs => refs.map(ref => ref.name)))];
        const pluginScripts = Object.fromEntries(activePluginNames.map(name => [name, existingPluginScripts[name]])
            .filter(([, source]) => typeof source === "string" && source));
        assets[ASSETS.indexOf("pluginRuntime.js")] += Object.values(pluginScripts)
            .map(src => `<script>${_escJs(src)}</script>`).join("\n");
        const scripts = assets.filter((_, i) => ASSETS[i] !== "viewer.css").join("\n");
        const reports = await _snapshotCrashReports(crashes, sources, session);


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
            `window.TABS = ${_safeJson(tabs)};\n` +
            `window.PANES = ${_safeJson(panes)};\n` +
            `window.PANE_LABELS = ${_safeJson(labels)};\n` +
            `window.__embedLogFrontendPlugins = ${_safeJson(Object.fromEntries(activePluginNames.map(name => [name, frontendPlugins[name]])))};\n` +
            `window.__embedLogPanePlugins = ${_safeJson(panePlugins)};\n` +
            `window.__embedLogPluginScripts = ${_safeJson(pluginScripts)};\n` +
            `window.__embedLogInitialPanePluginUiState = ${_safeJson(panePluginUiState)};\n` +
            `window.__embedLogRuntimeConfig = ${_safeJson(config)};\n` +
            `window.__embedLogCrashReports = ${_safeJson(reports)};\n` +
            `window.__embedLogMerges = ${_safeJson(merges)};\n` +
            `window.__embedLogSourceLevels = ${_safeJson(levels)};\n` +
            `window.__embedLogInitialThemeState = ${_safeJson(themeState)};\n` +
            `window.__embedLogInitialTimestampMode = ${_safeJson(timestampMode)};\n` +
            `window.__embedLogFirstLogAt = ${_safeJson(firstLogAt)};\n` +
            `window.__embedLogInitialFontSize = ${_safeJson(fontSize)};`;



        // ------------------------------------------------------------------
        // Build pane + tab HTML for selection-only snapshots, with the TX
        // input row hidden in static mode.
        // ------------------------------------------------------------------
        function _paneHtml(paneId) {
            const raw = document.getElementById(`pane-${paneId}`)?.querySelector(".pane-name")?.textContent.trim() || paneId;
            return renderPaneShell(paneId, raw, { showTx: false });
        }

        const tabContentsHtml = tabs.map((tab, i) => {
            const inner = tab.panes.map((paneId, j) =>
                (j > 0 ? '        <div class="splitter"></div>\n' : "") + _paneHtml(paneId)
            ).join("\n");
            return `    <div class="tab-content" id="tab-content-${i}">\n${inner}\n    </div>`;
        }).join("\n");

        // ------------------------------------------------------------------
        // Bootstrap: lazy hydration via compact JSON + windowed rendering
        // ------------------------------------------------------------------
        const bootstrapJs = `(function () {
    "use strict";
    window.wsSend = function () {};
    if (typeof hydratePanesFromJson === "function") {
        hydratePanesFromJson();
    }
    if (typeof window.__embedLogUpdateTimestampModeUi === "function") {
        window.__embedLogUpdateTimestampModeUi();
    }
    var _markers = ${_safeJson(markers)};
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
                flat.push({ paneId: pid, lineIdx: mk.lineIdx, numTs: mk.numTs });
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
        if (typeof ensureLineVisible === 'function') ensureLineVisible(target.paneId, target.lineIdx, { align: 'center' });
        var div = document.querySelector('#log-' + target.paneId + ' [data-idx="' + target.lineIdx + '"]');
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
        const rawTitle = options.title || tabs.map(t => t.label).join(" + ");
        const title = rawTitle.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        const themeAttr = currentTheme ? ` data-theme="${currentTheme}"` : "";

        const html = `<!DOCTYPE html>
<html lang="en"${themeAttr}>
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>embed-log \u2014 ${title}</title>
${css}
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
</div>

<script>${configJs}</script>
${paneDataTags}
${scripts}
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
    if (btn?.contains(e.target)) {
        _toggleRawMenu();
    } else if (menu?.classList.contains("open") && !menu.contains(e.target)) {
        _closeRawMenu();
    }
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
    window.__embedLogRefreshEventBadges?.();
}

window.__embedLogHydratePanes = hydratePanesFromJson;
