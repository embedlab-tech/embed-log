import threading
import unittest

from backend.parsers import TextParser
from backend.sources import ParsedSource, RawLogSource


class _FakeRawSource(RawLogSource):
    def __init__(self, events):
        self._events = list(events)

    def start(self, on_chunk, on_boundary, stop: threading.Event, name: str) -> None:
        for event in self._events:
            kind = event[0]
            if kind == "chunk":
                on_chunk(event[1])
            elif kind == "boundary":
                on_boundary()
            else:
                raise AssertionError(f"unexpected event kind: {kind!r}")
        stop.set()


class ParsedSourceTests(unittest.TestCase):
    def test_stream_parser_reassembles_split_utf8_until_newline(self):
        stop = threading.Event()
        lines = []
        src = ParsedSource(
            _FakeRawSource([
                ("chunk", b"prefix \xe2\x82"),
                ("chunk", b"\xac suffix\n"),
            ]),
            TextParser(),
        )

        src.start(lines.append, stop, "SRC")

        self.assertEqual(lines, ["prefix € suffix"])

    def test_boundary_flush_preserves_udp_datagram_separation(self):
        stop = threading.Event()
        lines = []
        src = ParsedSource(
            _FakeRawSource([
                ("chunk", b"left"),
                ("boundary", None),
                ("chunk", b"right\n"),
                ("boundary", None),
            ]),
            TextParser(),
        )

        src.start(lines.append, stop, "SRC")

        self.assertEqual(lines, ["left", "right"])

    def test_flush_drops_empty_buffer(self):
        parser = TextParser()
        self.assertEqual(parser.flush(), [])


if __name__ == "__main__":
    unittest.main()
