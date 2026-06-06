"""Onboarding-oriented CLI commands."""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

from ..config import ConfigError, load_config
from .config_resolution import ENV_CONFIG_PATH, resolve_active_config_path
from .diagnostics import _load_install_identity
from .sample_config import _describe, _list_samples


_RECOMMENDED_SAMPLE_NAMES = (
    "single_uart_single_tab.yml",
    "double_uart_single_tab.yml",
    "double_uart_udp_two_tabs.yml",
    "double_uart_network_two_tabs.yml",
    "double_uart_udp_coap_two_tabs.yml",
    "single_file_single_tab.yml",
    "double_uart_file_two_tabs.yml",
)

_DEFAULT_INIT_SAMPLE = "double_uart_udp_two_tabs.yml"

_SAMPLE_ALIASES = {
    "uart": "single_uart_single_tab.yml",
    "two-uart": "double_uart_single_tab.yml",
    "two_uart": "double_uart_single_tab.yml",
    "uart-pytest": "double_uart_udp_two_tabs.yml",
    "uart_pytest": "double_uart_udp_two_tabs.yml",
    "udp": "double_uart_udp_two_tabs.yml",
    "file-tail": "single_file_single_tab.yml",
    "file_tail": "single_file_single_tab.yml",
    "network-capture": "single_network_single_tab.yml",
    "network_capture": "single_network_single_tab.yml",
    "annotated_full_config": "reference_full_annotated.yml",
    "double_uart_single_tab_single_udp": "double_uart_udp_two_tabs.yml",
    "double_uart_single_tab_network_tab": "double_uart_network_two_tabs.yml",
    "double_uart_single_tab_coap_plugin": "double_uart_udp_coap_two_tabs.yml",
    "double_uart_single_tab_file_tab": "double_uart_file_two_tabs.yml",
    "file_tail_single_tab": "single_file_single_tab.yml",
    "multi_tab_multi_baud": "double_uart_udp_multi_baud_two_tabs.yml",
    "single_tab_dual_pane": "double_uart_minimal_single_tab.yml",
    "three_tab_uart_file_udp_coap": "double_uart_file_udp_coap_three_tabs.yml",
    "udp_cbor_datagram": "three_udp_cbor_two_tabs.yml",
}


def _config_summary(path: Path) -> dict[str, object] | None:
    try:
        cfg = load_config(path)
    except ConfigError:
        return None
    return {
        "sources": [
            {"name": source.name, "type": source.type, "port": source.port}
            for source in cfg.sources
        ],
        "tabs": [
            {
                "label": tab.label,
                "panes": [pane.source for pane in tab.panes],
            }
            for tab in cfg.tabs
        ],
        "log_dir": cfg.logs.dir,
        "ui": f"http://{cfg.server.host}:{cfg.server.ws_port}/"
        if cfg.server.ws_port
        else None,
    }


def _recommended_sample_payload() -> list[dict[str, str]]:
    samples = {sample.name: sample for sample in _list_samples()}
    payload: list[dict[str, str]] = []
    for name in _RECOMMENDED_SAMPLE_NAMES:
        sample = samples.get(name)
        if sample is None:
            continue
        payload.append(
            {
                "name": name,
                "sample": name.removesuffix(".yml"),
                "description": _describe(sample),
                "command": f"embed-log init --sample {name.removesuffix('.yml')}",
            }
        )
    return payload


def _available_sample_payload() -> list[dict[str, str]]:
    payload: list[dict[str, str]] = []
    for sample in _list_samples():
        stem = sample.name.removesuffix(".yml")
        payload.append(
            {
                "name": sample.name,
                "sample": stem,
                "title": sample.name,
                "description": _describe(sample),
                "command": f"embed-log init --sample {stem}",
            }
        )
    return payload
def _sample_name_for_cli(filename: str) -> str:
    return filename.removesuffix(".yml")


def _resolve_sample_path(sample_arg: str | None) -> Path | None:
    requested = (sample_arg or _DEFAULT_INIT_SAMPLE).strip()
    if not requested:
        requested = _DEFAULT_INIT_SAMPLE
    requested = _SAMPLE_ALIASES.get(requested.removesuffix(".yml"), requested)
    if not requested.endswith(".yml"):
        requested = f"{requested}.yml"
    for sample in _list_samples():
        if sample.name == requested:
            return sample
    return None


