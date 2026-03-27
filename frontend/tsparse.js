"use strict";

// ---------------------------------------------------------------------------
// Multi-format log-line timestamp parser
//
// Formats supported (timestamps are taken AS-IS — no timezone conversion):
//   [YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]]   server / full ISO in brackets
//   [MM-DD HH:MM:SS[.frac]]                   short, space-sep, in brackets
//   [MM-DDTHH:MM:SS[.frac]]                   short ISO (T-sep), in brackets
//   YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]      bare ISO 8601 (no brackets)
//   YYYY-MM-DD HH:MM:SS[.frac]                 space separator, no brackets
//
// Fractional seconds (any length) are truncated to 3 digits (ms).
// Timezone suffixes are stripped — the local clock time is preserved so that
// UART logs (the time reference) and UTC-stamped logs synchronise with a
// constant offset that the user can reason about.
//
// Output timestamp format: "MM-DD HH:MM:SS.mmm"
// ---------------------------------------------------------------------------

function _ms3(frac) {
    if (!frac) return "000";
    return (frac + "000").slice(0, 3);
}

// Returns { ts: "MM-DD HH:MM:SS.mmm", data: <rest of line> } or null.
function parseLogLine(raw) {
    const s = raw.trimStart();
    let m;

    // [YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]]
    m = /^\[(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|[+-]\d{2}:\d{2})?\]\s*(.*)/.exec(s);
    if (m) return { ts: `${m[2]}-${m[3]} ${m[4]}:${m[5]}:${m[6]}.${_ms3(m[7])}`, data: m[8] };

    // [MM-DD HH:MM:SS[.frac]]  — space-separated, no T, no year
    m = /^\[(\d{2})-(\d{2})\s+(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?\]\s*(.*)/.exec(s);
    if (m) return { ts: `${m[1]}-${m[2]} ${m[3]}:${m[4]}:${m[5]}.${_ms3(m[6])}`, data: m[7] };

    // [MM-DDTHH:MM:SS[.frac]]
    m = /^\[(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?\]\s*(.*)/.exec(s);
    if (m) return { ts: `${m[1]}-${m[2]} ${m[3]}:${m[4]}:${m[5]}.${_ms3(m[6])}`, data: m[7] };

    // YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]
    m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|[+-]\d{2}:\d{2})?\s*(.*)/.exec(s);
    if (m) return { ts: `${m[2]}-${m[3]} ${m[4]}:${m[5]}:${m[6]}.${_ms3(m[7])}`, data: m[8] };

    // YYYY-MM-DD HH:MM:SS[.frac]
    m = /^(\d{4})-(\d{2})-(\d{2})\s+(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?\s*(.*)/.exec(s);
    if (m) return { ts: `${m[2]}-${m[3]} ${m[4]}:${m[5]}:${m[6]}.${_ms3(m[7])}`, data: m[8] };

    return null;
}
