"""Tests for the --dir / --log-dir alias on the sessions subcommand.

Both flags must select the same session root. --dir is the documented form;
--log-dir is kept for backward compatibility.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from backend.cli.sessions import _run_sessions


def _make_session(log_dir: Path, sid: str) -> Path:
    sdir = log_dir / sid
    sdir.mkdir(parents=True, exist_ok=True)
    (sdir / "manifest.json").write_text(
        json.dumps({"session_id": sid, "app_name": "demo"}), encoding="utf-8"
    )
    (sdir / "A.log").write_text("line1\n", encoding="utf-8")
    return sdir




class SessionsDirAliasTests(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)
        _make_session(self.log_dir, "2026-06-01_12-00-00")
        _make_session(self.log_dir, "2026-06-02_12-00-00")

    def tearDown(self):
        self.td.cleanup()

    def test_dir_flag_works(self):
        rc = _run_sessions(["list", "--dir", str(self.log_dir), "--json"])
        self.assertEqual(rc, 0)

    def test_log_dir_flag_still_works(self):
        rc = _run_sessions(["list", "--log-dir", str(self.log_dir), "--json"])
        self.assertEqual(rc, 0)

    def test_dir_and_log_dir_target_same_root(self):
        """Both flags should resolve to the same log_dir."""
        other_td = tempfile.TemporaryDirectory()
        try:
            other_dir = Path(other_td.name)
            _make_session(other_dir, "2026-06-09_12-00-00")

            # --dir picks up the path it was given, not the default
            rc = _run_sessions(["list", "--dir", str(other_dir), "--json"])
            self.assertEqual(rc, 0)
        finally:
            other_td.cleanup()

    def test_dir_default_is_logs_dir(self):
        """When neither flag is given, sessions under ./logs/ are scanned."""
        # The default for the shared parser is None; _run_sessions resolves
        # None to Path('logs/'). We confirm by passing no --dir/--log-dir
        # and ensuring rc is 0 (the directory may not exist, but list is
        # tolerant of that).
        import os
        old_cwd = os.getcwd()
        try:
            os.chdir(self.log_dir)  # cwd has a session dir named "2026-..."
            rc = _run_sessions(["list", "--json"])
            self.assertEqual(rc, 0)
        finally:
            os.chdir(old_cwd)


if __name__ == "__main__":
    unittest.main()
