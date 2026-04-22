import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend.session import SessionExporter, SessionManager


class SessionManagerTests(unittest.TestCase):
    def test_build_info_and_manifest(self):
        with tempfile.TemporaryDirectory() as td:
            session_dir = Path(td) / "2026-01-01_00-00-00"
            session_dir.mkdir(parents=True, exist_ok=True)
            source_files = {"A": str(session_dir / "A.log")}

            mgr = SessionManager(
                session_id="2026-01-01_00-00-00",
                session_dir=session_dir,
                tabs=[{"label": "T", "panes": ["A"]}],
                source_files=source_files,
                started_at="2026-01-01T00:00:00+00:00",
                config_path="embed-log.yml",
                job_id="CI-1",
                app_name="demo",
            )

            info = mgr.build_session_info()
            self.assertEqual(info["id"], "2026-01-01_00-00-00")
            self.assertEqual(info["job_id"], "CI-1")
            self.assertEqual(info["app_name"], "demo")
            self.assertFalse(info["html_ready"])
            self.assertEqual("pending", info["html_status"])

            mgr.write_manifest(reason="start", exported_html=False)
            manifest = json.loads(mgr.manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["session_id"], "2026-01-01_00-00-00")
            self.assertIsNone(manifest["session_html"])
            self.assertEqual("pending", manifest["html_status"])

            mgr.write_manifest(reason="signal", exported_html=True)
            manifest = json.loads(mgr.manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["last_export_reason"], "signal")
            self.assertEqual("pending", manifest["html_status"])
            self.assertTrue(str(mgr.html_path).endswith("session.html"))


class SessionExporterTests(unittest.TestCase):
    def test_export_success(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            merge_script = td_path / "merge_logs.py"
            merge_script.write_text("# dummy", encoding="utf-8")
            html_out = td_path / "session.html"

            exporter = SessionExporter(
                session_html_path=html_out,
                source_files={"A": str(td_path / "A.log")},
                tabs=[{"label": "Tab", "panes": ["A"]}],
                merge_script=merge_script,
                python_executable="python3",
            )

            class Proc:
                returncode = 0
                stderr = ""

            with patch("subprocess.run", return_value=Proc()) as run_mock:
                ok = exporter.export_html("test")

            self.assertTrue(ok)
            args = run_mock.call_args[0][0]
            self.assertIn("--tab", args)
            self.assertIn("--output", args)

    def test_export_failure_nonzero(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            merge_script = td_path / "merge_logs.py"
            merge_script.write_text("# dummy", encoding="utf-8")

            exporter = SessionExporter(
                session_html_path=td_path / "session.html",
                source_files={"A": str(td_path / "A.log")},
                tabs=[{"label": "Tab", "panes": ["A"]}],
                merge_script=merge_script,
                python_executable="python3",
            )

            class Proc:
                returncode = 1
                stderr = "boom"

            with patch("subprocess.run", return_value=Proc()):
                ok = exporter.export_html("test")

            self.assertFalse(ok)


if __name__ == "__main__":
    unittest.main()
