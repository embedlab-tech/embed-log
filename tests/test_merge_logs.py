import tempfile
import unittest
from pathlib import Path

from utils.merge_logs import parse_log_file


class MergeLogsParseTests(unittest.TestCase):
    def test_parse_strips_leading_system_timestamp(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "a.log"
            path.write_text(
                "[2026-04-22T10:11:12.123+02:00] boot ok\n"
                "[2026-04-22T10:11:13.456+02:00] [TX::UI] ping\n"
                "[2026-04-22T10:11:14.000+02:00] [CONTROLLER] [SERIAL] payload\n",
                encoding="utf-8",
            )

            rows = parse_log_file(str(path), "CONTROLLER")

            self.assertEqual(3, len(rows))
            self.assertEqual("04-22 10:11:12.123", rows[0]["ts"])
            self.assertEqual("boot ok", rows[0]["text"])
            self.assertEqual("[TX::UI] ping", rows[1]["text"])
            self.assertTrue(rows[1]["isTx"])
            self.assertEqual("payload", rows[2]["text"])
    def test_parse_relative_timestamps(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "relative.log"
            path.write_text(
                "[T+00:00:00.000] boot ok\n"
                "[T+00:00:01.250] [TX::UI] ping\n"
                "[T+00:00:02.000] [CONTROLLER] [SERIAL] payload\n",
                encoding="utf-8",
            )

            rows = parse_log_file(str(path), "CONTROLLER")

            self.assertEqual(3, len(rows))
            self.assertEqual("T+00:00:00.000", rows[0]["ts"])
            self.assertEqual("boot ok", rows[0]["text"])
            self.assertEqual("T+00:00:01.250", rows[1]["ts"])
            self.assertTrue(rows[1]["isTx"])
            self.assertEqual("payload", rows[2]["text"])


if __name__ == "__main__":
    unittest.main()
