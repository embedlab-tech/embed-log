// Tab definitions — each tab shows 1 or 2 panes side-by-side.
// In live mode TABS/PANES start empty; ws.js populates them dynamically.
// In static mode (export/merge_logs) a classic <script> sets window.TABS and
// window.PANES before this module runs so the per-pane state is pre-seeded.
export const TABS  = window.TABS  ?? [];
export const PANES = window.PANES ?? [...new Set(TABS.flatMap(t => t.panes))];
export const PANE_LABELS = window.PANE_LABELS ?? {};
const INITIAL_TIMESTAMP_MODE =
    window.__embedLogInitialTimestampMode === "relative" ? "relative" : "absolute";
const INITIAL_FIRST_LOG_AT =
    typeof window.__embedLogFirstLogAt === "string" && window.__embedLogFirstLogAt.trim()
        ? window.__embedLogFirstLogAt.trim()
        : null;

export function paneLabel(paneId) {
    return PANE_LABELS[paneId] || paneId;
}

export function unwrapPaneLabel(paneId) {
    const base = paneLabel(paneId);
    const tab = TABS.find(t => t.panes.includes(paneId));
    if (!tab) return base;
    return base + '-' + tab.label;
}

function _formatMs3(value) {
    return String(value).padStart(3, "0");
}

export function formatRelativeTimestamp(totalMs) {
    const safeMs = Number.isFinite(totalMs) && totalMs >= 0 ? Math.floor(totalMs) : 0;
    const hours = Math.floor(safeMs / 3_600_000);
    const minutes = Math.floor((safeMs % 3_600_000) / 60_000);
    const seconds = Math.floor((safeMs % 60_000) / 1_000);
    const millis = safeMs % 1_000;
    return `T+${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${_formatMs3(millis)}`;
}

export function parseRelativeTimestamp(ts) {
    const m = /^T\+(\d+):(\d{2}):(\d{2})\.(\d{3})$/.exec(ts || "");
    if (!m) return null;
    return (parseInt(m[1], 10) * 3_600_000)
        + (parseInt(m[2], 10) * 60_000)
        + (parseInt(m[3], 10) * 1_000)
        + parseInt(m[4], 10);
}

export function formatAbsoluteTimestampFromIso(iso) {
    const m = /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?/.exec(iso || "");
    if (!m) return null;
    return `${m[2]}-${m[3]} ${m[4]}:${m[5]}:${m[6]}.${_formatMs3((m[7] || "").slice(0, 3))}`;
}

function _isoToEpochMs(iso) {
    const ms = Date.parse(iso || "");
    return Number.isFinite(ms) ? ms : null;
}

function _formatAbsoluteTimestampFromMs(ms) {
    if (!Number.isFinite(ms)) return null;
    const d = new Date(ms);
    if (!Number.isFinite(d.getTime())) return null;
    return `${_formatMs3(d.getMonth() + 1).slice(1)}-${_formatMs3(d.getDate()).slice(1)} ${_formatMs3(d.getHours()).slice(1)}:${_formatMs3(d.getMinutes()).slice(1)}:${_formatMs3(d.getSeconds()).slice(1)}.${_formatMs3(d.getMilliseconds())}`;
}

function _enrichExistingTimestampVariants() {
    if (!Number.isFinite(state.firstLogAtMs)) return false;
    let changed = false;
    PANES.forEach(paneId => {
        const lines = state.rawLines[paneId] || [];
        lines.forEach(line => {
            if (!line || Array.isArray(line)) return;
            if (!line.absTs && Number.isFinite(line.relNum)) {
                line.absNum = state.firstLogAtMs + line.relNum;
                line.absTs = _formatAbsoluteTimestampFromMs(line.absNum);
                changed = true;
            }
            if ((!line.relTs || !Number.isFinite(line.relNum)) && Number.isFinite(line.absNum)) {
                line.relNum = Math.max(0, line.absNum - state.firstLogAtMs);
                line.relTs = formatRelativeTimestamp(line.relNum);
                changed = true;
            }
            applyTimestampModeToLine(line);
        });
    });
    return changed;
}


export const state = {
    showTs:      true,

    fontSize:    14,
    activeTab:   0,
    activePaneTab: 0,
    syncTs:      null,   // last-clicked numeric timestamp
    syncTabSwitch: false, // true after explicit line sync; next tab switches follow syncTs
    filters:     {},
    wrap:        {},
    rawLines:    {},
    markers:     {},       // paneId → [{lineIdx, numTs, description, createdAt}]
    markerNavIdx: -1,      // index into flat marker list for navigation
    atBottom:    {},
    highlighted: {},
    highlightedIdx: {},
    selected:    {},
    selectionScope:  'exact', // 'exact', 'context', or 'context-selected'
    contextPanes:    {},       // paneId → bool; only used when selectionScope === 'context-selected'
    unwrap:        false,
    timestampMode: INITIAL_TIMESTAMP_MODE,
    sessionTimestampMode: INITIAL_TIMESTAMP_MODE,
    firstLogAt: INITIAL_FIRST_LOG_AT,
    firstLogAtMs: _isoToEpochMs(INITIAL_FIRST_LOG_AT),
    useClientRelativeBase: false,
    clientRelativeBaseMs: null,

    // ── Event detection ──
    events: [],             // [{event_id, source_id, severity, timestamp_num, ...}]
    eventsEnabled: false,   // true when config has ≥1 event rule
    eventsTabActive: false, // true while the Events timeline tab is shown
    includeEventMarkers: false, // nav includes kind:"event" markers when true
    eventRules: {},         // source → [{name, severity}] from config message
};

