import io
import json
import sys
import tempfile
from unittest.mock import patch
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from backend.cli import main
from backend.cli.sessions import _run_sessions
from backend.cli.util import read_manifest


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
        rc = _run_sessions(["list", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    def test_list_empty(self):
        rc = _run_sessions(["list", "--log-dir", "/tmp/nonexistent"])
        self.assertEqual(rc, 0)

    def test_list_json(self):
        rc = _run_sessions(["list", "--log-dir", str(self.log_dir), "--json"])
        self.assertEqual(rc, 0)

    def test_list_limit(self):
        rc = _run_sessions(["list", "--log-dir", str(self.log_dir), "--limit", "1"])
        self.assertEqual(rc, 0)

    def test_list_sort_name(self):
        rc = _run_sessions(["list", "--log-dir", str(self.log_dir), "--sort", "name"])
        self.assertEqual(rc, 0)

    # -- info --

    def test_info(self):
        rc = _run_sessions(["info", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    def test_info_json(self):
        rc = _run_sessions(["info", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--json"])
        self.assertEqual(rc, 0)

    def test_info_missing(self):
        rc = _run_sessions(["info", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    def test_info_no_manifest(self):
        (self.sess_dir / "manifest.json").unlink()
        rc = _run_sessions(["info", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    # -- logs --

    def test_logs(self):
        rc = _run_sessions(["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    def test_logs_with_pane(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--pane", "A"]
        )
        self.assertEqual(rc, 0)

    def test_logs_missing_session(self):
        rc = _run_sessions(["logs", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    # -- logs -- grep/search --

    def test_logs_grep_substring(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "line1"]
        )
        self.assertEqual(rc, 0)

    def test_logs_grep_no_match(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "NONEXISTENT"]
        )
        self.assertEqual(rc, 0)

    def test_logs_grep_with_tail(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "line", "--tail", "2"]
        )
        self.assertEqual(rc, 0)

    def test_logs_context_requires_grep(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--context", "3"]
        )
        self.assertEqual(rc, 1)

    def test_logs_head_tail_mutually_exclusive(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--head", "2", "--tail", "3"]
        )
        self.assertEqual(rc, 1)

    def test_logs_grep_regex(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "^line[13]", "--regex"]
        )
        self.assertEqual(rc, 0)

    def test_logs_grep_ignore_case(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "LINE1", "--ignore-case"]
        )
        self.assertEqual(rc, 0)

    def test_logs_grep_regex_no_match(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "\\d{5}", "--regex"]
        )
        self.assertEqual(rc, 0)

    def test_logs_regex_without_grep_returns_1(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--regex"]
        )
        self.assertEqual(rc, 1)

    def test_logs_head_basic(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--head", "2"]
        )
        self.assertEqual(rc, 0)

    def test_logs_grep_with_context(self):
        rc = _run_sessions(
            ["logs", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir), "--grep", "line2", "--context", "1"]
        )
        self.assertEqual(rc, 0)

    def test_logs_after_filter(self):
        # Create a session with timestamped log lines for time filtering
        ts_dir = self.log_dir / "2026-06-01_12-00-00"
        ts_dir.mkdir(parents=True, exist_ok=True)
        ts_log = ts_dir / "SENSOR.log"
        ts_log.write_text(
            "[2026-06-01T12:00:00.000Z] line one\n"
            "[2026-06-01T12:05:00.000Z] line two\n"
            "[2026-06-01T12:10:00.000Z] line three\n",
            encoding="utf-8",
        )

        ts_mf = {
            "session_id": "2026-06-01_12-00-00",
            "app_name": "test",
            "started_at": "2026-06-01T12:00:00+00:00",
            "source_files": {"SENSOR": str(ts_log)},
            "session_html": None,
            "html_status": "pending",
        }
        (ts_dir / "manifest.json").write_text(json.dumps(ts_mf), encoding="utf-8")
        # After 12:06 should only return line three
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_sessions(
                ["logs", "2026-06-01_12-00-00", "--log-dir", str(self.log_dir), "--after", "2026-06-01T12:06:00"]
            )
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue(), "[2026-06-01T12:10:00.000Z] line three\n")


    def test_logs_after_iso_timezone_aware(self):
        """Verify --after with timezone-aware ISO does not crash."""
        ts_dir = self.log_dir / "2026-06-02_12-00-00"
        ts_dir.mkdir(parents=True, exist_ok=True)
        ts_log = ts_dir / "SENSOR.log"
        ts_log.write_text("[2026-06-02T12:00:00.000Z] line\n", encoding="utf-8")

        ts_mf = {
            "session_id": "2026-06-02_12-00-00",
            "app_name": "test",
            "started_at": "2026-06-02T12:00:00+00:00",
            "source_files": {"SENSOR": str(ts_log)},
            "session_html": None,
            "html_status": "pending",
        }
        (ts_dir / "manifest.json").write_text(json.dumps(ts_mf), encoding="utf-8")
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_sessions(
                ["logs", "2026-06-02_12-00-00", "--log-dir", str(self.log_dir), "--after", "2026-06-02T10:00:00+00:00"]
            )
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue(), "[2026-06-02T12:00:00.000Z] line\n")


    def test_logs_before_filter(self):
        ts_dir = self.log_dir / "2026-06-03_12-00-00"
        ts_dir.mkdir(parents=True, exist_ok=True)
        ts_log = ts_dir / "SENSOR.log"
        ts_log.write_text(
            "[2026-06-03T12:00:00.000Z] line one\n"
            "[2026-06-03T12:10:00.000Z] line two\n",
            encoding="utf-8",
        )

        ts_mf = {
            "session_id": "2026-06-03_12-00-00",
            "app_name": "test",
            "started_at": "2026-06-03T12:00:00+00:00",
            "source_files": {"SENSOR": str(ts_log)},
            "session_html": None,
            "html_status": "pending",
        }
        (ts_dir / "manifest.json").write_text(json.dumps(ts_mf), encoding="utf-8")
        # Before 12:05 should only return line one
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_sessions(
                ["logs", "2026-06-03_12-00-00", "--log-dir", str(self.log_dir), "--before", "2026-06-03T12:05:00"]
            )
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue(), "[2026-06-03T12:00:00.000Z] line one\n")

    def test_logs_tail_is_chronological_across_panes(self):
        sess = self.log_dir / "2026-06-04_12-00-00"
        sess.mkdir(parents=True, exist_ok=True)
        a_log = sess / "A.log"
        b_log = sess / "B.log"
        a_log.write_text("[2026-06-04T12:02:00.000Z] A latest\n", encoding="utf-8")

        b_log.write_text(
            "[2026-06-04T12:00:00.000Z] B early\n"
            "[2026-06-04T12:01:00.000Z] B middle\n",
            encoding="utf-8",
        )

        mf = {
            "session_id": "2026-06-04_12-00-00",
            "app_name": "test",
            "started_at": "2026-06-04T12:00:00+00:00",
            "source_files": {"A": str(a_log), "B": str(b_log)},
            "session_html": None,
            "html_status": "pending",
        }
        (sess / "manifest.json").write_text(json.dumps(mf), encoding="utf-8")
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_sessions(
                ["logs", "2026-06-04_12-00-00", "--log-dir", str(self.log_dir), "--tail", "1"]
            )
        self.assertEqual(rc, 0)
        self.assertEqual(buf.getvalue(), "[2026-06-04T12:02:00.000Z] A latest\n")



    # -- list -- filters --

    def test_list_search(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--search", "demo"]
        )
        self.assertEqual(rc, 0)

    def test_list_search_no_match(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--search", "nonexistent"]
        )
        self.assertEqual(rc, 0)

    def test_list_with_markers_no_markers(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--with-markers"]
        )
        self.assertEqual(rc, 0)

    def test_list_app_match(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--app", "demo"]
        )
        self.assertEqual(rc, 0)

    def test_list_app_no_match(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--app", "other"]
        )
        self.assertEqual(rc, 0)

    def test_list_no_html(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--no-html"]
        )
        self.assertEqual(rc, 0)

    def test_list_no_html_and_html_ready_conflict(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--no-html", "--html-ready"]
        )
        self.assertEqual(rc, 1)

    def test_list_html_ready(self):
        sess2 = self.log_dir / "2026-01-02_00-00-00"
        sess2.mkdir(parents=True, exist_ok=True)
        mf2 = dict(self.manifest, session_id="2026-01-02_00-00-00", html_status="ready", session_html=str(sess2 / "session.html"))
        (sess2 / "manifest.json").write_text(json.dumps(mf2), encoding="utf-8")
        rc = _run_sessions(["list", "--log-dir", str(self.log_dir), "--html-ready"])
        self.assertEqual(rc, 0)

    def test_list_after_before(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--after", "2026-01-01", "--before", "2026-01-02"]
        )
        self.assertEqual(rc, 0)

    def test_list_after_no_match(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--after", "2099-01-01"]
        )
        self.assertEqual(rc, 0)

    def test_list_after_invalid_date(self):
        rc = _run_sessions(
            ["list", "--log-dir", str(self.log_dir), "--after", "not-a-date"]
        )
        self.assertEqual(rc, 1)


    # -- marker -- search/pane --

    def test_marker_list_search(self):
        from backend.cli.sessions.marker import _run_sessions_marker
        markers = [
            {"paneId": "A", "lineIdx": 0, "description": "boot complete", "numTs": 100, "createdAt": "2026-01-01T00:00:00Z"},
            {"paneId": "B", "lineIdx": 5, "description": "error timeout", "numTs": 200, "createdAt": "2026-01-01T00:00:01Z"},
        ]
        import json
        (self.sess_dir / "markers.json").write_text(json.dumps({"markers": markers}), encoding="utf-8")
        args = type("NS", (), {"marker_cmd": "list", "session_id": "2026-01-01_00-00-00", "pane": None, "search": "boot"})()
        rc = _run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_marker_list_pane_filter(self):
        from backend.cli.sessions.marker import _run_sessions_marker
        markers = [
            {"paneId": "A", "lineIdx": 0, "description": "boot", "numTs": 100, "createdAt": "2026-01-01T00:00:00Z"},
            {"paneId": "B", "lineIdx": 5, "description": "boot", "numTs": 200, "createdAt": "2026-01-01T00:00:01Z"},
        ]
        import json
        (self.sess_dir / "markers.json").write_text(json.dumps({"markers": markers}), encoding="utf-8")
        args = type("NS", (), {"marker_cmd": "list", "session_id": "2026-01-01_00-00-00", "pane": "A", "search": None})()
        rc = _run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_marker_list_search_preserves_original_index(self):
        """Filtered marker list uses original index, so marker show N works."""
        from backend.cli.sessions.marker import _run_sessions_marker
        markers = [
            {"paneId": "A", "lineIdx": 0, "description": "alpha", "numTs": 100, "createdAt": "2026-01-01T00:00:00Z"},
            {"paneId": "B", "lineIdx": 5, "description": "beta target", "numTs": 200, "createdAt": "2026-01-01T00:00:01Z"},
            {"paneId": "A", "lineIdx": 10, "description": "gamma", "numTs": 300, "createdAt": "2026-01-01T00:00:02Z"},
        ]
        import json
        (self.sess_dir / "markers.json").write_text(json.dumps({"markers": markers}), encoding="utf-8")
        # Search for "target" should only match marker 2 (original index 2)
        args = type("NS", (), {"marker_cmd": "list", "session_id": "2026-01-01_00-00-00", "pane": None, "search": "target"})()
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)
        self.assertIn("  2. [B] line 5", buf.getvalue())
        # Verify marker show 2 (original index) works for the filtered result
        args2 = type("NS", (), {"marker_cmd": "show", "session_id": "2026-01-01_00-00-00", "marker_index": 2})()
        rc2 = _run_sessions_marker(self.log_dir, args2)
        self.assertEqual(rc2, 0)

    # -- export (minimal - needs merge_logs.py) --

    def test_export_missing_session(self):
        rc = _run_sessions(["export", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    # -- open --

    def test_open_explicit_session(self):
        """Open with an explicit session_id that has HTML."""
        html = self.sess_dir / "session.html"
        html.write_text("<html></html>", encoding="utf-8")
        with patch("webbrowser.open") as mock_open:
            rc = _run_sessions(["open", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)
        mock_open.assert_called_once()

    def test_open_no_html(self):
        """Open with an explicit session_id that has no HTML."""
        rc = _run_sessions(["open", "2026-01-01_00-00-00", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    def test_open_nonexistent_session(self):
        rc = _run_sessions(["open", "nonexistent", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)

    def test_open_latest_with_html(self):
        """Open with no session_id resolves to latest session with HTML."""
        # setUp created 2026-01-01_00-00-00; add a newer one with HTML
        new_dir = self.log_dir / "2026-01-02_00-00-00"
        new_dir.mkdir(parents=True, exist_ok=True)
        (new_dir / "manifest.json").write_text(
            '{"session_id":"2026-01-02_00-00-00","tabs":[],"source_files":{}}',
            encoding="utf-8",
        )
        (new_dir / "session.html").write_text("<html></html>", encoding="utf-8")
        with patch("webbrowser.open") as mock_open:
            rc = _run_sessions(["open", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)
        mock_open.assert_called_once()

    def test_open_latest_no_sessions(self):
        """Open with no session_id when no sessions exist."""
        rc = _run_sessions(["open", "--log-dir", "/tmp/nonexistent"])
        self.assertEqual(rc, 1)

    def test_open_latest_missing_html(self):
        """Open with no session_id when latest session has no HTML."""
        new_dir = self.log_dir / "2026-01-02_00-00-00"
        new_dir.mkdir(parents=True, exist_ok=True)
        (new_dir / "manifest.json").write_text(
            '{"session_id":"2026-01-02_00-00-00","tabs":[],"source_files":{}}',
            encoding="utf-8",
        )
        rc = _run_sessions(["open", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 1)
    # -- dispatch via main --

    def test_dispatch_from_main(self):
        rc = main(["sessions", "list", "--log-dir", str(self.log_dir)])
        self.assertEqual(rc, 0)

    # -- _read_manifest --

    def test_read_manifest_returns_none_for_missing(self):
        result = read_manifest(Path("/tmp/nonexistent"))
        self.assertIsNone(result)

    def test_read_manifest_corrupted_json(self):
        (self.sess_dir / "manifest.json").write_text("{bad json", encoding="utf-8")
        result = read_manifest(self.sess_dir)
        self.assertIsNone(result)


if __name__ == "__main__":
    unittest.main()
