"""Self-update orchestration for the embed-log CLI."""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Mapping, Sequence

from . import _repo_root

DEFAULT_REPO = "krezolekcoder/embed-log"
DEFAULT_REPO_URL = f"https://github.com/{DEFAULT_REPO}.git"
_INSTALLER_RAW_BASE = f"https://raw.githubusercontent.com/{DEFAULT_REPO}/main"
_SHA_RE = re.compile(r"^[0-9a-fA-F]{7,40}$")


class UpdateError(RuntimeError):
    """Raised when update planning cannot safely continue."""


@dataclass(frozen=True)
class LatestRelease:
    version: str
    cutoff_at: datetime


@dataclass(frozen=True)
class UpdateTarget:
    ref_type: str
    ref: str


Fetcher = Callable[[str], Mapping[str, object]]
Runner = Callable[[Sequence[str], Mapping[str, str]], int]


def _parse_github_datetime(value: object, field: str) -> datetime:
    if not isinstance(value, str) or not value:
        raise UpdateError(f"GitHub response did not include {field}.")
    text = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        parsed = datetime.fromisoformat(text)
    except ValueError as exc:
        raise UpdateError(f"GitHub response had invalid {field}: {value!r}.") from exc
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def _github_json(url: str) -> Mapping[str, object]:
    req = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "embed-log-update",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        raise UpdateError(f"GitHub request failed ({exc.code}): {url}") from exc
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        raise UpdateError(f"GitHub request failed: {url}: {exc}") from exc
    if not isinstance(data, dict):
        raise UpdateError(f"GitHub response was not an object: {url}")
    return data


def _latest_release(repo: str, fetcher: Fetcher = _github_json) -> LatestRelease:
    data = fetcher(f"https://api.github.com/repos/{repo}/releases/latest")
    tag = data.get("tag_name")
    if not isinstance(tag, str) or not tag:
        raise UpdateError("Latest release response did not include tag_name.")
    published_at = _parse_github_datetime(data.get("published_at"), "published_at")
    try:
        cutoff_at = _commit_date(repo, tag, fetcher)
    except UpdateError:
        cutoff_at = published_at
    return LatestRelease(version=tag, cutoff_at=cutoff_at)


def _commit_date(repo: str, sha: str, fetcher: Fetcher = _github_json) -> datetime:
    data = fetcher(f"https://api.github.com/repos/{repo}/commits/{sha}")
    commit = data.get("commit")
    if not isinstance(commit, dict):
        raise UpdateError(f"Commit response did not include commit data for {sha}.")
    committer = commit.get("committer")
    if not isinstance(committer, dict):
        raise UpdateError(f"Commit response did not include committer data for {sha}.")
    return _parse_github_datetime(committer.get("date"), "commit.committer.date")


def _validate_sha(sha: str) -> str:
    text = sha.strip()
    if not _SHA_RE.fullmatch(text):
        raise UpdateError("--sha must be a 7-40 character hexadecimal commit SHA.")
    return text


def _resolve_target(args: argparse.Namespace, fetcher: Fetcher = _github_json) -> UpdateTarget:
    if args.sha is None:
        return UpdateTarget(ref_type="release", ref="latest")

    sha = _validate_sha(args.sha)
    if not args.allow_rollback:
        latest = _latest_release(DEFAULT_REPO, fetcher)
        requested_date = _commit_date(DEFAULT_REPO, sha, fetcher)
        if requested_date < latest.cutoff_at:
            raise UpdateError(
                f"Refusing to install {sha}: commit is older than latest release {latest.version}.\n"
                "Use --allow-rollback if you really want this."
            )
    return UpdateTarget(ref_type="commit", ref=sha)


def _platform_installer_name() -> str:
    return "install.ps1" if platform.system().lower().startswith("win") else "install.sh"


def _local_installer_path(installer_name: str) -> Path | None:
    path = _repo_root() / installer_name
    return path if path.is_file() else None


def _download_installer(installer_name: str, directory: Path) -> Path:
    url = f"{_INSTALLER_RAW_BASE}/{installer_name}"
    target = directory / installer_name
    try:
        with urllib.request.urlopen(url, timeout=20) as resp:
            target.write_bytes(resp.read())
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise UpdateError(f"Failed to download installer {url}: {exc}") from exc
    if installer_name == "install.sh":
        target.chmod(0o755)
    return target


def _installer_command(installer: Path) -> list[str]:
    if installer.name == "install.ps1":
        shell = shutil.which("pwsh") or shutil.which("powershell")
        if shell is None:
            raise UpdateError("PowerShell is required to run install.ps1.")
        return [shell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(installer)]
    shell = shutil.which("bash")
    if shell is None:
        raise UpdateError("bash is required to run install.sh.")
    return [shell, str(installer)]


def _default_runner(command: Sequence[str], env: Mapping[str, str]) -> int:
    completed = subprocess.run(command, env=dict(env), check=False)
    return completed.returncode


def _run_update_with(
    args: argparse.Namespace,
    *,
    fetcher: Fetcher = _github_json,
    runner: Runner = _default_runner,
) -> int:
    try:
        target = _resolve_target(args, fetcher)
        installer_name = _platform_installer_name()
        with tempfile.TemporaryDirectory(prefix="embed-log-update-") as tmp:
            installer = _local_installer_path(installer_name)
            if installer is None:
                installer = _download_installer(installer_name, Path(tmp))
            env = os.environ.copy()
            env.update(
                {
                    "EMBED_LOG_INSTALL_MODE": "update",
                    "EMBED_LOG_REF_TYPE": target.ref_type,
                    "EMBED_LOG_REF": target.ref,
                    "EMBED_LOG_REPO": DEFAULT_REPO,
                    "EMBED_LOG_REPO_URL": DEFAULT_REPO_URL,
                }
            )
            print(f"Updating embed-log via {installer.name} ({target.ref_type}:{target.ref})")
            return runner(_installer_command(installer), env)
    except UpdateError as exc:
        print(str(exc), file=sys.stderr)
        return 1


def _run_update(args: argparse.Namespace) -> int:
    return _run_update_with(args)


def add_subparser(subparsers) -> None:
    p = subparsers.add_parser(
        "update",
        help="update an existing embed-log install",
        description="Update an existing embed-log install by orchestrating the platform installer.",
        epilog=(
            "Examples:\n"
            "  embed-log update\n"
            "  embed-log update --sha <commit_sha>\n"
            "  embed-log update --sha <commit_sha> --allow-rollback\n"
            "\n"
            "install.sh / install.ps1 are for first install. After embed-log is\n"
            "installed, use `embed-log update` for later updates. The Python CLI\n"
            "only resolves the requested target and safety checks; the platform\n"
            "installer performs the install/update steps.\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--sha", metavar="COMMIT_SHA", default=None, help="install a specific commit")
    p.add_argument(
        "--allow-rollback",
        action="store_true",
        help="allow --sha when that commit is older than the latest release",
    )
