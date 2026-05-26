import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

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

    def test_version_command_json(self):
        args = _ns(config=None, json=True)
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = cli._run_version(args)
        self.assertEqual(rc, 0)
        payload = json.loads(buf.getvalue())
        checks = {entry["check"]: entry["status"] for entry in payload["checks"]}
        self.assertIn("version", checks)
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


if __name__ == "__main__":
    unittest.main()
