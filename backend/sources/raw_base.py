from __future__ import annotations

import threading
from abc import ABC, abstractmethod
from typing import Callable


class RawLogSource(ABC):
    @abstractmethod
    def start(
        self,
        on_chunk: Callable[[bytes], None],
        on_boundary: Callable[[], None],
        stop: threading.Event,
        name: str,
    ) -> None:
        """Start reading in a background thread. Emit bytes and transport boundaries."""

    def write(self, data: bytes) -> None:
        raise TypeError(f"{type(self).__name__} does not support write")

    @property
    def supports_write(self) -> bool:
        return False
