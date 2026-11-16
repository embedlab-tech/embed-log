// Per-pane log level filter.
//
// The server classifies every received line with the level profile of its
// source and sends the result as `level` (see crates/embed-log-core/src/levels.rs).
// Profiles reach the UI as window.__embedLogSourceLevels:
//   { SOURCE: { levels: [{ id, label, color, default_visible }] } | null }
// `null` means detection is off for that source. When the global is missing
// (older servers, older reports and caches) every source uses the built-in
// Zephyr levels and lines are classified here with the same patterns.

export const NO_LEVEL = "";

const STORAGE_PREFIX = "embed-log:level-filter:";

const ZEPHYR_PROFILE = {
    levels: [
        { id: "err", label: "Error", color: null, default_visible: true },
        { id: "wrn", label: "Warning", color: null, default_visible: true },
        { id: "inf", label: "Info", color: null, default_visible: true },
        { id: "dbg", label: "Debug", color: null, default_visible: false },
    ],
};
const ZEPHYR_RE = /<(err|wrn|inf|dbg)>|\[(err|error|wrn|warn|warning|inf|info|dbg|debug)\]/i;
const ZEPHYR_ALIASES = { error: "err", warn: "wrn", warning: "wrn", info: "inf", debug: "dbg" };
const ANSI_RE = /\x1b(?:\[[0-9;]*[A-Za-z]|\][^\x07]*\x07|[^[\]])/g;

// Built-in level ids keep the existing theme colors and weights.
const LEVEL_CLASSES = {
    fatal: "line-err", crit: "line-err", critical: "line-err", err: "line-err", error: "line-err",
    wrn: "line-wrn", warn: "line-wrn", warning: "line-wrn",
    dbg: "line-dbg", debug: "line-dbg",
};

/** Level of a line in the built-in Zephyr format, or NO_LEVEL. */
export function detectZephyrLevel(text) {
    const match = ZEPHYR_RE.exec(String(text || "").replace(ANSI_RE, ""));
    if (!match) return NO_LEVEL;
    const tag = (match[1] || match[2]).toLowerCase();
    return ZEPHYR_ALIASES[tag] || tag;
}

function serverLevels() {
    const all = window.__embedLogSourceLevels;
    return all && typeof all === "object" ? all : null;
}

/** Level profile of a source, or null when detection is off for it. */
export function sourceProfile(sourceId) {
    const all = serverLevels();
    if (!all) return ZEPHYR_PROFILE;
    return Object.prototype.hasOwnProperty.call(all, sourceId) ? all[sourceId] : null;
}

function paneSources(paneId) {
    const merge = (window.__embedLogMerges || []).find(m => m && m.name === paneId);
    return merge && Array.isArray(merge.of) ? merge.of : [paneId];
}

let _defsVersion = 0;
const _defsCache = new Map(); // paneId -> { version, defs }

/**
 * Levels offered by a pane's filter: the union of its sources' profiles in
 * profile order, plus "No level". Empty when no source has a profile.
 */
export function paneLevelDefs(paneId) {
    const cached = _defsCache.get(paneId);
    if (cached && cached.version === _defsVersion) return cached.defs;
    const defs = [];
    const seen = new Set();
    let enabled = false;
    for (const source of paneSources(paneId)) {
        const profile = sourceProfile(source);
        if (!profile || !Array.isArray(profile.levels)) continue;
        enabled = true;
        for (const level of profile.levels) {
            if (!level || typeof level.id !== "string" || !level.id || seen.has(level.id)) continue;
            seen.add(level.id);
            defs.push({
                id: level.id,
                label: level.label || level.id,
                defaultVisible: level.default_visible !== false,
            });
        }
    }
    if (enabled) defs.push({ id: NO_LEVEL, label: "No level", defaultVisible: true });
    _defsCache.set(paneId, { version: _defsVersion, defs });
    return defs;
}

/**
 * Level id of a stored line or compact tuple. `null` for TX lines, which are
 * never filtered by level; NO_LEVEL for lines without a level.
 */
export function entryLevel(entry) {
    let raw, isTx, level;
    if (Array.isArray(entry)) {
        raw = entry[1];
        isTx = entry[2];
        level = entry[3] && typeof entry[3] === "object" ? entry[3].level : undefined;
    } else {
        raw = entry.rawText;
        isTx = entry.isTx;
        level = entry.level;
    }
    if (isTx) return null;
    if (typeof level === "string" && level) return level;
    return serverLevels() ? NO_LEVEL : detectZephyrLevel(raw);
}

