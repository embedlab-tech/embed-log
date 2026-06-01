from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class ParserConfig:
    type: str = "text"


@dataclass
class SourceConfig:
    name: str
    type: str  # "uart", "udp", or "file"
    port: str | int  # str for uart, int for udp
    parser: ParserConfig = field(default_factory=ParserConfig)
    baudrate: int | None = None


@dataclass
class FrontendPluginDefinition:
    builtin: str | None = None
    path: str | None = None


@dataclass
class PanePluginConfig:
    name: str
    options: dict[str, Any] = field(default_factory=dict)


@dataclass
class PaneConfig:
    source: str
    plugins: list[PanePluginConfig] = field(default_factory=list)


@dataclass
class TabConfig:
    label: str
    panes: list[PaneConfig]


@dataclass
class ServerConfig:
    host: str = "127.0.0.1"
    ws_port: int = 8080
    ws_ui: str | None = None
    app_name: str = "embed-log"
    open_browser: bool = False
    verbosity: str | None = None
    verbose: bool = False
    job_id: str | None = None
    default_light_theme: str | None = None
    default_dark_theme: str | None = None
    timestamp_mode: str = "absolute"
    queue_size: int = 20000


@dataclass
class LogsConfig:
    dir: str = "logs/"


@dataclass
class AppConfig:
    sources: list[SourceConfig] = field(default_factory=list)
    source_labels: dict[str, str] = field(default_factory=dict)
    injects: list[tuple[str, int]] = field(default_factory=list)
    forwards: list[tuple[str, int]] = field(default_factory=list)
    frontend_plugins: dict[str, FrontendPluginDefinition] = field(default_factory=dict)
    tabs: list[TabConfig] = field(default_factory=list)
    server: ServerConfig = field(default_factory=ServerConfig)
    logs: LogsConfig = field(default_factory=LogsConfig)
    baudrate: int = 115200
