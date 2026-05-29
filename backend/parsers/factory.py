from __future__ import annotations

from .base import StreamParser
from .text import TextParser


def create_parser(config: dict | None) -> StreamParser:
    parser_type = "text" if config is None else str(config.get("type", "text")).strip().lower()
    if parser_type == "text":
        return TextParser()
    raise ValueError(f"unsupported parser type: {parser_type!r} (use 'text')")
