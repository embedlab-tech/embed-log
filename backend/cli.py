from __future__ import annotations

import argparse
import hashlib
import json
import logging
import re
import shutil
import datetime
import sys
import webbrowser
from pathlib import Path
from typing import Callable, Optional

import yaml
from serial.tools import list_ports

from .app import DEFAULT_WS_UI, parse_source, run_app
from .file_tail_udp import parse_udp_target, run_tail_file
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


def _run_create_config(args: argparse.Namespace, *, input_fn: Callable[[str], str] = input) -> int:
    print("embed-log config wizard")
    print("Press Enter to accept defaults.")
    print("")
    output_path = Path(_prompt("Config file path", default=args.output, input_fn=input_fn))
    if output_path.exists() and not args.force:
        print(f"file already exists: {output_path}. Use --force to overwrite.", file=sys.stderr)
        return 1

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


def _run_validate(args: argparse.Namespace) -> int:
    try:
        cfg = load_config(args.config)
    except ConfigError as exc:
        if args.json:
            print(json.dumps({"ok": False, "error": str(exc), "config": args.config}))
        else:
            print(f"Config INVALID: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps({
            "ok": True,
            "config": args.config,
            "sources": len(cfg.get("sources", [])),
            "injects": len(cfg.get("injects", [])),
            "forwards": len(cfg.get("forwards", [])),
            "tabs": len(cfg.get("tabs", [])),
        }))
        return 0

    print("Config OK")
    print(f"  sources: {len(cfg.get('sources', []))}")
    print(f"  injects: {len(cfg.get('injects', []))}")
    print(f"  forwards: {len(cfg.get('forwards', []))}")
    print(f"  tabs: {len(cfg.get('tabs', []))}")
    return 0
def _run_sessions(argv: list[str]) -> int:
    # Shared arguments that each subcommand inherits
    shared = argparse.ArgumentParser(add_help=False)
    shared.add_argument("--log-dir", default="logs/",
                        help="log directory (default: logs/)")
    shared.add_argument("--json", action="store_true",
                        help="machine-readable JSON output")

    parser = argparse.ArgumentParser(
        prog="embed-log sessions",
        description="Inspect recorded sessions from disk.",
    )
    sub = parser.add_subparsers(dest="command")

    p_list = sub.add_parser("list", parents=[shared],
                            help="list recorded sessions")
    p_list.add_argument("--sort", choices=["date", "name"], default="date")
    p_list.add_argument("--limit", type=int, default=None)

    p_info = sub.add_parser("info", parents=[shared],
                            help="show session details")
    p_info.add_argument("session_id")

    p_logs = sub.add_parser("logs", parents=[shared],
                            help="print session log files")
    p_logs.add_argument("session_id")
    p_logs.add_argument("--pane", default=None, help="filter by pane name")

    p_export = sub.add_parser("export", parents=[shared],
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
                              formatter_class=argparse.RawDescriptionHelpFormatter)
    p_export.add_argument("session_id", nargs="?", default=None,
                           help="session ID or short alias to export")
    p_export.add_argument("--missing", action="store_true",
                           help="export all sessions that don't have HTML yet")
    p_export.add_argument("--output", default=None,
                          help="output file path")
    p_export.add_argument("--format", choices=["html", "raw"], default="html",
                          help="output format: html (default) or raw merged log")
    p_export.add_argument("--after", default=None,
                          help="include lines after this time (relative: 5m, 2h, 30s or ISO timestamp)")
    p_export.add_argument("--before", default=None,
                          help="include lines before this time (relative or ISO, default: end of data)")
    p_export.add_argument("--first", default=None,
                          help="include only the first N minutes/hours of the session (e.g. 10m, 1h)")
    p_export.add_argument("--last", default=None,
                          help="include only the last N minutes/hours of the session (e.g. 30m, 15m)")
    p_export.add_argument("--pane", action="append", default=None, dest="panes",
                          help="include only this pane (repeatable, default: all)")

    p_open = sub.add_parser("open", parents=[shared],
                            help="open session HTML in the default browser")
    p_open.add_argument("session_id")

    # ── delete ──
    p_delete = sub.add_parser("delete", parents=[shared],
                               help="delete recorded session(s)",
                               epilog=(
                                   "Examples:\n"
                                   "  sessions delete <session-id>\n"
                                   "  sessions delete <session-id> --yes\n"
                                   "  sessions delete --older-than 7d\n"
                                   "  sessions delete --older-than 30d --yes\n"
                                   "  sessions delete --all\n"
                               ),
                               formatter_class=argparse.RawDescriptionHelpFormatter)
    p_delete.add_argument("session_id", nargs="?", default=None,
                           help="session ID or short alias to delete")
    p_delete.add_argument("--older-than", default=None,
                           help="delete sessions older than this duration (e.g. 7d, 30d, 24h)")
    p_delete.add_argument("--all", action="store_true",
                           help="delete all sessions")
    p_delete.add_argument("--yes", "-y", action="store_true",
                           help="skip confirmation prompt")

    args = parser.parse_args(argv)

    if args.command is None:
        parser.print_help()
        return 0

    log_dir = Path(args.log_dir) if hasattr(args, "log_dir") else Path("logs/")

    if args.command == "list":
        return _run_sessions_list(log_dir, args)
    if args.command == "info":
        return _run_sessions_info(log_dir, args)
    if args.command == "logs":
        return _run_sessions_logs(log_dir, args)
    if args.command == "export":
        return _run_sessions_export(log_dir, args)
    if args.command == "open":
        return _run_sessions_open(log_dir, args)
    if args.command == "delete":
        return _run_sessions_delete(log_dir, args)
    return 1


