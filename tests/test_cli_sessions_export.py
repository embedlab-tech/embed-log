import argparse
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from backend.cli.sessions import _run_sessions_export
from backend.core.runtime import LogServer


class SessionsExportTimestampTests(unittest.TestCase):
    def test_sessions_export_passes_timestamp_metadata_to_exporter(self):
        with tempfile.TemporaryDirectory() as td:
            log_dir = Path(td)
            session_id = "2026-01-01_00-00-00"
            session_dir = log_dir / session_id
            session_dir.mkdir(parents=True, exist_ok=True)

            source_file = session_dir / "A.log"
            source_file.write_text("[T+00:00:00.000] boot\n", encoding="utf-8")
            manifest = {
                "session_id": session_id,
                "timestamp_mode": "relative",
                "first_log_at": "2026-01-01T00:00:01.234+00:00",
                "tabs": [{"label": "Tab", "panes": ["A"]}],
                "pane_labels": {"A": "READER"},
                "source_files": {"A": str(source_file)},
            }
            (session_dir / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")

            captured = {}

            class FakeExporter:
                def __init__(self, **kwargs):
                    captured.update(kwargs)

                def export_html(self, reason):
                    captured["reason"] = reason
                    return True

            args = argparse.Namespace(
                session_id=session_id,
                output=None,
                format="html",
                after=None,
                before=None,
                first=None,
                last=None,
                panes=None,
                missing=False,
                json=False,
                log_dir=str(log_dir),
                first_log_at=None,
            )

            with patch("backend.session.SessionExporter", FakeExporter):
                rc = _run_sessions_export(log_dir, args)

        self.assertEqual(rc, 0)
        self.assertEqual(captured["timestamp_mode"], "relative")
        self.assertEqual(captured["first_log_at"], "2026-01-01T00:00:01.234+00:00")
        self.assertEqual(captured["reason"], "sessions_export")

    def test_sessions_export_override_first_log_at_updates_manifest(self):
        with tempfile.TemporaryDirectory() as td:
            log_dir = Path(td)
            session_id = "2026-01-01_00-00-00"
            session_dir = log_dir / session_id
            session_dir.mkdir(parents=True, exist_ok=True)

            source_file = session_dir / "A.log"
            source_file.write_text("[T+00:00:00.000] boot\n", encoding="utf-8")
            manifest = {
                "session_id": session_id,
                "timestamp_mode": "relative",
                "first_log_at": None,
                "tabs": [{"label": "Tab", "panes": ["A"]}],
                "pane_labels": {"A": "READER"},
                "source_files": {"A": str(source_file)},
            }
            manifest_path = session_dir / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            class FakeExporter:
                def __init__(self, **_kwargs):
                    pass

                def export_html(self, _reason):
                    return True

            args = argparse.Namespace(
                session_id=session_id,
                output=None,
                format="html",
                after=None,
                before=None,
                first=None,
                last=None,
                panes=None,
                missing=False,
                json=False,
                log_dir=str(log_dir),
                first_log_at="2026-01-01T00:00:01.234+00:00",
            )

            with patch("backend.session.SessionExporter", FakeExporter):
                rc = _run_sessions_export(log_dir, args)

            updated = json.loads(manifest_path.read_text(encoding="utf-8"))

        self.assertEqual(rc, 0)
        self.assertEqual(updated["first_log_at"], "2026-01-01T00:00:01.234+00:00")


class RuntimeFirstLogPersistenceTests(unittest.TestCase):
    def test_handle_first_log_at_writes_manifest(self):
        calls = []

        class DummySession:
            def set_first_log_at(self, value):
                self.first_log_at = value

            def write_manifest(self, **kwargs):
                calls.append(kwargs)

        class DummyExporter:
            def set_first_log_at(self, value):
                self.first_log_at = value

        server = LogServer.__new__(LogServer)
        server._session = DummySession()
        server._session_info = {
            "html_ready": False,
            "html_status": "pending",
            "html_updated_at": None,
            "html_error": None,
        }
        server._exporter = DummyExporter()
        server._broadcaster = None
        server._session_lock = threading.Lock()

        LogServer._handle_first_log_at(server, "2026-01-01T00:00:01.234+00:00")

        self.assertEqual(server._session.first_log_at, "2026-01-01T00:00:01.234+00:00")
        self.assertEqual(server._exporter.first_log_at, "2026-01-01T00:00:01.234+00:00")

class SessionsExportMissingTests(unittest.TestCase):
    def test_export_missing_only_exports_sessions_without_html(self):
        with tempfile.TemporaryDirectory() as td:
            log_dir = Path(td)

            # Session A: has session.html already → should be skipped
            sdir_a = log_dir / "2026-01-01_00-00-00"
            sdir_a.mkdir(parents=True)
            (sdir_a / "A.log").write_text("line1\n", encoding="utf-8")
            (sdir_a / "session.html").write_text("<html></html>", encoding="utf-8")
            (sdir_a / "manifest.json").write_text(
                json.dumps({
                    "session_id": "2026-01-01_00-00-00",
                    "tabs": [{"label": "T", "panes": ["A"]}],
                    "pane_labels": {"A": "R"},
                    "source_files": {"A": str(sdir_a / "A.log")},
                }),
                encoding="utf-8",
            )

            # Session B: no session.html → should be exported
            sdir_b = log_dir / "2026-01-02_00-00-00"
            sdir_b.mkdir(parents=True)
            (sdir_b / "B.log").write_text("line1\n", encoding="utf-8")
            (sdir_b / "manifest.json").write_text(
                json.dumps({
                    "session_id": "2026-01-02_00-00-00",
                    "tabs": [{"label": "T", "panes": ["B"]}],
                    "pane_labels": {"B": "R"},
                    "source_files": {"B": str(sdir_b / "B.log")},
                }),
                encoding="utf-8",
            )

            exported_dirs = []

            class TrackingExporter:
                def __init__(self, session_html_path=None, **kwargs):
                    self.session_html_path = session_html_path

                def export_html(self, reason):
                    if self.session_html_path:
                        exported_dirs.append(Path(self.session_html_path).parent.name)
                    return True

            args = argparse.Namespace(
                session_id=None,
                output=None,
                format="html",
                after=None,
                before=None,
                first=None,
                last=None,
                panes=None,
                missing=True,
                log_dir=str(log_dir),
                first_log_at=None,
            )

            with patch("backend.session.SessionExporter", TrackingExporter):
                rc = _run_sessions_export(log_dir, args)

        self.assertEqual(rc, 0)
        self.assertEqual(exported_dirs, ["2026-01-02_00-00-00"],
                         "Only session B (no HTML) should be exported")


if __name__ == "__main__":
    unittest.main()
