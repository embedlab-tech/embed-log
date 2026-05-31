import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

from backend.cli import main
from backend.cli.diagnostics import _run_version


def _ns(**kw):
    return type("Args", (), kw)()


class VersionCommandTests(unittest.TestCase):
    def test_version_command_prints_version_header(self):
        args = _ns(config=None, json=False)
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_version(args)
        self.assertEqual(rc, 0)
        self.assertIn("embed-log version", buf.getvalue())

    def test_version_command_prints_source_and_commit(self):
        args = _ns(config=None, json=False)
        buf = io.StringIO()
        with patch(
            "backend.cli.diagnostics._load_install_identity",
            return_value=("1.1.3", "abc1234", "branch", "branch", "cli-version", ""),
        ):
            with redirect_stdout(buf):
                rc = _run_version(args)
        self.assertEqual(rc, 0)
        text = buf.getvalue()
        self.assertIn("[OK] version: 1.1.3", text)
        self.assertIn("[OK] source: branch cli-version", text)
        self.assertIn("[OK] commit: abc1234", text)

    def test_version_command_json(self):
        args = _ns(config=None, json=True)
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_version(args)
        self.assertEqual(rc, 0)
        payload = json.loads(buf.getvalue())
        checks = {entry["check"]: entry["status"] for entry in payload["checks"]}
        self.assertIn("version", checks)
        self.assertIn("source", checks)
        self.assertIn("commit", checks)
        self.assertIn("python", checks)

    def test_main_dispatches_version_and_doctor_alias(self):
        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            old = Path.cwd()
            try:
                import os
                os.chdir(cwd)
                for command in ("version",):
                    buf = io.StringIO()
                    with redirect_stdout(buf):
                        rc = main([command])
                    self.assertEqual(rc, 0)
                    self.assertIn("embed-log version", buf.getvalue())
            finally:
                os.chdir(old)

    def test_main_version_flag_shows_source_and_commit(self):
        buf = io.StringIO()
        with patch("backend.cli.dispatch._display_version_line", return_value="embed-log 1.1.3 (branch:cli-version, abc1234)"):
            with redirect_stdout(buf):
                rc = main(["--version"])
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue().strip(), "embed-log 1.1.3 (branch:cli-version, abc1234)")


if __name__ == "__main__":
    unittest.main()
