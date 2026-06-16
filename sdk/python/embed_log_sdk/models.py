"""Data models for the embed-log control API."""

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class SourceInfo:
    """Metadata about a log source returned by the hello handshake."""

    name: str
    source_type: str  # "uart", "udp", "file", "network_capture"
    label: str
    writable: bool


@dataclass
class SessionInfo:
    """Current session metadata."""

    id: str


@dataclass
class HelloResult:
    """Response to a hello command."""

    sources: dict[str, SourceInfo]
    session: SessionInfo


@dataclass
class LogEntry:
    """A single structured log entry received via subscription."""

    source_id: str
    origin: str  # "SERIAL", "TX::<origin>", or caller-provided origin
    message: str
    timestamp_iso: str
    line_idx: int
    color: Optional[str] = None
    is_tx: bool = False

    @classmethod
    def from_dict(cls, data: dict) -> "LogEntry":
        return cls(
            source_id=data.get("source_id", ""),
            origin=data.get("origin", ""),
            message=data.get("message", ""),
            timestamp_iso=data.get("timestamp_iso", ""),
            line_idx=data.get("line_idx", 0),
            color=data.get("color"),
            is_tx=data.get("is_tx", False),
        )


@dataclass
class Event:
    """A backend-detected event received via event subscription."""

    event_id: str
    source_id: str
    severity: str
    timestamp_num: float
    rel_num: float
    line_idx: int
    message: str
    captures: list[str] = field(default_factory=list)
    timestamp_iso: str = ""
    timestamp: str = ""
    origin: str = ""

    @classmethod
    def from_dict(cls, data: dict) -> "Event":
        captures = data.get("captures", [])
        if not isinstance(captures, list):
            captures = []
        return cls(
            event_id=data.get("event_id", ""),
            source_id=data.get("source_id", ""),
            severity=data.get("severity", "info"),
            timestamp_num=float(data.get("timestamp_num") or 0.0),
            rel_num=float(data.get("rel_num") or 0.0),
            line_idx=int(data.get("line_idx") or 0),
            message=data.get("message", ""),
            captures=[str(c) for c in captures],
            timestamp_iso=data.get("timestamp_iso", ""),
            timestamp=data.get("timestamp", ""),
            origin=data.get("origin", ""),
        )


@dataclass
class Marker:
    """A marker attached to a log line."""

    pane_id: str
    line_idx: int
    end_idx: int
    num_ts: float
    description: str
    created_at: str
    origin: str = "watcher"

    @classmethod
    def from_dict(cls, data: dict) -> "Marker":
        return cls(
            pane_id=data.get("paneId", ""),
            line_idx=data.get("lineIdx", 0),
            end_idx=data.get("endIdx", 0),
            num_ts=data.get("numTs", 0.0),
            description=data.get("description", ""),
            created_at=data.get("createdAt", ""),
            origin=data.get("origin", "watcher"),
        )
