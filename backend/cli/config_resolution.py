"""Config path resolution for embed-log CLI flows.

Single source of truth for picking explicitly configured YAML files. The
legacy resolver keeps its original precedence:

    1. Explicit CLI flag (e.g. ``--config``)
    2. ``EMBED_LOG_CONFIG_YML_PATH`` environment variable
    3. None (no config — caller picks a default)

Onboarding/diagnostics use :func:`resolve_active_config_path`, which adds
``./embed-log.yml`` as the local human-friendly fallback.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path

ENV_CONFIG_PATH = "EMBED_LOG_CONFIG_YML_PATH"

@dataclass(frozen=True)
class ConfigPathResolution:
    path: Path | None
    source: str | None


def resolve_config_path(cli_path: str | os.PathLike[str] | None) -> Path | None:
    """Return the effective config path, honoring the env var as a fallback.

    Empty strings from either the CLI flag or the environment are treated
    as "not set" so a blank ``--config ""`` cannot mask the env var.
    """
    if cli_path is not None:
        text = str(cli_path).strip()
        if text:
            return Path(text)
    env_value = os.environ.get(ENV_CONFIG_PATH)
    if env_value and env_value.strip():
        return Path(env_value.strip())
    return None


__all__ = [
    "ENV_CONFIG_PATH",
    "ConfigPathResolution",
    "resolve_active_config_path",
    "resolve_config_path",
]
def resolve_active_config_path(
    cli_path: str | os.PathLike[str] | None,
) -> ConfigPathResolution:
    """Return the active config path and where it came from.

    The returned path may not exist; callers decide whether absence is a
    warning, failure, or acceptable onboarding state.
    """
    if cli_path is not None:
        text = str(cli_path).strip()
        if text:
            return ConfigPathResolution(Path(text), "--config")

    env_value = os.environ.get(ENV_CONFIG_PATH)
    if env_value and env_value.strip():
        return ConfigPathResolution(Path(env_value.strip()), ENV_CONFIG_PATH)

    local = Path("embed-log.yml")
    if local.is_file():
        return ConfigPathResolution(local, "local embed-log.yml")

    return ConfigPathResolution(None, None)
