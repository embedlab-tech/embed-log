from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path
from typing import Optional

from .app import DEFAULT_WS_UI, parse_source, run_app
from .config import ConfigError, load_config
from .sources import LogSource


def _default_init_yaml() -> str:
    return """version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  # optional override, otherwise built-in default UI is used
  # ws_ui: /absolute/path/to/index.html
  app_name: embed-log
  open_browser: false
  # quiet | events | full
  # quiet: warnings/errors only
  # events: connection/request/source activity logs
  # full: events + print every log line to stdout
  verbosity: quiet
  # legacy switch still supported: verbose: true (maps to full)
  # optional: include CI/job id in session directory and log file names
  # job_id: GH-12345

logs:
  dir: logs/

# optional default UART baudrate for uart sources without per-source baudrate
baudrate: 115200

sources:
  - name: DUT_UART
    type: uart
    port: /dev/ttyUSB0
    inject_port: 5001
    # optional: mirror raw RX lines to one or more read-only TCP forward ports
    # forward_ports: [7001]

  - name: SENSOR_A
    type: udp
    port: 6000
    inject_port: 5002

tabs:
  - label: Devices
    panes: [DUT_UART, SENSOR_A]
"""


def _run_init(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="embed-log init",
        description="Create a starter embed-log YAML config.",
    )
    parser.add_argument("--output", "-o", default="embed-log.yml", help="output config path")
    parser.add_argument("--force", action="store_true", help="overwrite if file already exists")
    args = parser.parse_args(argv)

    out = Path(args.output)
    if out.exists() and not args.force:
        parser.error(f"file already exists: {out}. Use --force to overwrite.")

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(_default_init_yaml(), encoding="utf-8")
    print(f"Wrote config: {out}")
    print(f"Next: embed-log validate --config {out}")
    print(f"Then: embed-log run --config {out}")
    return 0


