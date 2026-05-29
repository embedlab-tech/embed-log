from __future__ import annotations

from .base import StreamParser
from .cbor_datagram import CborDatagramParser
from .text import TextParser


def create_parser(config: dict | None) -> StreamParser:
    parser_type = "text" if config is None else str(config.get("type", "text")).strip().lower()
    if parser_type == "text":
        return TextParser()
    if parser_type == "cbor-datagram":
        return CborDatagramParser()
    raise ValueError(f"unsupported parser type: {parser_type!r} (use 'text' or 'cbor-datagram')")
