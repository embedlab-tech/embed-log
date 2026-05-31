import unittest
from datetime import datetime, timedelta, timezone

from backend.core.models import LogEntry
from backend.core.clock import SessionClock, TIMESTAMP_MODE_RELATIVE
from backend.core.runtime import SourceManager


class RelativeTimestampModeTests(unittest.TestCase):
    def test_relative_mode_formats_logs_and_ws_payload_from_first_line(self):
        mgr = SourceManager.__new__(SourceManager)
        mgr.name = "SENSOR_A"
        mgr.verbose = False
        mgr.session_clock = SessionClock(TIMESTAMP_MODE_RELATIVE)

        t0 = datetime(2026, 1, 1, 12, 0, 0, 123000, tzinfo=timezone.utc)
        t1 = t0 + timedelta(milliseconds=1234)

        first = LogEntry(t0, "SERIAL", "boot ok")
        second = LogEntry(t1, "SERIAL", "ready")

        self.assertEqual("[T+00:00:00.000] boot ok", mgr._format(first))
        self.assertEqual("[T+00:00:01.234] ready", mgr._format(second))

        payload = mgr._ws_payload(second)
        self.assertEqual("T+00:00:01.234", payload["timestamp"])
        self.assertEqual(1234, payload["timestamp_num"])
        self.assertEqual(t1.isoformat(timespec="milliseconds"), payload["timestamp_iso"])
        self.assertEqual(t0.isoformat(timespec="milliseconds"), mgr.session_clock.first_log_at())


if __name__ == "__main__":
    unittest.main()
