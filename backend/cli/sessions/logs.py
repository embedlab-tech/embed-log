"""sessions logs — print session log files with search and filtering."""

from __future__ import annotations

import datetime as _dt
import re
import sys
from pathlib import Path

from ..util import parse_duration, parse_log_timestamp, read_manifest, read_session_dir


def _parse_log_dt(line: str) -> _dt.datetime | None:
    """Parse a log line timestamp into a naive UTC datetime when present."""
    ts_str = parse_log_timestamp(line.rstrip("\n").rstrip("\r"))
    if not ts_str:
        return None
    try:
        return _dt.datetime.fromisoformat(ts_str.rstrip("Z"))
    except ValueError:
        return None


def _run_sessions_logs(log_dir: Path, args) -> int:
    sdir = read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = read_manifest(sdir)
    source_files = manifest.get("source_files", {}) if manifest else {}
    log_files = list(sdir.glob("*.log")) + list(sdir.glob("*.txt"))

    if args.pane:
        specific = source_files.get(args.pane)
        if specific:
            log_files = [Path(specific)]
        else:
            matched = [f for f in log_files if args.pane in f.name]
            if not matched:
                print(f"No log files matching pane {args.pane!r}", file=sys.stderr)
                return 1
            log_files = matched

    # ── Validation ──
    if args.context is not None and not args.grep:
        print("error: --context requires --grep", file=sys.stderr)
        return 1
    if args.head is not None and args.tail is not None:
        print("error: --head and --tail are mutually exclusive", file=sys.stderr)
        return 1
    if args.regex and not args.grep:
        print("error: --regex requires --grep", file=sys.stderr)
        return 1

    # ── Compile grep pattern ──
    grep_pattern = None
    if args.grep:
        flags = re.IGNORECASE if args.ignore_case else 0
        try:
            if args.regex:
                grep_pattern = re.compile(args.grep, flags)
            else:
                grep_pattern = re.compile(re.escape(args.grep), flags)
        except re.error as exc:
            print(f"error: invalid regex --grep: {exc}", file=sys.stderr)
            return 1

    # ── Resolve time filters ──
    now_local = _dt.datetime.now()
    after_dt = None
    before_dt = None

    def _parse_iso_naive_utc(value: str) -> _dt.datetime:
        """Parse ISO datetime and normalize to naive UTC for comparison with log timestamps."""
        d = _dt.datetime.fromisoformat(value)
        if d.tzinfo is not None:
            d = d.astimezone(_dt.timezone.utc).replace(tzinfo=None)
        return d

    if args.after:
        secs = parse_duration(args.after)
        if secs is not None:
            after_dt = now_local - _dt.timedelta(seconds=secs)
        else:
            try:
                after_dt = _parse_iso_naive_utc(args.after)
            except ValueError:
                print(
                    f"Invalid --after value: {args.after!r} (use 5m, 2h, or ISO)",
                    file=sys.stderr,
                )
                return 1

    if args.before:
        secs = parse_duration(args.before)
        if secs is not None:
            before_dt = now_local - _dt.timedelta(seconds=secs)
        else:
            try:
                before_dt = _parse_iso_naive_utc(args.before)
            except ValueError:
                print(
                    f"Invalid --before value: {args.before!r} (use 5m, 2h, or ISO)",
                    file=sys.stderr,
                )
                return 1

    # ── Read, filter, collect ──
    entries: list[tuple[_dt.datetime | None, int, str]] = []
    seq = 0
    for lf in sorted(log_files):
        try:
            text = lf.read_text(encoding="utf-8")
        except OSError as exc:
            print(f"Error reading {lf}: {exc}", file=sys.stderr)
            return 1

        if not text:
            continue

        lines = text.splitlines(keepends=True)

        # Time filter
        if after_dt is not None or before_dt is not None:
            filtered = []
            for line in lines:
                ts_dt = _parse_log_dt(line)
                if ts_dt is None:
                    continue
                if after_dt is not None and ts_dt < after_dt:
                    continue
                if before_dt is not None and ts_dt > before_dt:
                    continue
                filtered.append(line)
            lines = filtered

        # Grep filter
        if grep_pattern is not None:
            matching_indices = [
                i for i, line in enumerate(lines) if grep_pattern.search(line)
            ]
            if not matching_indices:
                continue

            if args.context is not None:
                ctx = args.context
                seen = set()
                context_lines = []
                for idx in matching_indices:
                    start = max(0, idx - ctx)
                    end = min(len(lines), idx + ctx + 1)
                    for ci in range(start, end):
                        if ci not in seen:
                            seen.add(ci)
                            context_lines.append(lines[ci])
                lines = context_lines
            else:
                lines = [lines[i] for i in matching_indices]

        for line in lines:
            entries.append((_parse_log_dt(line), seq, line))
            seq += 1

    if not entries:
        return 0

    # If multiple panes are involved and all kept lines have timestamps, sort chronologically
    # before applying head/tail so those operations reflect session time rather than file order.
    if len(log_files) > 1 and all(ts is not None for ts, _, _ in entries):
        entries.sort(key=lambda e: (e[0], e[1]))

    all_lines = [line for _, _, line in entries]

    # ── head / tail ──
    if args.head is not None:
        all_lines = all_lines[: args.head]
    elif args.tail is not None:
        all_lines = all_lines[-args.tail :]

    # ── Output ──
    for line in all_lines:
        sys.stdout.write(line)
    return 0
