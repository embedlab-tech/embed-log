from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime


@dataclass(slots=True)
class LogEntry:
    timestamp: datetime
    source: str
    message: str
    color: str | None = None
    no_ws: bool = False


@dataclass
class QueueStats:
    maxsize: int
    depth: int
    utilization_pct: float
    enqueued: int
    dequeued: int
    peak_depth: int
    near_full_events: int