def _print_sample_list() -> None:
    print("Available config samples:")
    for sample in _list_samples():
        name = _sample_name_for_cli(sample.name)
        print(f"  {name:<40} {_describe(sample)}")




def _docs_payload() -> list[dict[str, str]]:
    return [
        {"name": "README", "path": "README.md"},
        {"name": "Backend", "path": "docs/BACKEND.md"},
        {"name": "Frontend", "path": "docs/FRONTEND.md"},
        {"name": "Testing", "path": "docs/TESTING.md"},
    ]


def _commands_payload() -> list[dict[str, str]]:
    return [
        {"command": "embed-log ports", "purpose": "list detected serial ports"},
        {"command": "embed-log doctor", "purpose": "inspect config, install, and runtime status"},
        {"command": "embed-log init", "purpose": "export sample configs; --add-uart-shell also writes UART TX suggestions"},
        {"command": "embed-log onboard", "purpose": "show orientation, recommended samples, and next steps"},
        {"command": "embed-log run", "purpose": "start the browser UI and collectors"},
        {"command": "embed-log sessions list", "purpose": "list saved sessions"},
    ]


def _active_config_payload(cli_path: str | None = None) -> dict[str, object]:
    resolution = resolve_active_config_path(cli_path)
    if resolution.path is None:
        return {"path": None, "source": None, "exists": False, "summary": None}
    exists = resolution.path.is_file()
    return {
        "path": str(resolution.path),
        "source": resolution.source,
        "exists": exists,
        "summary": _config_summary(resolution.path) if exists else None,
    }


def _onboard_payload(args: argparse.Namespace) -> dict[str, object]:
    version, _commit, source_kind, ref_type, ref, local_path = _load_install_identity()
    active_config = _active_config_payload(args.config)
    next_steps = [
        "embed-log init --output embed-log.yml",
        "embed-log doctor --config embed-log.yml",
        "embed-log run --config embed-log.yml",
    ]
    if active_config["path"] and active_config["exists"]:
        next_steps = [
            f"embed-log doctor --config {active_config['path']}",
            f"embed-log run --config {active_config['path']}",
            "embed-log sessions list",
        ]
    return {
        "version": version,
        "install_source": {
            "kind": source_kind,
            "ref_type": ref_type,
            "ref": ref,
            "local_path": local_path,
        },
        "active_config": active_config,
        "available_samples": _available_sample_payload(),
        "recommended_samples": _recommended_sample_payload(),
        "commands": _commands_payload(),
        "docs": _docs_payload(),
        "next_steps": next_steps,
    }


def _print_onboard_human(payload: dict[str, object]) -> None:
    active = payload["active_config"]
    assert isinstance(active, dict)
    print("embed-log onboarding")
    print()
    if active["path"]:
        state = "present" if active["exists"] else "missing"
        print(f"Active config: {active['path']}  (from {active['source']}, {state})")
        summary = active.get("summary")
        if isinstance(summary, dict):
            print(f"  logs: {summary['log_dir']}")
            if summary.get("ui"):
                print(f"  UI: {summary['ui']}")
    else:
        print("Active config: none")
    print()
    print("Recommended config samples:")
    recommended_samples = payload["recommended_samples"]
    assert isinstance(recommended_samples, list)
    for sample in recommended_samples:
        assert isinstance(sample, dict)
        print(
            f"  {sample['sample']:<36} {sample['description']}  "
            f"(embed-log init --sample {sample['sample']})"
        )
    print()
    print("Generate a config:")
    print("  embed-log init --list")
    print("  embed-log init --output embed-log.yml")
    print("  embed-log init --add-uart-shell  # also writes UART TX autocomplete suggestions")
    print("  embed-log init --config embed-log.yml --add-uart-shell  # add suggestions to an existing config")
    print("  embed-log init --sample single_network_single_tab --output embed-log.yml")
    print("  embed-log init --sample single_file_single_tab --output embed-log.yml")
    print()
    print("Start the UI:")
    print("  embed-log run --config embed-log.yml")
    print(f"  export {ENV_CONFIG_PATH}=\"$PWD/embed-log.yml\" && embed-log run")
    print()
    print("Sessions/logs:")
    print("  saved under the config logs.dir value (default: logs/)")
    print("  embed-log sessions list")
    print()
    print("Important commands:")
    commands = payload["commands"]
    assert isinstance(commands, list)
    for item in commands:
        assert isinstance(item, dict)
        print(f"  {item['command']:<28} {item['purpose']}")


