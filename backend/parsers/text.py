from __future__ import annotations

from .base import StreamParser


class TextParser(StreamParser):
    def __init__(self) -> None:
        self._buffer = b""

    def feed(self, data: bytes) -> list[str]:
        if not data:
            return []

        self._buffer += data
        lines: list[str] = []
        while True:
            newline_at = self._buffer.find(b"\n")
            if newline_at < 0:
                break
            raw_line = self._buffer[:newline_at]
            self._buffer = self._buffer[newline_at + 1 :]
            line = raw_line.rstrip(b"\r").decode("utf-8", errors="replace").rstrip()
            if line:
                lines.append(line)
        return lines

    def flush(self) -> list[str]:
        if not self._buffer:
            return []
        raw_line = self._buffer
        self._buffer = b""
        line = raw_line.rstrip(b"\r").decode("utf-8", errors="replace").rstrip()
        return [line] if line else []
