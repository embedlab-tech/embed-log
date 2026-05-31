"""sessions list — list recorded sessions with filtering."""

from __future__ import annotations

import datetime as _dt
import json
import sys
from pathlib import Path

from ..util import format_session_row, iter_sessions


def _run_sessions_list(log_dir: Path, args) -> int:
    sessions = iter_sessions(log_dir)

    # ── Validation ──
    if args.no_html and args.html_ready:
        print("error: --no-html and --html-ready are mutually exclusive", file=sys.stderr)
        return 1

    # ── Filters ──
    if args.after or args.before:
        after_dt = None
        before_dt = None
        def _parse_dt(value: str) -> _dt.datetime:
            """Parse ISO datetime; assume UTC if timezone is omitted."""
            d = _dt.datetime.fromisoformat(value)
            if d.tzinfo is None:
                d = d.replace(tzinfo=_dt.timezone.utc)
            return d

        if args.after:
            try:
                after_dt = _parse_dt(args.after)
            except ValueError:
                print(f"Invalid --after value: {args.after!r} (use ISO format like 2026-05-01)", file=sys.stderr)
                return 1
        if args.before:
            try:
                before_dt = _parse_dt(args.before)
            except ValueError:
                print(f"Invalid --before value: {args.before!r} (use ISO format like 2026-05-30)", file=sys.stderr)
                return 1

        filtered = []
        for s in sessions:
            started = s.get("started_at")
            if not started:
                continue
            try:
                ts = _dt.datetime.fromisoformat(started)
            except (ValueError, TypeError):
                continue
            if after_dt is not None and ts < after_dt:
                continue
            if before_dt is not None and ts > before_dt:
                continue
            filtered.append(s)
        sessions = filtered

    if args.app:
        sessions = [s for s in sessions if s.get("app_name") == args.app]

    if args.with_markers:
        sessions = [s for s in sessions if s.get("markers", 0) > 0]

    if args.no_html:
        sessions = [s for s in sessions if not s.get("session_html")]

    if args.html_ready:
        sessions = [s for s in sessions if s.get("html_status") == "ready"]

    if args.search:
        q = args.search.lower()
        matches = []
        for s in sessions:
            sid = (s.get("session_id") or "").lower()
            alias = (s.get("_alias") or "").lower()
            app = (s.get("app_name") or "").lower()
            job = (s.get("job_id") or "").lower()
            config = (s.get("config_path") or "").lower()
            if q in sid or q in alias or q in app or q in job or q in config:
                matches.append(s)
        sessions = matches

    # ── Sort ──
    if args.sort == "name":
        sessions.sort(key=lambda m: m.get("session_id", ""))

    # ── Limit ──
    if args.limit is not None and args.limit > 0:
        sessions = sessions[: args.limit]

    # ── Output ──
    if args.json:
        print(json.dumps(sessions, indent=2, default=str))
        return 0

    if not sessions:
        print(f"No sessions found in {log_dir}")
        return 0

    print(
        f"{'ALIAS':<6s}  {'ID':<40s}  {'APP':<16s}  {'LINES':>6s}  {'SIZE':>6s}  {'MRK':>4s}  {'HTML':>6s}"
    )
    print("-" * 100)
    for m in sessions:
        print(format_session_row(m))
    return 0
