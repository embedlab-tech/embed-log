"""Main CLI entry point dispatcher for embed-log."""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Optional

from .parser import build_parser

from .sessions import _run_sessions
from .diagnostics import _display_version_line, _run_ports, _run_version
from .wizard import _run_create_config
from .run import _run_merge, _run_run, _run_validate
from .update import _run_update
from ..file_tail_udp import run_tail_file
from ..parse import run_parse


def main(argv: Optional[list[str]] = None) -> int:
    """Parse CLI arguments and dispatch to the appropriate handler."""
    argv = list(sys.argv[1:] if argv is None else argv)

    # ── No arguments → show guided message ──
    if not argv:
        cfg_path = Path("embed-log.yml")
        if cfg_path.exists():
            print("Config found: embed-log.yml")
            print("")
            print("  embed-log validate --config embed-log.yml")
            print("  embed-log run --config embed-log.yml")
            print("")
            print("  embed-log --help             all options")
        else:
            print("embed-log — collect UART/UDP logs with a browser UI")
            print("")
            print("Quick start:")
            print("")
            print(
                "  embed-log run --config embed-log.yml    (if you already have a config)"
            )
            print("")
            print("  embed-log create-config                 (otherwise, create one)")
            print("")
            print("  embed-log --help                        all options")
            print("")
            print("Development (run from source):")
            print("  python3 -m backend.server <command>")
        return 0

    # ── sessions uses its own internal sub-sub-parsers ──
    if argv[0] == "sessions":
        return _run_sessions(argv[1:])
    # ── Version requested ──
    if argv[0] in {"--version", "-V"}:
        print(_display_version_line())
        return 0

    # ── Help requested ──
    if argv[0] in {"-h", "--help"}:
        parser = build_parser()
        parser.print_help()
        return 0

    # ── Backward compat: bare flags → run subcommand ──
    if argv[0].startswith("-"):
        # Bare flags → run subcommand
        parser = build_parser()
        run_argv = ["run"] + argv
        args = parser.parse_args(run_argv)
        return _run_run(args)

    # ── Parse with subparser tree ──
    parser = build_parser()
    args = parser.parse_args(argv)

    # Dispatch based on parsed subcommand
    if args.command == "create-config":
        return _run_create_config(args)
    if args.command == "validate":
        return _run_validate(args)
    if args.command == "run":
        return _run_run(args)
    if args.command == "merge":
        return _run_merge(args)
    if args.command == "parse":
        return run_parse(argv[1:])  # keep old parse signature
    if args.command == "tail-file":
        return run_tail_file(args)
    if args.command in {"version", "doctor"}:
        return _run_version(args)
    if args.command == "ports":
        return _run_ports(args)
    if args.command == "update":
        return _run_update(args)

    # Should not reach here
    parser.print_help()
    return 0
