from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml
from .models import AppConfig, LogsConfig, ParserConfig, ServerConfig, SourceConfig, TabConfig


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


def _load_parser_config(value: Any, field: str) -> ParserConfig:
    if value is None:
        return ParserConfig()

    parser = _require_dict(value, field)
    parser_type = _require_choice(parser.get("type"), f"{field}.type", {"text", "cbor-datagram"})
    extra_fields = sorted(key for key in parser if key != "type")
    if extra_fields:
        raise ConfigError(f"{field}.{extra_fields[0]} unsupported for parser type {parser_type!r}")
    return ParserConfig(type=parser_type)


def load_config(path: str | Path) -> AppConfig:
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

    sources_raw = _require_list(cfg.get("sources", []), "sources")

    source_names: set[str] = set()
    source_labels: dict[str, str] = {}
    sources: list[SourceConfig] = []
    injects: list[tuple[str, int]] = []
    forwards: list[tuple[str, int]] = []

    for i, item in enumerate(sources_raw):
        src = _require_dict(item, f"sources[{i}]")
        name = _require_str(src.get("name"), f"sources[{i}].name")
        if name in source_names:
            raise ConfigError(f"duplicate source name: {name!r}")
        source_names.add(name)

        src_type = _require_str(src.get("type"), f"sources[{i}].type").lower()
        parser = _load_parser_config(src.get("parser"), f"sources[{i}].parser")
        if parser.type == "cbor-datagram" and src_type != "udp":
            raise ConfigError(
                f"sources[{i}].parser.type 'cbor-datagram' is only valid for UDP sources "
                f"(got source type {src_type!r})"
            )

        if src_type == "uart":
            port: str | int = _require_str(src.get("port"), f"sources[{i}].port")
            baud = src.get("baudrate")
            baudrate = _as_int(baud, f"sources[{i}].baudrate") if baud is not None else None
            source_config = SourceConfig(
                name=name, type=src_type, port=port, parser=parser, baudrate=baudrate
            )
        elif src_type == "udp":
            port = _as_int(src.get("port"), f"sources[{i}].port")
            source_config = SourceConfig(name=name, type=src_type, port=port, parser=parser)
        else:
            raise ConfigError(f"sources[{i}].type unsupported: {src_type!r} (use 'uart' or 'udp')")

        label = src.get("label")
        source_labels[name] = _require_str(label, f"sources[{i}].label") if label is not None else name
        sources.append(source_config)

        inject_port = src.get("inject_port")
        if inject_port is not None:
            injects.append((name, _as_int(inject_port, f"sources[{i}].inject_port")))

        forward_port = src.get("forward_port")
        if forward_port is not None:
            forwards.append((name, _as_int(forward_port, f"sources[{i}].forward_port")))

        forward_ports = src.get("forward_ports")
        if forward_ports is not None:
            fp_list = _require_list(forward_ports, f"sources[{i}].forward_ports")
            for j, fp in enumerate(fp_list):
                forwards.append((name, _as_int(fp, f"sources[{i}].forward_ports[{j}]")))

    tabs_raw = _require_list(cfg.get("tabs", []), "tabs")
    tabs: list[TabConfig] = []

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

        tabs.append(TabConfig(label=label, panes=pane_names))

    server_kwargs: dict[str, Any] = {}
    if "host" in server:
        server_kwargs["host"] = _require_str(server.get("host"), "server.host")
    if "ws_port" in server:
        server_kwargs["ws_port"] = _as_int(server.get("ws_port"), "server.ws_port")
    if "ws_ui" in server:
        server_kwargs["ws_ui"] = _require_str(server.get("ws_ui"), "server.ws_ui")
    if "app_name" in server:
        server_kwargs["app_name"] = _require_str(server.get("app_name"), "server.app_name")
    if "open_browser" in server:
        server_kwargs["open_browser"] = bool(server.get("open_browser"))
    if "verbosity" in server:
        server_kwargs["verbosity"] = _require_choice(
            server.get("verbosity"), "server.verbosity", {"quiet", "events", "full"}
        )
    if "verbose" in server:
        server_kwargs["verbose"] = bool(server.get("verbose"))
    if "job_id" in server:
        server_kwargs["job_id"] = _require_str(server.get("job_id"), "server.job_id")
    if "default_light_theme" in server:
        server_kwargs["default_light_theme"] = _require_str(
            server.get("default_light_theme"), "server.default_light_theme"
        )
    if "default_dark_theme" in server:
        server_kwargs["default_dark_theme"] = _require_str(
            server.get("default_dark_theme"), "server.default_dark_theme"
        )
    if "timestamp_mode" in server:
        server_kwargs["timestamp_mode"] = _require_choice(
            server.get("timestamp_mode"), "server.timestamp_mode", {"absolute", "relative"}
        )
    if "queue_size" in server:
        server_kwargs["queue_size"] = _as_int(server.get("queue_size"), "server.queue_size")

    logs_kwargs: dict[str, Any] = {}
    if "dir" in logs:
        logs_kwargs["dir"] = _require_str(logs.get("dir"), "logs.dir")

    app_config = AppConfig(
        sources=sources,
        source_labels=source_labels,
        injects=injects,
        forwards=forwards,
        tabs=tabs,
        server=ServerConfig(**server_kwargs),
        logs=LogsConfig(**logs_kwargs),
    )

    if "baudrate" in cfg:
        app_config.baudrate = _as_int(cfg.get("baudrate"), "baudrate")

    return app_config
