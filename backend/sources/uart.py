from __future__ import annotations

from ..parsers import TextParser

from .parsed import ParsedSource
from .raw_uart import RawUartSource


class UartSource(ParsedSource):
    def __init__(self, port: str, baudrate: int = 115200):
        super().__init__(RawUartSource(port, baudrate), TextParser())

    @property
    def port(self) -> str:
        return self.raw_source.port

    @property
    def baudrate(self) -> int:
        return self.raw_source.baudrate

    def _run(self, on_line, stop, name) -> None:
        parser = TextParser()

        def on_chunk(data: bytes) -> None:
            for line in parser.feed(data):
                on_line(line)

        def on_boundary() -> None:
            for line in parser.flush():
                on_line(line)

        self.raw_source._run(on_chunk, on_boundary, stop, name)
