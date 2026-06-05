"""Run/merge subcommand handlers."""

from __future__ import annotations

import argparse
import os
import logging
import sys
from pathlib import Path

from ..app import DEFAULT_WS_UI, build_source, parse_source, run_app
from .config_resolution import ENV_CONFIG_PATH, resolve_config_path
from ..config import AppConfig, ConfigError, load_config
from ..frontend_plugins import resolve_frontend_plugin
from ..sources import LogSource
import yaml


def _run_merge(args: argparse.Namespace) -> int:
    import subprocess

    merge_script = Path(__file__).resolve().parents[2] / "utils" / "merge_logs.py"
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


def _run_run(args: argparse.Namespace) -> int:
    # Resolve config: explicit --config > EMBED_LOG_CONFIG_YML_PATH > none.
    # The env var is treated as deliberate intent: when set but pointing at
    # a missing or unreadable file, surface a clear error rather than
    # silently falling back to inline defaults.
    config_path = resolve_config_path(args.config)
    if config_path is not None and not config_path.is_file():
        if args.config:
            source = "--config"
            shown = args.config
        else:
            source = ENV_CONFIG_PATH
            shown = os.environ.get(ENV_CONFIG_PATH, "")
        print(
            f"config error: {source} points to {shown!r} but the file is missing or unreadable.",
            file=sys.stderr,
        )
        return 1

    cfg = AppConfig()
    if config_path is not None:
        try:
            cfg = load_config(str(config_path))
        except ConfigError as exc:
            print(f"config error: {exc}", file=sys.stderr)
            return 1

    source_specs = args.sources if args.sources else cfg.sources
    inject_specs = args.injects if args.injects else cfg.injects
    forward_specs = args.forwards if args.forwards else cfg.forwards
    source_labels = cfg.source_labels if (config_path is not None and not args.sources) else {}

    baudrate = args.baudrate if args.baudrate is not None else cfg.baudrate
    logs_root = Path(args.log_dir if args.log_dir is not None else cfg.logs.dir)
    host = args.host if args.host is not None else cfg.server.host
    ws_port = args.ws_port if args.ws_port is not None else cfg.server.ws_port
    ws_ui = args.ws_ui if args.ws_ui is not None else cfg.server.ws_ui or DEFAULT_WS_UI
    app_name = args.app_name if args.app_name is not None else cfg.server.app_name
    cfg_verbosity = cfg.server.verbosity
    cfg_legacy_verbose = cfg.server.verbose
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
    open_browser = args.open_browser if args.open_browser is not None else cfg.server.open_browser
    job_id = args.job_id if args.job_id is not None else cfg.server.job_id
    timestamp_mode = args.timestamp_mode if args.timestamp_mode is not None else cfg.server.timestamp_mode
    default_light_theme = (
        args.default_light_theme if args.default_light_theme is not None else cfg.server.default_light_theme
    )
    default_dark_theme = (
        args.default_dark_theme if args.default_dark_theme is not None else cfg.server.default_dark_theme
    )
    queue_maxsize = cfg.server.queue_size if config_path is not None else 20000

    logging.basicConfig(
        level=logging.INFO if verbosity in {"events", "full"} else logging.WARNING,
        format="%(asctime)s  %(levelname)-8s  %(message)s",
        datefmt="%H:%M:%S",
    )

    if not source_specs:
        print(
            f"no sources configured. Use --config FILE, {ENV_CONFIG_PATH}, embed-log sample-config, or --source ...",
            file=sys.stderr,
        )
        return 1

    source_names: list[str] = []
    source_objects: dict[str, LogSource] = {}
    if args.sources:
        for name, spec in args.sources:
            if name in source_objects:
                print(f"duplicate --source name: {name!r}", file=sys.stderr)
                return 1
            try:
                source_objects[name] = parse_source(name, spec, baudrate)
            except ValueError as exc:
                print(str(exc), file=sys.stderr)
                return 1
            source_names.append(name)
    else:
        for source_config in source_specs:
            name = source_config.name
            if name in source_objects:
                print(f"duplicate source name: {name!r}", file=sys.stderr)
                return 1
            try:
                source_dict = {
                    "name": source_config.name,
                    "type": source_config.type,
                    "port": source_config.port,
                    "parser": {"type": source_config.parser.type},
                }
                if source_config.baudrate is not None:
                    source_dict["baudrate"] = source_config.baudrate
                if source_config.type == "network_capture":
                    source_dict["interface"] = source_config.interface
                    source_dict["bpf_filter"] = source_config.bpf_filter
                    source_dict["pcap_enabled"] = source_config.pcap_enabled
                    source_dict["pcap_path"] = source_config.pcap_path
                    source_dict["include_preview"] = source_config.include_preview
                    source_dict["max_preview_bytes"] = source_config.max_preview_bytes
                    source_dict["network_backend"] = source_config.network_backend
                    source_dict["mock_interval"] = source_config.mock_interval
                source_objects[name] = build_source(source_dict)
            except ValueError as exc:
                print(f"source {name!r}: {exc}", file=sys.stderr)
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
            print(
                f"--inject {name!r}: port must be an integer, got {port_value!r}",
                file=sys.stderr,
            )
            return 1

    forward_ports: dict[str, list[int]] = {}
    for name, port_value in forward_specs:
        if name not in source_objects:
            print(f"--forward {name!r}: no --source with that name", file=sys.stderr)
            return 1
        try:
            port = int(port_value)
        except ValueError:
            print(
                f"--forward {name!r}: port must be an integer, got {port_value!r}",
                file=sys.stderr,
            )
            return 1
        forward_ports.setdefault(name, []).append(port)

    frontend_plugins: dict[str, dict] = {}
    pane_plugins: dict[str, list[dict]] = {}
    plugin_scripts: dict[str, str] = {}

    tabs: list[dict] = []
    if args.tabs:
        tab_specs = args.tabs
        for tab_entry in tab_specs:
            if len(tab_entry) < 2:
                print(
                    f"--tab requires at least LABEL SOURCE, got: {tab_entry}",
                    file=sys.stderr,
                )
                return 1
            if len(tab_entry) > 3:
                print(
                    f"--tab takes at most 2 sources per tab, got: {tab_entry}",
                    file=sys.stderr,
                )
                return 1
            label = tab_entry[0]
            panes = tab_entry[1:]
            pane_labels: dict[str, str] = {}
            for pane in panes:
                if pane not in source_objects:
                    print(f"--tab {label!r}: unknown source {pane!r}", file=sys.stderr)
                    return 1
                pane_labels[pane] = source_labels.get(pane, pane)
            tabs.append({"label": label, "panes": panes, "pane_labels": pane_labels})
    else:
        for plugin_name, definition in cfg.frontend_plugins.items():
            resolved = resolve_frontend_plugin(plugin_name, builtin=definition.builtin, path=definition.path)
            frontend_plugins[plugin_name] = resolved.public_metadata()
            plugin_scripts[plugin_name] = resolved.script

        for tab in cfg.tabs:
            panes: list[str] = []
            pane_labels: dict[str, str] = {}
            for pane in tab.panes:
                source_name = pane.source
                if source_name not in source_objects:
                    print(f"tab {tab.label!r}: unknown source {source_name!r}", file=sys.stderr)
                    return 1
                panes.append(source_name)
                pane_labels[source_name] = source_labels.get(source_name, source_name)
                refs = [
                    {"name": plugin.name, "options": dict(plugin.options)}
                    for plugin in pane.plugins
                ]
                if refs:
                    previous = pane_plugins.get(source_name)
                    if previous is None:
                        pane_plugins[source_name] = refs
                    elif previous != refs:
                        print(
                            f"source {source_name!r} uses conflicting frontend plugins across tabs",
                            file=sys.stderr,
                        )
                        return 1
            tabs.append({"label": tab.label, "panes": panes, "pane_labels": pane_labels})

    # Load per-source TX command suggestions (embed-log.commands.yml)
    pane_commands: dict[str, list[str]] = {}
    commands_candidates = []
    if config_path is not None:
        cf = config_path
        commands_candidates.append(str(cf.parent / f"{cf.stem}.commands.yml"))
    commands_candidates.append("embed-log.commands.yml")
    for cmdfile in commands_candidates:
        try:
            data = yaml.safe_load(Path(cmdfile).read_text())
            srcs = data.get("sources", {}) if isinstance(data, dict) else {}
            for name in source_names:
                cmds = srcs.get(name)
                if isinstance(cmds, list):
                    pane_commands[name] = [str(c) for c in cmds if isinstance(c, str) and c]
            if pane_commands:
                break
        except (OSError, yaml.YAMLError, AttributeError):
            continue

    return run_app(
        source_names=source_names,
        source_objects=source_objects,
        inject_ports=inject_ports,
        source_labels=source_labels,
        forward_ports=forward_ports,
        tabs=tabs,
        frontend_plugins=frontend_plugins,
        pane_plugins=pane_plugins,
        pane_commands=pane_commands,
        plugin_scripts=plugin_scripts,
        logs_root=logs_root,
        host=host,
        verbose=full_verbose,
        ws_port=ws_port,
        ws_ui=ws_ui,
        config_path=str(config_path) if config_path is not None else None,
        job_id=job_id,
        open_browser=open_browser,
        app_name=app_name,
        default_light_theme=default_light_theme,
        default_dark_theme=default_dark_theme,
        timestamp_mode=timestamp_mode,
        queue_maxsize=queue_maxsize,
    )
