"""Session subcommand handlers for the embed-log CLI."""
from __future__ import annotations

import argparse
from pathlib import Path

from .delete import _run_sessions_delete
from .export import _run_sessions_export
from .info import _run_sessions_info
from .list import _run_sessions_list
from .logs import _run_sessions_logs
from .marker import _run_sessions_marker
from .open import _run_sessions_open
from .snippet import _run_sessions_snippet


def _run_sessions(argv: list[str]) -> int:
    # Shared arguments that each subcommand inherits.
    # --dir is the documented form; --log-dir is kept as a compatibility alias
    # because session commands read/manage an existing session root rather
    # than configure runtime logging (which is what `run --log-dir` does).
    shared = argparse.ArgumentParser(add_help=False)
    shared.add_argument(
        "--dir",
        "--log-dir",
        dest="log_dir",
        default=None,
        help="session log root directory (default: logs/)",
    )

    parser = argparse.ArgumentParser(
        prog="embed-log sessions",
        description=(
            "Inspect and manage recorded sessions.\n"
            "\n"
            "Common workflows:\n"
            "  sessions list                       list all sessions (markers shown in MRK col)\n"
            "  sessions info <session-id>           session details\n"
            "  sessions export <session-id>         export HTML for one session\n"
            "  sessions export --missing            export HTML for all sessions without it\n"
            "  sessions open <session-id>           open session HTML\n"
            "  sessions open <session-id> marker N  open and jump to marker N\n"
            "  sessions marker list <session-id>    list markers for a session\n"
            "  sessions marker show <session-id> N  show marker N details\n"
            "  sessions snippet list <session-id>   list saved selection snippets\n"
            "  sessions snippet show <session-id>   show the most recent snippet\n"
            "  sessions delete --all                delete all sessions\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command")

    p_list = sub.add_parser("list", parents=[shared], help="list recorded sessions")
    p_list.add_argument("--sort", choices=["date", "name"], default="date")
    p_list.add_argument("--limit", type=int, default=None)
    p_list.add_argument("--json", action="store_true", help="machine-readable JSON output")
    p_list.add_argument("--search", default=None, help="free-text search session id, alias, app name, job id, config path")
    p_list.add_argument("--with-markers", action="store_true", help="only sessions with markers")
    p_list.add_argument("--no-html", action="store_true", help="only sessions without HTML export")
    p_list.add_argument("--html-ready", action="store_true", help="only sessions with ready HTML export")
    p_list.add_argument("--app", default=None, help="filter by app name")
    p_list.add_argument("--after", default=None, help="only sessions started after this time (ISO or date)")
    p_list.add_argument("--before", default=None, help="only sessions started before this time (ISO or date)")

    p_info = sub.add_parser("info", parents=[shared], help="show session details")
    p_info.add_argument("session_id")
    p_info.add_argument("--json", action="store_true", help="machine-readable JSON output")
    p_logs = sub.add_parser("logs", parents=[shared], help="print session log files")
    p_logs.add_argument("session_id")
    p_logs.add_argument("--pane", default=None, help="filter by pane name")
    p_logs.add_argument("--grep", default=None, help="search for text in log lines (substring or regex)")
    p_logs.add_argument("--regex", action="store_true", help="treat --grep as a Python regex")
    p_logs.add_argument("--ignore-case", action="store_true", help="case-insensitive search")
    p_logs.add_argument("--tail", type=int, default=None, help="show only last N matching lines")
    p_logs.add_argument("--head", type=int, default=None, help="show only first N matching lines")
    p_logs.add_argument("--context", type=int, default=None, help="show N lines of context around matches (requires --grep)")
    p_logs.add_argument("--after", default=None, help="only lines after this time (relative: 5m, 2h, 30s or ISO timestamp)")
    p_logs.add_argument("--before", default=None, help="only lines before this time (relative or ISO)")
    p_export = sub.add_parser(
        "export",
        parents=[shared],
        help="export session data (HTML or merged raw log)",
        epilog=(
            "Examples:\n"
            "  sessions export <session-id>\n"
            "  sessions export <session-id> --format raw\n"
            "  sessions export <session-id> --format raw --after 1h\n"
            "  sessions export --missing\n"
            "  sessions export <session-id> --format raw --first 10m\n"
            "  sessions export <session-id> --format raw --last 30m\n"
            "  sessions export <session-id> --format raw --pane SENSOR_A\n"
            "  sessions export <session-id> --format raw --after 5m --output recent.log"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p_export.add_argument(
        "session_id",
        nargs="?",
        default=None,
        help="session ID or short alias to export",
    )
    p_export.add_argument(
        "--missing",
        action="store_true",
        help="export all sessions that don't have HTML yet",
    )
    p_export.add_argument("--output", default=None, help="output file path")
    p_export.add_argument(
        "--format",
        choices=["html", "raw"],
        default="html",
        help="output format: html (default) or raw merged log",
    )
    p_export.add_argument(
        "--after",
        default=None,
        help="include lines after this time (relative: 5m, 2h, 30s or ISO timestamp)",
    )
    p_export.add_argument(
        "--before",
        default=None,
        help="include lines before this time (relative or ISO, default: end of data)",
    )
    p_export.add_argument(
        "--first",
        default=None,
        help="include only the first N minutes/hours of the session (e.g. 10m, 1h)",
    )
    p_export.add_argument(
        "--last",
        default=None,
        help="include only the last N minutes/hours of the session (e.g. 30m, 15m)",
    )
    p_export.add_argument(
        "--pane",
        action="append",
        default=None,
        dest="panes",
        help="include only this pane (repeatable, default: all)",
    )
    p_export.add_argument(
        "--first-log-at",
        default=None,
        help="override the absolute ISO timestamp of the first log line when rebuilding HTML",
    )

    p_open = sub.add_parser(
        "open", parents=[shared], help="open session HTML in the default browser",
        epilog=(
            "Examples:\n"
            "  sessions open                       open latest session\n"
            "  sessions open <session-id>          open specific session\n"
            "  sessions open <session-id> marker N  open and jump to marker N\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p_open.add_argument(
        "session_id",
        nargs="?",
        default=None,
        help="session ID or short alias (default: latest session)",
    )
    p_open.add_argument("open_args", nargs="*", help="optional: specify marker N to jump to a marker (e.g. marker 2)")

    # ── delete ──
    p_delete = sub.add_parser(
        "delete",
        parents=[shared],
        help="delete recorded session(s)",
        epilog=(
            "Examples:\n"
            "  sessions delete <session-id>\n"
            "  sessions delete <session-id> --yes\n"
            "  sessions delete --older-than 7d\n"
            "  sessions delete --older-than 30d --yes\n"
            "  sessions delete --all\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p_delete.add_argument(
        "session_id",
        nargs="?",
        default=None,
        help="session ID or short alias to delete",
    )
    p_delete.add_argument(
        "--older-than",
        default=None,
        help="delete sessions older than this duration (e.g. 7d, 30d, 24h)",
    )
    p_delete.add_argument("--all", action="store_true", help="delete all sessions")
    p_delete.add_argument(
        "--yes", "-y", action="store_true", help="skip confirmation prompt"
    )

    # ── marker ──
    p_marker = sub.add_parser(
        "marker",
        parents=[shared],
        help="list/show session markers",
        epilog=(
            "Examples:\n"
            "  sessions marker list <session-id>\n"
            "  sessions marker show <session-id> 2\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p_marker_sub = p_marker.add_subparsers(dest="marker_cmd")

    p_marker_list = p_marker_sub.add_parser(
        "list", parents=[shared], help="list all markers for a session"
    )
    p_marker_list.add_argument("session_id")
    p_marker_list.add_argument("--search", default=None, help="filter markers by description text")
    p_marker_list.add_argument("--pane", default=None, help="filter markers by pane name")

    p_marker_show = p_marker_sub.add_parser(
        "show", parents=[shared], help="show a specific marker"
    )
    p_marker_show.add_argument("session_id")
    p_marker_show.add_argument(
        "marker_index", type=int, help="marker index (1-based, from list)"
    )

    # ── snippet ──
    p_snippet = sub.add_parser(
        "snippet",
        parents=[shared],
        help="manage selection snippets for a session",
        epilog=(
            "Examples:\n"
            "  sessions snippet list <session-id>\n"
            "  sessions snippet show <session-id>\n"
            "  sessions snippet show <session-id> --index 2\n"
            "  sessions snippet show <session-id> <snippet-file>\n"
            "  sessions snippet delete <session-id> --all\n"
            "  sessions snippet delete <session-id> --index 2\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p_snip_sub = p_snippet.add_subparsers(dest="snippet_cmd")

    p_snip_list = p_snip_sub.add_parser(
        "list", parents=[shared], help="list all snippets for a session"
    )
    p_snip_list.add_argument("session_id")
    p_snip_list.add_argument("--json", action="store_true", help="machine-readable JSON output")

    p_snip_show = p_snip_sub.add_parser(
        "show", parents=[shared], help="print snippet content to stdout"
    )
    p_snip_show.add_argument("session_id")
    p_snip_show.add_argument(
        "snippet_id",
        nargs="?",
        default=None,
        help="snippet filename or prefix (default: use --last)",
    )
    p_snip_show.add_argument(
        "--last", action="store_true", help="show the most recent snippet"
    )
    p_snip_show.add_argument(
        "--index", type=int, default=None, help="snippet index (1-based, from list)"
    )

    p_snip_delete = p_snip_sub.add_parser(
        "delete", parents=[shared], help="delete snippet(s)"
    )
    p_snip_delete.add_argument("session_id")
    p_snip_delete.add_argument(
        "--index", type=int, default=None, help="delete by index (1-based, from list)"
    )
    p_snip_delete.add_argument(
        "--all", action="store_true", help="delete all snippets for this session"
    )

    args = parser.parse_args(argv)

    if args.command is None:
        parser.print_help()
        return 0

    log_dir = Path(args.log_dir) if args.log_dir else Path("logs/")

    match args.command:
        case "list":
            return _run_sessions_list(log_dir, args)
        case "info":
            return _run_sessions_info(log_dir, args)
        case "logs":
            return _run_sessions_logs(log_dir, args)
        case "export":
            return _run_sessions_export(log_dir, args)
        case "open":
            return _run_sessions_open(log_dir, args)
        case "delete":
            return _run_sessions_delete(log_dir, args)
        case "snippet":
            return _run_sessions_snippet(log_dir, args)
        case "marker":
            return _run_sessions_marker(log_dir, args)
        case _:
            return 1


__all__ = [
    "_run_sessions",
    "_run_sessions_list",
    "_run_sessions_info",
    "_run_sessions_logs",
    "_run_sessions_export",
    "_run_sessions_open",
    "_run_sessions_marker",
    "_run_sessions_snippet",
    "_run_sessions_delete",
]
