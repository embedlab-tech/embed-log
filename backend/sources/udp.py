from __future__ import annotations

from ..parsers import TextParser

from .parsed import ParsedSource
from .raw_udp import RawUdpSource


class UdpSource(ParsedSource):
    """Listens for UDP datagrams; each datagram may contain multiple newline-separated lines."""

    def __init__(self, port: int):
        super().__init__(RawUdpSource(port), TextParser())

    @property
    def port(self) -> int:
        return self.raw_source.port
