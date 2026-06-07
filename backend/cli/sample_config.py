"""Sample config generator."""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path


_SAMPLE_DESCRIPTIONS: dict[str, str] = {
    "single_uart_single_tab.yml": "one UART source in one tab",
    "double_uart_single_tab.yml": "two UART panes side-by-side in one tab",
    "double_uart_udp_two_tabs.yml": "two UART panes plus one UDP/pytest tab",
    "double_uart_network_two_tabs.yml": "two UART panes plus a packet-capture network tab",
    "double_uart_udp_coap_two_tabs.yml": "two UART panes plus UDP panes with the CoAP plugin",
    "single_file_single_tab.yml": "one file-tail source in one tab",
    "double_uart_file_two_tabs.yml": "two UART panes plus a file-tail log tab",
    "double_uart_minimal_single_tab.yml": "minimal two-UART single-tab layout",
    "double_uart_udp_multi_baud_two_tabs.yml": "two UARTs with different baudrates plus a UDP tab",
    "double_uart_file_udp_coap_three_tabs.yml": "two UARTs, file tailing, UDP, and CoAP across three tabs",
    "single_network_single_tab.yml": "one packet-capture source in one tab",
    "three_udp_cbor_two_tabs.yml": "two CBOR UDP sources plus one text UDP monitor",
    "reference_full_annotated.yml": "full annotated reference config",
}

_SAMPLE_ORDER = {name: index for index, name in enumerate(_SAMPLE_DESCRIPTIONS)}




def _repo_samples_dir() -> Path:
    from . import _repo_root

    return _repo_root() / "config-samples"


def _list_samples() -> list[Path]:
    """Return canonical YAML sample files in user-facing order."""
    samples_dir = _repo_samples_dir()
    if not samples_dir.is_dir():
        return []
    return sorted(
        samples_dir.glob("*.yml"),
        key=lambda path: (_SAMPLE_ORDER.get(path.name, len(_SAMPLE_ORDER)), path.name),
    )


def _describe(sample: Path) -> str:
    """Return the short user-facing description for a sample config."""
    if description := _SAMPLE_DESCRIPTIONS.get(sample.name):
        return description
    return ""


def _resolve_default_sample() -> Path:
    from . import _require_bundled_file

    return _require_bundled_file(
        "examples/embed-log.yml",
        packaged_relative="examples/embed-log.yml",
        label="Bundled sample config",
    )

def _shell_config_path(output: Path) -> str:
    return str(output) if output.is_absolute() else f"$PWD/{output}"



def _run_sample_config(args: argparse.Namespace) -> int:
    samples = _list_samples()

    if args.list_only:
        if not samples:
            print("No config samples found.", file=sys.stderr)
            return 1
        print("Available config samples:")
        for sample in samples:
            print(f"  {sample.name.removesuffix('.yml'):<40} {_describe(sample)}")
        return 0

    if args.sample:
        matching = [sample for sample in samples if sample.name == args.sample]
        if not matching:
            print(f"Sample not found: {args.sample}", file=sys.stderr)
            if samples:
                print(f"Available: {', '.join(sample.name for sample in samples)}", file=sys.stderr)
            return 1
        chosen = matching[0]
    else:
        try:
            chosen = _resolve_default_sample()
        except FileNotFoundError as exc:
            print(str(exc), file=sys.stderr)
            return 1

    output = Path(args.output or "embed-log.yml")
    if output.exists() and not args.force:
        print(f"File exists: {output}. Use --force to overwrite.", file=sys.stderr)
        return 1

    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(chosen, output)
    print(f"Created: {output}  (from {chosen.name})")
    print("")
    print("Next commands:")
    print(f"  embed-log doctor --config {output}")
    print(f"  embed-log run --config {output}")
    print("")
    print("Or set the config once for this shell:")
    print(f"  export EMBED_LOG_CONFIG_YML_PATH=\"{_shell_config_path(output)}\"")
    print("  embed-log run")
    return 0


def add_subparser(subparsers) -> None:
    p = subparsers.add_parser(
        "sample-config",
        help=argparse.SUPPRESS,
        description="Deprecated alias for `embed-log init`.",
        epilog=(
            "Examples:\n"
            "  embed-log sample-config\n"
            "  embed-log sample-config --list\n"
            "  embed-log sample-config --sample double_uart_minimal_single_tab.yml\n"
            "  embed-log sample-config --sample double_uart_udp_multi_baud_two_tabs.yml -o my-config.yml --force\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--sample",
        help="pick a specific sample by filename; omit to write the bundled default config",
    )
    p.add_argument(
        "--output", "-o",
        default=None,
        help="output path (default: embed-log.yml)",
    )
    p.add_argument(
        "--force", action="store_true",
        help="overwrite if output file exists",
    )
    p.add_argument(
        "--list", action="store_true", dest="list_only",
        help="list available samples and exit",
    )