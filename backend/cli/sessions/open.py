"""sessions open — open session HTML in the default browser."""
from __future__ import annotations

import sys
import webbrowser
from pathlib import Path

from ..util import iter_sessions, read_session_dir


def _resolve_latest_session(log_dir: Path) -> str | None:
    """Return the session_id of the most recent session, or None."""
    sessions = iter_sessions(log_dir)
    if not sessions:
        return None
    # iter_sessions returns sessions sorted by directory name ascending.
    # Session dirs are named YYYY-MM-DD_HH-MM-SS, so the last entry is the newest.
    latest = sessions[-1]
    # session_id might be in the manifest, or we fall back to the directory name
    sid = latest.get("session_id")
    if sid:
        return sid
    # Derive from _dir
    sdir = latest.get("_dir", "")
    if sdir:
        return Path(sdir).name
    return None


def _run_sessions_open(log_dir: Path, args) -> int:
    session_id = args.session_id

    # No session_id given → resolve the latest
    if not session_id:
        session_id = _resolve_latest_session(log_dir)
        if not session_id:
            print("No sessions found.", file=sys.stderr)
            return 1

    sdir = read_session_dir(log_dir, session_id)
    if not sdir:
        print(f"Session not found: {session_id}", file=sys.stderr)
        return 1

    html_path = sdir / "session.html"
    if not html_path.is_file():
        print(f"No session HTML for session {session_id}", file=sys.stderr)
        print(f"Generate it with: sessions export {session_id}", file=sys.stderr)
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
    indicator = " (latest)" if not args.session_id else ""
    print(f"Opened: {html_path}{label}{indicator}")
    return 0
