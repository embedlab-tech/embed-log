"""sessions open — open session HTML in the default browser."""
from __future__ import annotations

import sys
import webbrowser
from pathlib import Path

from ..util import read_session_dir


def _run_sessions_open(log_dir: Path, args) -> int:
    sdir = read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    html_path = sdir / "session.html"
    if not html_path.is_file():
        print(f"No session HTML for session {args.session_id}", file=sys.stderr)
        print(f"Generate it with: sessions export {args.session_id}", file=sys.stderr)
        return 1

    # Parse optional marker spec: marker N
    fragment = ""
    marker_idx = None
    if hasattr(args, "open_args") and args.open_args:
        if len(args.open_args) >= 2 and args.open_args[0] == "marker":
            try:
                marker_idx = int(args.open_args[1])
                fragment = f"#marker-{marker_idx}"
            except ValueError:
                print(f"Invalid marker index: {args.open_args[1]}", file=sys.stderr)
                return 1
        else:
            print(f"Unknown open argument: {' '.join(args.open_args)}", file=sys.stderr)
            return 1

    uri = html_path.resolve().as_uri() + fragment
    webbrowser.open(uri)
    label = f"  (jumping to marker {marker_idx})" if fragment else ""
    print(f"Opened: {html_path}{label}")
    return 0
