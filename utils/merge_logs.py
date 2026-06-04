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

Each --tab takes:   TAB_LABEL  PANE_SPEC FILE  [PANE_SPEC FILE]
  TAB_LABEL  — label shown on the tab button
  PANE_SPEC  — either PANE_LABEL or PANE_ID=PANE_LABEL
  FILE       — path to the log file
Up to 2 panes per tab.

Assets (viewer.css, state.js, …) are read from the same directory as this script.
"""

import argparse
from datetime import datetime, timedelta
import html as _html
import json
import os
import re
import sys
from pathlib import Path

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
#   [T+HH:MM:SS[.frac]]                       relative elapsed time in brackets
#   YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]      bare ISO 8601 (no brackets)
#   YYYY-MM-DD HH:MM:SS[.frac]                space separator, no brackets
#   T+HH:MM:SS[.frac]                         bare relative elapsed time
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
def _relative_ts_to_ms(ts: str | None) -> int | None:
    if not ts:
        return None
    m = re.match(r"^T\+(\d+):(\d{2}):(\d{2})\.(\d{3})$", ts)
    if not m:
        return None
    return (
        int(m.group(1)) * 3_600_000
        + int(m.group(2)) * 60_000
        + int(m.group(3)) * 1_000
        + int(m.group(4))
    )


def _format_relative_ms(total_ms: int) -> str:
    if total_ms < 0:
        total_ms = 0
    hours, rem = divmod(total_ms, 3_600_000)
    minutes, rem = divmod(rem, 60_000)
    seconds, millis = divmod(rem, 1_000)
    return f"T+{hours:02d}:{minutes:02d}:{seconds:02d}.{millis:03d}"


def _format_absolute_display(dt: datetime) -> str:
    return dt.strftime("%m-%d %H:%M:%S.%f")[:-3]


def _parse_absolute_datetime(raw: str) -> datetime | None:
    stripped = _RE_ANSI_PREFIX.sub("", raw).lstrip()
    if stripped.startswith("["):
        end = stripped.find("]")
        token = stripped[1:end] if end > 0 else ""
    else:
        token = stripped.split(None, 1)[0]
    if not token or token.startswith("T+"):
        return None
    token = token.replace(",", ".")
    if "T" not in token and re.match(r"^\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}", token):
        token = token.replace(" ", "T", 1)
    if token.endswith("Z"):
        token = token[:-1] + "+00:00"
    try:
        return datetime.fromisoformat(token)
    except ValueError:
        return None


# ANSI prefix (e.g. \x1b[36m) can appear before timestamp when the whole
# line was colorised by the backend formatter.
_RE_ANSI_PREFIX = re.compile(r'^(?:\x1b\[[0-9;]*m)+')

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
# [T+HH:MM:SS[.frac]]
_RE_RELATIVE_BRACKET = re.compile(
    r"^\[T\+(\d+):(\d{2}):(\d{2})(?:[.,](\d+))?\]\s*(.*)",
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
# T+HH:MM:SS[.frac]
_RE_BARE_RELATIVE = re.compile(
    r"^T\+(\d+):(\d{2}):(\d{2})(?:[.,](\d+))?\s*(.*)",
    re.DOTALL,
)


def _parse_line(raw: str):
    """
    Try all supported timestamp formats on a raw log line.
    Returns (ts, text) where ts is "MM-DD HH:MM:SS.mmm" or "T+HH:MM:SS.mmm",
    or None if no match.

    Important: some backend lines are wrapped with ANSI color escapes, e.g.
    "\x1b[36m[2026-... ] ... \x1b[0m". We strip only the ANSI *prefix* for
    timestamp detection, keeping the rest of the line intact.
    """
    ansi_prefix = ""
    m_ansi = _RE_ANSI_PREFIX.match(raw)
    if m_ansi:
        ansi_prefix = m_ansi.group(0)
        raw = raw[m_ansi.end():]

    m = _RE_FULL_ISO_BRACKET.match(raw)
    if m:
        return (f"{m[2]}-{m[3]} {m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}", ansi_prefix + m[8])

    m = _RE_SHORT_SPACE_BRACKET.match(raw)
    if m:
        return (f"{m[1]}-{m[2]} {m[3]}:{m[4]}:{m[5]}.{_ms3(m[6])}", ansi_prefix + m[7])

    m = _RE_SHORT_ISO_BRACKET.match(raw)
    if m:
        return (f"{m[1]}-{m[2]} {m[3]}:{m[4]}:{m[5]}.{_ms3(m[6])}", ansi_prefix + m[7])

    m = _RE_RELATIVE_BRACKET.match(raw)
    if m:
        return (f"T+{m[1].zfill(2)}:{m[2]}:{m[3]}.{_ms3(m[4])}", ansi_prefix + m[5])

    m = _RE_BARE_ISO.match(raw)
    if m:
        return (f"{m[2]}-{m[3]} {m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}", ansi_prefix + m[8])

    m = _RE_SPACE_ISO.match(raw)
    if m:
        return (f"{m[2]}-{m[3]} {m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}", ansi_prefix + m[8])

    m = _RE_BARE_RELATIVE.match(raw)
    if m:
        return (f"T+{m[1].zfill(2)}:{m[2]}:{m[3]}.{_ms3(m[4])}", ansi_prefix + m[5])

    return None


def _strip_embedlog_prefixes(
    text: str,
    pane_id: str | None = None,
    pane_label: str | None = None,
) -> str:
    """Remove metadata prefixes added by embed-log's file writer.

    The live UI renders only the payload from WebSocket messages. Session HTML
    exports are rebuilt from .log files, where each line may contain extra
    prefixes such as [CONTROLLER][SERIAL] or [SYSTEM]. Strip those so saved
    session HTML looks like the live UI.
    """
    variants = set()
    for value in (pane_id, pane_label):
        if not value:
            continue
        variants.add(value)
        variants.add(value.replace("-", "_"))
        variants.add(value.replace("_", "-"))
    for variant in variants:
        text = re.sub(r"^\s*\[" + re.escape(variant) + r"\]\s*", "", text, flags=re.I)

    # Remove only redundant transport/source-type metadata that can be inferred
    # from the pane/session config. Event prefixes such as [TX::UI], [SYSTEM],
    # [demo], [TEST] carry meaning and must stay visible.
    text = re.sub(r"^\s*\[SERIAL\]\s*", "", text, flags=re.I)
    return text


def parse_log_file(
    path: str,
    pane_id: str | None = None,
    pane_label: str | None = None,
) -> list:
    """
    Read a .log file and return a list of line dicts:
        {
            "ts": str,
            "text": str,
            "isTx": bool,
            "absTs": str | None,
            "absNum": int | None,
            "relTs": str | None,
            "relNum": int | None,
        }

    Continuation lines (no timestamp) are appended to the preceding entry
    so multi-line stack traces stay together.
    """
    entries = []
    pending_ts: str | None = None
    pending_text: str | None = None
    pending_is_tx = False
    pending_abs_ts: str | None = None
    pending_abs_num: int | None = None
    pending_rel_ts: str | None = None
    pending_rel_num: int | None = None

    def _flush():
        nonlocal pending_ts, pending_text, pending_is_tx
        nonlocal pending_abs_ts, pending_abs_num, pending_rel_ts, pending_rel_num
        if pending_ts is None:
            return
        entries.append({
            "ts": pending_ts,
            "text": pending_text,
            "isTx": pending_is_tx,
            "absTs": pending_abs_ts,
            "absNum": pending_abs_num,
            "relTs": pending_rel_ts,
            "relNum": pending_rel_num,
        })
        pending_ts = pending_text = None
        pending_is_tx = False
        pending_abs_ts = pending_rel_ts = None
        pending_abs_num = pending_rel_num = None

    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            for raw in fh:
                raw = raw.rstrip("\n\r")
                parsed = _parse_line(raw)
                if parsed:
                    _flush()
                    pending_ts = parsed[0]
                    pending_is_tx = "[TX::" in parsed[1]
                    pending_text = _strip_embedlog_prefixes(parsed[1], pane_id, pane_label)

                    pending_rel_num = _relative_ts_to_ms(pending_ts)
                    if pending_rel_num is not None:
                        pending_rel_ts = pending_ts
                        pending_abs_ts = None
                        pending_abs_num = None
                    else:
                        pending_abs_ts = pending_ts
                        pending_rel_ts = None
                        dt = _parse_absolute_datetime(raw)
                        pending_abs_num = int(dt.timestamp() * 1000) if dt is not None else None
                elif pending_ts is not None and raw.strip():
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
    # Remove multi-line imports (import { ... } from '...')
    src = re.sub(r"^import\s*\{[^}]*\}\s*from\s*['\"][^'\"]*['\"]\s*;?\s*", "", src, flags=re.MULTILINE)
    # Remove export keyword from declarations (function, class, const, let, var)
    src = re.sub(r"^export\s+(async\s+)?(function|class|const|let|var)\b", r"\1\2", src, flags=re.MULTILINE)
    # Remove standalone export { ... } statements
    src = re.sub(r"^export\s*\{[^}]*\}\s*(?:from\s*['\"][^'\"]*['\"])?\s*;?\r?\n?", "", src, flags=re.MULTILINE)
    return src


def _esc_script_text(src: str) -> str:
    """Prevent embedded </script> from terminating the surrounding script tag."""
    return re.sub(r"</script", "<\\/script", src, flags=re.IGNORECASE)


# ---------------------------------------------------------------------------
# HTML generation
# ---------------------------------------------------------------------------

def _pane_html(pane_id: str, label: str, *, show_tx: bool = False, stats_text: str = "") -> str:
    """Render one pane div. TX input row is hidden by default (static mode)."""
    safe_label = _html.escape(label)
    safe_stats = _html.escape(stats_text) if stats_text else ""
    tx_display = '' if show_tx else ' style="display:none"'
    stats_span = f'<span class="pane-stats" data-pane-stats="{_html.escape(pane_id)}">{safe_stats}</span>' if stats_text else f'<span class="pane-stats" data-pane-stats="{_html.escape(pane_id)}"></span>'
    return f"""\
        <div class="pane" id="pane-{pane_id}">
            <div class="pane-header">
                <span class="pane-name">{safe_label}</span>
                {stats_span}

                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>

            </div>
            <div class="filter-bar">
                <input class="filter-input" data-pane="{pane_id}" placeholder="Filter (regex)…">
            </div>
            <div class="pane-body">
                <div class="log-area" id="log-{pane_id}"><div class="log-spacer"><div class="log-window"></div></div></div>
                <button class="jump-btn" id="jump-{pane_id}">jump to bottom</button>
            </div>
            <div class="input-row"{tx_display}>
                <input class="serial-input" id="input-{pane_id}" autocomplete="off">
                <button class="send-btn" data-pane="{pane_id}">Send</button>
            </div>
        </div>"""



def _tab_content_html(tab_idx: int, tab_panes: list, pane_stats: dict | None = None) -> str:
    """Render a tab-content div containing 1 or 2 panes (+ splitter if 2)."""
    parts = []
    for i, (pane_id, label) in enumerate(tab_panes):
        if i > 0:
            parts.append('        <div class="splitter"></div>')
        stats = (pane_stats or {}).get(pane_id, "")
        parts.append(_pane_html(pane_id, label, show_tx=False, stats_text=stats))
    inner = "\n".join(parts)
    return (
        f'    <div class="tab-content" id="tab-content-{tab_idx}">\n'
        f'{inner}\n'
        f'    </div>'
    )


def _render_toolbar(total_stats_text: str = "") -> str:
    """Render toolbar HTML for static/exported mode."""
    safe_total = _html.escape(total_stats_text) if total_stats_text else ""
    parts = ['<div id="toolbar">']
    parts.append('    <span class="app-name">embed-log</span>')

    # Actions present in static mode (matches JS renderToolbar with STATIC_PROFILE)
    static_actions = [
        ("btn-download-raw", "Download raw", "Download all logs as merged raw text file"),
        ("btn-unwrap",       "Unwrap",       "Unwrap multi-pane tabs into single-pane tabs"),
        ("btn-timestamp-mode", "Absolute",   "Switch timestamps"),
    ]
    sep_done = False
    for btn_id, label, title in static_actions:
        if not sep_done:
            parts.append('    <div class="sep"></div>')
            sep_done = True
        parts.append(f'    <button id="{btn_id}" title="{_html.escape(title)}">{label}</button>')

    # Theme toggle
    parts.append('    <div class="sep"></div>')
    parts.append('    <button id="btn-theme" title="Toggle light / dark theme">&#x1F319;</button>')

    if safe_total:
        parts.append(f'    <div id="toolbar-stats" class="toolbar-stats">· {safe_total}</div>')
    else:
        parts.append('    <div id="toolbar-stats" class="toolbar-stats"></div>')

    # Marker navigation (hidden by default, shown when markers are present)
    parts.append('    <div id="marker-nav" class="marker-nav" style="display:none">')
    parts.append('        <button id="marker-nav-prev" title="Previous marker">&#x25C0;</button>')
    parts.append('        <span id="marker-nav-idx">1</span>/<span id="marker-nav-total">0</span>')
    parts.append('        <button id="marker-nav-next" title="Next marker">&#x25B6;</button>')
    parts.append('    </div>')

    parts.append('</div>')
    return '\n'.join(parts)

def _enrich_timestamp_variants(
    log_data: dict[str, list],
    *,
    timestamp_mode: str,
    first_log_at: str | None,
) -> str | None:
    origin_dt = None
    if first_log_at:
        token = first_log_at[:-1] + "+00:00" if first_log_at.endswith("Z") else first_log_at
        try:
            origin_dt = datetime.fromisoformat(token)
        except ValueError:
            origin_dt = None

    if origin_dt is None:
        abs_candidates = [
            entry["absNum"]
            for entries in log_data.values()
            for entry in entries
            if isinstance(entry.get("absNum"), int)
        ]
        if abs_candidates:
            origin_dt = datetime.fromtimestamp(min(abs_candidates) / 1000).astimezone()

    origin_ms = int(origin_dt.timestamp() * 1000) if origin_dt is not None else None
    for entries in log_data.values():
        for entry in entries:
            abs_num = entry.get("absNum")
            rel_num = entry.get("relNum")
            if rel_num is None and abs_num is not None and origin_ms is not None:
                rel_num = max(0, abs_num - origin_ms)
                entry["relNum"] = rel_num
                entry["relTs"] = _format_relative_ms(rel_num)
            if abs_num is None and rel_num is not None and origin_dt is not None:
                abs_dt = origin_dt + timedelta(milliseconds=rel_num)
                entry["absNum"] = int(abs_dt.timestamp() * 1000)
                entry["absTs"] = _format_absolute_display(abs_dt)
            if timestamp_mode == "relative" and entry.get("relTs"):
                entry["ts"] = entry["relTs"]
            elif timestamp_mode == "absolute" and entry.get("absTs"):
                entry["ts"] = entry["absTs"]
            elif entry.get("absTs"):
                entry["ts"] = entry["absTs"]
            elif entry.get("relTs"):
                entry["ts"] = entry["relTs"]

    return origin_dt.isoformat(timespec="milliseconds") if origin_dt is not None else first_log_at

def generate_html(
    tab_specs: list,
    *,
    timestamp_mode: str = "absolute",
    first_log_at: str | None = None,
    markers_file: str | None = None,
    frontend_plugins: dict[str, dict] | None = None,
    pane_plugins: dict[str, list[dict]] | None = None,
    plugin_scripts: dict[str, str] | None = None,
    lazy: bool = True,
) -> str:
    """
    tab_specs: [
        { "label": str, "panes": [(pane_id, pane_label, file_path), ...] },
        ...
    ]
    Returns a complete self-contained HTML string.

    If lazy=True (default), line data is embedded as compact JSON arrays
    and hydrated via windowed rendering for fast load even with 100k+ lines.
    Set lazy=False or use --legacy-embed for the old full-DOM embedding.
    """
    frontend_plugins = frontend_plugins or {}
    pane_plugins = pane_plugins or {}
    plugin_scripts = plugin_scripts or {}

    log_data: dict[str, list] = {}
    for tab in tab_specs:
        for pane_id, pane_label, file_path in tab["panes"]:
            entries = parse_log_file(file_path, pane_id, pane_label)
            log_data[pane_id] = entries
            print(f"  [{tab['label']}] {pane_label!r}: {len(entries)} lines  ({file_path})")

    # Load markers from markers.json if provided
    markers_list: list[dict] = []
    if markers_file:
        try:
            markers_path = Path(markers_file)
            if markers_path.is_file():
                markers_data = json.loads(markers_path.read_text(encoding="utf-8"))
                markers_list = markers_data.get("markers", [])
        except (json.JSONDecodeError, OSError):
            pass
    effective_first_log_at = _enrich_timestamp_variants(
        log_data,
        timestamp_mode=timestamp_mode,
        first_log_at=first_log_at,
    )
    # Read frontend assets (strip ES module syntax for classic <script> embedding)
    def _js(filename: str) -> str:
        return _esc_script_text(_strip_module_syntax(_read_asset(filename)))

    css = _read_asset("viewer.css")
    profile_js = _js("profile.js")
    render_pane_js = _js("renderPane.js")
    render_toolbar_js = _js("renderToolbar.js")
    plugin_runtime_js = _js("pluginRuntime.js")
    state_js = _js("state.js")
    themes_js = _js("themes.js")
    settings_js = _js("settings.js")
    fontsize_js = _js("fontsize.js")
    ansi_js = _js("ansi.js")
    lines_js = _js("lines.js")
    tabs_js = _js("tabs.js")
    tabcreate_js = _js("tabcreate.js")
    ui_js = _js("ui.js")
    export_js = _js("export.js")
    selection_js = _js("selection.js")
    tsparse_js = _js("tsparse.js")
    import_js = _js("import.js")

    # ws.js intentionally omitted — no WebSocket in static mode

    tabs_json = json.dumps([
        {"id": f"tab-{i}", "label": tab["label"], "panes": [p[0] for p in tab["panes"]]}
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
    pane_labels_json = json.dumps({pane_id: pane_label for tab in tab_specs for pane_id, pane_label, _ in tab["panes"]}, ensure_ascii=False)

    active_plugin_names: list[str] = []
    active_seen: set[str] = set()
    for refs in pane_plugins.values():
        if not isinstance(refs, list):
            continue
        for ref in refs:
            if not isinstance(ref, dict):
                continue
            name = ref.get("name")
            if not isinstance(name, str) or not name:
                continue
            if name in active_seen:
                continue
            if name not in frontend_plugins or name not in plugin_scripts:
                continue
            active_seen.add(name)
            active_plugin_names.append(name)

    plugin_scripts_json = json.dumps(
        {name: plugin_scripts[name] for name in active_plugin_names},
        ensure_ascii=False,
    )
    frontend_plugins_json = json.dumps(
        {name: frontend_plugins[name] for name in active_plugin_names},
        ensure_ascii=False,
    )
    pane_plugins_json = json.dumps(pane_plugins, ensure_ascii=False)
    plugin_script_tags = "\n".join(
        f"<script>{_esc_script_text(plugin_scripts[name])}</script>"
        for name in active_plugin_names
    )

    # Build container HTML
    # ── Per-pane / total stats ──
    # Bytes use UTF-8 length of the raw text — matches what the live UI
    # computes so live mode and static replay show the same numbers.
    def _fmt_int(n: int) -> str:
        return f"{n:,}"

    def _fmt_bytes(n: int) -> str:
        if n < 1024:
            return f"{n} B"
        if n < 1024 * 1024:
            return f"{n / 1024:.1f} kB" if n < 10 * 1024 else f"{n / 1024:.0f} kB"
        return f"{n / (1024 * 1024):.1f} MB"

    def _stats_text(line_count: int, byte_count: int) -> str:
        if line_count <= 0:
            return ""
        return f"{_fmt_int(line_count)} lines · {_fmt_bytes(byte_count)}"

    pane_stats: dict[str, str] = {}
    total_lines = 0
    total_bytes = 0
    for tab in tab_specs:
        for pane_id, _pane_label, _file_path in tab["panes"]:
            entries = log_data.get(pane_id, [])
            byte_sum = sum(len((row.get("text") or "").encode("utf-8")) for row in entries)
            pane_stats[pane_id] = _stats_text(len(entries), byte_sum)
            total_lines += len(entries)
            total_bytes += byte_sum
    total_stats = _stats_text(total_lines, total_bytes)

    # Build container HTML
    tab_contents = "\n".join(
        _tab_content_html(i, [(p[0], p[1]) for p in tab["panes"]], pane_stats)
        for i, tab in enumerate(tab_specs)
    )

    title = _html.escape(" + ".join(tab["label"] for tab in tab_specs))

    # Config script: sets window globals before state.js loads.
    STATIC_PROFILE = {
        "kind": "static",
        "capabilities": {
            "clearAll": False,
            "downloadRaw": True,
            "exportHtml": False,
            "fontSize": True,
            "paneSwap": True,
            "persistCache": False,
            "selectionExportHtml": True,
            "sessionApi": False,
            "themeToggle": True,
            "tx": False,
            "unwrap": True,
            "wsStatus": False,
            "dynamicTabs": False,
        },
    }
    config_js = _esc_script_text(
        f"window.__embedLogProfile = {json.dumps(STATIC_PROFILE)};\n"
        f"window.TABS = {tabs_json};\n"
        f"window.PANES = {panes_json};\n"
        f"window.PANE_LABELS = {pane_labels_json};\n"
        f"window.__embedLogFrontendPlugins = {frontend_plugins_json};\n"
        f"window.__embedLogPanePlugins = {pane_plugins_json};\n"
        f"window.__embedLogPluginScripts = {plugin_scripts_json};\n"
        f"window.__embedLogInitialPanePluginUiState = {{}};\n"
        f"window.__embedLogInitialTimestampMode = {json.dumps(timestamp_mode)};\n"
        f"window.__embedLogFirstLogAt = {json.dumps(effective_first_log_at)};\n"
        f"window.__embedLogInitialFontSize = 14;"
    )

    # Bootstrap script: runs after all other scripts to inject log data
    if lazy:
        # ── Lazy / windowed mode ──
        # Embed compact JSON arrays in <script data-pane> tags.
        # The frontend hydrates them into state.rawLines[] without creating
        # DOM elements, then calls renderPaneWindow for initial visible window.
        def _compact_entry(entry: dict) -> list:
            meta = {}
            for k in ("absTs", "absNum", "relTs", "relNum"):
                v = entry.get(k)
                if v is not None:
                    meta[k] = v
            return [entry["ts"], entry["text"], entry["isTx"], meta or None]

        pane_data_tags = "\n".join(
            f'<script type="application/json" data-pane="{pane_id}">'
            f"{json.dumps([_compact_entry(e) for e in entries], ensure_ascii=False).replace('</', '<\\/')}"
            f"</script>"
            for pane_id, entries in log_data.items()
        )

        bootstrap_js = _esc_script_text(f"""\
