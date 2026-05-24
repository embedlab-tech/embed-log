from __future__ import annotations

import argparse
import logging
import re
import sys
from pathlib import Path
from typing import Callable, Optional

import yaml
from serial.tools import list_ports

from .app import DEFAULT_WS_UI, parse_source, run_app
from .config import ConfigError, load_config
from .parse import run_parse
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
  default_light_theme: whitesand
  default_dark_theme: one-dark
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


def _slug_name(value: str, fallback: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9]+", "_", value.strip()).strip("_")
    return cleaned or fallback


def _prompt(
    text: str,
    *,
    default: Optional[str] = None,
    input_fn: Callable[[str], str] = input,
    allow_empty: bool = False,
) -> str:
    prompt = f"{text} [{default}]: " if default is not None else f"{text}: "
    while True:
        value = input_fn(prompt).strip()
        if value:
            return value
        if default is not None:
            return default
        if allow_empty:
            return ""
        print("Value is required.")


def _prompt_yes_no(
    text: str,
    *,
    default: bool,
    input_fn: Callable[[str], str] = input,
) -> bool:
    suffix = "[Y/n]" if default else "[y/N]"
    while True:
        raw = input_fn(f"{text} {suffix}: ").strip().lower()
        if not raw:
            return default
        if raw in {"y", "yes"}:
            return True
        if raw in {"n", "no"}:
            return False
        print("Enter y or n.")


def _prompt_int(
    text: str,
    *,
    default: int,
    minimum: int = 1,
    maximum: Optional[int] = None,
    input_fn: Callable[[str], str] = input,
) -> int:
    while True:
        raw = _prompt(text, default=str(default), input_fn=input_fn)
        try:
            value = int(raw)
        except ValueError:
            print("Enter a whole number.")
            continue
        if value < minimum:
            print(f"Enter a value >= {minimum}.")
            continue
        if maximum is not None and value > maximum:
            print(f"Enter a value <= {maximum}.")
            continue
        return value


def _detected_serial_ports() -> list[dict[str, str]]:
    ports = []
    for info in list_ports.comports():
        device = (info.device or "").strip()
        if not device:
            continue
        desc = (info.description or "").strip()
        if device.startswith("/dev/tty.") and "/dev/cu." + device.split("/dev/tty.", 1)[1] not in {p["device"] for p in ports}:
            continue
        ports.append({"device": device, "label": desc})

    def _sort_key(item: dict[str, str]) -> tuple[int, str]:
        device = item["device"]
        if device.startswith("COM"):
            return (0, device)
        if device.startswith("/dev/cu."):
            return (1, device)
        return (2, device)

    ports.sort(key=_sort_key)
    seen = set()
    unique = []
    for port in ports:
        if port["device"] in seen:
            continue
        seen.add(port["device"])
        unique.append(port)
    return unique


def _choose_uart_port(
    *,
    input_fn: Callable[[str], str] = input,
) -> str:
    ports = _detected_serial_ports()
    if not ports:
        return _prompt("No serial ports detected. Enter serial port path manually", input_fn=input_fn)

    print("Detected serial ports:")
    for idx, port in enumerate(ports, start=1):
        suffix = f"  ({port['label']})" if port["label"] and port["label"] != "n/a" else ""
        print(f"  {idx}) {port['device']}{suffix}")

    while True:
        choice = _prompt("Choose serial port number or type a manual path", default="1", input_fn=input_fn)
        if choice.isdigit():
            index = int(choice)
            if 1 <= index <= len(ports):
                return ports[index - 1]["device"]
            print(f"Enter a number between 1 and {len(ports)}.")
            continue
        return choice


def _build_wizard_yaml(config: dict) -> str:
    return yaml.safe_dump(config, sort_keys=False, allow_unicode=True)


