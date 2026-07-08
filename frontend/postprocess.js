// Log-line postprocessing for the copy/download flow: denoising plus the
// "compact"/"json" compaction levels — a JS mirror of
// crates/embed-log-core/src/postprocess.rs (same rules, same regexes, kept
// in sync by hand since the two run in different languages/runtimes). See
// that file's doc comments for the "why" behind each rule; this file only
// restates the "what". Raw session data is never touched — everything here
// operates on already-extracted display text, at copy/download time.
//
// Deliberately NOT ported: ANSI stripping. The frontend already strips it for
// free — logs render to HTML first (ansi.js converts SGR codes into
// <span class="ansi-N">), and copy extracts plain text by stripping that
// HTML, so raw escape bytes never reach the clipboard to begin with.

const LEADING_TIMESTAMP_RE = /^\d{2}:\d{2}:\d{2}\.\d{3}\s*/;
const BRACKET_PADDING_RE = /\[\s+([A-Za-z]+)\]/g;
const UPTIME_COUNTER_RE = /^\[\d{8}\]\s+/;

// If `message` starts with an HH:MM:SS.mmm timestamp equal to `clockTime`
// (the line's own rendered time), strip it — some sources (e.g. pytest)
// stamp their own lines, duplicating the line's own timestamp.
export function stripDuplicateLeadingTimestamp(message, clockTime) {
    const m = LEADING_TIMESTAMP_RE.exec(message);
    if (m && m[0].trim() === clockTime) {
        return message.slice(m[0].length);
    }
    return message;
}

// Collapse column-padded bracketed log-level tags: "[   ERROR]" -> "[ERROR]".
export function unpadBracketLevel(text) {
    return text.replace(BRACKET_PADDING_RE, "[$1]");
}

// Strip a Zephyr-style device uptime counter ("[00000002] <inf> ...") while
// keeping the level tag ("<inf>"/"<err>"/"<wrn>" — real signal). Only strips
// when a "<...>" tag immediately follows, so an unrelated 8-digit bracket
// isn't eaten.
export function stripUptimeCounter(text) {
    const m = UPTIME_COUNTER_RE.exec(text);
    if (m) {
        const rest = text.slice(m[0].length);
        if (rest.startsWith("<")) return rest;
    }
    return text;
}

// Apply all denoising steps, in the same order as the Rust side: drop a
// duplicate leading timestamp, then un-pad bracketed level tags, then drop
// redundant device uptime counters. (ANSI stripping happens upstream — see
// the file header.)
export function denoiseMessage(message, clockTime) {
    let text = stripDuplicateLeadingTimestamp(message, clockTime);
    text = unpadBracketLevel(text);
    return stripUptimeCounter(text);
}

// Session-relative elapsed time (the line's `relNum` — ms since session
// start), formatted compactly: H:MM:SS.mmm once the session has run an hour,
// M:SS.mmm once it's run a minute, else just S.mmm. Falls back to
// `fallbackClock` if `relNum` isn't a finite number.
export function elapsedTime(line, fallbackClock) {
    const relMs = line?.relNum;
    if (!Number.isFinite(relMs)) return fallbackClock;
    const totalMs = Math.max(0, Math.trunc(relMs));
    const ms = totalMs % 1000;
    const totalS = Math.trunc(totalMs / 1000);
    const s = totalS % 60;
    const m = Math.trunc(totalS / 60) % 60;
    const h = Math.trunc(totalS / 3600);
    const pad = (n) => String(n).padStart(2, "0");
    const msStr = String(ms).padStart(3, "0");
    if (h > 0) return `${h}:${pad(m)}:${pad(s)}.${msStr}`;
    if (m > 0) return `${m}:${pad(s)}.${msStr}`;
    return `${s}.${msStr}`;
}

// Source-name shortcodes derived from the source's own name — initials of
// its `_`/`-`-separated words ("COUNTER" -> "C", "MCU_LINK_RX" -> "MLR",
// "NODE-RED-COAP" -> "NRC") — rather than an arbitrary scan-order letter, so
// codes are mnemonic and mostly stable across runs (the same source tends to
// get the same code regardless of when it's first seen). On a collision
// between two differently-named sources whose initials coincide, falls back
// to the shortest unique prefix of the full name — source names are already
// guaranteed unique by config validation, so this always terminates. One
// table per copy/download action, not persisted across actions — matches the
// CLI's per-invocation ShortcodeTable.
export class ShortcodeTable {
    constructor() {
        this._codes = new Map();
        this._used = new Set();
    }

    codeFor(sourceId) {
        let code = this._codes.get(sourceId);
        if (code === undefined) {
            code = this._assign(sourceId);
            this._used.add(code);
            this._codes.set(sourceId, code);
        }
        return code;
    }

    _assign(sourceId) {
        const initials = ShortcodeTable._initials(sourceId);
        if (!this._used.has(initials)) return initials;
        // Collision: widen to progressively longer prefixes of the full name.
        // Code-point based (Array.from), not raw string slicing, so this
        // can't split a surrogate pair on an exotic source name.
        const chars = Array.from(sourceId);
        for (let len = 2; len <= chars.length; len++) {
            const candidate = chars.slice(0, len).join("").toUpperCase();
            if (!this._used.has(candidate)) return candidate;
        }
        return sourceId.toUpperCase();
    }

    // First letter of each `_`/`-`-separated word, uppercased. A name with no
    // separators reduces to just its own first letter.
    static _initials(sourceId) {
        return sourceId
            .split(/[_-]/)
            .filter(Boolean)
            .map(segment => segment[0].toUpperCase())
            .join("");
    }
}

// Rough, tokenizer-agnostic estimate: ~4 characters per token, the common
// rule-of-thumb approximation for English/code text. Good enough for "will
// this fit in context," not meant to match any specific tokenizer exactly.
export function estimateTokens(text) {
    return Math.ceil(text.length / 4);
}
