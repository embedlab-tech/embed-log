"""Integration tests for CBOR datagram parser in the source pipeline."""

import socket
import threading
import time
import unittest

from backend.app import build_source
from backend.parsers import CborDatagramParser
from backend.sources import ParsedSource, RawUdpSource, UdpSource

try:
    import cbor2
except ImportError:
    cbor2 = None


class CborBuildSourceTests(unittest.TestCase):
    """Tests for building sources with cbor-datagram parser via build_source()."""

    def test_udp_cbor_parser_builds_parsed_source(self):
        """UDP + cbor-datagram builds as ParsedSource wrapping a RawUdpSource."""
        cfg = {
            "name": "CBOR_SRC",
            "type": "udp",
            "port": 6005,
            "parser": {"type": "cbor-datagram"},
        }
        src = build_source(cfg)
        self.assertIsInstance(src, ParsedSource)
        self.assertIsInstance(src.raw_source, RawUdpSource)
        self.assertIsInstance(src.parser, CborDatagramParser)
        self.assertEqual(src.raw_source.port, 6005)

    def test_udp_text_parser_still_builds_udp_source(self):
        """UDP + text parser still builds the optimised UdpSource."""
        cfg = {
            "name": "TEXT_SRC",
            "type": "udp",
            "port": 6006,
            "parser": {"type": "text"},
        }
        src = build_source(cfg)
        self.assertIsInstance(src, UdpSource)


class RealUdpCborIntegrationTest(unittest.TestCase):
    """End-to-end test: UDP datagram -> RawUdpSource -> CborDatagramParser -> text line."""

    def setUp(self):
        self._lines: list[str] = []
        self._received = threading.Event()
        # Find a free port
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.bind(("127.0.0.1", 0))
            self._port = s.getsockname()[1]

    def _on_line(self, line: str) -> None:
        self._lines.append(line)
        self._received.set()

    def test_cbor_datagram_is_decoded_to_text_line(self):
        raw = RawUdpSource(self._port)
        parser = CborDatagramParser()
        src = ParsedSource(raw, parser)
        stop = threading.Event()

        src.start(self._on_line, stop, "integration-test")

        # Allow the UDP listener to start
        time.sleep(0.1)

        # Send a CBOR-encoded datagram
        payload = cbor2.dumps({
            "level": "INFO",
            "event": "temp",
            "value": 23.4,
            "unit": "C",
            "tick": 17,
        })

        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.sendto(payload, ("127.0.0.1", self._port))

        # Wait for the decoded line
        ok = self._received.wait(timeout=3.0)
        stop.set()

        self.assertTrue(ok, "timed out waiting for decoded line")
        self.assertEqual(len(self._lines), 1)
        line = self._lines[0]
        self.assertIn("level=INFO", line)
        self.assertIn("event=temp", line)
        self.assertIn("value=23.4", line)
        self.assertIn("unit=C", line)
        self.assertIn("tick=17", line)

    def test_malformed_cbor_is_dropped(self):
        """A malformed CBOR datagram does not crash the source."""
        raw = RawUdpSource(self._port)
        parser = CborDatagramParser()
        src = ParsedSource(raw, parser)
        stop = threading.Event()

        src.start(self._on_line, stop, "integration-test-malformed")
        time.sleep(0.1)

        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.sendto(b"\x81\xff\xff\xff\xff", ("127.0.0.1", self._port))

        # Give it time to process (should not crash and should not emit a line)
        time.sleep(0.3)
        stop.set()

        self.assertEqual(self._lines, [])

    def test_non_dict_cbor_is_dropped(self):
        """A CBOR list datagram is rejected without crashing."""
        raw = RawUdpSource(self._port)
        parser = CborDatagramParser()
        src = ParsedSource(raw, parser)
        stop = threading.Event()

        src.start(self._on_line, stop, "integration-test-list")
        time.sleep(0.1)

        payload = cbor2.dumps([1, 2, 3])
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.sendto(payload, ("127.0.0.1", self._port))

        time.sleep(0.3)
        stop.set()

        self.assertEqual(self._lines, [])
