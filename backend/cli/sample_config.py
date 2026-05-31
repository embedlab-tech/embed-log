"""Sample config generator."""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path


def _bundled_samples_dir() -> Path | None:
    from . import _bundled_resource_root

    path = _bundled_resource_root() / "config-samples"
    return path if path.is_dir() else None


def _repo_samples_dir() -> Path:
    from . import _repo_root

    return _repo_root() / "config-samples"


def _list_samples() -> list[Path]:
    """Return sorted list of available YAML sample files."""
    seen: dict[str, Path] = {}

    bundled_dir = _bundled_samples_dir()
    if bundled_dir is not None:
        for sample in sorted(bundled_dir.glob("*.yml")):
            seen.setdefault(sample.name, sample)

    repo_dir = _repo_samples_dir()
    if repo_dir.is_dir():
        for sample in sorted(repo_dir.glob("*.yml")):
            seen.setdefault(sample.name, sample)

    return [seen[name] for name in sorted(seen)]


def _describe(sample: Path) -> str:
    """Extract the first useful comment line from a sample file."""
    try:
        text = sample.read_text("utf-8")
        for line in text.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                content = stripped.lstrip("#").strip()
                if content and not content.startswith("embed-log"):
                    return content
    except OSError:
        pass
    return ""


def _resolve_default_sample() -> Path:
    from . import _require_bundled_file

    return _require_bundled_file(
        "examples/embed-log.yml",
        packaged_relative="examples/embed-log.yml",
        label="Bundled sample config",
    )


def _run_sample_config(args: argparse.Namespace) -> int:
    samples = _list_samples()

    if args.list_only:
        if not samples:
            print("No config samples found.", file=sys.stderr)
            return 1
        print("Available config samples:")
        for sample in samples:
            desc = _describe(sample)
            if desc:
                print(f"  {sample.name}  — {desc}")
            else:
                print(f"  {sample.name}")
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
    return 0


def add_subparser(subparsers) -> None:
    p = subparsers.add_parser(
        "sample-config",
        help="generate a config file from a template",
        description="Copy the bundled default config or pick a specific sample by filename.",
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