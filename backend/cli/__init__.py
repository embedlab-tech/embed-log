from __future__ import annotations

from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _bundled_resource_root() -> Path:
    return Path(__file__).resolve().parents[1] / "resources"


def _resolve_bundled_file(
    repo_relative: str, *, packaged_relative: str | None = None
) -> Path | None:
    rel = packaged_relative or repo_relative
    packaged_path = _bundled_resource_root() / rel
    if packaged_path.is_file():
        return packaged_path

    repo_path = _repo_root() / repo_relative
    if repo_path.is_file():
        return repo_path

    return None


def _require_bundled_file(
    repo_relative: str, *, packaged_relative: str | None = None, label: str
) -> Path:
    path = _resolve_bundled_file(
        repo_relative, packaged_relative=packaged_relative
    )
    if path is not None:
        return path

    rel = packaged_relative or repo_relative
    raise FileNotFoundError(f"{label} not found: {_bundled_resource_root() / rel}")


from .dispatch import main


__all__ = [
    "main",
    "_repo_root",
    "_bundled_resource_root",
    "_resolve_bundled_file",
    "_require_bundled_file",
]