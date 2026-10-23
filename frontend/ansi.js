// SGR code → CSS class
export const ANSI_MAP = {
    "1":  "ansi-b",
    "31": "ansi-1", "32": "ansi-2", "33": "ansi-3",
    "34": "ansi-4", "35": "ansi-5", "36": "ansi-6", "37": "ansi-7",
};

// Convert raw text with ANSI escape sequences to safe HTML.
// SGR color/bold codes → <span class="ansi-N">
// All other sequences (cursor movement, erase, OSC, bare ESC) → stripped silently.
export function parseAnsi(raw) {
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

// Recognize a transient terminal row that was completely erased before the
// next log record. Do not guess prompt names or drop ordinary console output.
// Deliberately conservative: unknown controls, multiline text and non-ASCII
// column widths remain visible. Raw session data is never changed.
const ERASE_RE = /\x1b\[[0-9]*[JK]/;
const CSI_RE = /\x1b\[([0-9;]*)([A-Za-z])/y;

export function isErasedConsoleLine(raw) {
    // Fast path: most colored log lines contain SGR only and no erase.
    if (typeof raw !== "string" || !ERASE_RE.test(raw)) return false;
    let cells = [], column = 0, erased = false, printed = false;
    for (let i = 0; i < raw.length;) {
        const c = raw[i];
        if (c === "\x1b") {
            CSI_RE.lastIndex = i;
            const match = CSI_RE.exec(raw);
            if (!match) return false;
            i = CSI_RE.lastIndex;
            const [, params, command] = match;
            if (command === "m") continue; // SGR does not occupy a cell.
            if (params.includes(";")) return false;
            const n = Number(params || 0);
            if (!Number.isSafeInteger(n) || n > 4096) return false;
            if (command === "D") column = Math.max(0, column - (n || 1));
            else if (command === "C") column += n || 1;
            else if (command === "G") column = Math.max(0, (n || 1) - 1);
            else if (command === "K" || command === "J") {
                if (n === 0) cells.length = Math.min(cells.length, column);
                else if (n === 1) cells.fill(" ", 0, column + 1);
                else if (n === 2) cells = [];
                else return false;
                erased = true;
            } else return false;
            if (column > 4096) return false;
        } else {
            i++;
            if (c === "\r") column = 0;
            else if (c === "\b") column = Math.max(0, column - 1);
            else if (c >= " " && c <= "~" && column < 4096) {
                cells[column++] = c;
                printed = true;
            } else return false;
        }
    }
    // A control-only erase (for example "\x1b[0K" from GitLab/terminal
    // redraws) is transient as well; there are no cells left to inspect.
    return erased && (!printed || cells.every(c => c === " "));
}

// "03-25 11:50:00.123" → sortable integer
// "T+00:00:01.234"     → elapsed milliseconds
export function tsToNum(ts) {
    const rel = /^T\+(\d+):(\d{2}):(\d{2})\.(\d{3})$/.exec(ts || "");
    if (rel) {
        return (parseInt(rel[1], 10) * 3_600_000)
            + (parseInt(rel[2], 10) * 60_000)
            + (parseInt(rel[3], 10) * 1_000)
            + parseInt(rel[4], 10);
    }
    return parseInt((ts || "").replace(/\D/g, ""), 10) || 0;
}