def _run_create_config(argv: list[str], *, input_fn: Callable[[str], str] = input) -> int:
    parser = argparse.ArgumentParser(
        prog="embed-log create-config",
        description="Interactively create an embed-log YAML config.",
    )
    parser.add_argument("--output", "-o", default="embed-log.yml", help="output config path")
    parser.add_argument("--force", action="store_true", help="overwrite if file already exists")
    args = parser.parse_args(argv)

    print("embed-log config wizard")
    print("Press Enter to accept defaults.")
    print("")

    output_path = Path(_prompt("Config file path", default=args.output, input_fn=input_fn))
    if output_path.exists() and not args.force:
        parser.error(f"file already exists: {output_path}. Use --force to overwrite.")

    app_name = _prompt("App name", default="embed-log", input_fn=input_fn)
    open_browser = _prompt_yes_no("Open browser automatically on startup?", default=False, input_fn=input_fn)
    logs_dir = _prompt("Log directory", default="logs/", input_fn=input_fn)
    tab_count = _prompt_int("How many tabs?", default=1, minimum=1, input_fn=input_fn)

    sources: list[dict] = []
    tabs: list[dict] = []
    used_names: set[str] = set()
    used_udp_ports: set[int] = set()
    used_uart_ports: set[str] = set()

    for tab_index in range(tab_count):
        tab_default = f"Tab {tab_index + 1}"
        while True:
            tab_label = _prompt(f"Tab {tab_index + 1} label", default=tab_default, input_fn=input_fn).strip()
            if tab_label:
                break
            print("Tab label cannot be empty.")

        pane_count = _prompt_int(f"How many panes in \"{tab_label}\"?", default=1, minimum=1, maximum=2, input_fn=input_fn)
        pane_names: list[str] = []

        for pane_index in range(pane_count):
            fallback_name = _slug_name(f"{tab_label}_{pane_index + 1}", f"SOURCE_{len(sources) + 1}")
            while True:
                source_name = _prompt(f"Pane {pane_index + 1} source name", default=fallback_name, input_fn=input_fn).strip()
                source_name = _slug_name(source_name, fallback_name)
                if source_name in used_names:
                    print(f"Source name {source_name!r} is already used.")
                    continue
                used_names.add(source_name)
                break

            while True:
                source_type = _prompt(f"Source type for {source_name}", default="uart", input_fn=input_fn).strip().lower()
                if source_type in {"uart", "udp"}:
                    break
                print("Source type must be uart or udp.")

            source_cfg = {"name": source_name, "type": source_type}
            if source_type == "uart":
                while True:
                    port = _choose_uart_port(input_fn=input_fn).strip()
                    if not port:
                        print("Serial port cannot be empty.")
                        continue
                    if port in used_uart_ports:
                        print(f"Serial port {port!r} is already used.")
                        continue
                    used_uart_ports.add(port)
                    break
                baudrate = _prompt_int(f"Baudrate for {source_name}", default=115200, minimum=1, input_fn=input_fn)
                source_cfg["port"] = port
                source_cfg["baudrate"] = baudrate
            else:
                while True:
                    udp_default = 6000 + len(used_udp_ports)
                    udp_port = _prompt_int(f"UDP port for {source_name}", default=udp_default, minimum=1, maximum=65535, input_fn=input_fn)
                    if udp_port in used_udp_ports:
                        print(f"UDP port {udp_port} is already used.")
                        continue
                    used_udp_ports.add(udp_port)
                    source_cfg["port"] = udp_port
                    break

            sources.append(source_cfg)
            pane_names.append(source_name)

        tabs.append({"label": tab_label, "panes": pane_names})

    config = {
        "version": 1,
        "server": {
            "host": "127.0.0.1",
            "ws_port": 8080,
            "app_name": app_name,
            "open_browser": open_browser,
            "verbosity": "quiet",
        },
        "logs": {"dir": logs_dir},
        "sources": sources,
        "tabs": tabs,
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(_build_wizard_yaml(config), encoding="utf-8")
    print("")
    print(f"Wrote config: {output_path}")
    print(f"Next: embed-log validate --config {output_path}")
    print(f"Then: embed-log run --config {output_path}")
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
        description="embed-log — collect UART/UDP logs and view them in a browser UI.",
        epilog=(
            "Quick start:\n"
            "  embed-log run --config embed-log.yml         if you already have a config\n"
            "  embed-log create-config                      otherwise, create one\n"
            "\n"
            "Commands:\n"
            "  create-config   interactively create a config file\n"
            "  validate        validate a config file\n"
            "  run             start the log server from a config file\n"
            "\n"
            "Advanced:\n"
            "  embed-log parse session.html --output parsed-session\n"
            "  embed-log --help                                   all flags"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    # ── Config file ──
    cfg_grp = parser.add_argument_group("Config file")
    cfg_grp.add_argument(
        "--config", "-c", metavar="FILE", default=None,
        help="YAML config file. CLI flags override config values.",
    )

    # ── Advanced run options ──
    adv = parser.add_argument_group("Advanced run options")
    adv.add_argument(
        "--source", nargs=2, action="append", metavar=("NAME", "TYPE"),
        dest="sources", default=[],
        help="NAME  uart:/dev/path[@baud] | udp:PORT  — repeat for multiple sources",
    )
    adv.add_argument(
        "--inject", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="injects", default=[],
        help="NAME PORT — TCP inject/stream port for a source (optional, repeat)",
    )
    adv.add_argument(
        "--forward", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="forwards", default=[],
        help="NAME PORT — read-only TCP forward port (optional, repeat)",
    )
    adv.add_argument(
        "--tab", nargs="+", action="append", metavar="ARG",
        dest="tabs", default=[],
        help="LABEL SOURCE [SOURCE] — group 1–2 sources into a UI tab",
    )
    adv.add_argument("--baudrate", metavar="BAUD", type=int, default=None,
                     help="default UART baud rate")
    adv.add_argument("--log-dir", metavar="DIR", default=None, dest="log_dir",
                     help="log files output directory")
    adv.add_argument("--host", metavar="HOST", default=None,
                     help="bind address")

    # ── UI options ──
    ui = parser.add_argument_group("UI options")
    ui.add_argument("--ws-port", metavar="PORT", type=int, default=None, dest="ws_port",
                     help="HTTP/WebSocket port (0 = disabled)")
    ui.add_argument("--ws-ui", metavar="FILE", default=None, dest="ws_ui",
                     help="custom UI HTML file path")
    ui.add_argument("--app-name", metavar="NAME", default=None, dest="app_name",
                     help="name shown in UI top bar")
    ui.add_argument("--open-browser", dest="open_browser", action="store_const", const=True, default=None,
                     help="open browser on startup")
    ui.add_argument("--no-open-browser", dest="open_browser", action="store_const", const=False,
                     help="do not open browser (overrides config)")
    ui.add_argument("--default-light-theme", dest="default_light_theme", default=None,
                     help="light palette key")
    ui.add_argument("--default-dark-theme", dest="default_dark_theme", default=None,
                     help="dark palette key")

    # ── Job / logging ──
    misc = parser.add_argument_group("Logging and CI")
    misc.add_argument("--verbosity", choices=["quiet", "events", "full"], default=None,
                      help="logging verbosity mode")
    misc.add_argument("-v", "--verbose", action="store_const", const=True, default=None,
                      help="shortcut for --verbosity events")
    misc.add_argument("--verbose-full", action="store_const", const=True, default=None,
                      help="shortcut for --verbosity full")
    misc.add_argument("--job-id", metavar="ID", default=None, dest="job_id",
                      help="CI/job identifier for session naming")

    return parser


def main(argv: Optional[list[str]] = None) -> int:
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
            print("  embed-log run --config embed-log.yml    (if you already have a config)")
            print("")
            print("  embed-log create-config                 (otherwise, create one)")
            print("")
            print("  embed-log --help                        all options")
            print("")
            print("Development (run from source):")
            print("  python3 -m backend.server <command>")
        return 0

    if argv and argv[0] in {"init", "create-config"}:
        return _run_create_config(argv[1:])
    if argv and argv[0] == "validate":
        return _run_validate(argv[1:])
    if argv and argv[0] == "parse":
        return run_parse(argv[1:])
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
    ws_port = args.ws_port if args.ws_port is not None else cfg.get("ws_port", 8080)
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
    default_light_theme = args.default_light_theme if args.default_light_theme is not None else cfg.get("default_light_theme")
    default_dark_theme = args.default_dark_theme if args.default_dark_theme is not None else cfg.get("default_dark_theme")
    queue_maxsize = cfg.get("queue_size", 20000) if args.config else 20000

    logging.basicConfig(
        level=logging.INFO if verbosity in {"events", "full"} else logging.WARNING,
        format="%(asctime)s  %(levelname)-8s  %(message)s",
        datefmt="%H:%M:%S",
    )

    if not source_specs:
        parser.error("no sources configured. Use embed-log create-config, --source ..., or --config FILE.")

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
        default_light_theme=default_light_theme,
        default_dark_theme=default_dark_theme,
        queue_maxsize=queue_maxsize,
    )


if __name__ == "__main__":
    raise SystemExit(main())
