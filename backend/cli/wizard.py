"""Config creation wizard for embed-log."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Callable

import yaml
from serial.tools import list_ports


def _default_init_yaml() -> str:
    return """version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  # optional override, otherwise built-in default UI is used
  # ws_ui: /absolute/path/to/index.html
  app_name: embed-log
  open_browser: false
  # absolute | relative
  # absolute: local wall-clock date/time
  # relative: elapsed time since the first log line (T+00:00:00.000)
  timestamp_mode: absolute
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
    default: str | None = None,
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
    maximum: int | None = None,
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
        if device.startswith("/dev/tty.") and "/dev/cu." + device.split("/dev/tty.", 1)[
            1
        ] not in {p["device"] for p in ports}:
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
        return _prompt(
            "No serial ports detected. Enter serial port path manually",
            input_fn=input_fn,
        )

    print("Detected serial ports:")
    for idx, port in enumerate(ports, start=1):
        suffix = (
            f"  ({port['label']})" if port["label"] and port["label"] != "n/a" else ""
        )
        print(f"  {idx}) {port['device']}{suffix}")

    while True:
        choice = _prompt(
            "Choose serial port number or type a manual path",
            default="1",
            input_fn=input_fn,
        )
        if choice.isdigit():
            index = int(choice)
            if 1 <= index <= len(ports):
                return ports[index - 1]["device"]
            print(f"Enter a number between 1 and {len(ports)}.")
            continue
        return choice


def _build_wizard_yaml(config: dict) -> str:
    return yaml.safe_dump(config, sort_keys=False, allow_unicode=True)


def _run_create_config(
    args: argparse.Namespace, *, input_fn: Callable[[str], str] = input
) -> int:
    print("embed-log config wizard")
    print("Press Enter to accept defaults.")
    print("")
    output_path = Path(
        _prompt("Config file path", default=args.output, input_fn=input_fn)
    )
    if output_path.exists() and not args.force:
        print(
            f"file already exists: {output_path}. Use --force to overwrite.",
            file=sys.stderr,
        )
        return 1

    app_name = _prompt("App name", default="embed-log", input_fn=input_fn)
    open_browser = _prompt_yes_no(
        "Open browser automatically on startup?", default=False, input_fn=input_fn
    )
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
            tab_label = _prompt(
                f"Tab {tab_index + 1} label", default=tab_default, input_fn=input_fn
            ).strip()
            if tab_label:
                break
            print("Tab label cannot be empty.")

        pane_count = _prompt_int(
            f'How many panes in "{tab_label}"?',
            default=1,
            minimum=1,
            maximum=2,
            input_fn=input_fn,
        )
        pane_names: list[str] = []

        for pane_index in range(pane_count):
            fallback_name = _slug_name(
                f"{tab_label}_{pane_index + 1}", f"SOURCE_{len(sources) + 1}"
            )
            while True:
                source_name = _prompt(
                    f"Pane {pane_index + 1} source name",
                    default=fallback_name,
                    input_fn=input_fn,
                ).strip()
                source_name = _slug_name(source_name, fallback_name)
                if source_name in used_names:
                    print(f"Source name {source_name!r} is already used.")
                    continue
                used_names.add(source_name)
                break

            while True:
                source_type = (
                    _prompt(
                        f"Source type for {source_name}",
                        default="uart",
                        input_fn=input_fn,
                    )
                    .strip()
                    .lower()
                )
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
                baudrate = _prompt_int(
                    f"Baudrate for {source_name}",
                    default=115200,
                    minimum=1,
                    input_fn=input_fn,
                )
                source_cfg["port"] = port
                source_cfg["baudrate"] = baudrate
            else:
                while True:
                    udp_default = 6000 + len(used_udp_ports)
                    udp_port = _prompt_int(
                        f"UDP port for {source_name}",
                        default=udp_default,
                        minimum=1,
                        maximum=65535,
                        input_fn=input_fn,
                    )
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
            "timestamp_mode": "absolute",
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
