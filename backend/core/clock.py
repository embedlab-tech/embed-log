from __future__ import annotations

import threading
from datetime import datetime
from typing import Callable

TIMESTAMP_MODE_ABSOLUTE = "absolute"
TIMESTAMP_MODE_RELATIVE = "relative"
TIMESTAMP_MODES = {TIMESTAMP_MODE_ABSOLUTE, TIMESTAMP_MODE_RELATIVE}


def _format_relative_millis(total_ms: int) -> str:
    if total_ms < 0:
        total_ms = 0
    hours, rem = divmod(total_ms, 3_600_000)
    minutes, rem = divmod(rem, 60_000)
    seconds, millis = divmod(rem, 1_000)
    return f"T+{hours:02d}:{minutes:02d}:{seconds:02d}.{millis:03d}"


class SessionClock:
    def __init__(self, mode: str, *, on_origin_set: Callable[[str], None] | None = None):
        self.mode = mode if mode in TIMESTAMP_MODES else TIMESTAMP_MODE_ABSOLUTE
        self._on_origin_set = on_origin_set
        self._lock = threading.Lock()
        self._origin: datetime | None = None

    def reset(self) -> None:
        with self._lock:
            self._origin = None

    def first_log_at(self) -> str | None:
        with self._lock:
            if self._origin is None:
                return None
            return self._origin.isoformat(timespec="milliseconds")

    def _ensure_origin(self, timestamp: datetime) -> datetime:
        callback = None
        origin_iso = None
        with self._lock:
            if self._origin is None:
                self._origin = timestamp
                callback = self._on_origin_set
                origin_iso = timestamp.isoformat(timespec="milliseconds")
            origin = self._origin
        if callback is not None and origin_iso is not None:
            callback(origin_iso)
        return origin

    def observe(self, timestamp: datetime) -> None:
        self._ensure_origin(timestamp)

    def relative_millis(self, timestamp: datetime) -> int:
        origin = self._ensure_origin(timestamp)
        delta_ms = int((timestamp - origin).total_seconds() * 1000)
        return delta_ms if delta_ms >= 0 else 0

    def file_timestamp(self, timestamp: datetime) -> str:
        self.observe(timestamp)
        if self.mode == TIMESTAMP_MODE_RELATIVE:
            return _format_relative_millis(self.relative_millis(timestamp))
        return timestamp.isoformat(timespec="milliseconds")

    def display_timestamp(self, timestamp: datetime) -> str:
        self.observe(timestamp)
        if self.mode == TIMESTAMP_MODE_RELATIVE:
            return _format_relative_millis(self.relative_millis(timestamp))
        return timestamp.strftime("%m-%d %H:%M:%S.%f")[:-3]

    def numeric_timestamp(self, timestamp: datetime) -> int:
        self.observe(timestamp)
        if self.mode == TIMESTAMP_MODE_RELATIVE:
            return self.relative_millis(timestamp)
        return int(timestamp.timestamp() * 1000)
