"""Sample config generator — lists and copies config-samples from the repo."""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path


def _samples_dir() -> Path:
    """Return the path to the config-samples directory."""
    return Path(__file__).resolve().parents[2] / "config-samples"


def _list_samples() -> list[Path]:
    """Return sorted list of YAML sample files."""
    d = _samples_dir()
    if not d.is_dir():
        return []
    return sorted(d.glob("*.yml"))


def _describe(sample: Path) -> str:
    """Extract the first comment line from a sample file as a short description."""
    try:
        text = sample.read_text("utf-8")
        for line in text.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                # Return the first non-empty, non-preamble comment
                content = stripped.lstrip("#").strip()
                if content and not content.startswith("embed-log"):
                    return content
    except OSError:
        pass
    return ""


def _prompt_choice(options: list[str], default: int = 1) -> int:
    """Let the user pick an option by number."""
    print("Available config samples:")
    print("")
    for i, opt in enumerate(options, start=1):
        print(f"  {i}) {opt}")
    print("")
    while True:
        raw = input(f"Choose a config (1-{len(options)}, default {default}): ").strip()
        if not raw:
            return default - 1
        try:
            idx = int(raw) - 1
            if 0 <= idx < len(options):
                return idx
        except ValueError:
            pass
        print(f"Enter a number between 1 and {len(options)}.")


def _run_sample_config(args: argparse.Namespace) -> int:
    samples = _list_samples()
    if not samples:
        print("No config samples found. Run from the repository root.")
        return 1

    if args.list_only:
        print("Available config samples:")
        for s in samples:
            desc = _describe(s)
            if desc:
                print(f"  {s.name}  — {desc}")
            else:
                print(f"  {s.name}")
        return 0

    # Build options for the menu
    options = []
    for s in samples:
        desc = _describe(s)
        if desc:
            options.append(f"{s.name}  — {desc}")
        else:
            options.append(s.name)

    # If a specific sample was requested, find it
    if args.sample:
        matching = [s for s in samples if s.name == args.sample]
        if not matching:
            print(f"Sample not found: {args.sample}")
            print(f"Available: {', '.join(s.name for s in samples)}")
            return 1
        chosen = matching[0]
    elif args.output or not sys.stdin.isatty():
        # Non-interactive: require --sample or error
        print("Use --sample NAME to pick a config sample.", file=sys.stderr)
        return 1
    else:
        idx = _prompt_choice(options)
        chosen = samples[idx]

    output = Path(args.output or "embed-log.yml")
    if output.exists() and not args.force:
        print(f"File exists: {output}. Use --force to overwrite.", file=sys.stderr)
        return 1

    shutil.copy2(chosen, output)
    print(f"Created: {output}  (from {chosen.name})")
    return 0


def add_subparser(subparsers) -> None:
    p = subparsers.add_parser(
        "sample-config",
        help="generate a config file from a template",
        description="List available config templates and generate one for your setup.",
        epilog=(
            "Examples:\n"
            "  embed-log sample-config\n"
            "  embed-log sample-config --list\n"
            "  embed-log sample-config --sample single-tab-dual-pane.yml\n"
            "  embed-log sample-config --sample multi-tab-multi-baud.yml -o my-config.yml --force\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--sample",
        help="pick a specific sample by filename (omit for interactive menu)",
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
