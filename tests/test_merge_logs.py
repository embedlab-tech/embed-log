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
                "[2026-04-22T10:11:13.456+02:00] [TX::UI] ping\n",
                encoding="utf-8",
            )

            rows = parse_log_file(str(path))

            self.assertEqual(2, len(rows))
            self.assertEqual("04-22 10:11:12.123", rows[0]["ts"])
            self.assertEqual("boot ok", rows[0]["text"])
            self.assertEqual("[TX::UI] ping", rows[1]["text"])
            self.assertTrue(rows[1]["isTx"])


if __name__ == "__main__":
    unittest.main()
