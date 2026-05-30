"""sessions logs — print session log files."""
from __future__ import annotations

import sys
from pathlib import Path

from ..util import read_manifest, read_session_dir


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

    for lf in sorted(log_files):
        try:
            sys.stdout.write(lf.read_text(encoding="utf-8"))
        except OSError as exc:
            print(f"Error reading {lf}: {exc}", file=sys.stderr)
            return 1
    return 0
