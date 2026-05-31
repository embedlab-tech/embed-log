import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend.cli.parser import build_parser
from backend.cli.run import _run_run


class RunTimestampModeTests(unittest.TestCase):
    def test_cli_flag_overrides_config_timestamp_mode(self):
        cfg_text = """
version: 1
server:
  timestamp_mode: absolute
sources:
  - name: SRC
    type: udp
    port: 6000
tabs:
  - label: Demo
    panes: [SRC]
""".strip()

        with tempfile.TemporaryDirectory() as td:
            cfg_path = Path(td) / "embed-log.yml"
            cfg_path.write_text(cfg_text, encoding="utf-8")

            parser = build_parser()
            args = parser.parse_args([
                "run",
                "--config",
                str(cfg_path),
                "--timestamp-mode",
                "relative",
            ])

            with patch("backend.cli.run.run_app", return_value=0) as run_app:
                rc = _run_run(args)

        self.assertEqual(rc, 0)
        self.assertEqual("relative", run_app.call_args.kwargs["timestamp_mode"])


if __name__ == "__main__":
    unittest.main()
