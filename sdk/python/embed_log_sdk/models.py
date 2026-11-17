"""Data models for the embed-log control API."""

from dataclasses import dataclass
from typing import Optional


@dataclass
class SourceInfo:
    """Metadata about a log source returned by the hello handshake."""

    name: str
    source_type: str  # "uart", "udp", "file", or presentation-only "merge"
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
    # Session-global cursor; absent only when connected to a legacy backend.
    sequence: Optional[int] = None
    session_id: str = ""
    color: Optional[str] = None
    is_tx: bool = False
    # Detected log level id (e.g. "err", "dbg"), if the source has a level profile.
    level: Optional[str] = None
    # "boot" or "crash" on the first line of a detected restart or crash.
    event: Optional[str] = None

    @classmethod
    def from_dict(cls, data: dict) -> "LogEntry":
        return cls(
            source_id=data.get("source_id", ""),
            origin=data.get("origin", ""),
            message=data.get("message", ""),
            timestamp_iso=data.get("timestamp_iso", ""),
            line_idx=data.get("line_idx", 0),
            sequence=data.get("sequence"),
            session_id=data.get("session_id", ""),
            color=data.get("color"),
            is_tx=data.get("is_tx", False),
            level=data.get("level"),
            event=data.get("event"),
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


@dataclass
class DeviceEvent:
    """A detected device restart (``event == "boot"``) or crash (``"crash"``)."""

    sequence: int
    source_id: str
    event: str
    message: str
    timestamp_iso: str = ""

    @property
    def is_crash(self) -> bool:
        return self.event == "crash"

    @classmethod
    def from_dict(cls, data: dict) -> "DeviceEvent":
        return cls(
            sequence=data.get("sequence", 0),
            source_id=data.get("source_id", ""),
            event=data.get("event", ""),
            message=data.get("message", ""),
            timestamp_iso=data.get("timestamp_iso") or "",
        )


@dataclass
class EventCounts:
    """Restarts and crashes of one source in the current session."""

    boot: int = 0
    crash: int = 0


@dataclass
class DecodedFrame:
    """One symbolized frame; inlined frames come before the containing function."""

    function: Optional[str]
    file: Optional[str]
    line: Optional[int]
    inlined: bool = False

    def __str__(self) -> str:
        place = f"{self.file}:{self.line}" if self.file and self.line else (self.file or "??")
        return f"{self.function or '??'} ({place}){' [inlined]' if self.inlined else ''}"


@dataclass
class DecodedAddress:
    """A code address from the fault dump (pc, lr, call-trace frame) and its frames."""

    label: str
    address: int
    is_return: bool
    line: str
    frames: list[DecodedFrame]


@dataclass
class CrashReport:
    """A crash dump symbolized with the session's firmware ELF."""

    source_id: str
    sequence: int
    lines: list[str]
    addresses: list[DecodedAddress]
    elf: str

    @property
    def backtrace(self) -> list[str]:
        """One line per address: ``pc 0x0000102a scale_sample (app.c:7) [inlined] <- process_sample (app.c:12)``."""
        rows = []
        for address in self.addresses:
            frames = " <- ".join(str(frame) for frame in address.frames) or "?? (not in this ELF)"
            rows.append(f"{address.label} 0x{address.address:08x} {frames}")
        return rows

    @classmethod
    def from_dict(cls, data: dict) -> "CrashReport":
        return cls(
            source_id=data.get("source", ""),
            sequence=data.get("sequence", 0),
            lines=list(data.get("lines", [])),
            addresses=[
                DecodedAddress(
                    label=address.get("label", ""),
                    address=address.get("address", 0),
                    is_return=address.get("is_return", False),
                    line=address.get("line", ""),
                    frames=[
                        DecodedFrame(
                            function=frame.get("function"),
                            file=frame.get("file"),
                            line=frame.get("line"),
                            inlined=frame.get("inlined", False),
                        )
                        for frame in address.get("frames", [])
                    ],
                )
                for address in data.get("addresses", [])
            ],
            elf=data.get("elf", ""),
        )
