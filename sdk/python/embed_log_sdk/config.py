"""Narrow YAML config parser for the embed-log SDK.

Only extracts fields needed to connect and validate early:
- server.host, server.ws_port
- sources[].name, sources[].type, sources[].label
- companion command file data

Runtime hello.result is authoritative; local config is for early validation.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import yaml

from .exceptions import ConfigError


@dataclass
class SourceCfg:
    """A single source definition from the config file."""

    name: str
    source_type: str  # "uart", "udp", "file", "network_capture"
    label: str
    writable: bool  # derived from type


@dataclass
class ServerCfg:
    """Server connection settings."""

    host: str = "127.0.0.1"
    ws_port: int = 8080

    @property
    def ws_url(self) -> str:
        return f"ws://{self.host}:{self.ws_port}/api/v1/control"


@dataclass
class SdkConfig:
    """Parsed SDK configuration."""

    server: ServerCfg = field(default_factory=ServerCfg)
    sources: dict[str, SourceCfg] = field(default_factory=dict)
    commands: dict[str, list[str]] = field(default_factory=dict)

    @classmethod
    def from_file(cls, path: str | Path) -> "SdkConfig":
        """Parse an embed-log YAML config file (narrow parser)."""
        path = Path(path)
        if not path.exists():
            raise ConfigError(f"config file not found: {path}")

        with open(path, "r") as f:
            raw = yaml.safe_load(f)

        if not isinstance(raw, dict):
            raise ConfigError(f"expected a mapping at top level, got {type(raw).__name__}")

        return cls._parse(raw, path)

    @classmethod
    def from_dict(cls, raw: dict, config_path: Optional[Path] = None) -> "SdkConfig":
        """Parse from an already-loaded dict (useful for testing)."""
        return cls._parse(raw, config_path)

    @classmethod
    def _parse(cls, raw: dict, config_path: Optional[Path] = None) -> "SdkConfig":
        server = ServerCfg()
        server_raw = raw.get("server", {})
        if isinstance(server_raw, dict):
            server.host = str(server_raw.get("host", server.host))
            server.ws_port = int(server_raw.get("ws_port", server.ws_port))

        sources: dict[str, SourceCfg] = {}
        for src in raw.get("sources", []):
            if not isinstance(src, dict):
                continue
            name = str(src.get("name", ""))
            if not name:
                continue
            stype = str(src.get("type", "")).lower()
            writable = stype == "uart"
            label = str(src.get("label", name))
            sources[name] = SourceCfg(
                name=name, source_type=stype, label=label, writable=writable
            )

        # Load companion command file
        commands: dict[str, list[str]] = {}
        cmd_file = cls._resolve_commands_file(config_path)
        if cmd_file and cmd_file.exists():
            commands = cls._load_commands_file(cmd_file, set(sources.keys()))

        return cls(server=server, sources=sources, commands=commands)

    @staticmethod
    def _resolve_commands_file(config_path: Optional[Path]) -> Optional[Path]:
        """Resolve the companion commands file path.

        Priority:
        1. <config-stem>.commands.yml alongside the config file.
        2. embed-log.commands.yml in the config directory.
        3. embed-log.commands.yml in the current working directory.
        """
        if config_path is None:
            cwd = Path.cwd()
            fallback = cwd / "embed-log.commands.yml"
            return fallback if fallback.exists() else None

        config_dir = config_path.parent

        # <config-stem>.commands.yml
        stem = config_path.stem
        specific = config_dir / f"{stem}.commands.yml"
        if specific.exists():
            return specific

        # embed-log.commands.yml in config dir
        config_dir_fb = config_dir / "embed-log.commands.yml"
        if config_dir_fb.exists():
            return config_dir_fb

        # embed-log.commands.yml in CWD (if different)
        cwd = Path.cwd()
        if config_dir.resolve() != cwd.resolve():
            cwd_fb = cwd / "embed-log.commands.yml"
            if cwd_fb.exists():
                return cwd_fb

        return None

    @staticmethod
    def _load_commands_file(path: Path, known_sources: set[str]) -> dict[str, list[str]]:
        """Load and filter commands from a companion YAML file."""
        try:
            with open(path, "r") as f:
                raw = yaml.safe_load(f)
        except Exception:
            return {}

        if not isinstance(raw, dict):
            return {}

        sources_raw = raw.get("sources")
        if not isinstance(sources_raw, dict):
            return {}

        result: dict[str, list[str]] = {}
        for name, cmds in sources_raw.items():
            if name not in known_sources:
                continue
            if isinstance(cmds, list):
                filtered = [str(c) for c in cmds if isinstance(c, str) and c.strip()]
                if filtered:
                    result[name] = filtered
        return result

    def source_names(self) -> list[str]:
        return list(self.sources.keys())

    def is_writable(self, source_id: str) -> bool:
        src = self.sources.get(source_id)
        return src.writable if src else False

    @property
    def ws_url(self) -> str:
        return self.server.ws_url
