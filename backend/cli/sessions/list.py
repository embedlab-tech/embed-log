"""sessions list — list recorded sessions."""
from __future__ import annotations

import json
import sys
from pathlib import Path

from ..util import format_session_row, iter_sessions


def _run_sessions_list(log_dir: Path, args) -> int:
    sessions = iter_sessions(log_dir)
    if args.sort == "name":
        sessions.sort(key=lambda m: m.get("session_id", ""))
    if args.limit is not None and args.limit > 0:
        sessions = sessions[: args.limit]

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
