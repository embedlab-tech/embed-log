from __future__ import annotations

import threading

from ..parsers import StreamParser

from .base import LogSource
from .raw_base import RawLogSource


class ParsedSource(LogSource):
    def __init__(self, raw_source: RawLogSource, parser: StreamParser):
        self.raw_source = raw_source
        self.parser = parser
        self._emit_lock = threading.Lock()

    def start(self, on_line, stop: threading.Event, name: str) -> None:
        def emit_lines(lines: list[str]) -> None:
            for line in lines:
                on_line(line)

        def on_chunk(data: bytes) -> None:
            with self._emit_lock:
                emit_lines(self.parser.feed(data))

        def on_boundary() -> None:
            with self._emit_lock:
                emit_lines(self.parser.flush())

        self.raw_source.start(on_chunk, on_boundary, stop, name)

    def write(self, data: bytes) -> None:
        self.raw_source.write(data)

    @property
    def supports_write(self) -> bool:
        return self.raw_source.supports_write
