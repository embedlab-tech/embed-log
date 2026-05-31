"""skill subcommand — list and export built-in skills.

Skills are markdown files bundled in `backend/skills/`. They describe
workflows an agent or user can follow to interact with embed-log.

Usage:

    embed-log skill list        — enumerate available skills
    embed-log skill show <name> — print the skill markdown to stdout
"""

from __future__ import annotations

import sys
from importlib.resources import files as pkg_files
from pathlib import PurePath


def _discover_skills() -> list[str]:
    """Return sorted list of skill names (stem of each .md file in backend/skills/)."""
    skills: list[str] = []
    try:
        traversable = pkg_files("backend.skills")
        for entry in traversable.iterdir():
            if entry.is_file() and entry.name.endswith(".md"):
                stem = PurePath(entry.name).stem
                skills.append(stem)
    except (ModuleNotFoundError, FileNotFoundError):
        pass
    skills.sort()
    return skills


def _run_skill_list() -> int:
    """Handler for `embed-log skill list`."""
    skills = _discover_skills()
    if not skills:
        print("No skills found.")
        return 0

    print("Available skills (load via `embed-log skill show <name>` or `read skill://<name>`):")
    for name in skills:
        print(f"  {name}")
    return 0


def _run_skill_show(name: str) -> int:
    """Handler for `embed-log skill show <name>`."""
    skills = _discover_skills()
    if name not in skills:
        print(f"Unknown skill: {name}", file=sys.stderr)
        print("", file=sys.stderr)
        print("Available skills:", file=sys.stderr)
        for n in skills:
            print(f"  {n}", file=sys.stderr)
        return 1

    try:
        content = pkg_files("backend.skills").joinpath(f"{name}.md").read_text()
    except (ModuleNotFoundError, FileNotFoundError, OSError) as e:
        print(f"Error reading skill '{name}': {e}", file=sys.stderr)
        return 1

    sys.stdout.write(content)
    sys.stdout.flush()
    return 0


def _run_skill(argv: list[str]) -> int:
    """Parse skill subcommand and dispatch."""
    if not argv or argv[0] in ("-h", "--help"):
        print("Usage:")
        print("  embed-log skill list              — list available skills")
        print("  embed-log skill show <name>       — print skill markdown to stdout")
        print("")
        return 0

    cmd = argv[0]

    match cmd:
        case "list":
            return _run_skill_list()
        case "show":
            if len(argv) < 2:
                print("Usage: embed-log skill show <name>", file=sys.stderr)
                print("", file=sys.stderr)
                print("Available skills:", file=sys.stderr)
                for n in _discover_skills():
                    print(f"  {n}", file=sys.stderr)
                return 1
            return _run_skill_show(argv[1])
        case _:
            print(
                f"Unknown skill subcommand: {cmd}", file=sys.stderr
            )
            return 1
