#!/usr/bin/env python3
"""
merge_logs.py — offline log viewer for embed-log .log files.

Generates a self-contained static HTML file using the embed-log UI:
same themes, pane sync (including cross-tab), ANSI rendering, regex filter,
and HTML export. No server or browser extension required.

Usage:
    # Two panes in one tab
    python3 merge_logs.py \\
        --tab "UART" "Device A" logs/DEVICE_A.log \\
                     "Device B" logs/DEVICE_B.log \\
        --output merged.html

    # Two tabs: UART (2 panes) + PYTEST (1 pane)
    python3 merge_logs.py \\
        --tab "UART"   "Device A" logs/DEVICE_A.log \\
                       "Device B" logs/DEVICE_B.log \\
        --tab "PYTEST" "Pytest"             logs/pytest.log

Each --tab takes:   TAB_LABEL  PANE_LABEL FILE  [PANE_LABEL FILE]
  TAB_LABEL  — label shown on the tab button
  PANE_LABEL — display name shown in the pane header (also used as the pane ID)
  FILE       — path to the log file
Up to 2 panes per tab.

Assets (viewer.css, state.js, …) are read from the same directory as this script.
"""

import argparse
import html as _html
import json
import os
import re
import sys


def _slug(label: str) -> str:
    """Convert a display label to a safe HTML element-ID slug."""
    return re.sub(r"[^a-z0-9]+", "-", label.lower()).strip("-") or "pane"

# ---------------------------------------------------------------------------
# Log parsing — multi-format timestamp support
#
# Formats recognised (timestamps taken AS-IS — no timezone conversion):
#   [YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]]   server / full ISO in brackets
#   [MM-DD HH:MM:SS[.frac]]                   short, space-sep, in brackets
#   [MM-DDTHH:MM:SS[.frac]]                   short ISO (T-sep), in brackets
#   YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]      bare ISO 8601 (no brackets)
#   YYYY-MM-DD HH:MM:SS[.frac]                 space separator, no brackets
#
# Fractional seconds (any length) are truncated to 3 digits (ms).
# Timezone suffixes are stripped — the local clock time is preserved so that
# UART logs and UTC-stamped logs synchronise with a constant offset that the
# user can reason about.
#
# Continuation lines (no leading timestamp) are appended to the preceding
# timestamped entry, keeping multi-line stack traces together.
# ---------------------------------------------------------------------------

def _ms3(frac: str | None) -> str:
    """Normalise fractional seconds to exactly 3 digits."""
    if not frac:
        return "000"
    return (frac + "000")[:3]


# [YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]]
_RE_FULL_ISO_BRACKET = re.compile(
    r"^\[(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|[+-]\d{2}:\d{2})?\]\s*(.*)",
    re.DOTALL,
)
# [MM-DD HH:MM:SS[.frac]]  — space-separated, no T, no year
_RE_SHORT_SPACE_BRACKET = re.compile(
    r"^\[(\d{2})-(\d{2})\s+(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?\]\s*(.*)",
    re.DOTALL,
)
# [MM-DDTHH:MM:SS[.frac]]
_RE_SHORT_ISO_BRACKET = re.compile(
    r"^\[(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?\]\s*(.*)",
    re.DOTALL,
)
# YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]
_RE_BARE_ISO = re.compile(
    r"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|[+-]\d{2}:\d{2})?\s*(.*)",
    re.DOTALL,
)
# YYYY-MM-DD HH:MM:SS[.frac]
_RE_SPACE_ISO = re.compile(
    r"^(\d{4})-(\d{2})-(\d{2})\s+(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?\s*(.*)",
    re.DOTALL,
)


def _parse_line(raw: str):
    """
    Try all supported timestamp formats on a raw log line.
    Returns (ts, text) where ts is "MM-DD HH:MM:SS.mmm", or None if no match.
    """
    m = _RE_FULL_ISO_BRACKET.match(raw)
    if m:
        return (f"{m[2]}-{m[3]} {m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}", m[8])

    m = _RE_SHORT_SPACE_BRACKET.match(raw)
    if m:
        return (f"{m[1]}-{m[2]} {m[3]}:{m[4]}:{m[5]}.{_ms3(m[6])}", m[7])

    m = _RE_SHORT_ISO_BRACKET.match(raw)
    if m:
        return (f"{m[1]}-{m[2]} {m[3]}:{m[4]}:{m[5]}.{_ms3(m[6])}", m[7])

    m = _RE_BARE_ISO.match(raw)
    if m:
        return (f"{m[2]}-{m[3]} {m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}", m[8])

    m = _RE_SPACE_ISO.match(raw)
    if m:
        return (f"{m[2]}-{m[3]} {m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}", m[8])

    return None


