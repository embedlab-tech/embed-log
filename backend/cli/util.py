"""Pure helper functions shared across CLI subcommands.

Every function in this module is stateless and has no side effects beyond
the filesystem reads documented in their signatures. They are safe to call
from any context and straightforward to test in isolation.
"""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from backend.session.models import SessionStats

# ---------------------------------------------------------------------------
# Timestamp parsing for log lines
# Matches: [YYYY-MM-DDTHH:MM:SS.mmm+ZZ:ZZ]
# ---------------------------------------------------------------------------

_LOG_TS_RE = re.compile(
    r"^\[(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|[+-]\d{2}:\d{2})?\]",
)


def _ms3(frac: str | None) -> str:
    """Pad or truncate a fractional-second string to exactly 3 digits."""
    if not frac:
        return "000"
    return (frac + "000")[:3]


def parse_log_timestamp(line: str) -> str | None:
    """Extract leading ISO timestamp from a log line, return as sortable string."""
    m = _LOG_TS_RE.match(line)
    if not m:
        return None
    return f"{m[1]}-{m[2]}-{m[3]}T{m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}Z"


# ---------------------------------------------------------------------------
# Duration formatting / parsing
# ---------------------------------------------------------------------------

_DURATION_MULTIPLIERS: dict[str, int] = {
    "s": 1,
    "sec": 1,
    "m": 60,
    "min": 60,
    "h": 3600,
    "hr": 3600,
    "d": 86400,
    "day": 86400,
}

_DURATION_RE = re.compile(
    r"^(\d+)\s*(s|sec|m|min|h|hr|d|day)s?$", re.IGNORECASE
)


def format_duration(seconds: float) -> str:
    """Format a duration in seconds to a human-readable string."""
    total = int(seconds)
    if total < 60:
        return f"{total}s"
    minutes = total // 60
    secs = total % 60
    if minutes < 60:
        return f"{minutes}m {secs}s"
    hours = minutes // 60
    mins = minutes % 60
    return f"{hours}h {mins}m {secs}s"


def parse_duration(text: str) -> float | None:
    """Parse a human-friendly duration like 5m, 2h, 30s, 1d into seconds.

    Returns None if the text does not match the expected pattern.
    """
    m = _DURATION_RE.match(text.strip())
    if not m:
        return None
    value = int(m[1])
    unit = m[2].lower()
    return float(value * _DURATION_MULTIPLIERS.get(unit, 1))


# ---------------------------------------------------------------------------
# File stats
# ---------------------------------------------------------------------------

def count_lines(file_path: Path) -> int:
    """Count newline-delimited lines in a file. Returns 0 on missing/unreadable files."""
    try:
        with file_path.open("rb") as f:
            return sum(1 for _ in f)
    except OSError:
        return 0


def file_size_kb(file_path: Path) -> int:
    """Return file size in whole kilobytes. Returns 0 on missing files."""
    try:
        return file_path.stat().st_size // 1024
    except OSError:
        return 0


# ---------------------------------------------------------------------------
# Session ID helpers
# ---------------------------------------------------------------------------

def short_alias(session_id: str) -> str:
    """Return a 4-character hex alias derived from the session ID."""
    return hashlib.sha256(session_id.encode()).hexdigest()[:4]


def resolve_session_id(log_dir: Path, session_id: str) -> str | None:
    """Return the full session ID matching the given ID or short alias."""
    if (log_dir / session_id).is_dir():
        return session_id
    if not log_dir.is_dir():
        return None
    for child in sorted(log_dir.iterdir()):
        if child.is_dir() and short_alias(child.name) == session_id:
            return child.name
    return None


def read_session_dir(log_dir: Path, session_id: str) -> Path | None:
    """Resolve a session ID (or alias) to its directory path."""
    full_id = resolve_session_id(log_dir, session_id)
    if full_id is None:
        return None
    sdir = log_dir / full_id
    if not sdir.is_dir():
        return None
    return sdir


