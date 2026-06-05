"""Config path resolution for embed-log CLI flows.

Single source of truth for picking the active YAML config file. Resolution
precedence (highest first):

    1. Explicit CLI flag (e.g. ``--config``)
    2. ``EMBED_LOG_CONFIG_YML_PATH`` environment variable
    3. None (no config — caller picks a default)

Callers that need to *fail loudly* on a missing/unreadable env-var file do
their own check after calling :func:`resolve_config_path`; the resolver
returns a :class:`pathlib.Path` (or ``None``) without touching the
filesystem.
"""

from __future__ import annotations

import os
from pathlib import Path

ENV_CONFIG_PATH = "EMBED_LOG_CONFIG_YML_PATH"


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


__all__ = ["ENV_CONFIG_PATH", "resolve_config_path"]