def _read_session_dir(log_dir: Path, session_id: str) -> Path | None:
    full_id = _resolve_session_id(log_dir, session_id)
    if full_id is None:
        return None
    sdir = log_dir / full_id
    if not sdir.is_dir():
        return None
    return sdir


def _read_manifest(session_dir: Path) -> dict | None:
    mf = session_dir / "manifest.json"
    if not mf.is_file():
        return None
    try:
        return json.loads(mf.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None


def _short_alias(session_id: str) -> str:
    return hashlib.sha256(session_id.encode()).hexdigest()[:4]


def _resolve_session_id(log_dir: Path, session_id: str) -> str | None:
    """Return the full session ID matching the given ID or short alias."""
    if (log_dir / session_id).is_dir():
        return session_id
    if not log_dir.is_dir():
        return None
    for child in sorted(log_dir.iterdir()):
        if child.is_dir() and _short_alias(child.name) == session_id:
            return child.name
    return None


def _count_lines(file_path: Path) -> int:
    try:
        with file_path.open("rb") as f:
            return sum(1 for _ in f)
    except OSError:
        return 0


def _file_size_kb(file_path: Path) -> int:
    try:
        return file_path.stat().st_size // 1024
    except OSError:
        return 0


# Timestamp parsing for log lines — matches format: [YYYY-MM-DDTHH:MM:SS.mmm+ZZ:ZZ]
_LOG_TS_RE = re.compile(
    r"^\[(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|[+-]\d{2}:\d{2})?\]",
)


def _parse_log_timestamp(line: str) -> str | None:
    """Extract leading ISO timestamp from a log line, return as sortable string."""
    m = _LOG_TS_RE.match(line)
    if not m:
        return None
    return f"{m[1]}-{m[2]}-{m[3]}T{m[4]}:{m[5]}:{m[6]}.{_ms3(m[7])}Z"


def _ms3(frac: str | None) -> str:
    if not frac:
        return "000"
    return (frac + "000")[:3]


def _format_duration(seconds: float) -> str:
    """Format a duration in seconds to a human-readable string."""
    seconds = int(seconds)
    if seconds < 60:
        return f"{seconds}s"
    minutes = seconds // 60
    secs = seconds % 60
    if minutes < 60:
        return f"{minutes}m {secs}s"
    hours = minutes // 60
    mins = minutes % 60
    return f"{hours}h {mins}m {secs}s"


def _parse_duration(text: str) -> float | None:
    """Parse a human-friendly duration like 5m, 2h, 30s, 1d into seconds."""
    import re as _re
    m = _re.match(r"^(\d+)\s*(s|sec|m|min|h|hr|d|day)s?$", text.strip(), _re.IGNORECASE)
    if not m:
        return None
    value = int(m[1])
    unit = m[2].lower()
    multipliers = {"s": 1, "sec": 1, "m": 60, "min": 60, "h": 3600, "hr": 3600, "d": 86400, "day": 86400}
    return value * multipliers.get(unit, 1)


def _session_stats(session_dir: Path, manifest: dict | None) -> dict:
    """Return enriched metrics: alias, line count, size KB, time range, duration."""
    sid = (manifest or {}).get("session_id", session_dir.name)
    lines = 0
    size_kb = 0
    time_start: str | None = None
    time_end: str | None = None

    source_files = (manifest or {}).get("source_files", {})
    file_list: list[Path] = []
    for path_str in source_files.values():
        fp = Path(path_str)
        if fp.is_file():
            file_list.append(fp)
    if not file_list:
        file_list = sorted(session_dir.glob("*.log")) + sorted(session_dir.glob("*.txt"))

    for fp in file_list:
        lines += _count_lines(fp)
        size_kb += _file_size_kb(fp)
        try:
            with fp.open("r", encoding="utf-8") as f:
                # First non-empty line
                first_line = None
                last_line = None
                for raw_line in f:
                    stripped = raw_line.strip()
                    if stripped and first_line is None:
                        first_line = stripped
                    if stripped:
                        last_line = stripped
                if first_line:
                    ts = _parse_log_timestamp(first_line)
                    if ts and (time_start is None or ts < time_start):
                        time_start = ts
                if last_line:
                    ts = _parse_log_timestamp(last_line)
                    if ts and (time_end is None or ts > time_end):
                        time_end = ts
        except OSError:
            pass

    duration_secs: float | None = None
    if time_start and time_end:
        # ISO timestamps sort lexicographically — convert to datetime for delta
        import datetime as _dt
        try:
            t1 = _dt.datetime.fromisoformat(time_start.rstrip("Z"))
            t2 = _dt.datetime.fromisoformat(time_end.rstrip("Z"))
            duration_secs = (t2 - t1).total_seconds()
        except ValueError:
            pass

    return {
        "alias": _short_alias(sid),
        "lines": lines,
        "size_kb": size_kb,
        "time_start": time_start or "",
        "time_end": time_end or "",
        "duration_secs": duration_secs,
    }


def _iter_sessions(log_dir: Path) -> list[dict]:
    if not log_dir.is_dir():
        return []
    sessions = []
    for child in sorted(log_dir.iterdir()):
        if not child.is_dir():
            continue
        manifest = _read_manifest(child)
        if manifest:
            manifest["_dir"] = str(child)
        else:
            manifest = {"_dir": str(child)}
        stats = _session_stats(child, manifest)
        manifest["_alias"] = stats["alias"]
        manifest["_lines"] = stats["lines"]
        manifest["_size_kb"] = stats["size_kb"]
        manifest["_time_start"] = stats["time_start"]
        manifest["_time_end"] = stats["time_end"]
        manifest["_duration_secs"] = stats["duration_secs"]
        sessions.append(manifest)
    return sessions


def _format_session_row(m: dict) -> str:
    alias = m.get("_alias", m.get("session_id", "?")[:4])
    sid = m.get("session_id", m.get("_dir", "?"))
    app = m.get("app_name", "-")
    lines = m.get("_lines", 0)
    size_kb = m.get("_size_kb", 0)
    html = "yes" if m.get("session_html") else "-"
    return f"{alias:<6s}  {sid:<40s}  {app:<16s}  {lines:<6d}  {size_kb:<4d}KB  {html}"


def _run_sessions_list(log_dir: Path, args: argparse.Namespace) -> int:
    sessions = _iter_sessions(log_dir)
    if args.sort == "name":
        sessions.sort(key=lambda m: m.get("session_id", ""))
    if args.limit is not None and args.limit > 0:
        sessions = sessions[: args.limit]

    if args.json:
        print(json.dumps(sessions, indent=2, default=str))
        return 0

    if not sessions:
        print(f"No sessions found in {log_dir}")
        return 0

    print(f"{'ALIAS':<6s}  {'ID':<40s}  {'APP':<16s}  {'LINES':<6s}  {'SIZE':<6s}  {'HTML'}")
    print("-" * 90)
    for m in sessions:
        print(_format_session_row(m))
    return 0


def _run_sessions_info(log_dir: Path, args: argparse.Namespace) -> int:
    sdir = _read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = _read_manifest(sdir)
    if not manifest:
        print(f"No manifest found for session {args.session_id}", file=sys.stderr)
        print(f"Directory: {sdir}")
        return 1

    if args.json:
        print(json.dumps(manifest, indent=2, default=str))
        return 0

    # Human-readable format
    sid = manifest.get("session_id", "?")
    alias = _short_alias(sid)
    total_lines = 0
    total_size_kb = 0
    duration_secs = manifest.get("_duration_secs")
    time_start = manifest.get("_time_start", "")
    time_end = manifest.get("_time_end", "")

    print(f"Session:      {sid}")
    print(f"Alias:        {alias}")
    print(f"App:          {manifest.get('app_name', '-')}")
    print(f"Started:      {manifest.get('started_at', '-')}")
    if time_start and time_end:
        dur = _format_duration(duration_secs) if duration_secs else "?"
        print(f"Time range:   {time_start}  →  {time_end}")
        print(f"Duration:     {dur}")
    print(f"Job ID:       {manifest.get('job_id', '-')}")
    print(f"Config:       {manifest.get('config_path', '-')}")
    print(f"HTML export:  {manifest.get('session_html', '-')}")
    print(f"HTML status:  {manifest.get('html_status', '-')}")
    print(f"Sources:")
    for name, path in sorted(manifest.get("source_files", {}).items()):
        fp = Path(path)
        lines = _count_lines(fp)
        skb = _file_size_kb(fp)
        total_lines += lines
        total_size_kb += skb
        sizestr = f"{skb}KB" if skb else "?"
        print(f"  {name:<20s}  {lines:<6d} lines  {sizestr:<8s}  {path}")
    print(f"           {'─' * 50}")
    print(f"  {'Total':<20s}  {total_lines:<6d} lines  {total_size_kb:<4d}KB")
    print(f"Tabs:")
    for tab in manifest.get("tabs", []):
        panes = ", ".join(tab.get("panes", []))
        print(f"  {tab.get('label', '?')}:  {panes}")
    return 0


def _run_sessions_logs(log_dir: Path, args: argparse.Namespace) -> int:
    sdir = _read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = _read_manifest(sdir)
    source_files = manifest.get("source_files", {}) if manifest else {}
    log_files = list(sdir.glob("*.log")) + list(sdir.glob("*.txt"))

    if args.pane:
        specific = source_files.get(args.pane)
        if specific:
            log_files = [Path(specific)]
        else:
            matched = [f for f in log_files if args.pane in f.name]
            if not matched:
                print(f"No log files matching pane {args.pane!r}", file=sys.stderr)
                return 1
            log_files = matched

    for lf in sorted(log_files):
        try:
            sys.stdout.write(lf.read_text(encoding="utf-8"))
        except OSError as exc:
            print(f"Error reading {lf}: {exc}", file=sys.stderr)
            return 1
    return 0


def _run_sessions_export(log_dir: Path, args: argparse.Namespace) -> int:
    # ── --missing mode: export every session without existing HTML ──
    if args.missing and not args.session_id:
        if args.format != "html":
            print("error: --missing is only supported for --format html", file=sys.stderr)
            return 1
        sessions = _iter_sessions(log_dir)
        if not sessions:
            print("No sessions found.", file=sys.stderr)
            return 0
        ok = fail = 0
        for s in sessions:
            sid = s.get("session_id")
            if not sid:
                continue
            sdir = log_dir / sid
            html_path = sdir / "session.html"
            if html_path.is_file():
                # Already has an export, skip
                continue
            # Re-invoke the single-session export path
            sub_args = argparse.Namespace(
                session_id=sid, output=None, format="html",
                after=None, before=None, first=None, last=None,
                panes=None, missing=False, json=args.json, log_dir=str(log_dir),
            )
            rc = _run_sessions_export(log_dir, sub_args)
            if rc == 0:
                ok += 1
            else:
                fail += 1
        total = ok + fail
        if fail:
            print(f"Exported {ok}/{total} session(s), {fail} failed.")
        else:
            print(f"Exported {ok} session(s).")
        return 0 if fail == 0 else 1

    # ── Single-session export (existing behavior) ──
    sdir = _read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = _read_manifest(sdir)
    if not manifest:
        print(f"No manifest found for session {args.session_id}", file=sys.stderr)
        return 1

    source_files = manifest.get("source_files", {})
    if not source_files:
        print(f"No source files in manifest for session {args.session_id}", file=sys.stderr)
        return 1

    # ── Raw format: merge and filter log lines ──
    if args.format == "raw":
        # Resolve which panes to include
        panes: list[str] = []
        if args.panes:
            panes = args.panes
        else:
            panes = sorted(source_files.keys())
        # Parse time filters
        import datetime as _dt
        after_dt: _dt.datetime | None = None
        before_dt: _dt.datetime | None = None
        now_local = _dt.datetime.now()

        # Resolve --first / --last into after/before using session time range
        time_start = manifest.get("_time_start", "")
        time_end = manifest.get("_time_end", "")
        if (args.first or args.last) and (not time_start or not time_end):
            stats = _session_stats(sdir, manifest)
            time_start = stats["time_start"]
            time_end = stats["time_end"]
        time_conflicts = []
        if args.after:
            time_conflicts.append("--after")
        if args.before:
            time_conflicts.append("--before")
        if args.first:
            time_conflicts.append("--first")
        if args.last:
            time_conflicts.append("--last")
        if len(time_conflicts) > 2:
            print(f"Conflicting time options: {' and '.join(time_conflicts)}", file=sys.stderr)
            return 1
        if args.first and args.last:
            print("--first and --last are mutually exclusive", file=sys.stderr)
            return 1

        if args.first:
            secs = _parse_duration(args.first)
            if secs is None:
                print(f"Invalid --first value: {args.first!r} (use e.g. 10m, 1h)", file=sys.stderr)
                return 1
            if not time_start:
                print("Cannot use --first: session has no recorded start time", file=sys.stderr)
                return 1
            try:
                after_dt = _dt.datetime.fromisoformat(time_start.rstrip("Z"))
                before_dt = after_dt + _dt.timedelta(seconds=secs)
            except ValueError:
                print(f"Cannot parse session start time: {time_start}", file=sys.stderr)
                return 1

        elif args.last:
            secs = _parse_duration(args.last)
            if secs is None:
                print(f"Invalid --last value: {args.last!r} (use e.g. 30m, 15m)", file=sys.stderr)
                return 1
            if not time_end:
                print("Cannot use --last: session has no recorded end time", file=sys.stderr)
                return 1
            try:
                before_dt = _dt.datetime.fromisoformat(time_end.rstrip("Z"))
                after_dt = before_dt - _dt.timedelta(seconds=secs)
            except ValueError:
                print(f"Cannot parse session end time: {time_end}", file=sys.stderr)
                return 1

        if args.after:
            secs = _parse_duration(args.after)
            if secs is not None:
                after_dt = now_local - _dt.timedelta(seconds=secs)
            else:
                try:
                    after_dt = _dt.datetime.fromisoformat(args.after)
                except ValueError:
                    print(f"Invalid --after value: {args.after!r} (use 5m, 2h, or ISO)", file=sys.stderr)
                    return 1

        if args.before:
            secs = _parse_duration(args.before)
            if secs is not None:
                before_dt = now_local - _dt.timedelta(seconds=secs) if not args.after else (
                    after_dt + _dt.timedelta(seconds=secs) if after_dt else now_local - _dt.timedelta(seconds=secs)
                )
            else:
                try:
                    before_dt = _dt.datetime.fromisoformat(args.before)
                except ValueError:
                    print(f"Invalid --before value: {args.before!r} (use 5m, 2h, or ISO)", file=sys.stderr)
                    return 1

        # Collect and filter lines
        entries: list[dict] = []
        for pane_name in panes:
            fp_str = source_files.get(pane_name)
            if not fp_str:
                print(f"Pane {pane_name!r} not found in session", file=sys.stderr)
                return 1
            fp = Path(fp_str)
            if not fp.is_file():
                print(f"Log file not found: {fp}", file=sys.stderr)
                return 1
            try:
                with fp.open("r", encoding="utf-8") as f:
                    for raw_line in f:
                        stripped = raw_line.strip()
                        if not stripped:
                            continue
                        ts_str = _parse_log_timestamp(stripped)
                        if not ts_str:
                            continue
                        ts_dt = _dt.datetime.fromisoformat(ts_str.rstrip("Z"))
                        if after_dt and ts_dt < after_dt:
                            continue
                        if before_dt and ts_dt > before_dt:
                            continue
                        entries.append({"ts": ts_str, "line": stripped, "pane": pane_name})
            except OSError as exc:
                print(f"Error reading {fp}: {exc}", file=sys.stderr)
                return 1

        if not entries:
            print("No matching log entries found.", file=sys.stderr)
            return 1

        # Sort by timestamp, then by pane name for stability
        entries.sort(key=lambda e: (e["ts"], e["pane"]))

        output = Path(args.output) if args.output else sdir / "merged.log"
        try:
            output.parent.mkdir(parents=True, exist_ok=True)
            with output.open("w", encoding="utf-8") as f:
                for entry in entries:
                    if len(panes) > 1:
                        f.write(f"[{entry['pane']}] {entry['line']}\n")
                    else:
                        f.write(entry["line"] + "\n")
        except OSError as exc:
            print(f"Error writing {output}: {exc}", file=sys.stderr)
            return 1

        print(f"Exported: {output}  ({len(entries)} lines)")
        return 0

    # ── HTML format (existing behavior) ──
    from .session import SessionExporter

    tabs = manifest.get("tabs", [])
    output = Path(args.output) if args.output else sdir / "session.html"

    exporter = SessionExporter(
        session_html_path=output,
        source_files=source_files,
        tabs=tabs,
    )
    ok = exporter.export_html("sessions_export")
    if not ok:
        print(f"Export failed for session {args.session_id}", file=sys.stderr)
        return 1

    # Update manifest
    from datetime import datetime as _dt2
    manifest["session_html"] = str(output)
    manifest["html_status"] = "ready"
    manifest["html_updated_at"] = _dt2.now().astimezone().isoformat(timespec="seconds")
    (sdir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    print(f"Exported: {output}")
    return 0
def _run_sessions_open(log_dir: Path, args: argparse.Namespace) -> int:
    sdir = _read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    html_path = sdir / "session.html"
    if not html_path.is_file():
        print(f"No session HTML for session {args.session_id}", file=sys.stderr)
        print(f"Generate it with: sessions export {args.session_id}", file=sys.stderr)
        return 1

    webbrowser.open(html_path.resolve().as_uri())
    print(f"Opened: {html_path}")
    return 0


def _run_sessions_delete(log_dir: Path, args: argparse.Namespace) -> int:
    """Delete recorded session(s) by ID, age, or all."""
    spec = bool(args.session_id)
    older = bool(args.older_than)
    all_ = args.all
    modes = [spec, older, all_]
    if modes.count(True) > 1:
        print("error: specify one of session_id, --older-than, or --all", file=sys.stderr)
        return 1
    if modes.count(True) == 0:
        print("error: specify a session ID, --older-than, or --all", file=sys.stderr)
        return 1

    # ── Delete specific session ──
    if args.session_id:
        sdir = _read_session_dir(log_dir, args.session_id)
        if not sdir:
            print(f"Session not found: {args.session_id}", file=sys.stderr)
            return 1
        if not args.yes:
            ans = input(f"Delete session {sdir.name}? [y/N]: ").strip().lower()
            if ans not in ("y", "yes"):
                print("Aborted.")
                return 0
        shutil.rmtree(sdir)
        print(f"Deleted: {sdir}")
        return 0

    # ── Gather all sessions for age-based or all deletion ──
    sessions = _iter_sessions(log_dir)
    if not sessions:
        print("No sessions found.", file=sys.stderr)
        return 0

    now = datetime.datetime.now().timestamp()

    if all_:
        to_delete = sessions
    else:  # --older-than
        secs = _parse_duration(args.older_than)
        if secs is None:
            print(f"Invalid duration: {args.older_than!r} (use e.g. 7d, 30d, 24h)", file=sys.stderr)
            return 1
        cutoff = now - secs
        to_delete = []
        for s in sessions:
            sdir = Path(s.get("_dir", ""))
            if not sdir.is_dir():
                continue
            started = s.get("started_at")
            if started:
                try:
                    dt = datetime.datetime.fromisoformat(started)
                    age = dt.timestamp()
                except (ValueError, TypeError):
                    age = sdir.stat().st_mtime
            else:
                age = sdir.stat().st_mtime
            if age < cutoff:
                to_delete.append(s)

    if not to_delete:
        print("No sessions match the criteria.")
        return 0

    # ── Confirm ──
    if not args.yes:
        ans = input(f"Delete {len(to_delete)} session(s)? [y/N]: ").strip().lower()
        if ans not in ("y", "yes"):
            print("Aborted.")
            return 0

    count = 0
    for s in to_delete:
        sdir = Path(s.get("_dir", ""))
        if sdir.is_dir():
            shutil.rmtree(sdir)
            count += 1

    print(f"Deleted {count} session(s).")
    return 0


def _run_merge(args: argparse.Namespace) -> int:
    import subprocess

    merge_script = Path(__file__).resolve().parents[1] / "utils" / "merge_logs.py"
    if not merge_script.is_file():
        print(f"Merge script not found at {merge_script}", file=sys.stderr)
        return 1

    output_path = Path(args.output)
    cmd = [sys.executable, str(merge_script)]
    for tab_entry in args.tab:
        cmd.append("--tab")
        cmd.extend(tab_entry)
    cmd.extend(["--output", str(output_path)])
    try:
        proc = subprocess.run(cmd)
    except OSError as exc:
        print(f"Failed to run merge script: {exc}", file=sys.stderr)
        return 1
    if proc.returncode != 0:
        print(f"Merge failed (exit code {proc.returncode})", file=sys.stderr)
        return 1
    print(f"Merged: {output_path}")
    return 0


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="embed-log — collect UART/UDP logs and view them in a browser UI.",
        epilog=(
            "Common workflow:\n"
            "  embed-log create-config\n"
            "  embed-log validate --config embed-log.yml\n"
            "  embed-log run --config embed-log.yml\n"
            "  embed-log sessions list\n"
            "\n"
            "Use `embed-log <command> --help` for examples and detailed options."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command", metavar="COMMAND")

    # ── create-config ──
    p = sub.add_parser(
        "create-config",
        help="interactively create a config file",
        description="Interactively create an embed-log YAML config.",
        epilog=(
            "Examples:\n"
            "  embed-log create-config\n"
            "  embed-log create-config --force\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--output", "-o", default="embed-log.yml",
                   help="output config path (default: embed-log.yml)")
    p.add_argument("--force", action="store_true",
                   help="overwrite if file already exists")

    # ── validate ──
    p = sub.add_parser(
        "validate",
        help="validate a config file",
        description="Validate an embed-log YAML config file.",
        epilog=(
            "Examples:\n"
            "  embed-log validate --config embed-log.yml\n"
            "  embed-log validate --json\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--config", "-c", default="embed-log.yml",
                   help="config file path (default: embed-log.yml)")
    p.add_argument("--json", action="store_true",
                   help="machine-readable JSON output")

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
    p.add_argument("--config", "-c", metavar="FILE", default=None,
                   help="YAML config file. CLI flags override config values.")
    p.add_argument("--source", nargs=2, action="append", metavar=("NAME", "TYPE"),
                   dest="sources", default=[],
                   help="NAME uart:/dev/path[@baud] | udp:PORT (repeatable)")
    p.add_argument("--inject", nargs=2, action="append", metavar=("NAME", "PORT"),
                   dest="injects", default=[],
                   help="NAME PORT — TCP inject/stream port (repeatable)")
    p.add_argument("--forward", nargs=2, action="append", metavar=("NAME", "PORT"),
                   dest="forwards", default=[],
                   help="NAME PORT — read-only TCP forward port (repeatable)")
    p.add_argument("--tab", nargs="+", action="append", metavar="ARG",
                   dest="tabs", default=[],
                   help="LABEL SOURCE [SOURCE] — group 1-2 sources into a UI tab")
    p.add_argument("--baudrate", metavar="BAUD", type=int, default=None,
                   help="default UART baud rate")
    p.add_argument("--log-dir", metavar="DIR", default=None, dest="log_dir",
                   help="log files output directory")
    p.add_argument("--host", metavar="HOST", default=None,
                   help="bind address")
    p.add_argument("--ws-port", metavar="PORT", type=int, default=None, dest="ws_port",
                   help="HTTP/WebSocket port (0 = disabled)")
    p.add_argument("--ws-ui", metavar="FILE", default=None, dest="ws_ui",
                   help="custom UI HTML file path")
    p.add_argument("--app-name", metavar="NAME", default=None, dest="app_name",
                   help="name shown in UI top bar")
    p.add_argument("--open-browser", dest="open_browser", action="store_const",
                   const=True, default=None, help="open browser on startup")
    p.add_argument("--no-open-browser", dest="open_browser", action="store_const",
                   const=False, help="do not open browser (overrides config)")
    p.add_argument("--default-light-theme", dest="default_light_theme", default=None,
                   help="light palette key")
    p.add_argument("--default-dark-theme", dest="default_dark_theme", default=None,
                   help="dark palette key")
    p.add_argument("--verbosity", choices=["quiet", "events", "full"], default=None,
                   help="logging verbosity mode")
    p.add_argument("-v", "--verbose", action="store_const", const=True, default=None,
                   help="shortcut for --verbosity events")
    p.add_argument("--verbose-full", action="store_const", const=True, default=None,
                   help="shortcut for --verbosity full")
    p.add_argument("--job-id", metavar="ID", default=None, dest="job_id",
                   help="CI/job identifier for session naming")

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
    p.add_argument("--tab", nargs="+", action="append", metavar="ARG", required=True,
                   help="TAB_LABEL PANE_LABEL FILE [PANE_LABEL FILE] (repeatable)")
    p.add_argument("--output", default="merged.html",
                   help="output HTML file path (default: merged.html)")

    # ── parse ──
    p = sub.add_parser(
        "parse",
        help="parse exported HTML back into raw logs",
        description=(
            "Parse an exported embed-log session.html back into raw"
            " session log files."
        ),
        epilog=(
            "Examples:\n"
            "  embed-log parse session.html\n"
            "  embed-log parse session.html --output my-session\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("html", help="embed-log session.html file")
    p.add_argument("--output", "-o", default=None,
                   help="output session directory")

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
    p.add_argument("--from-start", action="store_true",
                   help="read the existing file contents first instead of starting at EOF")
    p.add_argument("--poll-interval", type=float, default=0.2,
                   help="seconds between file polls (default: 0.2)")
    p.add_argument("--encoding", default="utf-8",
                   help="file encoding (default: utf-8)")

    # ── doctor ──
    p = sub.add_parser(
        "doctor",
        help="diagnose common issues",
        description="Check environment, dependencies, and config for common issues.",
        epilog=(
            "Examples:\n"
            "  embed-log doctor\n"
            "  embed-log doctor --config embed-log.yml\n"
            "  embed-log doctor --json\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--config", "-c", default=None,
                   help="config file to check")
    p.add_argument("--json", action="store_true",
                   help="machine-readable JSON output")

    # ── ports ──
    p = sub.add_parser(
        "ports",
        help="list detected serial ports",
        description="List detected serial ports on the system.",
        epilog=(
            "Examples:\n"
            "  embed-log ports\n"
            "  embed-log ports --json\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--json", action="store_true",
                   help="machine-readable JSON output")

    return parser
def _run_run(args: argparse.Namespace) -> int:
    cfg = {}
    if args.config:
        try:
            cfg = load_config(args.config)
        except ConfigError as exc:
            print(f"config error: {exc}", file=sys.stderr)
            return 1

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
        print("no sources configured. Use embed-log create-config, --source ..., or --config FILE.", file=sys.stderr)
        return 1

    source_names: list[str] = []
    source_objects: dict[str, LogSource] = {}
    for name, spec in source_specs:
        if name in source_objects:
            print(f"duplicate --source name: {name!r}", file=sys.stderr)
            return 1
        try:
            source_objects[name] = parse_source(name, spec, baudrate)
        except ValueError as exc:
            print(str(exc), file=sys.stderr)
            return 1
        source_names.append(name)

    inject_ports: dict[str, int] = {}
    for name, port_value in inject_specs:
        if name not in source_objects:
            print(f"--inject {name!r}: no --source with that name", file=sys.stderr)
            return 1
        try:
            inject_ports[name] = int(port_value)
        except ValueError:
            print(f"--inject {name!r}: port must be an integer, got {port_value!r}", file=sys.stderr)
            return 1

    forward_ports: dict[str, list[int]] = {}
    for name, port_value in forward_specs:
        if name not in source_objects:
            print(f"--forward {name!r}: no --source with that name", file=sys.stderr)
            return 1
        try:
            port = int(port_value)
        except ValueError:
            print(f"--forward {name!r}: port must be an integer, got {port_value!r}", file=sys.stderr)
            return 1
        forward_ports.setdefault(name, []).append(port)

    tabs: list[dict] = []
    for tab_entry in tab_specs:
        if len(tab_entry) < 2:
            print(f"--tab requires at least LABEL SOURCE, got: {tab_entry}", file=sys.stderr)
            return 1
        if len(tab_entry) > 3:
            print(f"--tab takes at most 2 sources per tab, got: {tab_entry}", file=sys.stderr)
            return 1
        label = tab_entry[0]
        panes = tab_entry[1:]
        for pane in panes:
            if pane not in source_objects:
                print(f"--tab {label!r}: unknown source {pane!r}", file=sys.stderr)
                return 1
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


def _run_doctor(args: argparse.Namespace) -> int:
    checks: list[dict] = []
    ok = True

    # Python/runtime
    import sys as _sys
    checks.append(("python", f"{_sys.version_info.major}.{_sys.version_info.minor}.{_sys.version_info.micro}"))

    # Config
    cfg_path = Path(args.config) if args.config else Path("embed-log.yml")
    if cfg_path.is_file():
        try:
            cfg = load_config(str(cfg_path))
            checks.append(("config", str(cfg_path)))
            # Sources
            srcs = cfg.get("sources", [])
            names = [s.get("name") for s in srcs]
            if len(names) != len(set(names)):
                checks.append(("source-names", "DUPLICATE"))
                ok = False
            # Ports
            for s in srcs:
                if s.get("type") == "udp":
                    try:
                        int(s["port"])
                    except (ValueError, KeyError):
                        checks.append(("udp-port", f"INVALID: {s.get('port')}"))
                        ok = False
            checks.append(("sources", f"{len(srcs)} configured"))
            # Tabs
            tabs = cfg.get("tabs", [])
            for t in tabs:
                for p in t.get("panes", []):
                    if p not in names:
                        checks.append(("tab-refs", f"unknown source {p!r} in tab {t.get('label')!r}"))
                        ok = False
            checks.append(("tabs", f"{len(tabs)} configured"))
            # Log dir
            log_dir = Path(cfg.get("logs", {}).get("dir", "logs/"))
            checks.append(("log-dir", str(log_dir) if log_dir.is_dir() else "NOT_FOUND"))
            # Frontend assets
            ui_path = cfg.get("server", {}).get("ws_ui", "")
            if ui_path:
                checks.append(("ui-assets", "present" if Path(ui_path).is_file() else "MISSING"))
        except ConfigError as exc:
            checks.append(("config", f"PARSE_ERROR: {exc}"))
            ok = False
    else:
        checks.append(("config", "NOT_FOUND (optional)"))

    # Serial ports
    ports = _detected_serial_ports()
    checks.append(("serial-ports", f"{len(ports)} detected"))

    if args.json:
        print(json.dumps({"ok": ok, "checks": [{"check": c[0], "status": c[1]} for c in checks]}))
    else:
        print("embed-log doctor")
        print("")
        for name, status in checks:
            icon = "OK" if "NOT_FOUND" not in status and "MISSING" not in status and "INVALID" not in status and "DUPLICATE" not in status and "PARSE_ERROR" not in status else "!!"
            print(f"  [{icon}] {name}: {status}")
        print("")
        print("All checks passed." if ok else "Some checks failed.")

    return 0 if ok else 1


def _run_ports(args: argparse.Namespace) -> int:
    ports = _detected_serial_ports()
    if args.json:
        print(json.dumps(ports))
    else:
        if not ports:
            print("No serial ports detected.")
            return 0
        for p in ports:
            suffix = f"  ({p['label']})" if p["label"] and p["label"] != "n/a" else ""
            print(f"{p['device']}{suffix}")
    return 0


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

    # ── sessions uses its own internal sub-sub-parsers ──
    if argv[0] == "sessions":
        return _run_sessions(argv[1:])

    # ── Help requested ──
    if argv[0] in {"-h", "--help"}:
        parser = _build_parser()
        parser.print_help()
        return 0

    # ── Backward compat: bare flags → run subcommand ──
    if argv[0].startswith("-"):
        # Bare flags → run subcommand
        parser = _build_parser()
        run_argv = ["run"] + argv
        args = parser.parse_args(run_argv)
        return _run_run(args)

    # ── Parse with subparser tree ──
    parser = _build_parser()
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
    if args.command == "doctor":
        return _run_doctor(args)
    if args.command == "ports":
        return _run_ports(args)

    # Should not reach here
    parser.print_help()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
