"""Run/validate/merge subcommand handlers."""

from __future__ import annotations

import argparse
import json
import logging
import subprocess
import sys
from pathlib import Path

from ..app import DEFAULT_WS_UI, build_source, parse_source, run_app
from ..config import ConfigError, load_config
from ..sources import LogSource


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
        print(
            json.dumps(
                {
                    "ok": True,
                    "config": args.config,
                    "sources": len(cfg.get("sources", [])),
                    "injects": len(cfg.get("injects", [])),
                    "forwards": len(cfg.get("forwards", [])),
                    "tabs": len(cfg.get("tabs", [])),
                }
            )
        )
        return 0

    print("Config OK")
    print(f"  sources: {len(cfg.get('sources', []))}")
    print(f"  injects: {len(cfg.get('injects', []))}")
    print(f"  forwards: {len(cfg.get('forwards', []))}")
    print(f"  tabs: {len(cfg.get('tabs', []))}")
    return 0


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
    source_labels = (
        cfg.get("source_labels", {}) if (args.config and not args.sources) else {}
    )

    baudrate = (
        args.baudrate if args.baudrate is not None else cfg.get("baudrate", 115200)
    )
    logs_root = Path(
        args.log_dir if args.log_dir is not None else cfg.get("log_dir", "logs/")
    )
    host = args.host if args.host is not None else cfg.get("host", "127.0.0.1")
    ws_port = args.ws_port if args.ws_port is not None else cfg.get("ws_port", 8080)
    ws_ui = args.ws_ui if args.ws_ui is not None else cfg.get("ws_ui", DEFAULT_WS_UI)
    app_name = (
        args.app_name if args.app_name is not None else cfg.get("app_name", "embed-log")
    )
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
    open_browser = (
        args.open_browser
        if args.open_browser is not None
        else cfg.get("open_browser", False)
    )
    job_id = args.job_id if args.job_id is not None else cfg.get("job_id", None)
    timestamp_mode = (
        args.timestamp_mode
        if args.timestamp_mode is not None
        else cfg.get("timestamp_mode", "absolute")
    )
    default_light_theme = (
        args.default_light_theme
        if args.default_light_theme is not None
        else cfg.get("default_light_theme")
    )
    default_dark_theme = (
        args.default_dark_theme
        if args.default_dark_theme is not None
        else cfg.get("default_dark_theme")
    )
    queue_maxsize = cfg.get("queue_size", 20000) if args.config else 20000

    logging.basicConfig(
        level=logging.INFO if verbosity in {"events", "full"} else logging.WARNING,
        format="%(asctime)s  %(levelname)-8s  %(message)s",
        datefmt="%H:%M:%S",
    )

    if not source_specs:
        print(
            "no sources configured. Use embed-log create-config, --source ..., or --config FILE.",
            file=sys.stderr,
        )
        return 1

    source_names: list[str] = []
    source_objects: dict[str, LogSource] = {}
    if args.sources:
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
    else:
        for source_config in source_specs:
            name = source_config["name"]
            if name in source_objects:
                print(f"duplicate source name: {name!r}", file=sys.stderr)
                return 1
            try:
                source_objects[name] = build_source(source_config)
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

    tabs: list[dict] = []
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

    return run_app(
        source_names=source_names,
        source_objects=source_objects,
        inject_ports=inject_ports,
        source_labels=source_labels,
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
        timestamp_mode=timestamp_mode,
        queue_maxsize=queue_maxsize,
    )
