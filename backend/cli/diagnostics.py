"""Version and diagnostics subcommands for the embed-log CLI."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from ..config import ConfigError, load_config
from .wizard import _detected_serial_ports


def _display_source_label(source_kind: str, ref_type: str, ref: str, local_path: str) -> str:
    if source_kind == "local":
        return f"local:{local_path or '?'}"
    if source_kind == "unknown":
        return "unknown"
    return f"{ref_type}:{ref}"


def _display_source_status(source_kind: str, ref_type: str, ref: str, local_path: str) -> str:
    if source_kind == "local":
        return f"local {local_path or '?'}"
    if source_kind == "unknown":
        return "unknown"
    return f"{ref_type} {ref}"


def _load_install_identity() -> tuple[str, str, str, str, str, str]:
    try:
        from .._version import __version__, __commit__
    except ImportError:
        __version__, __commit__ = "1.0.1", "unknown"
    try:
        from .._install_source import (
            __local_path__ as local_path,
            __ref__ as ref,
            __ref_type__ as ref_type,
            __source_kind__ as source_kind,
        )
    except ImportError:
        source_kind, ref_type, ref, local_path = "unknown", "branch", "main", ""
    return __version__, __commit__, source_kind, ref_type, ref, local_path


def _display_version_line() -> str:
    version, commit, source_kind, ref_type, ref, local_path = _load_install_identity()
    source_label = _display_source_label(source_kind, ref_type, ref, local_path)
    return f"embed-log {version} ({source_label}, {commit})"


def _run_version(args: argparse.Namespace) -> int:

    checks: list[dict] = []
    ok = True

    version, commit, source_kind, ref_type, ref, local_path = _load_install_identity()
    checks.append(("version", version))
    checks.append(("source", _display_source_status(source_kind, ref_type, ref, local_path)))
    checks.append(("commit", commit))

    # Python/runtime
    import sys as _sys

    checks.append(
        (
            "python",
            f"{_sys.version_info.major}.{_sys.version_info.minor}.{_sys.version_info.micro}",
        )
    )

    # Config
    cfg_path = Path(args.config) if args.config else Path("embed-log.yml")
    if cfg_path.is_file():
        try:
            cfg = load_config(str(cfg_path))
            checks.append(("config", str(cfg_path)))
            # Sources
            # Sources
            srcs = cfg.sources
            names = [s.name for s in srcs]
            if len(names) != len(set(names)):
                checks.append(("source-names", "DUPLICATE"))
                ok = False
            # Ports
            for s in srcs:
                if s.type == "udp":
                    try:
                        int(s.port)
                    except (ValueError, TypeError):
                        checks.append(("udp-port", f"INVALID: {s.port}"))
                        ok = False
            checks.append(("sources", f"{len(srcs)} configured"))
            # Tabs
            tabs = cfg.tabs
            for t in tabs:
                for p in t.panes:
                    if p not in names:
                        checks.append(
                            (
                                "tab-refs",
                                f"unknown source {p!r} in tab {t.label!r}",
                            )
                        )
                        ok = False
            checks.append(("tabs", f"{len(tabs)} configured"))
            # Log dir
            log_dir = Path(cfg.logs.dir)
            checks.append(
                ("log-dir", str(log_dir) if log_dir.is_dir() else "NOT_FOUND")
            )
            # Frontend assets
            ui_path = cfg.server.ws_ui or ""
            if ui_path:
                checks.append(
                    ("ui-assets", "present" if Path(ui_path).is_file() else "MISSING")
                )
        except ConfigError as exc:
            checks.append(("config", f"PARSE_ERROR: {exc}"))
            ok = False
    else:
        checks.append(("config", "NOT_FOUND (optional)"))

    # Serial ports
    ports = _detected_serial_ports()
    checks.append(("serial-ports", f"{len(ports)} detected"))

    if args.json:
        print(
            json.dumps(
                {"ok": ok, "checks": [{"check": c[0], "status": c[1]} for c in checks]}
            )
        )
    else:
        print("embed-log version")

        print("")
        for name, status in checks:
            icon = (
                "OK"
                if "NOT_FOUND" not in status
                and "MISSING" not in status
                and "INVALID" not in status
                and "DUPLICATE" not in status
                and "PARSE_ERROR" not in status
                else "!!"
            )
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
