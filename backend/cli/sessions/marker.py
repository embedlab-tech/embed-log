"""sessions marker — list/show session markers."""
from __future__ import annotations

import json
import sys
from pathlib import Path

from ..util import read_session_dir


def _run_sessions_marker(log_dir: Path, args) -> int:
    if not hasattr(args, "marker_cmd") or not args.marker_cmd:
        print("error: specify a marker command: list or show", file=sys.stderr)
        return 1

    sdir = read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    markers_path = sdir / "markers.json"
    if not markers_path.is_file():
        print(f"No markers for session {args.session_id}")
        return 0

    try:
        data = json.loads(markers_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError) as e:
        print(f"Error reading markers: {e}", file=sys.stderr)
        return 1

    markers = data.get("markers", [])

    if args.marker_cmd == "list":
        if not markers:
            print(f"No markers for session {args.session_id}")
            return 0
        print(f"Session: {data.get('session_id', args.session_id)}")
        print(f"Markers: {len(markers)}")
        print()
        for i, m in enumerate(markers, 1):
            start = m.get("lineIdx", "?")
            end = m.get("endIdx", start)
            desc = m.get("description", "")
            pane = m.get("paneId", "?")
            line_range = f"line {start}" if start == end else f"lines {start}-{end}"
            print(f"  {i}. [{pane}] {line_range}")
            print(f"     {desc}")
            ts = m.get("numTs")
            if ts is not None:
                print(f"     numTs={ts}")
            print()
        return 0

    if args.marker_cmd == "show":
        idx = args.marker_index
        if idx < 1 or idx > len(markers):
            print(f"Marker index {idx} out of range (1-{len(markers)})", file=sys.stderr)
            return 1
        m = markers[idx - 1]
        print(f"Marker {idx}")
        print(f"  Pane:       {m.get('paneId', '?')}")
        start = m.get("lineIdx", "?")
        end = m.get("endIdx", start)
        print(f"  Lines:      {start}" if start == end else f"  Lines:      {start}-{end}")
        print(f"  Description: {m.get('description', '')}")
        ts = m.get("numTs")
        if ts is not None:
            print(f"  Timestamp:  {ts}")
        print(f"  Created:    {m.get('createdAt', '?')}")
        return 0

    print(f"error: unknown marker command '{args.marker_cmd}'", file=sys.stderr)
    return 1