def parse_log_file(path: str) -> list:
    """
    Read a .log file and return a list of line dicts:
        { "ts": "MM-DD HH:MM:SS.mmm", "text": str, "isTx": bool }

    Continuation lines (no timestamp) are appended to the preceding entry
    so multi-line stack traces stay together.
    """
    entries = []
    pending_ts: str | None = None
    pending_text: str | None = None

    def _flush():
        nonlocal pending_ts, pending_text
        if pending_ts is None:
            return
        is_tx = pending_text.startswith("[TX::")
        entries.append({"ts": pending_ts, "text": pending_text, "isTx": is_tx})
        pending_ts = pending_text = None

    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            for raw in fh:
                raw = raw.rstrip("\n\r")
                parsed = _parse_line(raw)
                if parsed:
                    _flush()
                    pending_ts, pending_text = parsed
                elif pending_ts is not None and raw.strip():
                    # Continuation line — append to current entry
                    pending_text += " " + raw.strip()
        _flush()
    except FileNotFoundError:
        print(f"Warning: file not found: {path}", file=sys.stderr)
    return entries


# ---------------------------------------------------------------------------
# Asset helpers
# ---------------------------------------------------------------------------

def _script_dir() -> str:
    """Return the path to the frontend/ directory (sibling of utils/)."""
    try:
        here = os.path.dirname(os.path.abspath(__file__))
    except NameError:
        here = os.getcwd()
    return os.path.join(here, "..", "frontend")


def _read_asset(filename: str) -> str:
    path = os.path.join(_script_dir(), filename)
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def _strip_module_syntax(src: str) -> str:
    """Remove ES module import/export statements so JS can be embedded as a
    classic <script> block in the self-contained static HTML output."""
    # Remove import statements (single-line)
    src = re.sub(r"^import\s+.*?['\"][^'\"]*['\"]\s*;?\r?\n?", "", src, flags=re.MULTILINE)
    # Remove export keyword from declarations (function, class, const, let, var)
    src = re.sub(r"^export\s+(async\s+)?(function|class|const|let|var)\b", r"\2", src, flags=re.MULTILINE)
    # Remove standalone export { ... } statements
    src = re.sub(r"^export\s*\{[^}]*\}\s*(?:from\s*['\"][^'\"]*['\"])?\s*;?\r?\n?", "", src, flags=re.MULTILINE)
    return src


def _esc_script_text(src: str) -> str:
    """Prevent embedded </script> from terminating the surrounding script tag."""
    return re.sub(r"</script", "<\\/script", src, flags=re.IGNORECASE)


# ---------------------------------------------------------------------------
# HTML generation
# ---------------------------------------------------------------------------

def _pane_html(pane_id: str, label: str, static: bool = True) -> str:
    """Render one pane div. TX input row is hidden in static mode."""
    safe_label = _html.escape(label)
    tx_style = ' style="display:none"' if static else ""
    return f"""\
        <div class="pane" id="pane-{pane_id}">
            <div class="pane-header">
                <span class="pane-name">{safe_label}</span>
                <button class="pane-clear-btn" data-pane="{pane_id}">clear</button>
            </div>
            <div class="filter-bar">
                <input class="filter-input" data-pane="{pane_id}" placeholder="Filter (regex)…">
            </div>
            <div class="pane-body">
                <div class="log-area" id="log-{pane_id}"></div>
                <button class="jump-btn" id="jump-{pane_id}">jump to bottom</button>
            </div>
            <div class="input-row"{tx_style}>
                <input class="serial-input" id="input-{pane_id}" autocomplete="off">
                <button class="send-btn" data-pane="{pane_id}">Send</button>
            </div>
        </div>"""


def _tab_content_html(tab_idx: int, tab_panes: list) -> str:
    """Render a tab-content div containing 1 or 2 panes (+ splitter if 2)."""
    parts = []
    for i, (pane_id, label) in enumerate(tab_panes):
        if i > 0:
            parts.append('        <div class="splitter"></div>')
        parts.append(_pane_html(pane_id, label, static=True))
    inner = "\n".join(parts)
    return (
        f'    <div class="tab-content" id="tab-content-{tab_idx}">\n'
        f'{inner}\n'
        f'    </div>'
    )


