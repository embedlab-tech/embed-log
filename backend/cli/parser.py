"""Argument parser construction for the embed-log CLI."""

from __future__ import annotations

import argparse

from .config_resolution import ENV_CONFIG_PATH


def build_parser() -> argparse.ArgumentParser:
    """Construct the full CLI argument parser with all subcommands."""
    parser = argparse.ArgumentParser(
        description="embed-log — collect UART/UDP logs and view them in a browser UI.",
        epilog=(
            "Common workflow:\n"
            "  embed-log onboard\n"
            "  embed-log init --output embed-log.yml\n"
            "  embed-log doctor --config embed-log.yml\n"
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
            f"  {ENV_CONFIG_PATH}=/path/embed-log.yml embed-log run\n"
            "\n"
            "Config precedence: --config > EMBED_LOG_CONFIG_YML_PATH > inline flags.\n"
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
        help=f"YAML config file (overrides {ENV_CONFIG_PATH}). CLI flags override config values.",
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

    from .onboarding import add_subparsers as add_onboarding_subparsers
    add_onboarding_subparsers(sub)

    from .update import add_subparser as add_update_subparser
    add_update_subparser(sub)
    # ── hello ──
    sub.add_parser(
        "hello",
        help="print a greeting and exit (smoke-test target for update verification)",
        description="Minimal command used to verify CLI update round-trips.",
    )

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

    # ── doctor ──
    p = sub.add_parser(
        "doctor",
        help="show environment, config, install, and runtime status",
        description=(
            "Show a sectioned diagnostic of the embed-log environment, the\n"
            f"effective config (explicit --config > {ENV_CONFIG_PATH} > inline),\n"
            "the install identity, and runtime detection (e.g. serial ports)."
        ),
        epilog=(
            "Examples:\n"
            "  embed-log doctor\n"
            "  embed-log doctor --json\n"
            "  embed-log doctor --config /path/to/embed-log.yml\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--config", "-c", default=None, help="config file to inspect (overrides env var)")
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
