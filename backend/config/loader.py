from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import yaml

from ..frontend_plugins import builtin_frontend_plugin_names
from .models import (
    AppConfig,
    FrontendPluginDefinition,
    LogsConfig,
    PaneConfig,
    PanePluginConfig,
    ParserConfig,
    ServerConfig,
    SourceConfig,
    TabConfig,
)


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


def _options_signature(options: dict[str, Any]) -> str:
    return json.dumps(options, sort_keys=True, separators=(",", ":"))


def _load_parser_config(value: Any, field: str) -> ParserConfig:
    if value is None:
        return ParserConfig()

    parser = _require_dict(value, field)
    parser_type = _require_choice(parser.get("type"), f"{field}.type", {"text", "cbor-datagram"})
    extra_fields = sorted(key for key in parser if key != "type")
    if extra_fields:
        raise ConfigError(f"{field}.{extra_fields[0]} unsupported for parser type {parser_type!r}")
    return ParserConfig(type=parser_type)


def _load_frontend_plugins(value: Any, *, config_dir: Path) -> dict[str, FrontendPluginDefinition]:
    if value is None:
        return {}

    root = _require_dict(value, "frontend_plugins")
    resolved: dict[str, FrontendPluginDefinition] = {}
    builtins = builtin_frontend_plugin_names()

    for name, raw_plugin in root.items():
        plugin_name = _require_str(name, f"frontend_plugins key {name!r}")
        plugin = _require_dict(raw_plugin, f"frontend_plugins.{plugin_name}")
        builtin = plugin.get("builtin")
        raw_path = plugin.get("path")
        extra_fields = sorted(key for key in plugin if key not in {"builtin", "path"})
        if extra_fields:
            raise ConfigError(f"frontend_plugins.{plugin_name}.{extra_fields[0]} is unsupported")
        if bool(builtin) == bool(raw_path):
            raise ConfigError(f"frontend_plugins.{plugin_name} must define exactly one of builtin or path")

        if builtin:
            builtin_name = _require_choice(builtin, f"frontend_plugins.{plugin_name}.builtin", builtins)
            resolved[plugin_name] = FrontendPluginDefinition(builtin=builtin_name)
            continue

        plugin_path = config_dir / _require_str(raw_path, f"frontend_plugins.{plugin_name}.path")
        plugin_path = plugin_path.resolve()
        if not plugin_path.is_file():
            raise ConfigError(f"frontend_plugins.{plugin_name}.path file not found: {plugin_path}")
        resolved[plugin_name] = FrontendPluginDefinition(path=str(plugin_path))

    return resolved


def _load_pane_plugin(value: Any, field: str, frontend_plugins: dict[str, FrontendPluginDefinition]) -> PanePluginConfig:
    if isinstance(value, str):
        name = _require_str(value, field)
        if name not in frontend_plugins:
            raise ConfigError(f"{field} unknown plugin: {name!r}")
        return PanePluginConfig(name=name)

    plugin = _require_dict(value, field)
    name = _require_str(plugin.get("name"), f"{field}.name")
    if name not in frontend_plugins:
        raise ConfigError(f"{field}.name unknown plugin: {name!r}")

    options = plugin.get("options", {})
    if options is None:
        options = {}
    options = _require_dict(options, f"{field}.options")

    extra_fields = sorted(key for key in plugin if key not in {"name", "options"})
    if extra_fields:
        raise ConfigError(f"{field}.{extra_fields[0]} is unsupported")

    return PanePluginConfig(name=name, options=options)


def _load_pane_config(
    value: Any,
    field: str,
    *,
    source_names: set[str],
    frontend_plugins: dict[str, FrontendPluginDefinition],
) -> PaneConfig:
    if isinstance(value, str):
        source = _require_str(value, field)
        if source not in source_names:
            raise ConfigError(f"{field} unknown source: {source!r}")
        return PaneConfig(source=source)

    pane = _require_dict(value, field)
    source = _require_str(pane.get("source"), f"{field}.source")
    if source not in source_names:
        raise ConfigError(f"{field}.source unknown source: {source!r}")

    raw_plugins = pane.get("plugins", [])
    if raw_plugins is None:
        raw_plugins = []
    plugins = [_load_pane_plugin(item, f"{field}.plugins[{idx}]", frontend_plugins) for idx, item in enumerate(_require_list(raw_plugins, f"{field}.plugins"))]

    extra_fields = sorted(key for key in pane if key not in {"source", "plugins"})
    if extra_fields:
        raise ConfigError(f"{field}.{extra_fields[0]} is unsupported")

    return PaneConfig(source=source, plugins=plugins)


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

    frontend_plugins = _load_frontend_plugins(cfg.get("frontend_plugins"), config_dir=p.parent.resolve())

    tabs_raw = _require_list(cfg.get("tabs", []), "tabs")
    tabs: list[TabConfig] = []
    pane_signatures: dict[str, tuple[tuple[str, str], ...]] = {}

    for i, item in enumerate(tabs_raw):
        tab = _require_dict(item, f"tabs[{i}]")
        label = _require_str(tab.get("label"), f"tabs[{i}].label")
        panes = _require_list(tab.get("panes"), f"tabs[{i}].panes")
        if not (1 <= len(panes) <= 2):
            raise ConfigError(f"tabs[{i}].panes must contain 1 or 2 pane definitions")

        pane_configs: list[PaneConfig] = []
        for j, pane in enumerate(panes):
            pane_config = _load_pane_config(
                pane,
                f"tabs[{i}].panes[{j}]",
                source_names=source_names,
                frontend_plugins=frontend_plugins,
            )
            signature = tuple((plugin.name, _options_signature(plugin.options)) for plugin in pane_config.plugins)
            previous = pane_signatures.get(pane_config.source)
            if previous is None:
                pane_signatures[pane_config.source] = signature
            elif previous != signature:
                raise ConfigError(
                    f"tabs[{i}].panes[{j}] plugin set conflicts with another tab using source {pane_config.source!r}"
                )
            pane_configs.append(pane_config)

        extra_fields = sorted(key for key in tab if key not in {"label", "panes"})
        if extra_fields:
            raise ConfigError(f"tabs[{i}].{extra_fields[0]} is unsupported")

        tabs.append(TabConfig(label=label, panes=pane_configs))

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
        frontend_plugins=frontend_plugins,
        tabs=tabs,
        server=ServerConfig(**server_kwargs),
        logs=LogsConfig(**logs_kwargs),
    )

    if "baudrate" in cfg:
        app_config.baudrate = _as_int(cfg.get("baudrate"), "baudrate")

    return app_config
