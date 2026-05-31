import os
import tempfile
import threading
import unittest
from datetime import datetime, timezone
from typing import Optional

from backend.core.models import LogEntry, QueueStats
from backend.core.queue import TrackedQueue
from backend.core.runtime import SourceManager


class DummySource:
    """Minimal LogSource stand-in for testing SourceManager without I/O."""

    def start(self, on_line, stop, name):
        pass

    def write(self, data):
        pass

    @property
    def supports_write(self):
        return True


class CapturingQueue(TrackedQueue):
    """TrackedQueue subclass that records every non-None item put."""

    def __init__(self):
        super().__init__(0)
        self.captured: list[Optional[LogEntry]] = []

    def put(self, item: Optional[LogEntry]) -> None:
        if item is not None:
            self.captured.append(item)
        super().put(item)


FIXED = datetime(2026, 6, 15, 10, 30, 0, tzinfo=timezone.utc)


def fixed_clock() -> datetime:
    return FIXED


class SourceManagerClockTests(unittest.TestCase):
    """Verify that SourceManager uses the injected clock for every
    LogEntry it creates."""

    # -- helpers ----------------------------------------------------------

    def _make_manager(self, clock=None) -> tuple[SourceManager, CapturingQueue]:
        fd, log_file = tempfile.mkstemp(suffix=".log")
        os.close(fd)
        self.addCleanup(lambda: os.unlink(log_file) if os.path.exists(log_file) else None)
        mgr = SourceManager(
            name="TEST",
            source=DummySource(),
            log_file=log_file,
            socket_host="127.0.0.1",
            clock=clock,
        )
        queue = CapturingQueue()
        mgr._queue = queue
        return mgr, queue

    # -- tests ------------------------------------------------------------

    def test_add_session_marker_uses_clock(self):
        mgr, q = self._make_manager(clock=fixed_clock)

        mgr.add_session_marker("session-start")

        self.assertEqual(len(q.captured), 1)
        entry = q.captured[0]
        self.assertIsInstance(entry, LogEntry)
        self.assertEqual(entry.timestamp, FIXED)
        self.assertEqual(entry.source, "SYSTEM")
        self.assertEqual(entry.message, "session-start")
        self.assertEqual(entry.color, "cyan")

    def test_add_ui_clear_marker_uses_clock(self):
        mgr, q = self._make_manager(clock=fixed_clock)

        mgr.add_ui_clear_marker("pane")

        # add_ui_clear_marker enqueues 3 entries; all share the fixed clock.
        self.assertEqual(len(q.captured), 3)
        for entry in q.captured:
            self.assertIsInstance(entry, LogEntry)
            self.assertEqual(entry.timestamp, FIXED)
            self.assertEqual(entry.source, "SYSTEM")

        # Second entry is the marker text.
        self.assertIn("UI clear", q.captured[1].message)

    def test_write_source_uses_clock(self):
        mgr, q = self._make_manager(clock=fixed_clock)

        mgr._write_source(b"hello", source="UART1")

        self.assertEqual(len(q.captured), 1)
        entry = q.captured[0]
        self.assertIsInstance(entry, LogEntry)
        self.assertEqual(entry.timestamp, FIXED)
        self.assertEqual(entry.source, "TX::UART1")
        self.assertEqual(entry.message, "hello")

    def test_on_source_line_uses_clock(self):
        mgr, q = self._make_manager(clock=fixed_clock)

        mgr._on_source_line("raw serial data")

        self.assertEqual(len(q.captured), 1)
        entry = q.captured[0]
        self.assertIsInstance(entry, LogEntry)
        self.assertEqual(entry.timestamp, FIXED)
        self.assertEqual(entry.source, "SERIAL")
        self.assertEqual(entry.message, "raw serial data")

    def test_ingest_json_log_uses_clock(self):
        mgr, q = self._make_manager(clock=fixed_clock)

        import json
        payload = json.dumps({"type": "log", "source": "EXT", "message": "ping"}).encode()
        mgr._ingest_json(payload)

        self.assertEqual(len(q.captured), 1)
        entry = q.captured[0]
        self.assertIsInstance(entry, LogEntry)
        self.assertEqual(entry.timestamp, FIXED)
        self.assertEqual(entry.source, "EXT")
        self.assertEqual(entry.message, "ping")

    def test_default_clock_uses_real_time(self):
        """When no clock is injected, timestamps reflect wall-clock time."""
        mgr, q = self._make_manager()  # clock=None → real time

        before = datetime.now().astimezone()
        mgr.add_session_marker("real-time")
        after = datetime.now().astimezone()

        self.assertEqual(len(q.captured), 1)
        entry = q.captured[0]
        self.assertGreaterEqual(entry.timestamp, before)
        self.assertLessEqual(entry.timestamp, after)

    def test_clock_called_per_entry(self):
        """Each entry gets a fresh timestamp from the clock (the clock is
        called, not cached)."""
        call_count = 0

        def counting_clock() -> datetime:
            nonlocal call_count
            ts = datetime(2026, 1, 1, 0, 0, call_count, tzinfo=timezone.utc)
            call_count += 1
            return ts

        mgr, q = self._make_manager(clock=counting_clock)

        mgr.add_session_marker("a")
        mgr.add_session_marker("b")
        mgr.add_session_marker("c")

        self.assertEqual(len(q.captured), 3)
        self.assertEqual(q.captured[0].timestamp.second, 0)
        self.assertEqual(q.captured[1].timestamp.second, 1)
        self.assertEqual(q.captured[2].timestamp.second, 2)
        self.assertEqual(call_count, 3)


if __name__ == "__main__":
    unittest.main()
