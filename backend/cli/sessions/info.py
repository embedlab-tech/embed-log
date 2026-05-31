"""sessions info — show session details."""
from __future__ import annotations

import json
import sys
from pathlib import Path

from ..util import (
    count_lines,
    file_size_kb,
    format_duration,
    read_manifest,
    read_session_dir,
    short_alias,
)


def _run_sessions_info(log_dir: Path, args) -> int:
    sdir = read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = read_manifest(sdir)
    if not manifest:
        print(f"No manifest found for session {args.session_id}", file=sys.stderr)
        print(f"Directory: {sdir}")
        return 1

    if args.json:
        print(json.dumps(manifest, indent=2, default=str))
        return 0

    # Human-readable format
    sid = manifest.get("session_id", "?")
    alias = short_alias(sid)
    total_lines = 0
    total_size_kb = 0
    duration_secs = manifest.get("_duration_secs")
    time_start = manifest.get("_time_start", "")
    time_end = manifest.get("_time_end", "")

    print(f"Session:      {sid}")
    print(f"Alias:        {alias}")
    print(f"App:          {manifest.get('app_name', '-')}")
    print(f"Started:      {manifest.get('started_at', '-')}")
    if time_start and time_end:
        dur = format_duration(duration_secs) if duration_secs else "?"
        print(f"Time range:   {time_start}  →  {time_end}")
        print(f"Duration:     {dur}")
    print(f"Job ID:       {manifest.get('job_id', '-')}")
    print(f"Config:       {manifest.get('config_path', '-')}")
    print(f"HTML export:  {manifest.get('session_html', '-')}")
    print(f"HTML status:  {manifest.get('html_status', '-')}")
    print(f"Sources:")
    for name, path in sorted(manifest.get("source_files", {}).items()):
        fp = Path(path)
        lines = count_lines(fp)
        skb = file_size_kb(fp)
        total_lines += lines
        total_size_kb += skb
        sizestr = f"{skb}KB" if skb else "?"
        print(f"  {name:<20s}  {lines:<6d} lines  {sizestr:<8s}  {path}")
    print(f"           {'─' * 50}")
    print(f"  {'Total':<20s}  {total_lines:<6d} lines  {total_size_kb:<4d}KB")
    print(f"Tabs:")
    for tab in manifest.get("tabs", []):
        panes = ", ".join(tab.get("panes", []))
        print(f"  {tab.get('label', '?')}:  {panes}")
    return 0