def read_manifest(session_dir: Path) -> dict | None:
    """Read and parse a session's manifest.json. Returns None on any failure."""
    mf = session_dir / "manifest.json"
    if not mf.is_file():
        return None
    try:
        return json.loads(mf.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None

# ---------------------------------------------------------------------------
# Session stats and iteration
# ---------------------------------------------------------------------------

def session_stats(session_dir: Path, manifest: dict | None) -> SessionStats:
    """Return enriched metrics: alias, line count, size KB, time range, duration."""
    sid = (manifest or {}).get("session_id", session_dir.name)
    lines = 0
    size_kb = 0
    time_start: str | None = None
    time_end: str | None = None

    source_files = (manifest or {}).get("source_files", {})
    file_list: list[Path] = []
    for path_str in source_files.values():
        fp = Path(path_str)
        if fp.is_file():
            file_list.append(fp)
    if not file_list:
        file_list = sorted(session_dir.glob("*.log")) + sorted(
            session_dir.glob("*.txt")
        )

    for fp in file_list:
        lines += count_lines(fp)
        size_kb += file_size_kb(fp)
        try:
            with fp.open("r", encoding="utf-8") as f:
                first_line = None
                last_line = None
                for raw_line in f:
                    stripped = raw_line.strip()
                    if stripped and first_line is None:
                        first_line = stripped
                    if stripped:
                        last_line = stripped
                if first_line:
                    ts = parse_log_timestamp(first_line)
                    if ts and (time_start is None or ts < time_start):
                        time_start = ts
                if last_line:
                    ts = parse_log_timestamp(last_line)
                    if ts and (time_end is None or ts > time_end):
                        time_end = ts
        except OSError:
            pass

    duration_secs: float | None = None
    if time_start and time_end:
        import datetime as _dt
        try:
            t1 = _dt.datetime.fromisoformat(time_start.rstrip("Z"))
            t2 = _dt.datetime.fromisoformat(time_end.rstrip("Z"))
            duration_secs = (t2 - t1).total_seconds()
        except ValueError:
            pass

    markers_path = session_dir / "markers.json"
    marker_count = 0
    if markers_path.is_file():
        try:
            marker_data = json.loads(markers_path.read_text(encoding="utf-8"))
            marker_count = len(marker_data.get("markers", []))
        except (json.JSONDecodeError, OSError):
            pass

    return SessionStats(
        alias=short_alias(sid),
        lines=lines,
        size_kb=size_kb,
        time_start=time_start or "",
        time_end=time_end or "",
        duration_secs=duration_secs,
        markers=marker_count,
    )


def iter_sessions(log_dir: Path) -> list[dict]:
    """Iterate session directories, enriching each manifest with computed stats."""
    if not log_dir.is_dir():
        return []
    sessions = []
    for child in sorted(log_dir.iterdir()):
        if not child.is_dir():
            continue
        manifest = read_manifest(child)
        if manifest:
            manifest["_dir"] = str(child)
        else:
            manifest = {"_dir": str(child)}
        stats = session_stats(child, manifest)
        manifest["_alias"] = stats.alias
        manifest["_lines"] = stats.lines
        manifest["_size_kb"] = stats.size_kb
        manifest["_time_start"] = stats.time_start
        manifest["_time_end"] = stats.time_end
        manifest["_duration_secs"] = stats.duration_secs
        manifest["markers"] = stats.markers
        sessions.append(manifest)
    return sessions


def format_session_row(m: dict) -> str:
    """Format a session manifest dict as a single table row."""
    alias = m.get("_alias", m.get("session_id", "?")[:4])
    sid = m.get("session_id", m.get("_dir", "?"))
    app = m.get("app_name", "-")
    lines = m.get("_lines", 0)
    size_kb = m.get("_size_kb", 0)
    html = "yes" if m.get("session_html") else "-"
    markers = m.get("markers", 0)
    return f"{alias:<6s}  {sid:<40s}  {app:<16s}  {lines:>6d}  {size_kb:>4d}KB  {markers:>3d}m  {html}"
