"""Version and diagnostics subcommands for the embed-log CLI."""

from __future__ import annotations

import argparse
import json
import os
import platform
import sys
from pathlib import Path
from serial.tools import list_ports

from ..config import ConfigError, load_config
from .config_resolution import ENV_CONFIG_PATH, resolve_config_path


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
        __version__, __commit__ = "1.1.5", "unknown"
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

    # Config: explicit --config > EMBED_LOG_CONFIG_YML_PATH > ./embed-log.yml (optional)

    def _inspect_config(path: Path) -> None:
        nonlocal ok
        try:
            cfg = load_config(str(path))
        except ConfigError as exc:
            checks.append(("config", f"PARSE_ERROR: {exc}"))
            ok = False
            return
        checks.append(("config", str(path)))
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
            for pane in t.panes:
                if pane.source not in names:
                    checks.append(
                        (
                            "tab-refs",
                            f"unknown source {pane.source!r} in tab {t.label!r}",
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

    resolved_cfg = resolve_config_path(args.config)
    if resolved_cfg is not None:
        cfg_path = resolved_cfg
        if cfg_path.is_file():
            _inspect_config(cfg_path)
        else:
            checks.append(
                ("config", f"NOT_FOUND: {cfg_path} (from --config or {ENV_CONFIG_PATH})")
            )
            ok = False
    else:
        cfg_path = Path("embed-log.yml")
        if cfg_path.is_file():
            _inspect_config(cfg_path)
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

# ---------------------------------------------------------------------------
# doctor — friendly sectioned environment/config/install/runtime diagnostic
# ---------------------------------------------------------------------------

_BAD_TOKENS = ("MISSING", "NOT_FOUND", "INVALID", "DUPLICATE", "PARSE_ERROR")


def _status_icon(status: str) -> str:
    return "!!" if any(token in status for token in _BAD_TOKENS) else "OK"


def _resolved_source_label(args: argparse.Namespace) -> str:
    """Return a human label for which source supplied the effective config."""
    if getattr(args, "config", None):
        return "--config"
    return ENV_CONFIG_PATH


def _collect_doctor_sections(args: argparse.Namespace) -> list[tuple[str, list[tuple[str, str]]]]:
    """Build the (section, checks) list rendered by ``_run_doctor``."""
    sections: list[tuple[str, list[tuple[str, str]]]] = []

    # ── Environment ──
    env_checks: list[tuple[str, str]] = [
        (
            "python",
            f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}",
        ),
        ("executable", sys.executable),
        ("platform", platform.platform()),
        ("cwd", str(Path.cwd())),
    ]
    env_value = os.environ.get(ENV_CONFIG_PATH, "")
    if env_value.strip():
        env_path = Path(env_value.strip())
        if env_path.is_file():
            env_checks.append(
                (ENV_CONFIG_PATH, f"{env_path}  (set, file present)")
            )
        else:
            env_checks.append(
                (ENV_CONFIG_PATH, f"{env_path}  (set, file MISSING)")
            )
    else:
        env_checks.append((ENV_CONFIG_PATH, "(not set)"))
    sections.append(("Environment", env_checks))

    # ── Config ──
    default_cfg = Path("embed-log.yml")
    config_checks: list[tuple[str, str]] = [
        (
            "default config",
            f"{default_cfg}  (present)"
            if default_cfg.is_file()
            else f"{default_cfg}  (not present)",
        )
    ]
    resolved = resolve_config_path(args.config)
    if resolved is not None:
        config_checks.append(
            (
                "effective config",
                f"{resolved}  (from {_resolved_source_label(args)})",
            )
        )
    else:
        config_checks.append(
            ("effective config", "(none — will use inline flags)")
        )
    sections.append(("Config", config_checks))

    # ── Install ──
    version, commit, source_kind, ref_type, ref, local_path = _load_install_identity()
    install_checks: list[tuple[str, str]] = [
        ("version", version),
        ("source", _display_source_status(source_kind, ref_type, ref, local_path)),
        ("commit", commit),
    ]
    sections.append(("Install", install_checks))

    # ── Runtime ──
    serial_count = len(_detected_serial_ports())
    runtime_checks: list[tuple[str, str]] = [
        ("serial ports", f"{serial_count} detected"),
    ]
    sections.append(("Runtime", runtime_checks))

    return sections


def _run_doctor(args: argparse.Namespace) -> int:
    """Show environment, config, install, and runtime status in one view."""
    sections = _collect_doctor_sections(args)
    ok = True
    for _, checks in sections:
        for _, status in checks:
            if any(token in status for token in _BAD_TOKENS):
                ok = False
                break

    if args.json:
        print(
            json.dumps(
                {
                    "ok": ok,
                    "sections": [
                        {
                            "name": name,
                            "checks": [
                                {"check": c, "status": s} for c, s in checks
                            ],
                        }
                        for name, checks in sections
                    ],
                },
                indent=2,
            )
        )
    else:
        print("embed-log doctor")
        print()
        for name, checks in sections:
            print(f"{name}:")
            for check, status in checks:
                icon = _status_icon(status)
                print(f"  [{icon}] {check}: {status}")
            print()
        print("All checks passed." if ok else "Some checks failed.")

    return 0 if ok else 1


def _run_ports(args: argparse.Namespace) -> int:

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
