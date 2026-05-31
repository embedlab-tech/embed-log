"""Argument parser construction for the embed-log CLI."""

from __future__ import annotations

import argparse

from ..file_tail_udp import parse_udp_target


def build_parser() -> argparse.ArgumentParser:
    """Construct the full CLI argument parser with all subcommands."""
    parser = argparse.ArgumentParser(
        description="embed-log — collect UART/UDP logs and view them in a browser UI.",
        epilog=(
            "Common workflow:\n"
            "  embed-log run --config embed-log.yml\n"
            "  embed-log sessions list\n"
            "\n"
            "Use `embed-log <command> --help` for examples and detailed options."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command", metavar="COMMAND")

    # ── run ──
    p = sub.add_parser(
        "run",
        help="start the log server from a config",
        description=(
            "Start the embed-log server from a config file or advanced inline flags.\n"
            "\n"
            "Common:\n"
            "  embed-log run --config embed-log.yml\n"
            "  embed-log run --config demo.yml --open-browser\n"
            "\n"
            "Advanced (inline sources, no config file):\n"
            "  embed-log run --source SENSOR_A uart:/dev/cu.usbmodem101@115200"
            " --tab Devices SENSOR_A"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--config",
        "-c",
        metavar="FILE",
        default=None,
        help="YAML config file. CLI flags override config values.",
    )
    p.add_argument(
        "--source",
        nargs=2,
        action="append",
        metavar=("NAME", "TYPE"),
        dest="sources",
        default=[],
        help="NAME uart:/dev/path[@baud] | udp:PORT (repeatable)",
    )
    p.add_argument(
        "--inject",
        nargs=2,
        action="append",
        metavar=("NAME", "PORT"),
        dest="injects",
        default=[],
        help="NAME PORT — TCP inject/stream port (repeatable)",
    )
    p.add_argument(
        "--forward",
        nargs=2,
        action="append",
        metavar=("NAME", "PORT"),
        dest="forwards",
        default=[],
        help="NAME PORT — read-only TCP forward port (repeatable)",
    )
    p.add_argument(
        "--tab",
        nargs="+",
        action="append",
        metavar="ARG",
        dest="tabs",
        default=[],
        help="LABEL SOURCE [SOURCE] — group 1-2 sources into a UI tab",
    )
    p.add_argument(
        "--baudrate",
        metavar="BAUD",
        type=int,
        default=None,
        help="default UART baud rate",
    )
    p.add_argument(
        "--log-dir",
        metavar="DIR",
        default=None,
        dest="log_dir",
        help="log files output directory",
    )
    p.add_argument("--host", metavar="HOST", default=None, help="bind address")
    p.add_argument(
        "--ws-port",
        metavar="PORT",
        type=int,
        default=None,
        dest="ws_port",
        help="HTTP/WebSocket port (0 = disabled)",
    )
    p.add_argument(
        "--ws-ui",
        metavar="FILE",
        default=None,
        dest="ws_ui",
        help="custom UI HTML file path",
    )
    p.add_argument(
        "--app-name",
        metavar="NAME",
        default=None,
        dest="app_name",
        help="name shown in UI top bar",
    )
    p.add_argument(
        "--open-browser",
        dest="open_browser",
        action="store_const",
        const=True,
        default=None,
        help="open browser on startup",
    )
    p.add_argument(
        "--no-open-browser",
        dest="open_browser",
        action="store_const",
        const=False,
        help="do not open browser (overrides config)",
    )
    p.add_argument(
        "--timestamp-mode",
        choices=["absolute", "relative"],
        default=None,
        dest="timestamp_mode",
        help="timestamp display/storage mode (overrides config)",
    )
    p.add_argument(
        "--default-light-theme",
        dest="default_light_theme",
        default=None,
        help="light palette key",
    )
    p.add_argument(
        "--default-dark-theme",
        dest="default_dark_theme",
        default=None,
        help="dark palette key",
    )
    p.add_argument(
        "--verbosity",
        choices=["quiet", "events", "full"],
        default=None,
        help="logging verbosity mode",
    )
    p.add_argument(
        "-v",
        "--verbose",
        action="store_const",
        const=True,
        default=None,
        help="shortcut for --verbosity events",
    )
    p.add_argument(
        "--verbose-full",
        action="store_const",
        const=True,
        default=None,
        help="shortcut for --verbosity full",
    )
    p.add_argument(
        "--job-id",
        metavar="ID",
        default=None,
        dest="job_id",
        help="CI/job identifier for session naming",
    )
    from .demo import add_subparser
    add_subparser(sub)
    # ── sessions ──
    p = sub.add_parser(
        "sessions",
        help="list, inspect, and export session artifacts",
        description="Manage embed-log session artifacts (list, info, export, logs).",
        epilog=(
            "Examples:\n"
            "  embed-log sessions list\n"
            "  embed-log sessions list --json\n"
            "  embed-log sessions info <session-id>\n"
            "  embed-log sessions export <session-id>\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    # ── skill ──
    p = sub.add_parser(
        "skill",
        help="list and export built-in skills for agent workflows",
        description=(
            "List and export built-in skill markdown files.\n"
            "\n"
            "Skills describe workflows an agent or user can follow.\n"
            "Use `skill show <name>` to print the markdown to stdout.\n"
            "\n"
            "Agents: capture the output of `skill show <name>`\n"
            "and inject it as context — no source checkout needed."
        ),
        epilog=(
            "Examples:\n"
            "  embed-log skill list\n"
            "  embed-log skill show sessions\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    # ── sample-config ──
    from .sample_config import add_subparser as add_sample_config_subparser
    add_sample_config_subparser(sub)

    # ── parse ──
    p = sub.add_parser(
        "parse",
        help="parse exported HTML back into raw logs",
        description=(
            "Parse an exported embed-log session.html back into raw session log files."
        ),
        epilog=(
            "Examples:\n"
            "  embed-log parse session.html\n"
            "  embed-log parse session.html --output my-session\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("html", help="embed-log session.html file")
    p.add_argument("--output", "-o", default=None, help="output session directory")

    # ── tail-file ──
    p = sub.add_parser(
        "tail-file",
        help="tail a file and forward lines to UDP",
        description="Tail a file and forward each appended line to a UDP port.",
        epilog=(
            "Examples:\n"
            "  embed-log tail-file app.log 127.0.0.1:6000\n"
            "  embed-log tail-file app.log 127.0.0.1:6000 --from-start\n"
            "  embed-log tail-file C:\\logs\\service.log 127.0.0.1:6000 --poll-interval 0.5\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("path", help="file to tail")
    p.add_argument("target", type=parse_udp_target, help="UDP target as HOST:PORT")
    p.add_argument(
        "--from-start",
        action="store_true",
        help="read the existing file contents first instead of starting at EOF",
    )
    p.add_argument(
        "--poll-interval",
        type=float,
        default=0.2,
        help="seconds between file polls (default: 0.2)",
    )
    p.add_argument("--encoding", default="utf-8", help="file encoding (default: utf-8)")

    # ── version ──
    p = sub.add_parser(
        "version",
        help="show version and environment information",
        description="Show version, environment, and config information.",
        epilog=(
            "Examples:\n"
            "  embed-log version\n"
            "  embed-log version --config embed-log.yml\n"
            "  embed-log version --json\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--config", "-c", default=None, help="config file to inspect")
    p.add_argument("--json", action="store_true", help="machine-readable JSON output")

    # ── ports ──
    p = sub.add_parser(
        "ports",
        help="list detected serial ports",
        description="List detected serial ports on the system.",
        epilog=("Examples:\n  embed-log ports\n  embed-log ports --json\n"),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--json", action="store_true", help="machine-readable JSON output")

    # ── update ──
    p = sub.add_parser(
        "update",
        help="update embed-log from its recorded install source",
        description="Update embed-log by re-running the appropriate installer.",
        epilog=(
            "Examples:\n"
            "  embed-log update\n"
            "  embed-log update --release\n"
            "  embed-log update --branch main\n"
            "  embed-log update --tag v1.0.1\n"
            "  embed-log update --ref abc1234\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--force", action="store_true",
                   help="force the underlying installer path when supported")
    source = p.add_mutually_exclusive_group()
    source.add_argument("--branch", help="update from a specific branch")
    source.add_argument("--tag", help="update from a specific tag")
    source.add_argument("--ref", help="update from a specific commit or git ref")
    source.add_argument("--release", action="store_true",
                        help="update to the latest GitHub release tag")
    # ── merge ──
    p = sub.add_parser(
        "merge",
        help="merge raw logs into static HTML",
        description="Merge raw log files into a standalone static HTML file.",
        epilog=(
            "Examples:\n"
            '  embed-log merge --tab "My Tab" SENSOR_A sensor.log\n'
            '  embed-log merge --tab "My Tab" SENSOR_A sensor.log --output merged.html\n'
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--tab",
        nargs="+",
        action="append",
        metavar="ARG",
        required=True,
        help="TAB_LABEL PANE_LABEL FILE [PANE_LABEL FILE] (repeatable)",
    )
    p.add_argument(
        "--output",
        default="merged.html",
        help="output HTML file path (default: merged.html)",
    )

    return parser