(function () {{
    "use strict";

    // No WebSocket in static mode — satisfy the reference in ui.js
    window.wsSend = function () {{}};

    if (typeof hydratePanesFromJson === "function") {{
        hydratePanesFromJson();
    }}

    if (typeof window.__embedLogUpdateTimestampModeUi === "function") {{
        window.__embedLogUpdateTimestampModeUi();
    }}

    var _markers = {json.dumps(markers_list, ensure_ascii=False)};
    if (_markers.length) {{
        state.markers = {{}};
        _markers.forEach(function (m) {{
            if (!m.paneId) return;
            state.markers[m.paneId] = state.markers[m.paneId] || [];
            state.markers[m.paneId].push(m);
        }});
        if (typeof applyMarkers === "function") applyMarkers();
        if (typeof window.__embedLogOnMarkers === "function") window.__embedLogOnMarkers();
    }}
}})();""")

        # Inject pane data tags between config scripts and the main body
        pane_data_block = "\n" + pane_data_tags
    else:
        # ── Legacy mode: embed all lines as DOM elements via appendLine ──
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
            appendLine(paneId, e.ts, e.text, e.isTx, e);
        }});
        document.getElementById("log-" + paneId).scrollTop = 0;
        state.atBottom[paneId] = false;
        updateJumpBtn(paneId);
    }}

    PANES.forEach(_loadPane);

    var _markers = {json.dumps(markers_list, ensure_ascii=False)};
    if (_markers.length) {{
        state.markers = {{}};
        _markers.forEach(function (m) {{
            if (!m.paneId) return;
            state.markers[m.paneId] = state.markers[m.paneId] || [];
            state.markers[m.paneId].push(m);
        }});
        if (typeof applyMarkers === "function") applyMarkers();
        if (typeof window.__embedLogOnMarkers === "function") window.__embedLogOnMarkers();
    }}
}})();""")
        config_with_data = config_js
        pane_data_block = ""

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