def _run_onboard(args: argparse.Namespace) -> int:
    payload = _onboard_payload(args)
    if args.samples:
        _print_sample_list()
    elif args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        _print_onboard_human(payload)
    return 0


def _shell_config_path(output: Path) -> str:
    return str(output) if output.is_absolute() else f"$PWD/{output}"


def _uart_shell_config_path(output: Path) -> Path:
    return output.with_name(f"{output.stem}.commands.yml")


def _uart_shell_config_text(config_path: Path, origin: str, source_names: list[str]) -> str:
    lines = [
        "# UART TX command suggestions for embed-log.",
        f"# Generated for {config_path.name} from {origin}.",
        "# embed-log run loads this file automatically when it is next to the config.",
        "# In the browser UI, focus a UART TX input and press Tab to cycle matching commands.",
        "# Replace these starter commands with the shell commands supported by your firmware.",
        "",
        "sources:",
    ]
    starter_commands = ("help\r\n", "version\r\n", "status\r\n", "reboot\r\n")
    for source_name in source_names:
        lines.append(f"  {json.dumps(source_name)}:")
        lines.append("    # Edit these for this UART device.")
        for command in starter_commands:
            lines.append(f"    - {json.dumps(command)}")
    return "\n".join(lines) + "\n"


def _uart_shell_source_names(config_path: Path) -> list[str] | None:
    try:
        cfg = load_config(config_path)
    except ConfigError as exc:
        print(f"Cannot generate UART shell commands from {config_path}: {exc}", file=sys.stderr)
        return None
    return [source.name for source in cfg.sources if source.type == "uart"]


def _write_uart_shell_config(
    *,
    config_path: Path,
    origin: str,
    source_names: list[str],
    force: bool,
) -> Path | None:
    if not source_names:
        print("No UART sources in selected config; no UART shell command file created.")
        return None

    output = _uart_shell_config_path(config_path)
    if output.exists() and not force:
        print(f"File exists: {output}. Use --force to overwrite.", file=sys.stderr)
        return None

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        _uart_shell_config_text(config_path, origin, source_names),
        encoding="utf-8",
    )
    print(f"Created: {output}  (UART TX command suggestions)")
    return output


def _run_init_existing_config(args: argparse.Namespace) -> int:
    if not args.add_uart_shell:
        print("--config is only used with --add-uart-shell.", file=sys.stderr)
        return 1
    if args.sample:
        print("--config cannot be combined with --sample.", file=sys.stderr)
        return 1
    if args.output:
        print("--config writes <config-stem>.commands.yml; do not pass --output.", file=sys.stderr)
        return 1

    config_path = Path(args.config)
    if not config_path.is_file():
        print(f"Config not found: {config_path}", file=sys.stderr)
        return 1

    source_names = _uart_shell_source_names(config_path)
    if source_names is None:
        return 1

    shell_output = _write_uart_shell_config(
        config_path=config_path,
        origin="existing config",
        source_names=source_names,
        force=args.force,
    )
    if shell_output is None and source_names:
        return 1

    print()
    print("Next commands:")
    print(f"  embed-log doctor --config {config_path}")
    print(f"  embed-log run --config {config_path}")
    if shell_output is not None:
        print(f"  # UART TX suggestions are loaded automatically from {shell_output}")
    return 0




