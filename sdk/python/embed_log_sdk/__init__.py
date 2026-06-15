"""embed-log SDK — Python client for the embed-log control WebSocket API."""

from .client import EmbedLogClient
from .config import SdkConfig
from .exceptions import (
    ConfigError,
    ConnectionError,
    EmbedLogError,
    NotWritableError,
    ProtocolError,
    ServerError,
    UnknownSourceError,
)
from .models import HelloResult, LogEntry, Marker, SessionInfo, SourceInfo

__all__ = [
    "EmbedLogClient",
    "SdkConfig",
    "EmbedLogError",
    "ConnectionError",
    "ProtocolError",
    "ServerError",
    "UnknownSourceError",
    "NotWritableError",
    "ConfigError",
    "HelloResult",
    "LogEntry",
    "Marker",
    "SessionInfo",
    "SourceInfo",
]
