import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend import cli


def _make_session(log_dir: Path, sid: str, markers: list[dict]) -> Path:
    sdir = log_dir / sid
    sdir.mkdir(parents=True, exist_ok=True)
    markers_path = sdir / "markers.json"
    markers_path.write_text(
        json.dumps({"session_id": sid, "markers": markers}, indent=2),
        encoding="utf-8",
    )
    # Create a source log so _session_stats finds something
    (sdir / "SENSOR_A.log").write_text("[T+00:00:00.000] boot\n", encoding="utf-8")
    manifest = {
        "session_id": sid,
        "source_files": {"SENSOR_A": str(sdir / "SENSOR_A.log")},
    }
    (sdir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return sdir


def _ns(**kw):
    """Build a argparse.Namespace stand-in."""
    return type("NS", (), kw)()


class CliMarkersTest(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)

    def tearDown(self):
        self.td.cleanup()

    # ── marker list ──

    def test_marker_list_shows_all_markers(self):
        _make_session(self.log_dir, "s1", [
            {"paneId": "A", "lineIdx": 0, "endIdx": 2, "numTs": 100, "description": "marker one", "createdAt": "2026-01-01T00:00:00Z"},
            {"paneId": "B", "lineIdx": 5, "numTs": 200, "description": "marker two", "createdAt": "2026-01-01T00:00:01Z"},
        ])
        args = _ns(marker_cmd="list", session_id="s1")
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_marker_list_empty_session_returns_zero(self):
        _make_session(self.log_dir, "s1", [])
        args = _ns(marker_cmd="list", session_id="s1")
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_marker_list_no_markers_file_returns_zero(self):
        sdir = self.log_dir / "s1"
        sdir.mkdir()
        args = _ns(marker_cmd="list", session_id="s1")
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_marker_list_unknown_session_returns_1(self):
        args = _ns(marker_cmd="list", session_id="nosuch")
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 1)

    # ── marker show ──

    def test_marker_show_valid_index(self):
        _make_session(self.log_dir, "s1", [
            {"paneId": "A", "lineIdx": 0, "numTs": 100, "description": "first", "createdAt": "2026-01-01T00:00:00Z"},
            {"paneId": "B", "lineIdx": 5, "numTs": 200, "description": "second", "createdAt": "2026-01-01T00:00:01Z"},
        ])
        args = _ns(marker_cmd="show", session_id="s1", marker_index=2)
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_marker_show_out_of_range_returns_1(self):
        _make_session(self.log_dir, "s1", [
            {"paneId": "A", "lineIdx": 0, "numTs": 100, "description": "first", "createdAt": "2026-01-01T00:00:00Z"},
        ])
        args = _ns(marker_cmd="show", session_id="s1", marker_index=5)
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 1)

    def test_marker_show_zero_index_returns_1(self):
        _make_session(self.log_dir, "s1", [
            {"paneId": "A", "lineIdx": 0, "numTs": 100, "description": "first", "createdAt": "2026-01-01T00:00:00Z"},
        ])
        args = _ns(marker_cmd="show", session_id="s1", marker_index=0)
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 1)

    def test_marker_show_negative_index_returns_1(self):
        _make_session(self.log_dir, "s1", [
            {"paneId": "A", "lineIdx": 0, "numTs": 100, "description": "first", "createdAt": "2026-01-01T00:00:00Z"},
        ])
        args = _ns(marker_cmd="show", session_id="s1", marker_index=-1)
        rc = cli._run_sessions_marker(self.log_dir, args)
        self.assertEqual(rc, 1)

    # ── sessions list marker count ──

    def test_sessions_list_shows_marker_count(self):
        _make_session(self.log_dir, "s1", [
            {"paneId": "A", "lineIdx": 0, "numTs": 100, "description": "first", "createdAt": "2026-01-01T00:00:00Z"},
        ])
        _make_session(self.log_dir, "s2", [
            {"paneId": "B", "lineIdx": 0, "numTs": 100, "description": "a", "createdAt": "2026-01-01T00:00:00Z"},
            {"paneId": "B", "lineIdx": 1, "numTs": 200, "description": "b", "createdAt": "2026-01-01T00:00:00Z"},
        ])
        _make_session(self.log_dir, "s3", [])
        sessions = cli._iter_sessions(self.log_dir)
        by_id = {s.get("session_id"): s.get("markers", -1) for s in sessions}
        self.assertEqual(by_id.get("s1"), 1)
        self.assertEqual(by_id.get("s2"), 2)
        self.assertEqual(by_id.get("s3"), 0)
