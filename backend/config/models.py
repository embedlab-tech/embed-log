from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class ParserConfig:
    type: str = "text"


@dataclass
class SourceConfig:
    name: str
    type: str  # "uart" or "udp"
    port: str | int  # str for uart, int for udp
    parser: ParserConfig = field(default_factory=ParserConfig)
    baudrate: int | None = None


@dataclass
class TabConfig:
    label: str
    panes: list[str]


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
    tabs: list[TabConfig] = field(default_factory=list)
    server: ServerConfig = field(default_factory=ServerConfig)
    logs: LogsConfig = field(default_factory=LogsConfig)
    baudrate: int = 115200