def _run_validate(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="embed-log validate",
        description="Validate an embed-log YAML config.",
    )
    parser.add_argument("--config", "-c", default="embed-log.yml", help="config file path")
    args = parser.parse_args(argv)

    try:
        cfg = load_config(args.config)
    except ConfigError as exc:
        print(f"Config INVALID: {exc}", file=sys.stderr)
        return 2

    print("Config OK")
    print(f"  sources: {len(cfg.get('sources', []))}")
    print(f"  injects: {len(cfg.get('injects', []))}")
    print(f"  forwards: {len(cfg.get('forwards', []))}")
    print(f"  tabs: {len(cfg.get('tabs', []))}")
    return 0


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="embed-log — log aggregator with WebSocket UI and TCP inject port.",
        epilog=(
            "Examples:\n"
            "  embed-log init\n"
            "  embed-log validate --config embed-log.yml\n"
            "  embed-log run --config embed-log.yml\n"
            "  python backend/server.py --config embed-log.yml\n"
            "  python backend/server.py --source DEVICE_A uart:/dev/ttyUSB0 --inject DEVICE_A 5001"
        ),
    )
    parser.add_argument(
        "--config", "-c", metavar="FILE", default=None,
        help="YAML config file (version: 1). CLI flags override config values.",
    )
    parser.add_argument(
        "--source", nargs=2, action="append", metavar=("NAME", "TYPE"),
        dest="sources", default=[],
        help="NAME  uart:/dev/path[@baud] | udp:PORT  — repeat for multiple sources",
    )
    parser.add_argument(
        "--inject", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="injects", default=[],
        help="NAME PORT — TCP inject/stream port for a source (optional, repeat)",
    )
    parser.add_argument(
        "--forward", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="forwards", default=[],
        help="NAME PORT — read-only TCP forward port for raw RX lines (optional, repeat)",
    )
    parser.add_argument(
        "--tab", nargs="+", action="append", metavar="ARG",
        dest="tabs", default=[],
        help=(
            "LABEL SOURCE [SOURCE] — group 1 or 2 sources into a UI tab "
            "(repeat for multiple tabs; omit to get one tab per source)"
        ),
    )
    parser.add_argument("--baudrate", metavar="BAUD", type=int, default=None,
                        help="default baud rate for uart sources without an explicit @baud suffix")
    parser.add_argument("--log-dir", metavar="DIR", default=None, dest="log_dir",
                        help="directory for log files (<log-dir>/<NAME>.log)")
    parser.add_argument("--host", metavar="HOST", default=None,
                        help="bind host for inject ports and WebSocket UI")
    parser.add_argument("--ws-port", metavar="PORT", type=int, default=None, dest="ws_port",
                        help="HTTP/WebSocket port for the browser UI (0 = disabled)")
    parser.add_argument("--ws-ui", metavar="FILE", default=None, dest="ws_ui",
                        help="path to the UI HTML file served at GET /")
    parser.add_argument("--app-name", metavar="NAME", default=None, dest="app_name",
                        help="app name shown in UI top-left bar")
    parser.add_argument("--open-browser", dest="open_browser", action="store_const", const=True, default=None,
                        help="open default browser automatically when UI server starts")
    parser.add_argument("--no-open-browser", dest="open_browser", action="store_const", const=False,
                        help="do not open browser automatically (overrides config)")
    parser.add_argument("--job-id", metavar="ID", default=None, dest="job_id",
                        help="optional CI/job identifier included in session/log naming")
    parser.add_argument("--verbosity", choices=["quiet", "events", "full"], default=None,
                        help="logging verbosity mode")
    parser.add_argument("-v", "--verbose", action="store_const", const=True, default=None,
                        help="shortcut for --verbosity events")
    parser.add_argument("--verbose-full", action="store_const", const=True, default=None,
                        help="shortcut for --verbosity full (prints every log line)")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)

    if argv and argv[0] == "init":
        return _run_init(argv[1:])
    if argv and argv[0] == "validate":
        return _run_validate(argv[1:])
    if argv and argv[0] == "run":
        argv = argv[1:]

    parser = _build_parser()
    args = parser.parse_args(argv)

    cfg = {}
    if args.config:
        try:
            cfg = load_config(args.config)
        except ConfigError as exc:
            parser.error(f"config error: {exc}")

    source_specs = args.sources if args.sources else cfg.get("sources", [])
    inject_specs = args.injects if args.injects else cfg.get("injects", [])
    forward_specs = args.forwards if args.forwards else cfg.get("forwards", [])
    tab_specs = args.tabs if args.tabs else cfg.get("tabs", [])

    baudrate = args.baudrate if args.baudrate is not None else cfg.get("baudrate", 115200)
    logs_root = Path(args.log_dir if args.log_dir is not None else cfg.get("log_dir", "logs/"))
    host = args.host if args.host is not None else cfg.get("host", "127.0.0.1")
    ws_port = args.ws_port if args.ws_port is not None else cfg.get("ws_port", 0)
    ws_ui = args.ws_ui if args.ws_ui is not None else cfg.get("ws_ui", DEFAULT_WS_UI)
    app_name = args.app_name if args.app_name is not None else cfg.get("app_name", "embed-log")
    cfg_verbosity = cfg.get("verbosity")
    cfg_legacy_verbose = bool(cfg.get("verbose", False))
    if args.verbosity is not None:
        verbosity = args.verbosity
    elif args.verbose_full is not None:
        verbosity = "full"
    elif args.verbose is not None:
        verbosity = "events"
    elif cfg_verbosity in {"quiet", "events", "full"}:
        verbosity = cfg_verbosity
    else:
        verbosity = "full" if cfg_legacy_verbose else "quiet"

    full_verbose = verbosity == "full"
    open_browser = args.open_browser if args.open_browser is not None else cfg.get("open_browser", False)
    job_id = args.job_id if args.job_id is not None else cfg.get("job_id", None)

    logging.basicConfig(
        level=logging.INFO if verbosity in {"events", "full"} else logging.WARNING,
        format="%(asctime)s  %(levelname)-8s  %(message)s",
        datefmt="%H:%M:%S",
    )

    if not source_specs:
        parser.error("no sources configured. Use --source ... or --config FILE with sources: ...")

    source_names: list[str] = []
    source_objects: dict[str, LogSource] = {}
    for name, spec in source_specs:
        if name in source_objects:
            parser.error(f"duplicate --source name: {name!r}")
        try:
            source_objects[name] = parse_source(name, spec, baudrate)
        except ValueError as exc:
            parser.error(str(exc))
        source_names.append(name)

    inject_ports: dict[str, int] = {}
    for name, port_value in inject_specs:
        if name not in source_objects:
            parser.error(f"--inject {name!r}: no --source with that name")
        try:
            inject_ports[name] = int(port_value)
        except ValueError:
            parser.error(f"--inject {name!r}: port must be an integer, got {port_value!r}")

    forward_ports: dict[str, list[int]] = {}
    for name, port_value in forward_specs:
        if name not in source_objects:
            parser.error(f"--forward {name!r}: no --source with that name")
        try:
            port = int(port_value)
        except ValueError:
            parser.error(f"--forward {name!r}: port must be an integer, got {port_value!r}")
        forward_ports.setdefault(name, []).append(port)

    tabs: list[dict] = []
    for tab_entry in tab_specs:
        if len(tab_entry) < 2:
            parser.error(f"--tab requires at least LABEL SOURCE, got: {tab_entry}")
        if len(tab_entry) > 3:
            parser.error(f"--tab takes at most 2 sources per tab, got: {tab_entry}")
        label = tab_entry[0]
        panes = tab_entry[1:]
        for pane in panes:
            if pane not in source_objects:
                parser.error(f"--tab {label!r}: unknown source {pane!r}")
        tabs.append({"label": label, "panes": panes})

    return run_app(
        source_names=source_names,
        source_objects=source_objects,
        inject_ports=inject_ports,
        forward_ports=forward_ports,
        tabs=tabs,
        logs_root=logs_root,
        host=host,
        verbose=full_verbose,
        ws_port=ws_port,
        ws_ui=ws_ui,
        config_path=args.config,
        job_id=job_id,
        open_browser=open_browser,
        app_name=app_name,
    )


if __name__ == "__main__":
    raise SystemExit(main())
