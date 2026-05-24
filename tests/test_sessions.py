import json
import sys
import tempfile
import unittest
from pathlib import Path

from backend import cli


class SessionsCommandTests(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)
        self.sess_dir = self.log_dir / "2026-01-01_00-00-00"
        self.sess_dir.mkdir(parents=True, exist_ok=True)
        self.manifest = {
            "session_id": "2026-01-01_00-00-00",
            "app_name": "demo",
            "started_at": "2026-01-01T00:00:00+00:00",
            "source_files": {"A": str(self.sess_dir / "A.log"), "B": str(self.sess_dir / "B.log")},
            "tabs": [{"label": "Tab1", "panes": ["A", "B"]}],
            "job_id": "CI-1",
            "config_path": "embed-log.yml",
            "session_html": None,
            "html_status": "pending",
        }
        (self.sess_dir / "manifest.json").write_text(json.dumps(self.manifest), encoding="utf-8")
        (self.sess_dir / "A.log").write_text("line1\nline2\n", encoding="utf-8")
        (self.sess_dir / "B.log").write_text("line3\nline4\n", encoding="utf-8")

    def tearDown(self):
        self.td.cleanup()

    # -- list --

    def test_list(self):
        rc = cli._run_sessions(["list", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    def test_list_empty(self):
        rc = cli._run_sessions(["list", "--log-dir", "/tmp/nonexistent"])
        self.assertEqual(rc, 0)

    def test_list_json(self):
        rc = cli._run_sessions(["list", "--log-dir", str(self.log_dir), "--json"])
        self.assertEqual(rc, 0)

    def test_list_limit(self):
        rc = cli._run_sessions(["list", "--log-dir", str(self.log_dir), "--limit", "1"])
        self.assertEqual(rc, 0)

    def test_list_sort_name(self):
        rc = cli._run_sessions(["list", "--log-dir", str(self.log_dir), "--sort", "name"])
        self.assertEqual(rc, 0)

    # -- info --

    def test_info(self):
        rc = cli._run_sessions(["info", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    def test_info_json(self):
        rc = cli._run_sessions(["info", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--json"])
        self.assertEqual(rc, 0)

    def test_info_missing(self):
        rc = cli._run_sessions(["info", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    def test_info_no_manifest(self):
        (self.sess_dir / "manifest.json").unlink()
        rc = cli._run_sessions(["info", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    # -- logs --

    def test_logs(self):
        rc = cli._run_sessions(["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    def test_logs_with_pane(self):
        rc = cli._run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--pane", "A"]
        )
        self.assertEqual(rc, 0)

    def test_logs_missing_session(self):
        rc = cli._run_sessions(["logs", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    # -- export (minimal - needs merge_logs.py) --

    def test_export_missing_session(self):
        rc = cli._run_sessions(["export", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    # -- dispatch via main --

    def test_dispatch_from_main(self):
        rc = cli.main(["sessions", "list", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    # -- _read_manifest --

    def test_read_manifest_returns_none_for_missing(self):
        from backend.cli import _read_manifest
        result = _read_manifest(Path("/tmp/nonexistent"))
        self.assertIsNone(result)

    def test_read_manifest_corrupted_json(self):
        from backend.cli import _read_manifest
        (self.sess_dir / "manifest.json").write_text("{bad json", encoding="utf-8")
        result = _read_manifest(self.sess_dir)
        self.assertIsNone(result)


if __name__ == "__main__":
    unittest.main()