def generate_html(tab_specs: list) -> str:
    """
    tab_specs: [
        { "label": str, "panes": [(pane_id, pane_label, file_path), ...] },
        ...
    ]
    Returns a complete self-contained HTML string.
    """
    # Parse all log data
    log_data: dict[str, list] = {}
    for tab in tab_specs:
        for pane_id, pane_label, file_path in tab["panes"]:
            entries = parse_log_file(file_path)
            log_data[pane_id] = entries
            print(f"  [{tab['label']}] {pane_label!r}: {len(entries)} lines  ({file_path})")

    # Read frontend assets (strip ES module syntax for classic <script> embedding)
    def _js(filename: str) -> str:
        return _esc_script_text(_strip_module_syntax(_read_asset(filename)))

    css          = _read_asset("viewer.css")
    state_js     = _js("state.js")
    ansi_js      = _js("ansi.js")
    lines_js     = _js("lines.js")
    tabs_js      = _js("tabs.js")
    ui_js        = _js("ui.js")
    settings_js  = _js("settings.js")
    tsparse_js   = _js("tsparse.js")
    import_js    = _js("import.js")
    selection_js = _js("selection.js")
    themes_js    = _js("themes.js")
    tabcreate_js = _js("tabcreate.js")
    export_js    = _js("export.js")
    # ws.js intentionally omitted — no WebSocket in static mode

    # Build JS structures
    tabs_json = json.dumps([
        {"id": f"tab-{i}", "label": tab["label"],
         "panes": [p[0] for p in tab["panes"]]}
        for i, tab in enumerate(tab_specs)
    ], ensure_ascii=False)

    all_pane_ids = []
    seen = set()
    for tab in tab_specs:
        for pane_id, _, _ in tab["panes"]:
            if pane_id not in seen:
                all_pane_ids.append(pane_id)
                seen.add(pane_id)
    panes_json = json.dumps(all_pane_ids, ensure_ascii=False)

    # Build container HTML
    tab_contents = "\n".join(
        _tab_content_html(i, [(p[0], p[1]) for p in tab["panes"]])
        for i, tab in enumerate(tab_specs)
    )

    title = _html.escape(" + ".join(tab["label"] for tab in tab_specs))

    # Config script: sets window.TABS and window.PANES before state.js loads.
    # state.js reads window.TABS ?? [] so pre-populated panes are initialised
    # correctly when the module runs.
    config_js = _esc_script_text(
        f"window.TABS = {tabs_json};\n"
        f"window.PANES = {panes_json};"
    )

    # Bootstrap script: runs after all other scripts to inject log data
    bootstrap_js = _esc_script_text(f"""\
(function () {{
    "use strict";

    // No WebSocket in static mode — satisfy the reference in ui.js
    window.wsSend = function () {{}};

    var _logData = {json.dumps(log_data, ensure_ascii=False)};

    function _loadPane(paneId) {{
        var entries = _logData[paneId];
        if (!entries || entries.length === 0) return;
        state.atBottom[paneId] = false;
        entries.forEach(function (e) {{
            appendLine(paneId, e.ts, e.text, e.isTx);
        }});
        document.getElementById("log-" + paneId).scrollTop = 0;
        state.atBottom[paneId] = false;
        updateJumpBtn(paneId);
    }}

    PANES.forEach(_loadPane);
}})();""")

    return f"""<!DOCTYPE html>
<html lang="en" data-theme="whitesand">
<head>
<meta charset="UTF-8">
<title>embed-log — {title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap" rel="stylesheet">
<style>{css}</style>
</head>
<body>

<!-- ── TOOLBAR ──────────────────────────────────────────────── -->
<div id="toolbar">
    <span class="app-name">embed-log</span>
    <button id="btn-wrap"     title="Toggle word wrap">Wrap</button>
    <button id="btn-ts"       title="Toggle timestamps" class="active">Time</button>
    <button id="btn-sync"     title="Click a line to sync the other pane to the same timestamp" class="active">Sync</button>
    <div class="sep"></div>
    <button id="btn-font-dec" title="Decrease font size">A-</button>
    <button id="btn-font-inc" title="Increase font size">A+</button>
    <div class="sep"></div>
    <button id="btn-clear"    title="Clear all panes">Clear</button>
    <button id="btn-export"   title="Export current tab to a self-contained HTML file">Export</button>
    <div class="sep"></div>
    <button id="btn-theme" title="Toggle light / dark theme">🌙</button>
    <!-- ws-status kept for DOM compatibility; invisible in static mode -->
    <div id="ws-status" style="display:none"></div>
</div>

<!-- ── TAB BAR — shown by tabs.js when there is more than one tab ── -->
<div id="tab-bar"></div>

<!-- ── PANES ────────────────────────────────────────────────── -->
<div id="container">
{tab_contents}
</div>

<script>{config_js}</script>
<script>{state_js}</script>
<script>{ansi_js}</script>
<script>{lines_js}</script>
<script>{tabs_js}</script>
<script>{ui_js}</script>
<script>{settings_js}</script>
<script>{tsparse_js}</script>
<script>{import_js}</script>
<script>{selection_js}</script>
<script>{themes_js}</script>
<script>{tabcreate_js}</script>
<script>{export_js}</script>
<script>{bootstrap_js}</script>
</body>
</html>
"""


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _parse_tab_arg(args: list) -> dict:
    """
    Parse one --tab argument list:
      TAB_LABEL  PANE_LABEL FILE  [PANE_LABEL FILE]

    Returns { "label": str, "panes": [(pane_id, pane_label, file), ...] }
    or raises argparse.ArgumentTypeError on bad input.
    """
    if len(args) < 3:
        raise argparse.ArgumentTypeError(
            f"--tab needs at least 3 values: TAB_LABEL PANE_LABEL FILE, got: {args}"
        )
    tab_label = args[0]
    rest = args[1:]
    if len(rest) % 2 != 0:
        raise argparse.ArgumentTypeError(
            f"After TAB_LABEL each pane needs exactly 2 values (PANE_LABEL FILE). "
            f"Got {len(rest)} remaining values in --tab {tab_label!r}: {rest}"
        )
    if len(rest) > 4:
        raise argparse.ArgumentTypeError(
            f"At most 2 panes per tab, got {len(rest) // 2} in --tab {tab_label!r}"
        )
    panes = [(_slug(rest[i]), rest[i], rest[i + 1]) for i in range(0, len(rest), 2)]
    return {"label": tab_label, "panes": panes}


