from __future__ import annotations

from abc import ABC, abstractmethod


class StreamParser(ABC):
    @abstractmethod
    def feed(self, data: bytes) -> list[str]:
        """Consume raw bytes and return any complete decoded log lines."""

    @abstractmethod
    def flush(self) -> list[str]:
        """Emit any buffered decoded log lines at a transport boundary."""
