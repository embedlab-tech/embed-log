"""sessions delete — delete recorded session(s) by ID, age, or all."""
from __future__ import annotations

import datetime
import json
import shutil
import sys
from pathlib import Path

from ..util import iter_sessions, parse_duration, read_session_dir


def _run_sessions_delete(log_dir: Path, args) -> int:
    """Delete recorded session(s) by ID, age, or all."""
    spec = bool(args.session_id)
    older = bool(args.older_than)
    all_ = args.all
    modes = [spec, older, all_]
    if modes.count(True) > 1:
        print(
            "error: specify one of session_id, --older-than, or --all", file=sys.stderr
        )
        return 1
    if modes.count(True) == 0:
        print("error: specify a session ID, --older-than, or --all", file=sys.stderr)
        return 1

    # ── Delete specific session ──
    if args.session_id:
        sdir = read_session_dir(log_dir, args.session_id)
        if not sdir:
            print(f"Session not found: {args.session_id}", file=sys.stderr)
            return 1
        if not args.yes:
            ans = input(f"Delete session {sdir.name}? [y/N]: ").strip().lower()
            if ans not in ("y", "yes"):
                print("Aborted.")
                return 0
        shutil.rmtree(sdir)
        print(f"Deleted: {sdir}")
        return 0

    # ── Gather all sessions for age-based or all deletion ──
    sessions = iter_sessions(log_dir)

    if not sessions:
        print("No sessions found.", file=sys.stderr)
        return 0

    now = datetime.datetime.now().timestamp()

    if all_:
        to_delete = sessions
    else:  # --older-than
        secs = parse_duration(args.older_than)
        if secs is None:
            print(
                f"Invalid duration: {args.older_than!r} (use e.g. 7d, 30d, 24h)",
                file=sys.stderr,
            )
            return 1
        cutoff = now - secs
        to_delete = []
        for s in sessions:
            sdir = Path(s.get("_dir", ""))
            if not sdir.is_dir():
                continue
            started = s.get("started_at")
            if started:
                try:
                    dt = datetime.datetime.fromisoformat(started)
                    age = dt.timestamp()
                except (ValueError, TypeError):
                    age = sdir.stat().st_mtime
            else:
                age = sdir.stat().st_mtime
            if age < cutoff:
                to_delete.append(s)

    if not to_delete:
        print("No sessions match the criteria.")
        return 0

    # ── Confirm ──
    if not args.yes:
        ans = input(f"Delete {len(to_delete)} session(s)? [y/N]: ").strip().lower()
        if ans not in ("y", "yes"):
            print("Aborted.")
            return 0

    count = 0
    for s in to_delete:
        sdir = Path(s.get("_dir", ""))
        if sdir.is_dir():
            shutil.rmtree(sdir)
            count += 1

    print(f"Deleted {count} session(s).")
    return 0