{_render_toolbar(total_stats)}

<div id="download-raw-menu">
    <div class="download-raw-head">Download raw logs</div>
    <div class="download-raw-body">
        <button id="btn-download-merged" class="download-raw-opt">Merged (.log) — all panes interleaved</button>
        <button id="btn-download-split" class="download-raw-opt">Per pane (.log files) — one file per source</button>
    </div>
</div>

<!-- ── TAB BAR — shown by tabs.js when there is more than one tab ── -->
<div id="tab-bar"></div>

<!-- ── PANES ────────────────────────────────────────────────── -->
<div id="container">
{tab_contents}
</div>

<script>{config_js}</script>
{pane_data_block}
<script>{profile_js}</script>
<script>{render_pane_js}</script>
<script>{render_toolbar_js}</script>
<script>{plugin_runtime_js}</script>
{plugin_script_tags}
<script>{state_js}</script>
<script>{themes_js}</script>
<script>{settings_js}</script>
<script>{fontsize_js}</script>
<script>{ansi_js}</script>
<script>{lines_js}</script>
<script>{tabs_js}</script>
<script>{tabcreate_js}</script>
<script>{ui_js}</script>
<script>{export_js}</script>
<script>{selection_js}</script>
<script>{tsparse_js}</script>
<script>{import_js}</script>
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

    panes = []
    for i in range(0, len(rest), 2):
        pane_spec = rest[i]
        file_path = rest[i + 1]
        if "=" in pane_spec:
            pane_id, pane_label = pane_spec.split("=", 1)
            pane_id = pane_id.strip()
            pane_label = pane_label.strip()
            if not pane_id or not pane_label:
                raise argparse.ArgumentTypeError(
                    f"Invalid pane spec {pane_spec!r}; use PANE_LABEL or PANE_ID=PANE_LABEL"
                )
        else:
            pane_label = pane_spec
            pane_id = _slug(pane_label)
        panes.append((pane_id, pane_label, file_path))
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
    parser.add_argument(
        "--timestamp-mode",
        choices=["absolute", "relative"],
        default="absolute",
        help="Initial timestamp mode for the exported viewer (default: absolute)",
    )
    parser.add_argument(
        "--first-log-at",
        default=None,
        help="Absolute ISO timestamp of the first log line; enables absolute/relative conversion in static replay",
    )
    parser.add_argument(
        "--markers-file",
        default=None,
        help="Path to markers.json; embedded markers will be navigable in the exported viewer",
    )
    parser.add_argument(
        "--frontend-plugins-json",
        default=None,
        help="JSON object mapping frontend plugin names to metadata",
    )
    parser.add_argument(
        "--pane-plugins-json",
        default=None,
        help="JSON object mapping pane ids to active frontend plugins",
    )
    parser.add_argument(
        "--plugin-scripts-json",
        default=None,
        help="JSON object mapping frontend plugin names to plain JS source",
    )
    parser.add_argument(
        "--no-lazy",
        dest="lazy",
        action="store_false",
        help="Disable lazy/windowed rendering (embed all lines as DOM elements, slow for large files)",
    )
    parser.add_argument(
        "--legacy-embed",
        dest="lazy",
        action="store_false",
        help="Alias for --no-lazy (legacy full-DOM embedding)",
    )
    parser.set_defaults(lazy=True)
    args = parser.parse_args()

    def _json_arg(name: str) -> dict:
        raw_value = getattr(args, name)
        if not raw_value:
            return {}
        try:
            value = json.loads(raw_value)
        except json.JSONDecodeError as exc:
            parser.error(f"--{name.replace('_', '-')} must be valid JSON: {exc}")
        if not isinstance(value, dict):
            parser.error(f"--{name.replace('_', '-')} must decode to a JSON object")
        return value

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
    html_content = generate_html(
        tab_specs,
        timestamp_mode=args.timestamp_mode,
        first_log_at=args.first_log_at,
        markers_file=args.markers_file,
        frontend_plugins=_json_arg("frontend_plugins_json"),
        pane_plugins=_json_arg("pane_plugins_json"),
        plugin_scripts=_json_arg("plugin_scripts_json"),
        lazy=args.lazy,
    )

    with open(args.output, "w", encoding="utf-8") as fh:
        fh.write(html_content)
    print(f"\nGenerated: {args.output}")


if __name__ == "__main__":
    main()
