from __future__ import annotations

from .parsed import ParsedSource
from .raw_file import RawFileSource


class FileSource(ParsedSource):
    """File source that watches a file and parses lines through a parser."""

    def __init__(self, path: str) -> None:
        # FileSource uses the default text parser.
        from ..parsers.text import TextParser

        super().__init__(RawFileSource(path), TextParser())