def _run_init(args: argparse.Namespace) -> int:
    if args.list_only:
        _print_sample_list()
        return 0

    if args.config:
        return _run_init_existing_config(args)

    chosen = _resolve_sample_path(args.sample)
    if chosen is None:
        print(f"Unknown config sample: {args.sample}", file=sys.stderr)
        print("Run `embed-log init --list` to see available samples.", file=sys.stderr)
        return 1

    output = Path(args.output or "embed-log.yml")
    uart_shell_output: Path | None = None
    uart_shell_source_names: list[str] = []
    if args.add_uart_shell:
        names = _uart_shell_source_names(chosen)
        if names is None:
            return 1
        uart_shell_source_names = names
        if uart_shell_source_names:
            uart_shell_output = _uart_shell_config_path(output)

    if output.exists() and not args.force:
        if args.add_uart_shell:
            print(
                f"Config already exists: {output}.\n"
                f"To generate only UART shell commands from it, run:\n"
                f"  embed-log init --config {output} --add-uart-shell",
                file=sys.stderr,
            )
        else:
            print(f"File exists: {output}. Use --force to overwrite.", file=sys.stderr)
        return 1
    if uart_shell_output is not None and uart_shell_output.exists() and not args.force:
        print(f"File exists: {uart_shell_output}. Use --force to overwrite.", file=sys.stderr)
        return 1

    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(chosen, output)
    print(f"Created: {output}  (sample: {_sample_name_for_cli(chosen.name)})")
    if args.add_uart_shell:
        uart_shell_output = _write_uart_shell_config(
            config_path=output,
            origin=f"sample {_sample_name_for_cli(chosen.name)}",
            source_names=uart_shell_source_names,
            force=True,
        )
    print()
    print("Next commands:")
    print(f"  embed-log doctor --config {output}")
    print(f"  embed-log run --config {output}")
    if uart_shell_output is not None:
        print(f"  # UART TX suggestions are loaded automatically from {uart_shell_output}")
    print()
    print("Or set the config once for this shell:")
    print(f"  export {ENV_CONFIG_PATH}=\"{_shell_config_path(output)}\"")
    print("  embed-log run")
    return 0


def add_subparsers(subparsers) -> None:
    onboard = subparsers.add_parser(
        "onboard",
        help="print a practical repo and CLI orientation",
        description="Show active config, examples, starter generation, run commands, and docs.",
        epilog=(
            "Examples:\n"
            "  embed-log onboard\n"
            "  embed-log onboard --json\n"
            "  embed-log onboard --config embed-log.yml\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    onboard.add_argument("--config", "-c", default=None, help="config file to inspect")
    onboard.add_argument("--json", action="store_true", help="machine-readable JSON output")
    onboard.add_argument("--samples", action="store_true", help="list config sample names and exit")

    init = subparsers.add_parser(
        "init",
        help="generate a config from a sample",
        description=(
            "Generate a config file from the canonical config-samples directory. "
            "Omit --sample for the default double-UART plus UDP setup. "
            "Use --add-uart-shell to also write a companion <config-stem>.commands.yml "
            "file with UART TX autocomplete suggestions; embed-log run loads it automatically. "
            "For an existing config, run --config FILE --add-uart-shell to generate only "
            "the companion commands file."
        ),
        epilog=(
            "Examples:\n"
            "  embed-log init\n"
            "  embed-log init --list\n"
            "  embed-log init --add-uart-shell\n"
            "  embed-log init --config embed-log.yml --add-uart-shell\n"
            "  embed-log init --sample single_network_single_tab --output embed-log.yml\n"
            "  embed-log init --sample single_file_single_tab -o file.yml\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    init.add_argument(
        "--sample",
        default=None,
        help=(
            "sample name from `embed-log init --list` "
            f"(default: {_sample_name_for_cli(_DEFAULT_INIT_SAMPLE)})"
        ),
    )
    init.add_argument("--output", "-o", default=None, help="output path (default: embed-log.yml)")
    init.add_argument("--config", "-c", default=None, help="existing config to inspect when generating only UART TX suggestions")
    init.add_argument("--force", action="store_true", help="overwrite if output file exists")
    init.add_argument("--list", action="store_true", dest="list_only", help="list init samples and exit")
    init.add_argument(
        "--add-uart-shell",
        action="store_true",
        help=(
            "also write <config-stem>.commands.yml with starter UART TX command "
            "suggestions; run loads it automatically for UART panes"
        ),
    )