function entrySource(entry, paneId) {
    const meta = Array.isArray(entry) ? entry[3] : entry;
    return (meta && typeof meta === "object" && meta.sourceId) || paneId;
}

const _hidden = new Map();   // paneId -> Set of hidden level ids
const _origin = new Map();   // paneId -> "default" | "saved"
const _versions = new Map(); // paneId -> number, bumped whenever visibility changes

/** Changes whenever the pane's visible levels change (projection cache key). */
export function levelFilterVersion(paneId) {
    return `${_defsVersion}:${_versions.get(paneId) || 0}`;
}

function _bump(paneId) {
    _versions.set(paneId, (_versions.get(paneId) || 0) + 1);
}

function _defaults(paneId) {
    return new Set(paneLevelDefs(paneId).filter(d => !d.defaultVisible).map(d => d.id));
}

function _load(paneId) {
    try {
        const raw = localStorage.getItem(STORAGE_PREFIX + paneId);
        if (!raw) return null;
        const ids = JSON.parse(raw);
        return Array.isArray(ids) ? new Set(ids.filter(id => typeof id === "string")) : null;
    } catch {
        return null;
    }
}

function _save(paneId) {
    try {
        localStorage.setItem(STORAGE_PREFIX + paneId, JSON.stringify([..._hidden.get(paneId)]));
    } catch { /* storage unavailable: keep the choice for this page only */ }
}

/** Hidden level ids of a pane (the viewer's saved choice, else profile defaults). */
export function hiddenLevels(paneId) {
    if (!_hidden.has(paneId)) {
        const saved = _load(paneId);
        _hidden.set(paneId, saved || _defaults(paneId));
        _origin.set(paneId, saved ? "saved" : "default");
    }
    return _hidden.get(paneId);
}

/**
 * Visibility test for loops over many rows, or null when the pane shows every
 * level (no profile, or nothing hidden).
 */
export function levelPredicate(paneId) {
    if (!paneLevelDefs(paneId).length) return null;
    const hidden = hiddenLevels(paneId);
    if (!hidden.size) return null;
    return entry => {
        const level = entryLevel(entry);
        return level === null || !hidden.has(level);
    };
}

/** Whether a line passes the pane's level filter. */
export function isEntryVisible(paneId, entry) {
    const visible = levelPredicate(paneId);
    return !visible || visible(entry);
}

/** Number of hidden levels, for the toolbar button. */
export function hiddenLevelCount(paneId) {
    const ids = new Set(paneLevelDefs(paneId).map(d => d.id));
    return [...hiddenLevels(paneId)].filter(id => ids.has(id)).length;
}

export function setLevelVisible(paneId, levelId, visible) {
    const hidden = new Set(hiddenLevels(paneId));
    if (visible) hidden.delete(levelId);
    else hidden.add(levelId);
    _hidden.set(paneId, hidden);
    _origin.set(paneId, "saved");
    _save(paneId);
    _bump(paneId);
}

export function showAllLevels(paneId) {
    _hidden.set(paneId, new Set());
    _origin.set(paneId, "saved");
    _save(paneId);
    _bump(paneId);
}

/**
 * Call after window.__embedLogSourceLevels or merges change. Panes still on
 * profile defaults pick up the new defaults; saved choices are kept.
 */
export function refreshLevelDefinitions() {
    _defsVersion++;
    _defsCache.clear();
    for (const [paneId, origin] of _origin) {
        if (origin === "default") _hidden.set(paneId, _defaults(paneId));
    }
}

/** Per-level line counts of a pane (TX lines are not counted). */
export function countLevels(paneId, rows) {
    const counts = new Map();
    for (const entry of rows || []) {
        const level = entryLevel(entry);
        if (level === null) continue;
        counts.set(level, (counts.get(level) || 0) + 1);
    }
    return counts;
}

/** CSS class (" line-err" …) for built-in ids, "" otherwise. */
export function levelClass(level) {
    const cls = level ? LEVEL_CLASSES[level.toLowerCase()] : null;
    return cls ? ` ${cls}` : "";
}

/** Explicit color configured for the line's level in its source profile. */
export function levelColor(entry, paneId) {
    const level = entryLevel(entry);
    if (!level) return "";
    const profile = sourceProfile(entrySource(entry, paneId));
    const def = profile && Array.isArray(profile.levels) ? profile.levels.find(l => l && l.id === level) : null;
    return (def && def.color) || "";
}

/** True when the line's source has a level profile (detection is on). */
export function hasLevelProfile(entry, paneId) {
    return !!sourceProfile(entrySource(entry, paneId));
}
