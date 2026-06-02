"""Data classes for the embed-log SDK."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import datetime


@dataclass
class LogEntry:
    """A single log line received from an embed-log stream.

    This mirrors the JSON payload sent by the inject-port stream
    (see SourceManager._stream_payload).
    """

    source_id: str
    """Pane / source name that produced this entry (e.g. ``"SENSOR_A"``)."""

    source: str
    """Origin kind: ``"SERIAL"`` for received lines, ``"TX::<name>"`` for transmits."""

    message: str
    """The log line text (no timestamp prefix, ANSI-reset applied)."""

    timestamp: datetime
    """Wall-clock timestamp of the entry."""

    color: str | None = None
    """ANSI colour name if the entry was styled (``"green"``, ``"red"``, …)."""

    @property
    def is_tx(self) -> bool:
        """True if this entry was a transmitted (TX) command."""
        return self.source.startswith("TX::")

    @classmethod
    def from_json(cls, payload: dict) -> "LogEntry":
        """Construct from the JSON object emitted by the inject-port stream."""
        return cls(
            source_id=payload["source_id"],
            source=payload.get("source", "SERIAL"),
            message=payload.get("message", ""),
            timestamp=datetime.fromisoformat(payload["timestamp"]),
            color=payload.get("color"),
        )


@dataclass
class WatchMatch:
    """Emitted by :class:`Watcher` when a regex pattern matches a log line."""

    name: str
    """User-defined label for the pattern that matched."""

    pattern: str
    """The regex pattern (as passed to the Watcher)."""

    entry: LogEntry
    """The :class:`LogEntry` that triggered the match."""

    match: re.Match
    """The :class:`re.Match` object — inspect ``.group()``, ``.span()``, etc."""

    groups: dict[str, str] = field(default_factory=dict)
    """Named capture groups from the regex, e.g. ``{"seconds": "30"}``."""

    def __repr__(self) -> str:
        snippet = self.entry.message[:80]
        return f"WatchMatch(name={self.name!r}, message={snippet!r})"