def main():
    parser = argparse.ArgumentParser(
        description="Merge log files into a self-contained static HTML viewer.",
        formatter_class=argparse.RawTextHelpFormatter,
        epilog="""examples:
  # One tab, two panes
  python merge_logs.py \\
      --tab "UART" "Device A" logs/DEVICE_A.log \\
                   "Device B" logs/DEVICE_B.log

  # Two tabs: UART (2 panes) + PYTEST (1 pane)
  python merge_logs.py \\
      --tab "UART"   "Device A" logs/DEVICE_A.log \\
                     "Device B" logs/DEVICE_B.log \\
      --tab "PYTEST" "Pytest"             logs/pytest.log \\
      --output run-42.html
""",
    )
    parser.add_argument(
        "--tab",
        nargs="+",
        action="append",
        metavar="ARG",
        required=True,
        help=(
            "Tab definition: TAB_LABEL  PANE_LABEL FILE  [PANE_LABEL FILE]\n"
            "Repeat for multiple tabs. Up to 2 panes per tab."
        ),
    )
    parser.add_argument(
        "--output",
        default="merged.html",
        help="Output file path (default: merged.html)",
    )
    args = parser.parse_args()

    tab_specs = []
    for raw in args.tab:
        try:
            tab_specs.append(_parse_tab_arg(raw))
        except argparse.ArgumentTypeError as e:
            parser.error(str(e))

    # Check for duplicate pane IDs across tabs
    seen_ids: dict[str, str] = {}
    for tab in tab_specs:
        for pane_id, _, _ in tab["panes"]:
            if pane_id in seen_ids:
                parser.error(
                    f"Duplicate PANE_ID {pane_id!r} in tabs "
                    f"{seen_ids[pane_id]!r} and {tab['label']!r}"
                )
            seen_ids[pane_id] = tab["label"]

    print("Parsing log files...")
    html_content = generate_html(tab_specs)

    with open(args.output, "w", encoding="utf-8") as fh:
        fh.write(html_content)
    print(f"\nGenerated: {args.output}")


if __name__ == "__main__":
    main()
