import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

from backend import cli


def _ns(**kw):
    return type("Args", (), kw)()


class VersionCommandTests(unittest.TestCase):
    def test_version_command_prints_version_header(self):
        args = _ns(config=None, json=False)
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = cli._run_version(args)
        self.assertEqual(rc, 0)
        self.assertIn("embed-log version", buf.getvalue())

    def test_version_command_prints_source_and_commit(self):
        args = _ns(config=None, json=False)
        buf = io.StringIO()
        with patch(
            "backend.cli._load_install_identity",
            return_value=("1.0.1", "abc1234", "branch", "branch", "cli-version", ""),
        ):
            with redirect_stdout(buf):
                rc = cli._run_version(args)
        self.assertEqual(rc, 0)
        text = buf.getvalue()
        self.assertIn("[OK] version: 1.0.1", text)
        self.assertIn("[OK] source: branch cli-version", text)
        self.assertIn("[OK] commit: abc1234", text)

    def test_version_command_json(self):
        args = _ns(config=None, json=True)
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = cli._run_version(args)
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
                for command in ("version", "doctor"):
                    buf = io.StringIO()
                    with redirect_stdout(buf):
                        rc = cli.main([command])
                    self.assertEqual(rc, 0)
                    self.assertIn("embed-log version", buf.getvalue())
            finally:
                os.chdir(old)

    def test_main_version_flag_shows_source_and_commit(self):
        buf = io.StringIO()
        with patch("backend.cli._display_version_line", return_value="embed-log 1.0.1 (branch:cli-version, abc1234)"):
            with redirect_stdout(buf):
                rc = cli.main(["--version"])
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue().strip(), "embed-log 1.0.1 (branch:cli-version, abc1234)")


if __name__ == "__main__":
    unittest.main()
