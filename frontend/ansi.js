"use strict";

// SGR code → CSS class
const ANSI_MAP = {
    "1":  "ansi-b",
    "31": "ansi-1", "32": "ansi-2", "33": "ansi-3",
    "34": "ansi-4", "35": "ansi-5", "36": "ansi-6", "37": "ansi-7",
};

// Convert raw text with ANSI escape sequences to safe HTML.
// SGR color/bold codes → <span class="ansi-N">
// All other sequences (cursor movement, erase, OSC, bare ESC) → stripped silently.
function parseAnsi(raw) {
    let s = raw
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");

    let result = "";
    let open    = false;
    // CSI: ESC [ params letter  |  OSC: ESC ] ... BEL  |  bare: ESC <other>
    const re = /\x1b(?:\[([0-9;]*)([A-Za-z])|\][^\x07]*\x07|[^[\]])/g;
    let last = 0, m;

    while ((m = re.exec(s)) !== null) {
        result += s.slice(last, m.index);
        last = m.index + m[0].length;

        if (m[2] === "m") {  // SGR — colour / bold
            const codes = (m[1] || "0").split(";");
            if (codes[0] === "0" || codes[0] === "") {
                if (open) { result += "</span>"; open = false; }
            } else {
                if (open) result += "</span>";
                const cls = codes.map(c => ANSI_MAP[c]).filter(Boolean).join(" ");
                if (cls) { result += `<span class="${cls}">`; open = true; }
                else open = false;
            }
        }
        // everything else: silently dropped
    }
    result += s.slice(last);
    if (open) result += "</span>";
    return result;
}

// "03-25 11:50:00.123"  →  strip non-digits  →  sortable integer
function tsToNum(ts) {
    return parseInt(ts.replace(/\D/g, ""), 10) || 0;
}
