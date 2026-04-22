from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml


class ConfigError(ValueError):
    pass


def _as_int(value: Any, field: str) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        raise ConfigError(f"{field} must be an integer")


def _require_dict(value: Any, field: str) -> dict:
    if not isinstance(value, dict):
        raise ConfigError(f"{field} must be a mapping/object")
    return value


def _require_list(value: Any, field: str) -> list:
    if not isinstance(value, list):
        raise ConfigError(f"{field} must be a list")
    return value


def _require_str(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ConfigError(f"{field} must be a non-empty string")
    return value.strip()


def _require_choice(value: Any, field: str, choices: set[str]) -> str:
    s = _require_str(value, field).lower()
    if s not in choices:
        raise ConfigError(f"{field} must be one of: {', '.join(sorted(choices))}")
    return s


def load_config(path: str | Path) -> dict:
    p = Path(path)
    if not p.is_file():
        raise ConfigError(f"config file not found: {p}")

    try:
        raw = yaml.safe_load(p.read_text(encoding="utf-8"))
    except yaml.YAMLError as exc:
        raise ConfigError(f"invalid YAML: {exc}") from exc

    if raw is None:
        raw = {}
    cfg = _require_dict(raw, "root")

    version = cfg.get("version", 1)
    if version != 1:
        raise ConfigError(f"unsupported config version: {version!r} (expected 1)")

    server = cfg.get("server", {})
    if server is None:
        server = {}
    server = _require_dict(server, "server")

    logs = cfg.get("logs", {})
    if logs is None:
        logs = {}
    logs = _require_dict(logs, "logs")

    # sources
    sources_raw = cfg.get("sources", [])
    sources_raw = _require_list(sources_raw, "sources")

    source_names: set[str] = set()
    sources: list[tuple[str, str]] = []
    injects: list[tuple[str, int]] = []
    forwards: list[tuple[str, int]] = []

    for i, item in enumerate(sources_raw):
        src = _require_dict(item, f"sources[{i}]")
        name = _require_str(src.get("name"), f"sources[{i}].name")
        if name in source_names:
            raise ConfigError(f"duplicate source name: {name!r}")
        source_names.add(name)

        src_type = _require_str(src.get("type"), f"sources[{i}].type").lower()
        if src_type == "uart":
            port = _require_str(src.get("port"), f"sources[{i}].port")
            baud = src.get("baudrate")
            spec = f"uart:{port}@{_as_int(baud, f'sources[{i}].baudrate')}" if baud is not None else f"uart:{port}"
        elif src_type == "udp":
            port = _as_int(src.get("port"), f"sources[{i}].port")
            spec = f"udp:{port}"
        else:
            raise ConfigError(f"sources[{i}].type unsupported: {src_type!r} (use 'uart' or 'udp')")

        sources.append((name, spec))

        inject_port = src.get("inject_port")
        if inject_port is not None:
            injects.append((name, _as_int(inject_port, f"sources[{i}].inject_port")))

        # Optional forwarding ports (read-only stream fanout)
        forward_port = src.get("forward_port")
        if forward_port is not None:
            forwards.append((name, _as_int(forward_port, f"sources[{i}].forward_port")))

        forward_ports = src.get("forward_ports")
        if forward_ports is not None:
            fp_list = _require_list(forward_ports, f"sources[{i}].forward_ports")
            for j, fp in enumerate(fp_list):
                forwards.append((name, _as_int(fp, f"sources[{i}].forward_ports[{j}]")))

    # tabs
    tabs_raw = cfg.get("tabs", [])
    tabs_raw = _require_list(tabs_raw, "tabs")
    tabs: list[list[str]] = []

    for i, item in enumerate(tabs_raw):
        tab = _require_dict(item, f"tabs[{i}]")
        label = _require_str(tab.get("label"), f"tabs[{i}].label")
        panes = _require_list(tab.get("panes"), f"tabs[{i}].panes")
        if not (1 <= len(panes) <= 2):
            raise ConfigError(f"tabs[{i}].panes must contain 1 or 2 source names")

        pane_names: list[str] = []
        for j, pane in enumerate(panes):
            pane_name = _require_str(pane, f"tabs[{i}].panes[{j}]")
            if pane_name not in source_names:
                raise ConfigError(f"tabs[{i}].panes[{j}] unknown source: {pane_name!r}")
            pane_names.append(pane_name)

        tabs.append([label, *pane_names])

    out = {
        "sources": sources,
        "injects": injects,
        "forwards": forwards,
        "tabs": tabs,
    }

    if "host" in server:
        out["host"] = _require_str(server.get("host"), "server.host")
    if "ws_port" in server:
        out["ws_port"] = _as_int(server.get("ws_port"), "server.ws_port")
    if "ws_ui" in server:
        out["ws_ui"] = _require_str(server.get("ws_ui"), "server.ws_ui")
    if "app_name" in server:
        out["app_name"] = _require_str(server.get("app_name"), "server.app_name")
    if "open_browser" in server:
        out["open_browser"] = bool(server.get("open_browser"))
    if "verbosity" in server:
        out["verbosity"] = _require_choice(server.get("verbosity"), "server.verbosity", {"quiet", "events", "full"})
    if "verbose" in server:
        out["verbose"] = bool(server.get("verbose"))
    if "job_id" in server:
        out["job_id"] = _require_str(server.get("job_id"), "server.job_id")

    if "dir" in logs:
        out["log_dir"] = _require_str(logs.get("dir"), "logs.dir")

    if "baudrate" in cfg:
        out["baudrate"] = _as_int(cfg.get("baudrate"), "baudrate")

    return out