export function setTimestampContext({ mode = null, firstLogAt = undefined, resetMode = false } = {}) {
    if (resetMode) {
        state.useClientRelativeBase = false;
        state.clientRelativeBaseMs = null;
    }
    if (mode === "absolute" || mode === "relative") {
        state.sessionTimestampMode = mode;
        if (resetMode) state.timestampMode = mode;
    }
    if (firstLogAt !== undefined) {
        state.firstLogAt = typeof firstLogAt === "string" && firstLogAt.trim() ? firstLogAt.trim() : null;
        state.firstLogAtMs = _isoToEpochMs(state.firstLogAt);
    }
    return _enrichExistingTimestampVariants();
}

export function resetRelativeTimestampBase() {
    state.useClientRelativeBase = true;
    state.clientRelativeBaseMs = null;
    state.firstLogAt = null;
    state.firstLogAtMs = null;
}

export function noteRelativeTimestampCandidate(meta = {}) {
    if (!state.useClientRelativeBase || Number.isFinite(state.clientRelativeBaseMs)) return;
    if (!meta || typeof meta !== "object") return;
    const absMs = Number.isFinite(meta.absNum)
        ? meta.absNum
        : (Number.isFinite(meta.numTs) ? meta.numTs : _isoToEpochMs(meta.timestampIso));
    if (Number.isFinite(absMs)) state.clientRelativeBaseMs = absMs;
}

export function lineHasTimestampMode(line, mode) {
    return mode === "relative" ? !!line?.relTs : !!line?.absTs;
}

export function applyTimestampModeToLine(line) {
    if (!line) return;
    if (state.timestampMode === "relative" && line.relTs) {
        line.ts = line.relTs;
        line.numTs = Number.isFinite(line.relNum) ? line.relNum : 0;
        return;
    }
    if (state.timestampMode === "absolute" && line.absTs) {
        line.ts = line.absTs;
        line.numTs = Number.isFinite(line.absNum) ? line.absNum : line.numTs;
        return;
    }
    if (line.absTs) {
        line.ts = line.absTs;
        line.numTs = Number.isFinite(line.absNum) ? line.absNum : line.numTs;
        return;
    }
    if (line.relTs) {
        line.ts = line.relTs;
        line.numTs = Number.isFinite(line.relNum) ? line.relNum : 0;
    }
}

export function buildTimestampInfo(ts, meta = {}) {
    const info = {
        ts: ts || "",
        numTs: Number.isFinite(meta.numTs) ? meta.numTs : null,
        absTs: typeof meta.absTs === "string" && meta.absTs ? meta.absTs : null,
        absNum: Number.isFinite(meta.absNum) ? meta.absNum : null,
        relTs: typeof meta.relTs === "string" && meta.relTs ? meta.relTs : null,
        relNum: Number.isFinite(meta.relNum) ? meta.relNum : null,
    };

    if (!info.absTs && typeof meta.timestampIso === "string") {
        info.absTs = formatAbsoluteTimestampFromIso(meta.timestampIso);
    }
    if (!Number.isFinite(info.absNum) && typeof meta.timestampIso === "string") {
        info.absNum = _isoToEpochMs(meta.timestampIso);
    }

    if (!info.relTs && typeof ts === "string" && ts.startsWith("T+")) {
        info.relTs = ts;
    }
    if (!Number.isFinite(info.relNum) && info.relTs) {
        info.relNum = parseRelativeTimestamp(info.relTs);
    }

    if (!info.absTs && typeof ts === "string" && ts && !ts.startsWith("T+")) {
        info.absTs = ts;
    }

    if (!info.absTs && Number.isFinite(info.relNum) && Number.isFinite(state.firstLogAtMs)) {
        info.absNum = state.firstLogAtMs + info.relNum;
        info.absTs = _formatAbsoluteTimestampFromMs(info.absNum);
    }

    if (state.useClientRelativeBase && Number.isFinite(info.absNum)) {
        if (!Number.isFinite(state.clientRelativeBaseMs)) {
            state.clientRelativeBaseMs = info.absNum;
        }
        info.relNum = Math.max(0, info.absNum - state.clientRelativeBaseMs);
        info.relTs = formatRelativeTimestamp(info.relNum);
    } else if ((!info.relTs || !Number.isFinite(info.relNum)) && Number.isFinite(info.absNum) && Number.isFinite(state.firstLogAtMs)) {
        info.relNum = Math.max(0, info.absNum - state.firstLogAtMs);
        info.relTs = formatRelativeTimestamp(info.relNum);
    }

    if (!Number.isFinite(info.numTs)) {
        if (state.timestampMode === "relative" && Number.isFinite(info.relNum)) {
            info.numTs = info.relNum;
        } else if (state.timestampMode === "absolute" && Number.isFinite(info.absNum)) {
            info.numTs = info.absNum;
        } else if (Number.isFinite(info.relNum)) {
            info.numTs = info.relNum;
        } else if (Number.isFinite(info.absNum)) {
            info.numTs = info.absNum;
        } else {
            info.numTs = 0;
        }
    }

    applyTimestampModeToLine(info);
    return info;
}

// Initialise per-pane state for every pane in the system
PANES.forEach(id => {
    state.filters[id]     = null;
    state.rawLines[id]    = [];
    state.atBottom[id]    = true;

    state.wrap[id]        = false;
    state.highlighted[id] = null;
    state.highlightedIdx[id] = null;
    state.selected[id]    = new Set();
});