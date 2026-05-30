"""Update command — extracted from backend.cli."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path


def _run_update(args: argparse.Namespace) -> int:
    """Update embed-log by re-running the appropriate installer."""

    default_repo = "krezolekcoder/embed-log"
    default_repo_url = f"https://github.com/{default_repo}.git"

    try:
        from .._version import __version__ as current_version
    except ImportError:
        current_version = "1.0.1"

    try:
        from .._install_source import (
            __local_path__ as source_local_path,
            __ref__ as source_ref,
            __ref_type__ as source_ref_type,
            __repo__ as source_repo,
            __repo_url__ as source_repo_url,
            __source_kind__ as source_kind,
        )
    except ImportError:
        source_kind = "unknown"
        source_repo = default_repo
        source_repo_url = default_repo_url
        source_ref_type = "branch"
        source_ref = "main"
        source_local_path = ""

    def _parse_pipx_install_spec(pipx_path: str) -> str | None:
        try:
            result = subprocess.run(
                [pipx_path, "list", "--json"],
                capture_output=True,
                text=True,
                timeout=30,
            )
        except (subprocess.TimeoutExpired, OSError):
            return None
        if result.returncode != 0 or not result.stdout:
            return None
        try:
            data = json.loads(result.stdout)
            return (
                data.get("venvs", {})
                .get("embed-log", {})
                .get("metadata", {})
                .get("main_package", {})
                .get("package_or_url")
            )
        except (json.JSONDecodeError, TypeError, AttributeError):
            return None

    def _local_installer_root(candidate: str | None) -> Path | None:
        if not candidate:
            return None
        root = Path(candidate).expanduser()
        if not root.is_dir():
            return None
        if (root / "install.sh").is_file() and (root / "install.ps1").is_file():
            return root
        return None

    def _repo_slug(repo: str | None, repo_url: str | None) -> str:
        if repo and "/" in repo:
            return repo
        if repo_url:
            cleaned = repo_url.rstrip("/")
            if cleaned.endswith(".git"):
                cleaned = cleaned[:-4]
            marker = "github.com/"
            idx = cleaned.find(marker)
            if idx != -1:
                return cleaned[idx + len(marker):]
        return default_repo
    def _resolve_release_ref(repo: str) -> str:
        url = f"https://api.github.com/repos/{repo}/releases/latest"
        with urllib.request.urlopen(url, timeout=30) as response:
            data = json.loads(response.read().decode("utf-8"))
        tag = data.get("tag_name")
        if not tag:
            raise OSError("latest release tag not found")
        return tag


    def _download_installer(repo: str, ref: str, script_name: str) -> Path:
        url = f"https://raw.githubusercontent.com/{repo}/{ref}/{script_name}"
        with urllib.request.urlopen(url, timeout=30) as response:
            content = response.read()
        fd, tmp_name = tempfile.mkstemp(
            prefix="embed-log-installer-",
            suffix=".ps1" if script_name.endswith(".ps1") else ".sh",
        )
        os.close(fd)
        tmp_path = Path(tmp_name)
        tmp_path.write_bytes(content)
        return tmp_path

    def _run_installer(script_path: Path, *, env: dict[str, str]) -> int:
        if sys.platform.startswith("win"):
            shell = shutil.which("powershell") or shutil.which("pwsh")
            if not shell:
                print("PowerShell not found.")
                return 1
            cmd = [shell, "-ExecutionPolicy", "Bypass", "-File", str(script_path)]
        else:
            shell = shutil.which("bash")
            if not shell:
                print("bash not found.")
                return 1
            cmd = [shell, str(script_path)]
        try:
            result = subprocess.run(cmd, env=env, timeout=300)
        except (subprocess.TimeoutExpired, OSError) as exc:
            print(f"Update failed: {exc}")
            return 1
        return result.returncode
    def _print_installed_version() -> None:
        app = shutil.which("embed-log")
        if app:
            try:
                result = subprocess.run(
                    [app, "--version"],
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                if result.returncode == 0 and result.stdout.strip():
                    print(result.stdout.strip())
                    return
            except (subprocess.TimeoutExpired, OSError):
                pass
        print(f"embed-log {current_version} (unknown)")


    pipx = shutil.which("pipx")
    if not pipx:
        print("pipx not found.")
        print("")
        print("  To update, re-run the install script:")
        print("    curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash")
        print("")
        print("  Or install pipx first:")
        print("    python3 -m pip install --user pipx")
        print("    python3 -m pipx ensurepath")
        return 1

    install_spec = _parse_pipx_install_spec(pipx)

    repo = source_repo or default_repo
    repo_url = source_repo_url or default_repo_url
    ref_type = source_ref_type or "branch"
    ref = source_ref or "main"

    has_override = False
    if args.branch:
        ref_type = "branch"
        ref = args.branch
        has_override = True
    elif args.tag:
        ref_type = "tag"
        ref = args.tag
        has_override = True
    elif args.ref:
        ref_type = "commit"
        ref = args.ref
        has_override = True
    elif args.release:
        ref_type = "release"
        ref = "latest"
        has_override = True


    local_root = None
    if not has_override:
        if source_kind == "local":
            local_root = _local_installer_root(source_local_path)
            if local_root is None:
                print(f"Local install source is unavailable: {source_local_path}")
                print("")
                print("  Re-run the installer from that repository, or choose an explicit remote ref:")
                print("    embed-log update --release")

                return 1
        else:
            local_root = _local_installer_root(install_spec)

    env = os.environ.copy()
    env["EMBED_LOG_REPO"] = _repo_slug(repo, repo_url)
    env["EMBED_LOG_REPO_URL"] = repo_url
    env["EMBED_LOG_REF_TYPE"] = ref_type
    env["EMBED_LOG_REF"] = ref
    if args.force:
        env["EMBED_LOG_FORCE"] = "1"

    if local_root is not None:
        print(f"Running installer from local source: {local_root}")
        script_name = "install.ps1" if sys.platform.startswith("win") else "install.sh"
        rc = _run_installer(local_root / script_name, env=env)
        if rc != 0:
            print(f"Update failed (exit code {rc}).")
            return rc
        _print_installed_version()

        return 0

    script_name = "install.ps1" if sys.platform.startswith("win") else "install.sh"
    repo_slug = _repo_slug(repo, repo_url)
    download_ref = ref
    if ref_type == "release":
        try:
            download_ref = _resolve_release_ref(repo_slug)
        except OSError as exc:
            print(f"Failed to resolve latest release: {exc}")
            return 1

    try:
        installer_path = _download_installer(repo_slug, download_ref, script_name)

    except OSError as exc:
        print(f"Failed to download installer: {exc}")
        return 1

    try:
        print(f"Running installer from {repo_slug}@{ref_type}:{ref if ref_type != 'release' else download_ref}")
        rc = _run_installer(installer_path, env=env)
    finally:
        installer_path.unlink(missing_ok=True)

    if rc != 0:
        print(f"Update failed (exit code {rc}).")
        return rc

    _print_installed_version()
    return 0
