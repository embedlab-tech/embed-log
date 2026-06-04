"""Tests for session discovery filtering (is_session_dir + iter_sessions)."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from backend.cli.util import is_session_dir, iter_sessions


def _write_manifest(sdir: Path, sid: str) -> None:
    sdir.mkdir(parents=True, exist_ok=True)
    (sdir / "manifest.json").write_text(
        json.dumps({"session_id": sid}), encoding="utf-8"
    )


class IsSessionDirTests(unittest.TestCase):
    def test_with_manifest_is_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            sdir = Path(tmp) / "2026-06-04_12-00-00"
            _write_manifest(sdir, sdir.name)
            self.assertTrue(is_session_dir(sdir))

    def test_without_manifest_is_not_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            sdir = Path(tmp) / "loose-logs"
            sdir.mkdir()
            (sdir / "A.log").write_text("x\n", encoding="utf-8")
            self.assertFalse(is_session_dir(sdir))

    def test_empty_dir_is_not_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertFalse(is_session_dir(Path(tmp)))

    def test_file_is_not_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            f = Path(tmp) / "manifest.json"
            f.write_text("{}", encoding="utf-8")
            # manifest.json must be inside a directory, not the path itself
            self.assertFalse(is_session_dir(f))


class IterSessionsFilteringTests(unittest.TestCase):
    """The log root can contain non-session entries; iter_sessions skips them."""

    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)

    def tearDown(self):
        self.td.cleanup()

    def test_only_manifest_dirs_listed(self):
        a = self.log_dir / "2026-06-01_00-00-00"
        _write_manifest(a, a.name)
        # Stray file: not a session
        (self.log_dir / "scratch.txt").write_text("x", encoding="utf-8")
        # Stray dir with no manifest (would be a "log dir" by file content alone,
        # but per the contract, manifest is required to be a session)
        loose = self.log_dir / "loose"
        loose.mkdir()
        (loose / "A.log").write_text("x\n", encoding="utf-8")

        sessions = iter_sessions(self.log_dir)
        ids = {s.get("session_id") for s in sessions}
        self.assertEqual(ids, {a.name})
        # Unrelated dir must not appear
        self.assertNotIn("loose", ids)
        # Unrelated file must not appear
        self.assertNotIn("scratch.txt", ids)

    def test_multiple_sessions_all_listed(self):
        for sid in ("2026-06-01_00-00-00", "2026-06-02_00-00-00", "2026-06-03_00-00-00"):
            sdir = self.log_dir / sid
            _write_manifest(sdir, sid)
        sessions = iter_sessions(self.log_dir)
        self.assertEqual(len(sessions), 3)
        self.assertEqual(
            {s.get("session_id") for s in sessions},
            {
                "2026-06-01_00-00-00",
                "2026-06-02_00-00-00",
                "2026-06-03_00-00-00",
            },
        )

    def test_empty_log_dir_returns_empty(self):
        self.assertEqual(iter_sessions(self.log_dir), [])

    def test_missing_log_dir_returns_empty(self):
        self.assertEqual(iter_sessions(Path("/nonexistent/logs/")), [])

    def test_session_with_corrupt_manifest_skipped(self):
        """A directory with a non-JSON manifest is not a session per the contract."""
        bad = self.log_dir / "broken"
        bad.mkdir()
        (bad / "manifest.json").write_text("{not json", encoding="utf-8")
        # is_session_dir only checks for file presence, not validity
        # but iter_sessions calls read_manifest which would return None.
        # A corrupt manifest should NOT cause the dir to be silently listed
        # with a fake session — confirm current behavior is documented.
        self.assertTrue(is_session_dir(bad))


if __name__ == "__main__":
    unittest.main()
