from __future__ import annotations

import threading
from abc import ABC, abstractmethod
from typing import Callable


class LogSource(ABC):
    """
    Abstract base for anything that produces log lines.
    Subclasses implement start(); only write-capable sources implement write().
    """

    @abstractmethod
    def start(self, on_line: Callable[[str], None], stop: threading.Event, name: str) -> None:
        """Start reading in a background thread. on_line(text) per line."""
    def close(self) -> None:
        """Release background resources for a prompt shutdown."""

    def write(self, data: bytes) -> None:
        raise TypeError(f"{type(self).__name__} does not support write")

    @property
    def supports_write(self) -> bool:
        return False
